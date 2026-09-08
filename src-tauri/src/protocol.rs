use std::{
    collections::{BTreeMap, BTreeSet},
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use libsignal_protocol::{
    CiphertextMessage, CiphertextMessageType, DeviceId, Direction, GenericSignedPreKey,
    IdentityChange, IdentityKey, IdentityKeyPair, IdentityKeyStore, KeyPair, KyberPreKeyId,
    KyberPreKeyRecord, KyberPreKeyStore, PreKeyBundle, PreKeyId, PreKeyRecord, PreKeySignalMessage,
    PreKeyStore, PrivateKey, ProtocolAddress, PublicKey, SessionRecord, SessionStore,
    SignalMessage, SignalProtocolError, SignedPreKeyId, SignedPreKeyRecord, SignedPreKeyStore,
    Timestamp, kem, message_decrypt, message_encrypt, process_prekey_bundle,
};
use rand::{Rng as _, TryRngCore as _, rngs::OsRng};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
    error::{Error, Result},
    validation,
};

const PROFILE_VERSION: u8 = 3;
const SECRET_VERSION: u8 = 2;
const MESSAGE_VERSION: u8 = 3;
const DEVICE_ID: u8 = 1;
const MAX_SIGNAL_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_SESSION_BYTES: usize = 64 * 1024;
const MAX_KYBER_PRE_KEY_BYTES: usize = 8 * 1024;
const MAX_RECORDS: usize = 1_024;

#[derive(Clone, Debug, Serialize, Deserialize, Zeroize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordEntry {
    id: u32,
    record: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Zeroize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddressRecord {
    address: String,
    record: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceSecret {
    version: u8,
    github_user_id: u64,
    repository_id: u64,
    registration_id: u32,
    identity_private_key: String,
    signed_pre_key: RecordEntry,
    kyber_pre_key: RecordEntry,
    sessions: Vec<AddressRecord>,
    trusted_identities: Vec<AddressRecord>,
    #[serde(default)]
    used_kyber_base_keys: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicPreKey {
    id: u32,
    key: String,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicProfile {
    version: u8,
    protocol: String,
    registration_id: u32,
    identity_key: String,
    signed_pre_key: PublicPreKey,
    kyber_pre_key: PublicPreKey,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RatchetEnvelope {
    version: u8,
    kind: u8,
    data: String,
    signature: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrivateMessage {
    version: u8,
    sent_at: String,
    text: String,
    padding: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DecryptedMessage {
    pub sent_at: String,
    pub text: String,
}

pub struct EncryptMessage<'a> {
    pub peer_profile: &'a PublicProfile,
    pub peer_github_user_id: u64,
    pub peer_repository_id: u64,
    pub conversation_id: &'a str,
    pub message_id: &'a str,
    pub sent_at: &'a str,
    pub text: &'a str,
}

pub struct Handshake<'a> {
    pub peer_profile: &'a PublicProfile,
    pub peer_github_user_id: u64,
    pub peer_repository_id: u64,
    pub conversation_id: &'a str,
    pub created_at: &'a str,
}

pub struct DecryptHandshake<'a> {
    pub peer_profile: &'a PublicProfile,
    pub peer_github_user_id: u64,
    pub peer_repository_id: u64,
    pub conversation_id: &'a str,
    pub envelope: &'a RatchetEnvelope,
}

#[derive(Clone)]
pub struct OwnedHandshake {
    pub peer_profile: PublicProfile,
    pub peer_github_user_id: u64,
    pub peer_repository_id: u64,
    pub conversation_id: String,
    pub created_at: String,
}

#[derive(Clone)]
pub struct OwnedMessage {
    pub peer_profile: PublicProfile,
    pub peer_github_user_id: u64,
    pub peer_repository_id: u64,
    pub conversation_id: String,
    pub message_id: String,
    pub sent_at: String,
    pub text: String,
}

#[derive(Clone)]
pub struct OwnedDecryptMessage {
    pub peer_profile: PublicProfile,
    pub peer_github_user_id: u64,
    pub peer_repository_id: u64,
    pub conversation_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeResult {
    pub created_at: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HandshakePayload {
    version: u8,
    kind: HandshakeKind,
    conversation_id: String,
    created_at: String,
    initiator_id: u64,
    responder_id: u64,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HandshakeKind {
    Invitation,
    Acceptance,
}

impl HandshakeKind {
    fn purpose(self) -> &'static str {
        match self {
            Self::Invitation => "friend_invitation",
            Self::Acceptance => "friend_acceptance",
        }
    }
}

struct ValidatedProfile {
    identity_key: IdentityKey,
    signed_pre_key: PublicKey,
    signed_pre_key_id: SignedPreKeyId,
    signed_pre_key_signature: Vec<u8>,
    kyber_pre_key: kem::PublicKey,
    kyber_pre_key_id: KyberPreKeyId,
    kyber_pre_key_signature: Vec<u8>,
    registration_id: u32,
}

struct JsonSessionStore {
    records: BTreeMap<String, SessionRecord>,
}

struct JsonIdentityStore {
    key_pair: IdentityKeyPair,
    registration_id: u32,
    known: BTreeMap<String, IdentityKey>,
}

struct EmptyPreKeyStore;

struct JsonSignedPreKeyStore {
    id: SignedPreKeyId,
    record: SignedPreKeyRecord,
}

struct JsonKyberPreKeyStore {
    id: KyberPreKeyId,
    record: KyberPreKeyRecord,
    used_base_keys: BTreeSet<String>,
}

struct SignalStores {
    github_user_id: u64,
    repository_id: u64,
    sessions: JsonSessionStore,
    identity: JsonIdentityStore,
    pre_keys: EmptyPreKeyStore,
    signed_pre_key: JsonSignedPreKeyStore,
    kyber_pre_key: JsonKyberPreKeyStore,
}

impl SignalStores {
    fn from_secret(secret: &DeviceSecret) -> Result<Self> {
        if secret.version != SECRET_VERSION
            || secret.github_user_id == 0
            || secret.repository_id == 0
            || secret.registration_id == 0
            || secret.sessions.len() > MAX_RECORDS
            || secret.trusted_identities.len() > MAX_RECORDS
            || secret.used_kyber_base_keys.len() > MAX_RECORDS
        {
            return Err(Error::Crypto);
        }
        let private_key = PrivateKey::deserialize(&decode(&secret.identity_private_key, 64)?)
            .map_err(|_| Error::Crypto)?;
        let key_pair = IdentityKeyPair::try_from(private_key).map_err(|_| Error::Crypto)?;

        let mut sessions = BTreeMap::new();
        for entry in &secret.sessions {
            validate_address(&entry.address)?;
            let record = SessionRecord::deserialize(&decode(&entry.record, MAX_SESSION_BYTES)?)
                .map_err(|_| Error::Crypto)?;
            if sessions.insert(entry.address.clone(), record).is_some() {
                return Err(Error::Crypto);
            }
        }

        let mut known = BTreeMap::new();
        for entry in &secret.trusted_identities {
            validate_address(&entry.address)?;
            let public =
                PublicKey::deserialize(&decode(&entry.record, 64)?).map_err(|_| Error::Crypto)?;
            if known
                .insert(entry.address.clone(), IdentityKey::new(public))
                .is_some()
            {
                return Err(Error::Crypto);
            }
        }

        let signed_pre_key =
            SignedPreKeyRecord::deserialize(&decode(&secret.signed_pre_key.record, 1_024)?)
                .map_err(|_| Error::Crypto)?;
        let signed_id = signed_pre_key.id().map_err(|_| Error::Crypto)?;
        if u32::from(signed_id) != secret.signed_pre_key.id {
            return Err(Error::Crypto);
        }

        let kyber_pre_key = KyberPreKeyRecord::deserialize(&decode(
            &secret.kyber_pre_key.record,
            MAX_KYBER_PRE_KEY_BYTES,
        )?)
        .map_err(|_| Error::Crypto)?;
        let kyber_id = kyber_pre_key.id().map_err(|_| Error::Crypto)?;
        if u32::from(kyber_id) != secret.kyber_pre_key.id {
            return Err(Error::Crypto);
        }

        Ok(Self {
            github_user_id: secret.github_user_id,
            repository_id: secret.repository_id,
            sessions: JsonSessionStore { records: sessions },
            identity: JsonIdentityStore {
                key_pair,
                registration_id: secret.registration_id,
                known,
            },
            pre_keys: EmptyPreKeyStore,
            signed_pre_key: JsonSignedPreKeyStore {
                id: signed_id,
                record: signed_pre_key,
            },
            kyber_pre_key: JsonKyberPreKeyStore {
                id: kyber_id,
                record: kyber_pre_key,
                used_base_keys: secret.used_kyber_base_keys.iter().cloned().collect(),
            },
        })
    }

    fn into_secret(self) -> Result<DeviceSecret> {
        let identity_private_key = encode(&self.identity.key_pair.private_key().serialize());
        let signed_pre_key = RecordEntry {
            id: self.signed_pre_key.id.into(),
            record: encode(
                &self
                    .signed_pre_key
                    .record
                    .serialize()
                    .map_err(|_| Error::Crypto)?,
            ),
        };
        let kyber_pre_key = RecordEntry {
            id: self.kyber_pre_key.id.into(),
            record: encode(
                &self
                    .kyber_pre_key
                    .record
                    .serialize()
                    .map_err(|_| Error::Crypto)?,
            ),
        };
        let sessions = self
            .sessions
            .records
            .into_iter()
            .map(|(address, record)| {
                Ok(AddressRecord {
                    address,
                    record: encode(&record.serialize().map_err(|_| Error::Crypto)?),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let trusted_identities = self
            .identity
            .known
            .into_iter()
            .map(|(address, identity)| AddressRecord {
                address,
                record: encode(&identity.serialize()),
            })
            .collect();
        Ok(DeviceSecret {
            version: SECRET_VERSION,
            github_user_id: self.github_user_id,
            repository_id: self.repository_id,
            registration_id: self.identity.registration_id,
            identity_private_key,
            signed_pre_key,
            kyber_pre_key,
            sessions,
            trusted_identities,
            used_kyber_base_keys: self.kyber_pre_key.used_base_keys.into_iter().collect(),
        })
    }
}

#[async_trait(?Send)]
impl SessionStore for JsonSessionStore {
    async fn load_session(
        &self,
        address: &ProtocolAddress,
    ) -> std::result::Result<Option<SessionRecord>, SignalProtocolError> {
        Ok(self.records.get(&address.to_string()).cloned())
    }

    async fn store_session(
        &mut self,
        address: &ProtocolAddress,
        record: &SessionRecord,
    ) -> std::result::Result<(), SignalProtocolError> {
        self.records.insert(address.to_string(), record.clone());
        Ok(())
    }
}

#[async_trait(?Send)]
impl IdentityKeyStore for JsonIdentityStore {
    async fn get_identity_key_pair(
        &self,
    ) -> std::result::Result<IdentityKeyPair, SignalProtocolError> {
        Ok(self.key_pair)
    }

    async fn get_local_registration_id(&self) -> std::result::Result<u32, SignalProtocolError> {
        Ok(self.registration_id)
    }

    async fn save_identity(
        &mut self,
        address: &ProtocolAddress,
        identity: &IdentityKey,
    ) -> std::result::Result<IdentityChange, SignalProtocolError> {
        let key = address.to_string();
        match self.known.get(&key) {
            None => {
                self.known.insert(key, *identity);
                Ok(IdentityChange::NewOrUnchanged)
            }
            Some(current) if current == identity => Ok(IdentityChange::NewOrUnchanged),
            Some(_) => Ok(IdentityChange::ReplacedExisting),
        }
    }

    async fn is_trusted_identity(
        &self,
        address: &ProtocolAddress,
        identity: &IdentityKey,
        _direction: Direction,
    ) -> std::result::Result<bool, SignalProtocolError> {
        Ok(self
            .known
            .get(&address.to_string())
            .is_none_or(|current| current == identity))
    }

    async fn get_identity(
        &self,
        address: &ProtocolAddress,
    ) -> std::result::Result<Option<IdentityKey>, SignalProtocolError> {
        Ok(self.known.get(&address.to_string()).copied())
    }
}

#[async_trait(?Send)]
impl PreKeyStore for EmptyPreKeyStore {
    async fn get_pre_key(
        &self,
        _prekey_id: PreKeyId,
    ) -> std::result::Result<PreKeyRecord, SignalProtocolError> {
        Err(SignalProtocolError::InvalidPreKeyId)
    }

    async fn save_pre_key(
        &mut self,
        _prekey_id: PreKeyId,
        _record: &PreKeyRecord,
    ) -> std::result::Result<(), SignalProtocolError> {
        Err(SignalProtocolError::InvalidPreKeyId)
    }

    async fn remove_pre_key(
        &mut self,
        _prekey_id: PreKeyId,
    ) -> std::result::Result<(), SignalProtocolError> {
        Ok(())
    }
}

#[async_trait(?Send)]
impl SignedPreKeyStore for JsonSignedPreKeyStore {
    async fn get_signed_pre_key(
        &self,
        signed_prekey_id: SignedPreKeyId,
    ) -> std::result::Result<SignedPreKeyRecord, SignalProtocolError> {
        if signed_prekey_id == self.id {
            Ok(self.record.clone())
        } else {
            Err(SignalProtocolError::InvalidSignedPreKeyId)
        }
    }

    async fn save_signed_pre_key(
        &mut self,
        signed_prekey_id: SignedPreKeyId,
        record: &SignedPreKeyRecord,
    ) -> std::result::Result<(), SignalProtocolError> {
        self.id = signed_prekey_id;
        self.record = record.clone();
        Ok(())
    }
}

#[async_trait(?Send)]
impl KyberPreKeyStore for JsonKyberPreKeyStore {
    async fn get_kyber_pre_key(
        &self,
        kyber_prekey_id: KyberPreKeyId,
    ) -> std::result::Result<KyberPreKeyRecord, SignalProtocolError> {
        if kyber_prekey_id == self.id {
            Ok(self.record.clone())
        } else {
            Err(SignalProtocolError::InvalidKyberPreKeyId)
        }
    }

    async fn save_kyber_pre_key(
        &mut self,
        kyber_prekey_id: KyberPreKeyId,
        record: &KyberPreKeyRecord,
    ) -> std::result::Result<(), SignalProtocolError> {
        self.id = kyber_prekey_id;
        self.record = record.clone();
        Ok(())
    }

    async fn mark_kyber_pre_key_used(
        &mut self,
        kyber_prekey_id: KyberPreKeyId,
        signed_prekey_id: SignedPreKeyId,
        base_key: &PublicKey,
    ) -> std::result::Result<(), SignalProtocolError> {
        let mut digest = Sha256::new();
        digest.update(u32::from(kyber_prekey_id).to_be_bytes());
        digest.update(u32::from(signed_prekey_id).to_be_bytes());
        digest.update(base_key.serialize());
        let fingerprint = URL_SAFE_NO_PAD.encode(digest.finalize());
        if !self.used_base_keys.insert(fingerprint) {
            return Err(SignalProtocolError::InvalidMessage(
                CiphertextMessageType::PreKey,
                "replayed pre-key".to_owned(),
            ));
        }
        Ok(())
    }
}

pub fn create_identity(
    github_user_id: u64,
    repository_id: u64,
) -> Result<(DeviceSecret, PublicProfile)> {
    if github_user_id == 0 || repository_id == 0 {
        return Err(Error::InvalidData);
    }
    let mut rng = OsRng.unwrap_err();
    let identity_pair = IdentityKeyPair::generate(&mut rng);
    let signed_pair = KeyPair::generate(&mut rng);
    let signed_signature = identity_pair
        .private_key()
        .calculate_signature(&signed_pair.public_key.serialize(), &mut rng)
        .map_err(|_| Error::Crypto)?;
    let kyber_pair = kem::KeyPair::generate(kem::KeyType::Kyber1024, &mut rng);
    let kyber_signature = identity_pair
        .private_key()
        .calculate_signature(&kyber_pair.public_key.serialize(), &mut rng)
        .map_err(|_| Error::Crypto)?;
    let signed_id = rng.random_range(1..0x7fff_ffff_u32);
    let kyber_id = rng.random_range(1..0x7fff_ffff_u32);
    let registration_id = rng.random_range(1..16_381_u32);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Local)?
        .as_millis()
        .try_into()
        .map_err(|_| Error::Local)?;
    let signed_record = SignedPreKeyRecord::new(
        signed_id.into(),
        Timestamp::from_epoch_millis(now),
        &signed_pair,
        &signed_signature,
    );
    let kyber_record = KyberPreKeyRecord::new(
        kyber_id.into(),
        Timestamp::from_epoch_millis(now),
        &kyber_pair,
        &kyber_signature,
    );
    let secret = DeviceSecret {
        version: SECRET_VERSION,
        github_user_id,
        repository_id,
        registration_id,
        identity_private_key: encode(&identity_pair.private_key().serialize()),
        signed_pre_key: RecordEntry {
            id: signed_id,
            record: encode(&signed_record.serialize().map_err(|_| Error::Crypto)?),
        },
        kyber_pre_key: RecordEntry {
            id: kyber_id,
            record: encode(&kyber_record.serialize().map_err(|_| Error::Crypto)?),
        },
        sessions: Vec::new(),
        trusted_identities: Vec::new(),
        used_kyber_base_keys: Vec::new(),
    };
    let profile = profile_from_secret(&secret)?;
    validate_profile(&profile, github_user_id, repository_id)?;
    Ok((secret, profile))
}

pub fn restore_identity(
    serialized: &str,
    github_user_id: u64,
    repository_id: u64,
) -> Result<(DeviceSecret, PublicProfile)> {
    if serialized.len() > 2 * 1024 * 1024 {
        return Err(Error::Crypto);
    }
    let mut secret: DeviceSecret = serde_json::from_str(serialized).map_err(|_| Error::Crypto)?;
    if secret.version == 1 {
        // The legacy client stored the same libsignal records as version 1, before
        // Kyber replay bookkeeping was persisted. Upgrade only after the
        // complete legacy structure has passed strict Serde validation.
        secret.version = SECRET_VERSION;
    }
    if secret.github_user_id != github_user_id || secret.repository_id != repository_id {
        return Err(Error::Crypto);
    }
    let profile = profile_from_secret(&secret)?;
    validate_profile(&profile, github_user_id, repository_id)?;
    Ok((secret, profile))
}

pub fn serialize_identity(secret: &DeviceSecret) -> Result<SecretString> {
    let serialized = serde_json::to_string(secret).map_err(|_| Error::Local)?;
    if serialized.len() > 2 * 1024 * 1024 {
        return Err(Error::Crypto);
    }
    Ok(SecretString::from(serialized))
}

pub fn forget_peer_owned(mut secret: DeviceSecret, github_user_id: u64) -> Result<DeviceSecret> {
    let peer = address(github_user_id)?.to_string();
    secret.sessions.retain(|entry| entry.address != peer);
    secret
        .trusted_identities
        .retain(|entry| entry.address != peer);
    Ok(secret)
}

pub fn validate_public_profile(
    profile: &PublicProfile,
    github_user_id: u64,
    repository_id: u64,
) -> Result<()> {
    validate_profile(profile, github_user_id, repository_id).map(|_| ())
}

pub fn validate_public_timestamp(value: &str) -> Result<()> {
    validate_timestamp(value)
}

pub fn current_timestamp() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| Error::Local)
}

fn profile_from_secret(secret: &DeviceSecret) -> Result<PublicProfile> {
    let stores = SignalStores::from_secret(secret)?;
    let signed = &stores.signed_pre_key.record;
    let kyber = &stores.kyber_pre_key.record;
    let identity_public = stores.identity.key_pair.public_key().serialize();
    let signed_public = signed.public_key().map_err(|_| Error::Crypto)?.serialize();
    let signed_signature = signed.signature().map_err(|_| Error::Crypto)?;
    let kyber_public = kyber.public_key().map_err(|_| Error::Crypto)?.serialize();
    let kyber_signature = kyber.signature().map_err(|_| Error::Crypto)?;
    let unsigned = unsigned_profile(
        secret,
        identity_public.as_ref(),
        &signed_public,
        &signed_signature,
        &kyber_public,
        &kyber_signature,
    );
    let mut rng = OsRng.unwrap_err();
    let signature = stores
        .identity
        .key_pair
        .private_key()
        .calculate_signature(&canonical_bytes(&unsigned)?, &mut rng)
        .map_err(|_| Error::Crypto)?;
    Ok(PublicProfile {
        version: PROFILE_VERSION,
        protocol: "noosphere-signal".to_owned(),
        registration_id: secret.registration_id,
        identity_key: encode(&identity_public),
        signed_pre_key: PublicPreKey {
            id: secret.signed_pre_key.id,
            key: encode(&signed_public),
            signature: encode(&signed_signature),
        },
        kyber_pre_key: PublicPreKey {
            id: secret.kyber_pre_key.id,
            key: encode(&kyber_public),
            signature: encode(&kyber_signature),
        },
        signature: encode(&signature),
    })
}

pub async fn encrypt_message(
    secret: &mut DeviceSecret,
    message: EncryptMessage<'_>,
) -> Result<RatchetEnvelope> {
    encrypt_payload(secret, message, "message").await
}

async fn encrypt_payload(
    secret: &mut DeviceSecret,
    message: EncryptMessage<'_>,
    purpose: &str,
) -> Result<RatchetEnvelope> {
    let EncryptMessage {
        peer_profile,
        peer_github_user_id,
        peer_repository_id,
        conversation_id,
        message_id,
        sent_at,
        text,
    } = message;
    validation::conversation_id(conversation_id)?;
    validation::message_id(message_id)?;
    validate_payload_text(text, purpose)?;
    validate_timestamp(sent_at)?;
    let peer = validate_profile(peer_profile, peer_github_user_id, peer_repository_id)?;
    let mut stores = SignalStores::from_secret(secret)?;
    let remote = address(peer_github_user_id)?;
    let local = address(secret.github_user_id)?;
    if stores
        .sessions
        .load_session(&remote)
        .await
        .map_err(|_| Error::Crypto)?
        .is_none()
    {
        let bundle = PreKeyBundle::new(
            peer.registration_id,
            device_id()?,
            None,
            peer.signed_pre_key_id,
            peer.signed_pre_key,
            peer.signed_pre_key_signature,
            peer.kyber_pre_key_id,
            peer.kyber_pre_key,
            peer.kyber_pre_key_signature,
            peer.identity_key,
        )
        .map_err(|_| Error::Crypto)?;
        let mut rng = OsRng.unwrap_err();
        process_prekey_bundle(
            &remote,
            &local,
            &mut stores.sessions,
            &mut stores.identity,
            &bundle,
            SystemTime::now(),
            &mut rng,
        )
        .await
        .map_err(|_| Error::Crypto)?;
    }

    let private_message = padded_message(sent_at, text)?;
    let plaintext =
        canonical_bytes(&serde_json::to_value(private_message).map_err(|_| Error::Crypto)?)?;
    let mut rng = OsRng.unwrap_err();
    let ciphertext = message_encrypt(
        &plaintext,
        &remote,
        &local,
        &mut stores.sessions,
        &mut stores.identity,
        SystemTime::now(),
        &mut rng,
    )
    .await
    .map_err(|_| Error::Crypto)?;
    let kind = ciphertext.message_type() as u8;
    let data = encode(ciphertext.serialize());
    let unsigned = unsigned_envelope(conversation_id, message_id, purpose, kind, &data);
    let signature = stores
        .identity
        .key_pair
        .private_key()
        .calculate_signature(&canonical_bytes(&unsigned)?, &mut rng)
        .map_err(|_| Error::Crypto)?;
    let envelope = RatchetEnvelope {
        version: MESSAGE_VERSION,
        kind,
        data,
        signature: encode(&signature),
    };
    *secret = stores.into_secret()?;
    Ok(envelope)
}

pub async fn decrypt_message(
    secret: &mut DeviceSecret,
    sender_profile: &PublicProfile,
    sender_github_user_id: u64,
    sender_repository_id: u64,
    conversation_id: &str,
    message_id: &str,
    envelope: &RatchetEnvelope,
) -> Result<DecryptedMessage> {
    decrypt_payload(
        secret,
        sender_profile,
        sender_github_user_id,
        sender_repository_id,
        conversation_id,
        message_id,
        envelope,
        "message",
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn decrypt_payload(
    secret: &mut DeviceSecret,
    sender_profile: &PublicProfile,
    sender_github_user_id: u64,
    sender_repository_id: u64,
    conversation_id: &str,
    message_id: &str,
    envelope: &RatchetEnvelope,
    purpose: &str,
) -> Result<DecryptedMessage> {
    validate_ratchet_envelope_for_purpose(
        sender_profile,
        sender_github_user_id,
        sender_repository_id,
        conversation_id,
        message_id,
        purpose,
        envelope,
    )?;
    let bytes = decode(&envelope.data, MAX_SIGNAL_MESSAGE_BYTES)?;
    let ciphertext = match envelope.kind {
        2 => CiphertextMessage::SignalMessage(
            SignalMessage::try_from(bytes.as_slice()).map_err(|_| Error::Crypto)?,
        ),
        3 => CiphertextMessage::PreKeySignalMessage(
            PreKeySignalMessage::try_from(bytes.as_slice()).map_err(|_| Error::Crypto)?,
        ),
        _ => return Err(Error::Crypto),
    };
    let mut stores = SignalStores::from_secret(secret)?;
    let remote = address(sender_github_user_id)?;
    let local = address(secret.github_user_id)?;
    let mut rng = OsRng.unwrap_err();
    let plaintext = message_decrypt(
        &ciphertext,
        &remote,
        &local,
        &mut stores.sessions,
        &mut stores.identity,
        &mut stores.pre_keys,
        &stores.signed_pre_key,
        &mut stores.kyber_pre_key,
        &mut rng,
    )
    .await
    .map_err(|_| Error::Crypto)?;
    if plaintext.len() > 32 * 1024 {
        return Err(Error::Crypto);
    }
    let payload: PrivateMessage = serde_json::from_slice(&plaintext).map_err(|_| Error::Crypto)?;
    if payload.version != MESSAGE_VERSION
        || payload.padding.is_empty()
        || payload.padding.len() > 24 * 1024
        || !payload
            .padding
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(Error::Crypto);
    }
    validate_timestamp(&payload.sent_at)?;
    validate_payload_text(&payload.text, purpose)?;
    let result = DecryptedMessage {
        sent_at: payload.sent_at,
        text: payload.text,
    };
    *secret = stores.into_secret()?;
    Ok(result)
}

fn validate_payload_text(value: &str, purpose: &str) -> Result<()> {
    match purpose {
        "realtime_signal" if !value.is_empty() && value.len() <= 16 * 1024 => Ok(()),
        "realtime_signal" => Err(Error::InvalidData),
        _ => validation::message_text(value).map(|_| ()),
    }
}

pub fn validate_ratchet_envelope(
    sender_profile: &PublicProfile,
    sender_github_user_id: u64,
    sender_repository_id: u64,
    conversation_id: &str,
    message_id: &str,
    envelope: &RatchetEnvelope,
) -> Result<()> {
    validate_ratchet_envelope_for_purpose(
        sender_profile,
        sender_github_user_id,
        sender_repository_id,
        conversation_id,
        message_id,
        "message",
        envelope,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_ratchet_envelope_for_purpose(
    sender_profile: &PublicProfile,
    sender_github_user_id: u64,
    sender_repository_id: u64,
    conversation_id: &str,
    message_id: &str,
    purpose: &str,
    envelope: &RatchetEnvelope,
) -> Result<()> {
    validation::conversation_id(conversation_id)?;
    validation::message_id(message_id)?;
    if envelope.version != MESSAGE_VERSION
        || !matches!(envelope.kind, 2 | 3)
        || envelope.data.len() > MAX_SIGNAL_MESSAGE_BYTES.saturating_mul(2)
    {
        return Err(Error::Crypto);
    }
    let sender = validate_profile(sender_profile, sender_github_user_id, sender_repository_id)?;
    let unsigned = unsigned_envelope(
        conversation_id,
        message_id,
        purpose,
        envelope.kind,
        &envelope.data,
    );
    let signature = decode(&envelope.signature, 128)?;
    if sender
        .identity_key
        .public_key()
        .verify_signature(&canonical_bytes(&unsigned)?, &signature)
    {
        Ok(())
    } else {
        Err(Error::Crypto)
    }
}

pub fn validate_ratchet_envelope_shape(envelope: &RatchetEnvelope) -> Result<()> {
    if envelope.version != MESSAGE_VERSION
        || !matches!(envelope.kind, 2 | 3)
        || envelope.data.len() > MAX_SIGNAL_MESSAGE_BYTES.saturating_mul(2)
    {
        return Err(Error::Crypto);
    }
    decode(&envelope.data, MAX_SIGNAL_MESSAGE_BYTES)?;
    decode(&envelope.signature, 128)?;
    Ok(())
}

pub fn invitation_message_id(conversation_id: &str) -> Result<String> {
    validation::conversation_id(conversation_id)?;
    Ok(handshake_message_id(
        HandshakeKind::Invitation,
        conversation_id,
    ))
}

pub fn validate_invitation_envelope(
    sender_profile: &PublicProfile,
    sender_github_user_id: u64,
    sender_repository_id: u64,
    conversation_id: &str,
    envelope: &RatchetEnvelope,
) -> Result<()> {
    let message_id = invitation_message_id(conversation_id)?;
    validate_ratchet_envelope_for_purpose(
        sender_profile,
        sender_github_user_id,
        sender_repository_id,
        conversation_id,
        &message_id,
        HandshakeKind::Invitation.purpose(),
        envelope,
    )
}

pub fn acceptance_message_id(conversation_id: &str) -> Result<String> {
    validation::conversation_id(conversation_id)?;
    Ok(handshake_message_id(
        HandshakeKind::Acceptance,
        conversation_id,
    ))
}

pub fn validate_acceptance_envelope(
    sender_profile: &PublicProfile,
    sender_github_user_id: u64,
    sender_repository_id: u64,
    conversation_id: &str,
    envelope: &RatchetEnvelope,
) -> Result<()> {
    let message_id = acceptance_message_id(conversation_id)?;
    validate_ratchet_envelope_for_purpose(
        sender_profile,
        sender_github_user_id,
        sender_repository_id,
        conversation_id,
        &message_id,
        HandshakeKind::Acceptance.purpose(),
        envelope,
    )
}

pub async fn create_invitation(
    secret: &mut DeviceSecret,
    handshake: Handshake<'_>,
) -> Result<RatchetEnvelope> {
    create_handshake(secret, handshake, HandshakeKind::Invitation, true).await
}

pub fn create_invitation_owned(
    mut secret: DeviceSecret,
    handshake: OwnedHandshake,
) -> Result<(DeviceSecret, RatchetEnvelope)> {
    let envelope = run_local(create_invitation(
        &mut secret,
        Handshake {
            peer_profile: &handshake.peer_profile,
            peer_github_user_id: handshake.peer_github_user_id,
            peer_repository_id: handshake.peer_repository_id,
            conversation_id: &handshake.conversation_id,
            created_at: &handshake.created_at,
        },
    ))?;
    Ok((secret, envelope))
}

pub fn decrypt_acceptance_owned(
    mut secret: DeviceSecret,
    handshake: OwnedHandshake,
    envelope: RatchetEnvelope,
) -> Result<(DeviceSecret, HandshakeResult)> {
    let result = run_local(decrypt_acceptance(
        &mut secret,
        DecryptHandshake {
            peer_profile: &handshake.peer_profile,
            peer_github_user_id: handshake.peer_github_user_id,
            peer_repository_id: handshake.peer_repository_id,
            conversation_id: &handshake.conversation_id,
            envelope: &envelope,
        },
    ))?;
    Ok((secret, result))
}

pub fn accept_invitation_owned(
    mut secret: DeviceSecret,
    handshake: OwnedHandshake,
    invitation: RatchetEnvelope,
) -> Result<(DeviceSecret, HandshakeResult, RatchetEnvelope)> {
    let (result, acceptance) = run_local(async {
        let result = decrypt_invitation(
            &mut secret,
            DecryptHandshake {
                peer_profile: &handshake.peer_profile,
                peer_github_user_id: handshake.peer_github_user_id,
                peer_repository_id: handshake.peer_repository_id,
                conversation_id: &handshake.conversation_id,
                envelope: &invitation,
            },
        )
        .await?;
        let acceptance = create_acceptance(
            &mut secret,
            Handshake {
                peer_profile: &handshake.peer_profile,
                peer_github_user_id: handshake.peer_github_user_id,
                peer_repository_id: handshake.peer_repository_id,
                conversation_id: &handshake.conversation_id,
                created_at: &result.created_at,
            },
        )
        .await?;
        Ok((result, acceptance))
    })?;
    Ok((secret, result, acceptance))
}

pub fn encrypt_message_owned(
    mut secret: DeviceSecret,
    mut message: OwnedMessage,
) -> Result<(DeviceSecret, RatchetEnvelope)> {
    let result = run_local(encrypt_message(
        &mut secret,
        EncryptMessage {
            peer_profile: &message.peer_profile,
            peer_github_user_id: message.peer_github_user_id,
            peer_repository_id: message.peer_repository_id,
            conversation_id: &message.conversation_id,
            message_id: &message.message_id,
            sent_at: &message.sent_at,
            text: &message.text,
        },
    ));
    message.text.zeroize();
    result.map(|envelope| (secret, envelope))
}

pub fn decrypt_message_owned(
    mut secret: DeviceSecret,
    message: OwnedDecryptMessage,
    envelope: RatchetEnvelope,
) -> Result<(DeviceSecret, DecryptedMessage)> {
    let decrypted = run_local(decrypt_message(
        &mut secret,
        &message.peer_profile,
        message.peer_github_user_id,
        message.peer_repository_id,
        &message.conversation_id,
        &message.message_id,
        &envelope,
    ))?;
    Ok((secret, decrypted))
}

pub fn encrypt_realtime_owned(
    mut secret: DeviceSecret,
    mut message: OwnedMessage,
) -> Result<(DeviceSecret, RatchetEnvelope)> {
    let result = run_local(encrypt_payload(
        &mut secret,
        EncryptMessage {
            peer_profile: &message.peer_profile,
            peer_github_user_id: message.peer_github_user_id,
            peer_repository_id: message.peer_repository_id,
            conversation_id: &message.conversation_id,
            message_id: &message.message_id,
            sent_at: &message.sent_at,
            text: &message.text,
        },
        "realtime_signal",
    ));
    message.text.zeroize();
    result.map(|envelope| (secret, envelope))
}

pub fn decrypt_realtime_owned(
    mut secret: DeviceSecret,
    message: OwnedDecryptMessage,
    envelope: RatchetEnvelope,
) -> Result<(DeviceSecret, DecryptedMessage)> {
    let decrypted = run_local(decrypt_payload(
        &mut secret,
        &message.peer_profile,
        message.peer_github_user_id,
        message.peer_repository_id,
        &message.conversation_id,
        &message.message_id,
        &envelope,
        "realtime_signal",
    ))?;
    Ok((secret, decrypted))
}

pub fn realtime_message_id(conversation_id: &str, sender_id: u64) -> Result<String> {
    validation::conversation_id(conversation_id)?;
    if sender_id == 0 {
        return Err(Error::InvalidData);
    }
    let mut digest = Sha256::new();
    digest.update(b"noosphere-realtime-signal");
    digest.update([0]);
    digest.update(conversation_id.as_bytes());
    digest.update([0]);
    digest.update(sender_id.to_be_bytes());
    let digest = digest.finalize();
    Ok(format!("msg-{}", hex::encode(&digest[..16])))
}

fn run_local<T>(future: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| Error::Local)?
        .block_on(future)
}

pub async fn create_acceptance(
    secret: &mut DeviceSecret,
    handshake: Handshake<'_>,
) -> Result<RatchetEnvelope> {
    create_handshake(secret, handshake, HandshakeKind::Acceptance, false).await
}

pub async fn decrypt_invitation(
    secret: &mut DeviceSecret,
    handshake: DecryptHandshake<'_>,
) -> Result<HandshakeResult> {
    decrypt_handshake(secret, handshake, HandshakeKind::Invitation, false).await
}

pub async fn decrypt_acceptance(
    secret: &mut DeviceSecret,
    handshake: DecryptHandshake<'_>,
) -> Result<HandshakeResult> {
    decrypt_handshake(secret, handshake, HandshakeKind::Acceptance, true).await
}

async fn create_handshake(
    secret: &mut DeviceSecret,
    handshake: Handshake<'_>,
    kind: HandshakeKind,
    local_is_initiator: bool,
) -> Result<RatchetEnvelope> {
    validation::conversation_id(handshake.conversation_id)?;
    validate_timestamp(handshake.created_at)?;
    let (initiator_id, responder_id) = if local_is_initiator {
        (secret.github_user_id, handshake.peer_github_user_id)
    } else {
        (handshake.peer_github_user_id, secret.github_user_id)
    };
    let payload = HandshakePayload {
        version: 1,
        kind,
        conversation_id: handshake.conversation_id.to_owned(),
        created_at: handshake.created_at.to_owned(),
        initiator_id,
        responder_id,
    };
    let text = serde_json::to_string(&payload).map_err(|_| Error::Crypto)?;
    let message_id = handshake_message_id(kind, handshake.conversation_id);
    encrypt_payload(
        secret,
        EncryptMessage {
            peer_profile: handshake.peer_profile,
            peer_github_user_id: handshake.peer_github_user_id,
            peer_repository_id: handshake.peer_repository_id,
            conversation_id: handshake.conversation_id,
            message_id: &message_id,
            sent_at: handshake.created_at,
            text: &text,
        },
        kind.purpose(),
    )
    .await
}

async fn decrypt_handshake(
    secret: &mut DeviceSecret,
    handshake: DecryptHandshake<'_>,
    expected_kind: HandshakeKind,
    local_is_initiator: bool,
) -> Result<HandshakeResult> {
    let message_id = handshake_message_id(expected_kind, handshake.conversation_id);
    let decrypted = decrypt_payload(
        secret,
        handshake.peer_profile,
        handshake.peer_github_user_id,
        handshake.peer_repository_id,
        handshake.conversation_id,
        &message_id,
        handshake.envelope,
        expected_kind.purpose(),
    )
    .await?;
    let payload: HandshakePayload =
        serde_json::from_str(&decrypted.text).map_err(|_| Error::Crypto)?;
    let (initiator_id, responder_id) = if local_is_initiator {
        (secret.github_user_id, handshake.peer_github_user_id)
    } else {
        (handshake.peer_github_user_id, secret.github_user_id)
    };
    if payload.version != 1
        || payload.kind != expected_kind
        || payload.conversation_id != handshake.conversation_id
        || payload.created_at != decrypted.sent_at
        || payload.initiator_id != initiator_id
        || payload.responder_id != responder_id
    {
        return Err(Error::Crypto);
    }
    Ok(HandshakeResult {
        created_at: payload.created_at,
    })
}

fn handshake_message_id(kind: HandshakeKind, conversation_id: &str) -> String {
    let label = match kind {
        HandshakeKind::Invitation => b"noosphere-invitation".as_slice(),
        HandshakeKind::Acceptance => b"noosphere-acceptance".as_slice(),
    };
    let mut digest = Sha256::new();
    digest.update(label);
    digest.update([0]);
    digest.update(conversation_id.as_bytes());
    let digest = digest.finalize();
    format!("msg-{}", hex::encode(&digest[..16]))
}

fn validate_profile(
    profile: &PublicProfile,
    github_user_id: u64,
    repository_id: u64,
) -> Result<ValidatedProfile> {
    if profile.version != PROFILE_VERSION
        || profile.protocol != "noosphere-signal"
        || github_user_id == 0
        || repository_id == 0
        || profile.registration_id == 0
    {
        return Err(Error::Crypto);
    }
    let identity_public =
        PublicKey::deserialize(&decode(&profile.identity_key, 64)?).map_err(|_| Error::Crypto)?;
    let signed_bytes = decode(&profile.signed_pre_key.key, 64)?;
    let signed_public = PublicKey::deserialize(&signed_bytes).map_err(|_| Error::Crypto)?;
    let signed_signature = decode(&profile.signed_pre_key.signature, 128)?;
    let kyber_bytes = decode(&profile.kyber_pre_key.key, 4_096)?;
    let kyber_public = kem::PublicKey::deserialize(&kyber_bytes).map_err(|_| Error::Crypto)?;
    let kyber_signature = decode(&profile.kyber_pre_key.signature, 128)?;
    if !identity_public.verify_signature(&signed_bytes, &signed_signature)
        || !identity_public.verify_signature(&kyber_bytes, &kyber_signature)
    {
        return Err(Error::Crypto);
    }
    let unsigned = serde_json::json!({
        "context": {
            "deviceId": DEVICE_ID,
            "githubUserId": github_user_id,
            "repositoryId": repository_id,
        },
        "identityKey": profile.identity_key,
        "kyberPreKey": profile.kyber_pre_key,
        "protocol": profile.protocol,
        "registrationId": profile.registration_id,
        "signedPreKey": profile.signed_pre_key,
        "version": profile.version,
    });
    if !identity_public.verify_signature(
        &canonical_bytes(&unsigned)?,
        &decode(&profile.signature, 128)?,
    ) {
        return Err(Error::Crypto);
    }
    Ok(ValidatedProfile {
        identity_key: IdentityKey::new(identity_public),
        signed_pre_key: signed_public,
        signed_pre_key_id: profile.signed_pre_key.id.into(),
        signed_pre_key_signature: signed_signature,
        kyber_pre_key: kyber_public,
        kyber_pre_key_id: profile.kyber_pre_key.id.into(),
        kyber_pre_key_signature: kyber_signature,
        registration_id: profile.registration_id,
    })
}

fn unsigned_profile(
    secret: &DeviceSecret,
    identity_key: &[u8],
    signed_key: &[u8],
    signed_signature: &[u8],
    kyber_key: &[u8],
    kyber_signature: &[u8],
) -> serde_json::Value {
    serde_json::json!({
        "context": {
            "deviceId": DEVICE_ID,
            "githubUserId": secret.github_user_id,
            "repositoryId": secret.repository_id,
        },
        "identityKey": encode(identity_key),
        "kyberPreKey": {
            "id": secret.kyber_pre_key.id,
            "key": encode(kyber_key),
            "signature": encode(kyber_signature),
        },
        "protocol": "noosphere-signal",
        "registrationId": secret.registration_id,
        "signedPreKey": {
            "id": secret.signed_pre_key.id,
            "key": encode(signed_key),
            "signature": encode(signed_signature),
        },
        "version": PROFILE_VERSION,
    })
}

fn unsigned_envelope(
    conversation_id: &str,
    message_id: &str,
    purpose: &str,
    kind: u8,
    data: &str,
) -> serde_json::Value {
    serde_json::json!({
        "context": {
            "conversationId": conversation_id,
            "messageId": message_id,
            "purpose": purpose,
        },
        "envelope": {
            "data": data,
            "kind": kind,
            "version": MESSAGE_VERSION,
        }
    })
}

fn padded_message(sent_at: &str, text: &str) -> Result<PrivateMessage> {
    let mut payload = PrivateMessage {
        version: MESSAGE_VERSION,
        sent_at: sent_at.to_owned(),
        text: text.to_owned(),
        padding: String::new(),
    };
    let current =
        canonical_bytes(&serde_json::to_value(&payload).map_err(|_| Error::Crypto)?)?.len();
    let target = [512, 1_024, 2_048, 4_096, 8_192, 16_384, 24_576]
        .into_iter()
        .find(|bucket| *bucket > current)
        .ok_or(Error::Crypto)?;
    let padding_length = target - current;
    let mut random = vec![0_u8; padding_length.div_ceil(4).saturating_mul(3)];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|_| Error::Crypto)?;
    payload.padding = URL_SAFE_NO_PAD.encode(random);
    payload.padding.truncate(padding_length);
    let encoded = canonical_bytes(&serde_json::to_value(&payload).map_err(|_| Error::Crypto)?)?;
    if encoded.len() != target {
        return Err(Error::Crypto);
    }
    Ok(payload)
}

fn canonical_bytes(value: &serde_json::Value) -> Result<Vec<u8>> {
    fn write(value: &serde_json::Value, output: &mut String) -> Result<()> {
        match value {
            serde_json::Value::Null => output.push_str("null"),
            serde_json::Value::Bool(value) => {
                output.push_str(if *value { "true" } else { "false" })
            }
            serde_json::Value::Number(value) => {
                if !value.is_i64() && !value.is_u64() {
                    return Err(Error::Crypto);
                }
                output.push_str(&value.to_string());
            }
            serde_json::Value::String(value) => {
                output.push_str(&serde_json::to_string(value).map_err(|_| Error::Crypto)?)
            }
            serde_json::Value::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    write(value, output)?;
                }
                output.push(']');
            }
            serde_json::Value::Object(values) => {
                output.push('{');
                let mut entries = values.iter().collect::<Vec<_>>();
                entries.sort_unstable_by_key(|(key, _)| *key);
                for (index, (key, value)) in entries.into_iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    output.push_str(&serde_json::to_string(key).map_err(|_| Error::Crypto)?);
                    output.push(':');
                    write(value, output)?;
                }
                output.push('}');
            }
        }
        Ok(())
    }

    let mut output = String::new();
    write(value, &mut output)?;
    Ok(output.into_bytes())
}

fn address(github_user_id: u64) -> Result<ProtocolAddress> {
    if github_user_id == 0 {
        return Err(Error::InvalidData);
    }
    Ok(ProtocolAddress::new(
        github_user_id.to_string(),
        device_id()?,
    ))
}

fn device_id() -> Result<DeviceId> {
    DeviceId::new(DEVICE_ID).map_err(|_| Error::Crypto)
}

fn validate_address(value: &str) -> Result<()> {
    let Some((user, device)) = value.rsplit_once('.') else {
        return Err(Error::Crypto);
    };
    if user.parse::<u64>().is_err() || device != DEVICE_ID.to_string() {
        return Err(Error::Crypto);
    }
    Ok(())
}

fn validate_timestamp(value: &str) -> Result<()> {
    if value.len() >= 20
        && value.len() <= 32
        && value.ends_with('Z')
        && value.bytes().all(|byte| byte.is_ascii_graphic())
        && OffsetDateTime::parse(value, &Rfc3339).is_ok()
    {
        Ok(())
    } else {
        Err(Error::InvalidData)
    }
}

fn encode(value: &[u8]) -> String {
    STANDARD.encode(value)
}

fn decode(value: &str, maximum: usize) -> Result<Vec<u8>> {
    if value.is_empty() || value.len() > maximum.div_ceil(3).saturating_mul(4) + 4 {
        return Err(Error::Crypto);
    }
    let decoded = STANDARD.decode(value).map_err(|_| Error::Crypto)?;
    if decoded.is_empty() || decoded.len() > maximum || STANDARD.encode(&decoded) != value {
        return Err(Error::Crypto);
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret as _;

    use super::*;

    const CONVERSATION: &str = "dm-0123456789abcdef0123456789abcdef";

    fn message_id(value: u8) -> String {
        format!("msg-{value:032x}")
    }

    #[tokio::test]
    async fn ratchet_handles_out_of_order_messages_replay_and_restoration() {
        let (mut alice_secret, alice_profile) = create_identity(42, 420).unwrap();
        let (mut bob_secret, bob_profile) = create_identity(99, 990).unwrap();
        let first = encrypt_message(
            &mut alice_secret,
            EncryptMessage {
                peer_profile: &bob_profile,
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: CONVERSATION,
                message_id: &message_id(1),
                sent_at: "2026-09-04T08:00:00.000Z",
                text: "premier",
            },
        )
        .await
        .unwrap();
        let second = encrypt_message(
            &mut alice_secret,
            EncryptMessage {
                peer_profile: &bob_profile,
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: CONVERSATION,
                message_id: &message_id(2),
                sent_at: "2026-09-04T08:00:01.000Z",
                text: "deuxième",
            },
        )
        .await
        .unwrap();

        let decrypted_second = decrypt_message(
            &mut bob_secret,
            &alice_profile,
            42,
            420,
            CONVERSATION,
            &message_id(2),
            &second,
        )
        .await
        .unwrap();
        assert_eq!(decrypted_second.text, "deuxième");

        let serialized = serialize_identity(&bob_secret).unwrap();
        let (mut restored, restored_profile) =
            restore_identity(serialized.expose_secret(), 99, 990).unwrap();
        assert_eq!(restored_profile.identity_key, bob_profile.identity_key);
        let decrypted_first = decrypt_message(
            &mut restored,
            &alice_profile,
            42,
            420,
            CONVERSATION,
            &message_id(1),
            &first,
        )
        .await
        .unwrap();
        assert_eq!(decrypted_first.text, "premier");
        assert!(
            decrypt_message(
                &mut restored,
                &alice_profile,
                42,
                420,
                CONVERSATION,
                &message_id(1),
                &first,
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn signature_tampering_and_wrong_identity_are_rejected() {
        let (mut alice_secret, alice_profile) = create_identity(42, 420).unwrap();
        let (mut bob_secret, bob_profile) = create_identity(99, 990).unwrap();
        let (_, impostor_profile) = create_identity(42, 420).unwrap();
        let envelope = encrypt_message(
            &mut alice_secret,
            EncryptMessage {
                peer_profile: &bob_profile,
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: CONVERSATION,
                message_id: &message_id(3),
                sent_at: "2026-09-04T08:00:00.000Z",
                text: "authentique",
            },
        )
        .await
        .unwrap();
        let mut altered = envelope.clone();
        altered.data.push('A');
        assert!(
            decrypt_message(
                &mut bob_secret,
                &alice_profile,
                42,
                420,
                CONVERSATION,
                &message_id(3),
                &altered,
            )
            .await
            .is_err()
        );
        assert!(
            decrypt_message(
                &mut bob_secret,
                &impostor_profile,
                42,
                420,
                CONVERSATION,
                &message_id(3),
                &envelope,
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn invitation_and_acceptance_share_the_authenticated_ratchet() {
        let (mut alice_secret, alice_profile) = create_identity(42, 420).unwrap();
        let (mut bob_secret, bob_profile) = create_identity(99, 990).unwrap();
        let created_at = "2026-09-04T09:00:00.000Z";

        let invitation = create_invitation(
            &mut alice_secret,
            Handshake {
                peer_profile: &bob_profile,
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: CONVERSATION,
                created_at,
            },
        )
        .await
        .unwrap();
        let opened = decrypt_invitation(
            &mut bob_secret,
            DecryptHandshake {
                peer_profile: &alice_profile,
                peer_github_user_id: 42,
                peer_repository_id: 420,
                conversation_id: CONVERSATION,
                envelope: &invitation,
            },
        )
        .await
        .unwrap();
        assert_eq!(opened.created_at, created_at);

        let acceptance = create_acceptance(
            &mut bob_secret,
            Handshake {
                peer_profile: &alice_profile,
                peer_github_user_id: 42,
                peer_repository_id: 420,
                conversation_id: CONVERSATION,
                created_at,
            },
        )
        .await
        .unwrap();
        decrypt_acceptance(
            &mut alice_secret,
            DecryptHandshake {
                peer_profile: &bob_profile,
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: CONVERSATION,
                envelope: &acceptance,
            },
        )
        .await
        .unwrap();

        let reply = encrypt_message(
            &mut bob_secret,
            EncryptMessage {
                peer_profile: &alice_profile,
                peer_github_user_id: 42,
                peer_repository_id: 420,
                conversation_id: CONVERSATION,
                message_id: &message_id(8),
                sent_at: "2026-09-04T09:00:01.000Z",
                text: "accepté",
            },
        )
        .await
        .unwrap();
        let reply = decrypt_message(
            &mut alice_secret,
            &bob_profile,
            99,
            990,
            CONVERSATION,
            &message_id(8),
            &reply,
        )
        .await
        .unwrap();
        assert_eq!(reply.text, "accepté");
    }

    #[test]
    fn two_peer_repository_simulation_survives_restart_and_bidirectional_messages() {
        let (alice, alice_profile) = create_identity(42, 420).unwrap();
        let (bob, bob_profile) = create_identity(99, 990).unwrap();
        let created_at = "2026-09-04T09:00:00.000Z";
        let alice_handshake = OwnedHandshake {
            peer_profile: bob_profile.clone(),
            peer_github_user_id: 99,
            peer_repository_id: 990,
            conversation_id: CONVERSATION.to_owned(),
            created_at: created_at.to_owned(),
        };
        let bob_handshake = OwnedHandshake {
            peer_profile: alice_profile.clone(),
            peer_github_user_id: 42,
            peer_repository_id: 420,
            conversation_id: CONVERSATION.to_owned(),
            created_at: created_at.to_owned(),
        };
        let (alice, invitation) = create_invitation_owned(alice, alice_handshake.clone()).unwrap();
        let mut alice_repository = BTreeMap::from([(
            format!("friends/outgoing/99/{CONVERSATION}.enc.json"),
            invitation.clone(),
        )]);
        let (bob, opened, acceptance) =
            accept_invitation_owned(bob, bob_handshake, invitation.clone()).unwrap();
        assert_eq!(opened.created_at, created_at);
        let mut bob_repository = BTreeMap::from([(
            format!("conv/{CONVERSATION}/conversation.enc.json"),
            acceptance.clone(),
        )]);
        let (alice, confirmed) =
            decrypt_acceptance_owned(alice, alice_handshake, acceptance).unwrap();
        assert_eq!(confirmed.created_at, created_at);
        alice_repository.insert(
            format!("conv/{CONVERSATION}/conversation.enc.json"),
            invitation,
        );

        let alice = serialize_identity(&alice).unwrap();
        let bob = serialize_identity(&bob).unwrap();
        let (mut alice, _) = restore_identity(alice.expose_secret(), 42, 420).unwrap();
        let (mut bob, _) = restore_identity(bob.expose_secret(), 99, 990).unwrap();

        let first_id = message_id(20);
        let second_id = message_id(21);
        let (next_alice, first) = encrypt_message_owned(
            alice,
            OwnedMessage {
                peer_profile: bob_profile.clone(),
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: CONVERSATION.to_owned(),
                message_id: first_id.clone(),
                sent_at: "2026-09-04T09:01:00.000Z".to_owned(),
                text: "premier message".to_owned(),
            },
        )
        .unwrap();
        alice = next_alice;
        let (next_alice, second) = encrypt_message_owned(
            alice,
            OwnedMessage {
                peer_profile: bob_profile.clone(),
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: CONVERSATION.to_owned(),
                message_id: second_id.clone(),
                sent_at: "2026-09-04T09:01:01.000Z".to_owned(),
                text: "second message".to_owned(),
            },
        )
        .unwrap();
        alice = next_alice;
        alice_repository.insert(
            format!("conv/{CONVERSATION}/messages/{first_id}.enc.json"),
            first.clone(),
        );
        alice_repository.insert(
            format!("conv/{CONVERSATION}/messages/{second_id}.enc.json"),
            second.clone(),
        );
        let decrypt = |id: &str| OwnedDecryptMessage {
            peer_profile: alice_profile.clone(),
            peer_github_user_id: 42,
            peer_repository_id: 420,
            conversation_id: CONVERSATION.to_owned(),
            message_id: id.to_owned(),
        };
        let (next_bob, opened_second) =
            decrypt_message_owned(bob, decrypt(&second_id), second.clone()).unwrap();
        bob = next_bob;
        let (next_bob, opened_first) =
            decrypt_message_owned(bob, decrypt(&first_id), first.clone()).unwrap();
        bob = next_bob;
        assert_eq!(opened_second.text, "second message");
        assert_eq!(opened_first.text, "premier message");
        bob_repository.insert(
            format!("conv/{CONVERSATION}/messages/{first_id}.enc.json"),
            first,
        );
        bob_repository.insert(
            format!("conv/{CONVERSATION}/messages/{second_id}.enc.json"),
            second,
        );

        let reply_id = message_id(22);
        let (bob, reply) = encrypt_message_owned(
            bob,
            OwnedMessage {
                peer_profile: alice_profile.clone(),
                peer_github_user_id: 42,
                peer_repository_id: 420,
                conversation_id: CONVERSATION.to_owned(),
                message_id: reply_id.clone(),
                sent_at: "2026-09-04T09:01:02.000Z".to_owned(),
                text: "réponse".to_owned(),
            },
        )
        .unwrap();
        let (alice, opened_reply) = decrypt_message_owned(
            alice,
            OwnedDecryptMessage {
                peer_profile: bob_profile,
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: CONVERSATION.to_owned(),
                message_id: reply_id.clone(),
            },
            reply.clone(),
        )
        .unwrap();
        assert_eq!(opened_reply.text, "réponse");
        assert!(serialize_identity(&alice).is_ok());
        assert!(serialize_identity(&bob).is_ok());
        bob_repository.insert(
            format!("conv/{CONVERSATION}/messages/{reply_id}.enc.json"),
            reply.clone(),
        );
        alice_repository.insert(
            format!("conv/{CONVERSATION}/messages/{reply_id}.enc.json"),
            reply,
        );
        assert_eq!(
            alice_repository.get(&format!("conv/{CONVERSATION}/messages/{reply_id}.enc.json")),
            bob_repository.get(&format!("conv/{CONVERSATION}/messages/{reply_id}.enc.json"))
        );
    }

    #[test]
    fn crossed_invitations_converge_on_the_selected_conversation() {
        let (alice, alice_profile) = create_identity(42, 420).unwrap();
        let (bob, bob_profile) = create_identity(99, 990).unwrap();
        let alice_conversation = "dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let bob_conversation = "dm-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let created_at = "2026-09-04T09:00:00.000Z";

        let alice_handshake = OwnedHandshake {
            peer_profile: bob_profile.clone(),
            peer_github_user_id: 99,
            peer_repository_id: 990,
            conversation_id: alice_conversation.to_owned(),
            created_at: created_at.to_owned(),
        };
        let bob_handshake = OwnedHandshake {
            peer_profile: alice_profile.clone(),
            peer_github_user_id: 42,
            peer_repository_id: 420,
            conversation_id: bob_conversation.to_owned(),
            created_at: created_at.to_owned(),
        };

        let (alice, alice_invitation) =
            create_invitation_owned(alice, alice_handshake.clone()).unwrap();
        let (bob, _bob_invitation) = create_invitation_owned(bob, bob_handshake.clone()).unwrap();
        let bob_acceptance = OwnedHandshake {
            conversation_id: alice_conversation.to_owned(),
            ..bob_handshake
        };
        let (bob, opened, acceptance) =
            accept_invitation_owned(bob, bob_acceptance, alice_invitation).unwrap();
        assert_eq!(opened.created_at, created_at);

        let (alice, confirmed) =
            decrypt_acceptance_owned(alice, alice_handshake, acceptance).unwrap();
        assert_eq!(confirmed.created_at, created_at);

        let message = OwnedMessage {
            peer_profile: bob_profile,
            peer_github_user_id: 99,
            peer_repository_id: 990,
            conversation_id: alice_conversation.to_owned(),
            message_id: message_id(30),
            sent_at: "2026-09-04T09:01:00.000Z".to_owned(),
            text: "demande croisee".to_owned(),
        };
        let message_id = message.message_id.clone();
        let (alice, envelope) = encrypt_message_owned(alice, message).unwrap();
        let (_, decrypted) = decrypt_message_owned(
            bob,
            OwnedDecryptMessage {
                peer_profile: alice_profile,
                peer_github_user_id: 42,
                peer_repository_id: 420,
                conversation_id: alice_conversation.to_owned(),
                message_id,
            },
            envelope,
        )
        .unwrap();
        let _ = alice;
        assert_eq!(decrypted.text, "demande croisee");
    }

    #[test]
    fn a_new_peer_identity_can_replace_a_pending_session() {
        let (_, old_alice_profile) = create_identity(42, 420).unwrap();
        let (bob, bob_profile) = create_identity(99, 990).unwrap();
        let old_handshake = OwnedHandshake {
            peer_profile: old_alice_profile,
            peer_github_user_id: 42,
            peer_repository_id: 420,
            conversation_id: "dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            created_at: "2026-09-04T09:00:00.000Z".to_owned(),
        };
        let (bob, _) = create_invitation_owned(bob, old_handshake).unwrap();

        let (new_alice, new_alice_profile) = create_identity(42, 420).unwrap();
        let conversation_id = "dm-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let alice_handshake = OwnedHandshake {
            peer_profile: bob_profile,
            peer_github_user_id: 99,
            peer_repository_id: 990,
            conversation_id: conversation_id.to_owned(),
            created_at: "2026-09-04T09:01:00.000Z".to_owned(),
        };
        let (new_alice, invitation) =
            create_invitation_owned(new_alice, alice_handshake.clone()).unwrap();
        let bob = forget_peer_owned(bob, 42).unwrap();
        let current_handshake = OwnedHandshake {
            peer_profile: new_alice_profile,
            peer_github_user_id: 42,
            peer_repository_id: 420,
            conversation_id: conversation_id.to_owned(),
            created_at: "2026-09-04T09:01:00.000Z".to_owned(),
        };

        let (bob, opened, acceptance) =
            accept_invitation_owned(bob, current_handshake, invitation).unwrap();
        assert_eq!(opened.created_at, "2026-09-04T09:01:00.000Z");

        let (new_alice, confirmed) =
            decrypt_acceptance_owned(new_alice, alice_handshake, acceptance).unwrap();
        assert_eq!(confirmed.created_at, opened.created_at);
        let message_id = message_id(31);
        let (new_alice, message) = encrypt_message_owned(
            new_alice,
            OwnedMessage {
                peer_profile: profile_from_secret(&bob).unwrap(),
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: conversation_id.to_owned(),
                message_id: message_id.clone(),
                sent_at: "2026-09-04T09:02:00.000Z".to_owned(),
                text: "session renouvelee".to_owned(),
            },
        )
        .unwrap();
        let (_, decrypted) = decrypt_message_owned(
            bob,
            OwnedDecryptMessage {
                peer_profile: profile_from_secret(&new_alice).unwrap(),
                peer_github_user_id: 42,
                peer_repository_id: 420,
                conversation_id: conversation_id.to_owned(),
                message_id,
            },
            message,
        )
        .unwrap();
        assert_eq!(decrypted.text, "session renouvelee");
    }

    #[test]
    fn realtime_envelopes_are_domain_separated_and_replay_protected() {
        let (alice, alice_profile) = create_identity(42, 420).unwrap();
        let (bob, bob_profile) = create_identity(99, 990).unwrap();
        let message_id = realtime_message_id(CONVERSATION, 42).unwrap();
        let session = "rtc-0123456789abcdef0123456789abcdef";
        let serialized = format!(
            "{{\"version\":1,\"kind\":\"offer\",\"sessionId\":\"{session}\",\"createdAt\":\"2026-09-04T09:00:00Z\",\"expiresAt\":\"2026-09-04T09:02:00Z\",\"sdp\":\"v=0\\r\\n\"}}"
        );
        let (alice, envelope) = encrypt_realtime_owned(
            alice,
            OwnedMessage {
                peer_profile: bob_profile,
                peer_github_user_id: 99,
                peer_repository_id: 990,
                conversation_id: CONVERSATION.to_owned(),
                message_id: message_id.clone(),
                sent_at: "2026-09-04T09:00:00Z".to_owned(),
                text: serialized.clone(),
            },
        )
        .unwrap();
        assert!(!serde_json::to_string(&envelope).unwrap().contains(session));
        assert!(
            validate_ratchet_envelope(
                &alice_profile,
                42,
                420,
                CONVERSATION,
                &message_id,
                &envelope,
            )
            .is_err()
        );
        let owned = OwnedDecryptMessage {
            peer_profile: alice_profile,
            peer_github_user_id: 42,
            peer_repository_id: 420,
            conversation_id: CONVERSATION.to_owned(),
            message_id,
        };
        let (bob, opened) = decrypt_realtime_owned(bob, owned.clone(), envelope.clone()).unwrap();
        assert_eq!(opened.text, serialized);
        assert!(decrypt_realtime_owned(bob, owned, envelope).is_err());
        assert!(serialize_identity(&alice).is_ok());
    }

    #[test]
    fn realtime_signaling_accepts_video_sdp_without_relaxing_chat_messages() {
        let large_sdp = "a".repeat(8 * 1024);
        assert!(validate_payload_text(&large_sdp, "realtime_signal").is_ok());
        assert!(validate_payload_text(&large_sdp, "message").is_err());
        assert!(validate_payload_text(&"a".repeat(16 * 1024 + 1), "realtime_signal").is_err());
    }

    #[test]
    fn public_profile_hides_github_identity_and_rejects_unknown_fields() {
        let (_, profile) = create_identity(42, 420).unwrap();
        let serialized = serde_json::to_string(&profile).unwrap();
        assert!(!serialized.contains("githubUserId"));
        assert!(!serialized.contains("repositoryId"));
        let mut value = serde_json::to_value(profile).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("login".to_owned(), serde_json::json!("forged"));
        assert!(serde_json::from_value::<PublicProfile>(value).is_err());
    }

    #[test]
    fn legacy_identity_secret_version_one_is_upgraded_without_key_rotation() {
        let (secret, profile) = create_identity(42, 420).unwrap();
        let mut legacy = serde_json::to_value(secret).unwrap();
        let object = legacy.as_object_mut().unwrap();
        object.insert("version".to_owned(), serde_json::json!(1));
        object.remove("usedKyberBaseKeys");
        let serialized = serde_json::to_string(&legacy).unwrap();
        let (upgraded, upgraded_profile) = restore_identity(&serialized, 42, 420).unwrap();
        assert_eq!(upgraded.version, SECRET_VERSION);
        assert_eq!(upgraded_profile.identity_key, profile.identity_key);
    }
}

//! Device registration and directed, encrypted control mailboxes. These messages
//! use the existing libsignal identity without sharing a messaging ratchet between
//! machines. The receiver must pin the sender separately before granting access.
use crate::{Error, Result, identity::SignalIdentity, permissions::Principal};
use libsignal_protocol::{KeyPair, PublicKey};
use rand::{TryRngCore as _, rngs::OsRng};
use ring::{aead, hkdf};
use serde::{Deserialize, Serialize};

const DOMAIN: &[u8] = b"noosphere/remote/directory/v1\0";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Registration {
    pub version: u8,
    pub repository_id: u64,
    pub principal: Principal,
    pub signature: Vec<u8>,
}

impl Registration {
    pub fn create(
        identity: &SignalIdentity,
        machine: [u8; 16],
        repository_id: u64,
    ) -> Result<Self> {
        let mut value = Self {
            version: 1,
            repository_id,
            principal: identity.principal(machine)?,
            signature: Vec::new(),
        };
        value.signature = sign(identity, &value.transcript()?)?;
        Ok(value)
    }
    fn transcript(&self) -> Result<Vec<u8>> {
        transcript(
            b"registration",
            &(self.version, self.repository_id, &self.principal),
        )
    }
    pub fn verify(&self, github_id: u64, repository_id: u64) -> Result<()> {
        if self.version != 1
            || self.repository_id != repository_id
            || repository_id == 0
            || self.principal.github_user_id != github_id
        {
            return Err(Error::Authentication);
        }
        verify(&self.principal, &self.transcript()?, &self.signature)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sealed {
    pub sender: Principal,
    pub recipient: Principal,
    pub ephemeral_key: Vec<u8>,
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
    pub signature: Vec<u8>,
}

fn transcript<T: Serialize>(kind: &[u8], data: &T) -> Result<Vec<u8>> {
    let mut output = DOMAIN.to_vec();
    output.extend_from_slice(kind);
    output.extend_from_slice(&serde_json::to_vec(data).map_err(|_| Error::InvalidPacket)?);
    if output.len() > 64 * 1024 {
        return Err(Error::ResourceLimit);
    }
    Ok(output)
}
fn sign(identity: &SignalIdentity, bytes: &[u8]) -> Result<Vec<u8>> {
    identity
        .private
        .calculate_signature(bytes, &mut OsRng.unwrap_err())
        .map(|s| s.to_vec())
        .map_err(|_| Error::Authentication)
}
fn verify(principal: &Principal, bytes: &[u8], signature: &[u8]) -> Result<()> {
    principal.validate()?;
    let key = PublicKey::deserialize(&principal.identity_key).map_err(|_| Error::Authentication)?;
    if signature.len() != 64 || !key.verify_signature(bytes, signature) {
        return Err(Error::Authentication);
    }
    Ok(())
}
fn key(shared: &[u8], context: &[u8]) -> Result<aead::LessSafeKey> {
    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, DOMAIN);
    let prk = salt.extract(shared);
    let info = [context];
    let okm = prk
        .expand(&info, &aead::CHACHA20_POLY1305)
        .map_err(|_| Error::Authentication)?;
    Ok(aead::LessSafeKey::new(aead::UnboundKey::from(okm)))
}

impl Sealed {
    fn context(&self) -> Result<Vec<u8>> {
        transcript(
            b"mailbox",
            &(
                &self.sender,
                &self.recipient,
                &self.ephemeral_key,
                self.nonce,
            ),
        )
    }
    fn signed_bytes(&self) -> Result<Vec<u8>> {
        transcript(b"ciphertext", &(self.context()?, &self.ciphertext))
    }
    pub fn seal(
        identity: &SignalIdentity,
        machine: [u8; 16],
        recipient: Principal,
        bytes: &[u8],
    ) -> Result<Self> {
        recipient.validate()?;
        if bytes.len() > 12 * 1024 {
            return Err(Error::ResourceLimit);
        }
        let ephemeral = KeyPair::generate(&mut OsRng.unwrap_err()).private_key;
        let peer =
            PublicKey::deserialize(&recipient.identity_key).map_err(|_| Error::Authentication)?;
        let shared = zeroize::Zeroizing::new(
            ephemeral
                .calculate_agreement(&peer)
                .map_err(|_| Error::Authentication)?,
        );
        let mut value = Self {
            sender: identity.principal(machine)?,
            recipient,
            ephemeral_key: ephemeral
                .public_key()
                .map_err(|_| Error::Authentication)?
                .serialize()
                .to_vec(),
            nonce: crate::signaling::random_id()?[..12].try_into().unwrap(),
            ciphertext: bytes.to_vec(),
            signature: Vec::new(),
        };
        let context = value.context()?;
        key(&shared, &context)?
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(value.nonce),
                aead::Aad::from(context),
                &mut value.ciphertext,
            )
            .map_err(|_| Error::Authentication)?;
        value.signature = sign(identity, &value.signed_bytes()?)?;
        Ok(value)
    }
    pub fn open(
        &self,
        identity: &SignalIdentity,
        machine: [u8; 16],
        sender: &Principal,
    ) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        if &self.sender != sender
            || self.recipient != identity.principal(machine)?
            || self.ciphertext.len() > 12 * 1024 + 16
        {
            return Err(Error::Authentication);
        }
        verify(sender, &self.signed_bytes()?, &self.signature)?;
        let ephemeral =
            PublicKey::deserialize(&self.ephemeral_key).map_err(|_| Error::Authentication)?;
        let shared = zeroize::Zeroizing::new(
            identity
                .private
                .calculate_agreement(&ephemeral)
                .map_err(|_| Error::Authentication)?,
        );
        let context = self.context()?;
        let mut bytes = zeroize::Zeroizing::new(self.ciphertext.clone());
        let length = key(&shared, &context)?
            .open_in_place(
                aead::Nonce::assume_unique_for_key(self.nonce),
                aead::Aad::from(context),
                &mut bytes,
            )
            .map_err(|_| Error::Authentication)?
            .len();
        bytes.truncate(length);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_registration_binds_github_repository_machine_and_identity() {
        let identity = SignalIdentity::from_private_key(42, &[1; 32]).unwrap();
        let mut registration = Registration::create(&identity, [1; 16], 420).unwrap();
        registration.verify(42, 420).unwrap();
        assert!(registration.verify(43, 420).is_err());
        assert!(registration.verify(42, 421).is_err());
        registration.principal.machine_id = [2; 16];
        assert!(registration.verify(42, 420).is_err());
    }
    #[test]
    fn encrypted_mailbox_rejects_tampering_and_other_machines_even_on_same_account() {
        let alice = SignalIdentity::from_private_key(42, &[1; 32]).unwrap();
        let bob = SignalIdentity::from_private_key(42, &[2; 32]).unwrap();
        let sender = alice.principal([1; 16]).unwrap();
        let sealed = Sealed::seal(
            &alice,
            [1; 16],
            bob.principal([2; 16]).unwrap(),
            b"private machine name and permissions",
        )
        .unwrap();
        let mut sealed: Sealed =
            serde_json::from_slice(&serde_json::to_vec(&sealed).unwrap()).unwrap();
        assert_eq!(
            sealed.open(&bob, [2; 16], &sender).unwrap().as_slice(),
            b"private machine name and permissions"
        );
        assert!(sealed.open(&bob, [3; 16], &sender).is_err());
        sealed.ciphertext[0] ^= 1;
        assert!(sealed.open(&bob, [2; 16], &sender).is_err());
    }
}

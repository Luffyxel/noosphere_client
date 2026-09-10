use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    time::{SystemTime, UNIX_EPOCH},
};

use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
    error::{Error, Result},
    github::GitHubClient,
    instance_profile::InstanceProfile,
    models::{GitHubViewer, Message, NoosphereUser},
    secure_blob::SecureBlobStore,
    secure_store::{SecretStore, SystemSecretStore},
    validation,
};

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredSession {
    version: u8,
    access_token: String,
    refresh_token: Option<String>,
    token_expires_at: Option<u64>,
    refresh_expires_at: Option<u64>,
    viewer: String,
}

pub struct Session {
    pub access_token: SecretString,
    pub refresh_token: Option<SecretString>,
    pub token_expires_at: Option<u64>,
    pub refresh_expires_at: Option<u64>,
    pub viewer: GitHubViewer,
}

pub struct IdentityState {
    pub secret: crate::protocol::DeviceSecret,
}

struct AccountLock {
    repository_id: u64,
    _file: File,
}

enum IdentitySource {
    Shared,
    Legacy(u8),
    New,
}

struct IdentityCandidate {
    secret: crate::protocol::DeviceSecret,
    profile: crate::protocol::PublicProfile,
    source: IdentitySource,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeerState {
    pub user: NoosphereUser,
    pub repository_id: u64,
    pub profile: crate::protocol::PublicProfile,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationState {
    pub id: String,
    pub created_at: String,
    pub peer: PeerState,
    pub handshake_envelope: crate::protocol::RatchetEnvelope,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutgoingRequestState {
    pub id: String,
    pub created_at: String,
    pub peer: PeerState,
    pub envelope: crate::protocol::RatchetEnvelope,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalMessageState {
    pub message: Message,
    pub envelope: crate::protocol::RatchetEnvelope,
    pub published: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalSocialState {
    version: u8,
    pub conversations: Vec<ConversationState>,
    pub outgoing: Vec<OutgoingRequestState>,
    pub messages: Vec<LocalMessageState>,
    #[serde(default)]
    pub seen_message_ids: Vec<String>,
    #[serde(default)]
    pub declined_request_ids: Vec<String>,
    pub temporary_stars: Vec<u64>,
}

#[derive(Clone)]
pub struct CachedDirectory {
    pub etag: String,
    pub names: Vec<String>,
}

impl Default for LocalSocialState {
    fn default() -> Self {
        Self {
            version: 1,
            conversations: Vec::new(),
            outgoing: Vec::new(),
            messages: Vec::new(),
            seen_message_ids: Vec::new(),
            declined_request_ids: Vec::new(),
            temporary_stars: Vec::new(),
        }
    }
}

pub struct AppState {
    pub github: GitHubClient,
    pub profile: InstanceProfile,
    pub secrets: SystemSecretStore,
    pub blobs: SecureBlobStore,
    pub session: RwLock<Option<Session>>,
    pub identity: RwLock<Option<IdentityState>>,
    pub social: RwLock<LocalSocialState>,
    pub setup: Mutex<()>,
    pub operations: Mutex<()>,
    account_lock: Mutex<Option<AccountLock>>,
    pub wake_stargazers: Mutex<Option<BTreeMap<u64, String>>>,
    etags: Mutex<BTreeMap<String, String>>,
    directories: Mutex<BTreeMap<String, CachedDirectory>>,
    refresh: Mutex<()>,
}

impl AppState {
    pub fn new(profile: InstanceProfile) -> Result<Self> {
        let blobs = SecureBlobStore::new(profile.base_directory())?;
        Ok(Self {
            github: GitHubClient::new()?,
            profile,
            secrets: SystemSecretStore,
            blobs,
            session: RwLock::new(None),
            identity: RwLock::new(None),
            social: RwLock::new(LocalSocialState::default()),
            setup: Mutex::new(()),
            operations: Mutex::new(()),
            account_lock: Mutex::new(None),
            wake_stargazers: Mutex::new(None),
            etags: Mutex::new(BTreeMap::new()),
            directories: Mutex::new(BTreeMap::new()),
            refresh: Mutex::new(()),
        })
    }

    pub async fn restore_session(&self) -> Result<Option<GitHubViewer>> {
        let account = self.profile.secret_account("github-session")?;
        let Some(secret) = self.secrets.load(&account)? else {
            return Ok(None);
        };
        let stored: StoredSession = serde_json::from_str(secret.expose_secret())
            .map_err(|_| Error::SecureStorageUnavailable)?;
        if stored.version != 1
            || !valid_stored_token(&stored.access_token)
            || stored
                .refresh_token
                .as_deref()
                .is_some_and(|token| !valid_stored_token(token))
        {
            return Err(Error::SecureStorageUnavailable);
        }
        let viewer: GitHubViewer =
            serde_json::from_str(&stored.viewer).map_err(|_| Error::SecureStorageUnavailable)?;
        if viewer.id == 0
            || viewer.repository.id == 0
            || validation::github_login(&viewer.login).is_err()
            || validation::repository_name_exact(&viewer.repository.name).is_err()
        {
            return Err(Error::SecureStorageUnavailable);
        }
        let returned = viewer.clone();
        *self.session.write().await = Some(Session {
            access_token: SecretString::from(stored.access_token.clone()),
            refresh_token: stored.refresh_token.clone().map(SecretString::from),
            token_expires_at: stored.token_expires_at,
            refresh_expires_at: stored.refresh_expires_at,
            viewer,
        });
        Ok(Some(returned))
    }

    pub async fn save_session(&self) -> Result<()> {
        let session = self.session.read().await;
        let session = session.as_ref().ok_or(Error::SessionExpired)?;
        let account = self.profile.secret_account("github-session")?;
        let stored = StoredSession {
            version: 1,
            access_token: session.access_token.expose_secret().to_owned(),
            refresh_token: session
                .refresh_token
                .as_ref()
                .map(|value| value.expose_secret().to_owned()),
            token_expires_at: session.token_expires_at,
            refresh_expires_at: session.refresh_expires_at,
            viewer: serde_json::to_string(&session.viewer).map_err(|_| Error::Local)?,
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| Error::Local)?;
        self.secrets.save(&account, SecretString::from(serialized))
    }

    pub async fn replace_session(&self, session: Session) -> Result<GitHubViewer> {
        let viewer = session.viewer.clone();
        *self.session.write().await = Some(session);
        self.save_session().await?;
        Ok(viewer)
    }

    pub async fn update_viewer(&self, viewer: GitHubViewer) -> Result<GitHubViewer> {
        {
            let mut session = self.session.write().await;
            session.as_mut().ok_or(Error::SessionExpired)?.viewer = viewer.clone();
        }
        self.save_session().await?;
        Ok(viewer)
    }

    pub async fn install_session(
        &self,
        token: crate::github::OAuthToken,
        viewer: GitHubViewer,
    ) -> Result<GitHubViewer> {
        let now = unix_time()?;
        self.replace_session(Session {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            token_expires_at: token
                .expires_in
                .map(|duration| now.saturating_add(duration.as_secs())),
            refresh_expires_at: token
                .refresh_expires_in
                .map(|duration| now.saturating_add(duration.as_secs())),
            viewer,
        })
        .await
    }

    pub async fn access_token(&self) -> Result<SecretString> {
        if let Some(token) = self.unexpired_access_token().await? {
            return Ok(token);
        }
        let _refresh_guard = self.refresh.lock().await;
        if let Some(token) = self.unexpired_access_token().await? {
            return Ok(token);
        }
        let refresh_token = {
            let session = self.session.read().await;
            let session = session.as_ref().ok_or(Error::SessionExpired)?;
            let refresh_cutoff = unix_time()?.saturating_add(60);
            if session
                .refresh_expires_at
                .is_some_and(|expires| expires <= refresh_cutoff)
            {
                return Err(Error::SessionExpired);
            }
            session.refresh_token.clone().ok_or(Error::SessionExpired)?
        };
        let refreshed = self.github.refresh_access_token(&refresh_token).await?;
        let now = unix_time()?;
        let access_token = refreshed.access_token.clone();
        {
            let mut session = self.session.write().await;
            let session = session.as_mut().ok_or(Error::SessionExpired)?;
            session.access_token = refreshed.access_token;
            if refreshed.refresh_token.is_some() {
                session.refresh_token = refreshed.refresh_token;
            }
            session.token_expires_at = refreshed
                .expires_in
                .map(|duration| now.saturating_add(duration.as_secs()));
            if refreshed.refresh_expires_in.is_some() {
                session.refresh_expires_at = refreshed
                    .refresh_expires_in
                    .map(|duration| now.saturating_add(duration.as_secs()));
            }
        }
        self.save_session().await?;
        Ok(access_token)
    }

    async fn unexpired_access_token(&self) -> Result<Option<SecretString>> {
        let session = self.session.read().await;
        let session = session.as_ref().ok_or(Error::SessionExpired)?;
        let access_cutoff = unix_time()?.saturating_add(60);
        if session
            .token_expires_at
            .is_none_or(|expires| expires > access_cutoff)
        {
            Ok(Some(session.access_token.clone()))
        } else {
            Ok(None)
        }
    }

    pub async fn load_or_create_identity(
        &self,
        github_user_id: u64,
        repository_id: u64,
        expected_profile: Option<&crate::protocol::PublicProfile>,
    ) -> Result<crate::protocol::PublicProfile> {
        self.ensure_account_lock(repository_id).await?;
        let identity_kind = format!("identity-{repository_id}");
        let shared_account = self.profile.shared_account(&identity_kind)?;
        let mut candidates = Vec::new();
        if let Some(serialized) = self.blobs.load(&shared_account)? {
            let (secret, profile) = crate::protocol::restore_identity(
                serialized.expose_secret(),
                github_user_id,
                repository_id,
            )?;
            candidates.push(IdentityCandidate {
                secret,
                profile,
                source: IdentitySource::Shared,
            });
        }
        for slot in std::iter::once(self.profile.slot()).chain(
            (0_u8..crate::instance_profile::MAX_INSTANCE_PROFILES)
                .filter(|candidate| *candidate != self.profile.slot()),
        ) {
            let directory = self.profile.directory_for_slot(slot)?;
            if !directory.exists() {
                continue;
            }
            let store = SecureBlobStore::new(&directory)?;
            let account = InstanceProfile::secret_account_for_slot(slot, &identity_kind)?;
            let Some(serialized) = store.load(&account)? else {
                continue;
            };
            let Ok((secret, profile)) = crate::protocol::restore_identity(
                serialized.expose_secret(),
                github_user_id,
                repository_id,
            ) else {
                continue;
            };
            if candidates
                .iter()
                .any(|candidate| crate::protocol::same_public_profile(&candidate.profile, &profile))
            {
                continue;
            }
            candidates.push(IdentityCandidate {
                secret,
                profile,
                source: IdentitySource::Legacy(slot),
            });
        }
        let selected = expected_profile
            .and_then(|expected| {
                candidates.iter().position(|candidate| {
                    crate::protocol::same_public_profile(&candidate.profile, expected)
                })
            })
            .or_else(|| {
                candidates
                    .iter()
                    .position(|candidate| matches!(candidate.source, IdentitySource::Shared))
            })
            .unwrap_or(0);
        let identity = if candidates.is_empty() {
            let (secret, profile) =
                crate::protocol::create_identity(github_user_id, repository_id)?;
            IdentityCandidate {
                secret,
                profile,
                source: IdentitySource::New,
            }
        } else {
            candidates.swap_remove(selected)
        };
        self.blobs.save(
            &shared_account,
            crate::protocol::serialize_identity(&identity.secret)?,
        )?;
        let profile = identity.profile.clone();
        let social =
            self.load_social_for_identity(github_user_id, repository_id, &identity.source)?;
        *self.identity.write().await = Some(IdentityState {
            secret: identity.secret,
        });
        *self.social.write().await = social;
        self.persist_social(repository_id).await?;
        Ok(profile)
    }

    async fn ensure_account_lock(&self, repository_id: u64) -> Result<()> {
        let mut account_lock = self.account_lock.lock().await;
        if account_lock
            .as_ref()
            .is_some_and(|lock| lock.repository_id == repository_id)
        {
            return Ok(());
        }
        *account_lock = None;
        let file = self.profile.acquire_account_lock(repository_id)?;
        *account_lock = Some(AccountLock {
            repository_id,
            _file: file,
        });
        Ok(())
    }

    pub async fn persist_identity(&self, repository_id: u64) -> Result<()> {
        let account = self
            .profile
            .shared_account(&format!("identity-{repository_id}"))?;
        let identity = self.identity.read().await;
        let identity = identity.as_ref().ok_or(Error::Crypto)?;
        self.blobs.save(
            &account,
            crate::protocol::serialize_identity(&identity.secret)?,
        )
    }

    pub async fn persist_social(&self, repository_id: u64) -> Result<()> {
        let account = self
            .profile
            .shared_account(&format!("social-{repository_id}"))?;
        let social = self.social.read().await;
        validate_social(&social, None)?;
        let serialized = serde_json::to_string(&*social).map_err(|_| Error::Local)?;
        if serialized.len() > 24 * 1024 * 1024 {
            return Err(Error::Local);
        }
        self.blobs.save(&account, SecretString::from(serialized))
    }

    pub async fn record_message(&self, record: LocalMessageState) -> Result<()> {
        validation::message_id(&record.message.id)?;
        validation::conversation_id(&record.message.conversation_id)?;
        validation::message_text(&record.message.text)?;
        crate::protocol::validate_public_timestamp(&record.message.sent_at)?;
        crate::protocol::validate_ratchet_envelope_shape(&record.envelope)?;
        if record.message.version != 1 || record.message.sender_id == 0 {
            return Err(Error::InvalidData);
        }
        let mut social = self.social.write().await;
        if social
            .seen_message_ids
            .iter()
            .any(|message_id| message_id == &record.message.id)
        {
            return Ok(());
        }
        social.seen_message_ids.push(record.message.id.clone());
        social.messages.push(record);
        social.messages.sort_by(|left, right| {
            left.message
                .sent_at
                .cmp(&right.message.sent_at)
                .then_with(|| left.message.id.cmp(&right.message.id))
        });
        if social.messages.len() > 500 {
            let remove = social.messages.len() - 500;
            social.messages.drain(..remove);
        }
        if social.seen_message_ids.len() > 10_000 {
            let remove = social.seen_message_ids.len() - 10_000;
            social.seen_message_ids.drain(..remove);
        }
        Ok(())
    }

    fn load_social_for_identity(
        &self,
        github_user_id: u64,
        repository_id: u64,
        source: &IdentitySource,
    ) -> Result<LocalSocialState> {
        let kind = format!("social-{repository_id}");
        let serialized = match source {
            IdentitySource::Legacy(slot) => {
                let directory = self.profile.directory_for_slot(*slot)?;
                let store = SecureBlobStore::new(&directory)?;
                let account = InstanceProfile::secret_account_for_slot(*slot, &kind)?;
                store.load(&account)?
            }
            IdentitySource::Shared => self.blobs.load(&self.profile.shared_account(&kind)?)?,
            IdentitySource::New => None,
        };
        let Some(serialized) = serialized else {
            return Ok(LocalSocialState::default());
        };
        deserialize_social(serialized.expose_secret(), github_user_id)
    }

    pub async fn clear_session(&self) -> Result<()> {
        *self.session.write().await = None;
        *self.identity.write().await = None;
        *self.social.write().await = LocalSocialState::default();
        *self.account_lock.lock().await = None;
        *self.wake_stargazers.lock().await = None;
        self.etags.lock().await.clear();
        self.directories.lock().await.clear();
        let account = self.profile.secret_account("github-session")?;
        self.secrets.remove(&account)
    }

    pub async fn etag(&self, resource: &str) -> Option<String> {
        self.etags.lock().await.get(resource).cloned()
    }

    pub async fn remember_etag(&self, resource: String, etag: Option<String>) {
        let Some(etag) = etag else {
            return;
        };
        let mut cache = self.etags.lock().await;
        if cache.len() >= 512
            && !cache.contains_key(&resource)
            && let Some(oldest) = cache.keys().next().cloned()
        {
            cache.remove(&oldest);
        }
        cache.insert(resource, etag);
    }

    pub async fn cached_directory(&self, resource: &str) -> Option<CachedDirectory> {
        self.directories.lock().await.get(resource).cloned()
    }

    pub async fn remember_directory(
        &self,
        resource: String,
        etag: Option<String>,
        names: Vec<String>,
    ) {
        let Some(etag) = etag else {
            return;
        };
        let mut cache = self.directories.lock().await;
        if cache.len() >= 512
            && !cache.contains_key(&resource)
            && let Some(oldest) = cache.keys().next().cloned()
        {
            cache.remove(&oldest);
        }
        cache.insert(resource, CachedDirectory { etag, names });
    }

    pub async fn forget_directory(&self, resource: &str) {
        self.directories.lock().await.remove(resource);
    }
}

fn unix_time() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| Error::Local)
}

fn valid_stored_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4_096 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn deserialize_social(serialized: &str, github_user_id: u64) -> Result<LocalSocialState> {
    let mut social: LocalSocialState =
        serde_json::from_str(serialized).map_err(|_| Error::SecureStorageUnavailable)?;
    if social.seen_message_ids.is_empty() && !social.messages.is_empty() {
        social.seen_message_ids = social
            .messages
            .iter()
            .map(|record| record.message.id.clone())
            .collect();
    }
    validate_social(&social, Some(github_user_id))?;
    Ok(social)
}

fn validate_social(social: &LocalSocialState, own_id: Option<u64>) -> Result<()> {
    if social.version != 1
        || social.conversations.len() > 500
        || social.outgoing.len() > 500
        || social.messages.len() > 500
        || social.seen_message_ids.len() > 10_000
        || social.declined_request_ids.len() > 2_000
        || social.temporary_stars.len() > 500
    {
        return Err(Error::SecureStorageUnavailable);
    }
    let mut conversations = BTreeSet::new();
    let mut peers = BTreeSet::new();
    for conversation in &social.conversations {
        validate_conversation_record(
            &conversation.id,
            &conversation.created_at,
            &conversation.peer,
            own_id,
        )?;
        if !conversations.insert(conversation.id.as_str())
            || !peers.insert(conversation.peer.user.id)
        {
            return Err(Error::SecureStorageUnavailable);
        }
    }
    let mut requests = BTreeSet::new();
    for request in &social.outgoing {
        validate_conversation_record(&request.id, &request.created_at, &request.peer, own_id)?;
        if !requests.insert(request.id.as_str()) {
            return Err(Error::SecureStorageUnavailable);
        }
    }
    let mut messages = BTreeSet::new();
    for record in &social.messages {
        let message = &record.message;
        validation::message_id(&message.id).map_err(|_| Error::SecureStorageUnavailable)?;
        validation::conversation_id(&message.conversation_id)
            .map_err(|_| Error::SecureStorageUnavailable)?;
        validation::message_text(&message.text).map_err(|_| Error::SecureStorageUnavailable)?;
        crate::protocol::validate_public_timestamp(&message.sent_at)
            .map_err(|_| Error::SecureStorageUnavailable)?;
        if message.version != 1
            || message.sender_id == 0
            || !conversations.contains(message.conversation_id.as_str())
            || !messages.insert(message.id.as_str())
            || crate::protocol::validate_ratchet_envelope_shape(&record.envelope).is_err()
        {
            return Err(Error::SecureStorageUnavailable);
        }
    }
    let mut seen = BTreeSet::new();
    for message_id in &social.seen_message_ids {
        validation::message_id(message_id).map_err(|_| Error::SecureStorageUnavailable)?;
        if !seen.insert(message_id.as_str()) {
            return Err(Error::SecureStorageUnavailable);
        }
    }
    if messages.iter().any(|message_id| !seen.contains(message_id)) {
        return Err(Error::SecureStorageUnavailable);
    }
    let mut declined = BTreeSet::new();
    for request_id in &social.declined_request_ids {
        validation::conversation_id(request_id).map_err(|_| Error::SecureStorageUnavailable)?;
        if !declined.insert(request_id.as_str()) {
            return Err(Error::SecureStorageUnavailable);
        }
    }
    let mut stars = BTreeSet::new();
    if social
        .temporary_stars
        .iter()
        .any(|value| *value == 0 || !stars.insert(*value))
    {
        return Err(Error::SecureStorageUnavailable);
    }
    Ok(())
}

fn validate_conversation_record(
    id: &str,
    created_at: &str,
    peer: &PeerState,
    own_id: Option<u64>,
) -> Result<()> {
    validation::conversation_id(id).map_err(|_| Error::SecureStorageUnavailable)?;
    crate::protocol::validate_public_timestamp(created_at)
        .map_err(|_| Error::SecureStorageUnavailable)?;
    validation::github_login(&peer.user.login).map_err(|_| Error::SecureStorageUnavailable)?;
    validation::repository_name_exact(&peer.user.repository)
        .map_err(|_| Error::SecureStorageUnavailable)?;
    if peer.user.id == 0
        || peer.repository_id == 0
        || own_id.is_some_and(|value| value == peer.user.id)
        || crate::protocol::validate_public_profile(&peer.profile, peer.user.id, peer.repository_id)
            .is_err()
    {
        return Err(Error::SecureStorageUnavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn legacy_identity_follows_the_account_when_profile_order_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let first = InstanceProfile::acquire(temporary.path()).unwrap();
        let second = InstanceProfile::acquire(temporary.path()).unwrap();
        assert_eq!(first.slot(), 0);
        assert_eq!(second.slot(), 1);

        let legacy_store =
            SecureBlobStore::new(&second.directory_for_slot(second.slot()).unwrap()).unwrap();
        let (secret, published_profile) = crate::protocol::create_identity(42, 420).unwrap();
        legacy_store
            .save(
                &InstanceProfile::secret_account_for_slot(1, "identity-420").unwrap(),
                crate::protocol::serialize_identity(&secret).unwrap(),
            )
            .unwrap();
        let mut legacy_social = LocalSocialState::default();
        legacy_social
            .declined_request_ids
            .push("dm-0123456789abcdef0123456789abcdef".to_owned());
        legacy_store
            .save(
                &InstanceProfile::secret_account_for_slot(1, "social-420").unwrap(),
                SecretString::from(serde_json::to_string(&legacy_social).unwrap()),
            )
            .unwrap();

        let state = AppState::new(first).unwrap();
        let migrated = state
            .load_or_create_identity(42, 420, Some(&published_profile))
            .await
            .unwrap();
        assert!(crate::protocol::same_public_profile(
            &migrated,
            &published_profile
        ));
        assert_eq!(
            state.social.read().await.declined_request_ids,
            legacy_social.declined_request_ids
        );
        drop(state);
        drop(second);

        let replacement = InstanceProfile::acquire(temporary.path()).unwrap();
        let restored = AppState::new(replacement).unwrap();
        let profile = restored
            .load_or_create_identity(42, 420, Some(&published_profile))
            .await
            .unwrap();
        assert!(crate::protocol::same_public_profile(
            &profile,
            &published_profile
        ));
        assert_eq!(
            restored.social.read().await.declined_request_ids,
            legacy_social.declined_request_ids
        );
    }

    #[tokio::test]
    async fn message_cache_is_bounded_without_forgetting_deduplication_ids() {
        let temporary = tempfile::tempdir().unwrap();
        let profile = InstanceProfile::acquire(temporary.path()).unwrap();
        let state = AppState::new(profile).unwrap();
        let envelope: crate::protocol::RatchetEnvelope =
            serde_json::from_value(serde_json::json!({
                "version": 3,
                "kind": 2,
                "data": "AQ==",
                "signature": "AQ=="
            }))
            .unwrap();
        for index in 0..501_u16 {
            state
                .record_message(LocalMessageState {
                    message: Message {
                        version: 1,
                        id: format!("msg-{index:032x}"),
                        conversation_id: "dm-0123456789abcdef0123456789abcdef".to_owned(),
                        sent_at: "2026-09-04T09:00:00Z".to_owned(),
                        text: "message".to_owned(),
                        sender_id: 42,
                        own: true,
                    },
                    envelope: envelope.clone(),
                    published: true,
                })
                .await
                .unwrap();
        }
        let social = state.social.read().await;
        assert_eq!(social.messages.len(), 500);
        assert_eq!(social.seen_message_ids.len(), 501);
        assert_eq!(
            social.messages[0].message.id,
            "msg-00000000000000000000000000000001"
        );
    }
}

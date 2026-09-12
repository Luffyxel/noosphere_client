//! GitHub control plane, independent of the selected conversation and media UI.
use crate::{commands, github::OAuthToken, models::GitHubViewer, state::AppState};
use noosphere_remote::{
    Error as RemoteError,
    access::{AccessBook, AccessDecision, AccessRequest, Decision},
    directory::{Registration, Sealed},
    identity::SignalIdentity,
    live::{GuestHello, HostOffer},
    permissions::{Grant, Permissions, Principal},
};
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Mutex;

type Result<T> = std::result::Result<T, String>;
fn error(value: impl std::fmt::Display) -> String {
    match value.to_string().as_str() {
        "remote desktop permission denied" => {
            "Cette demande a expiré ou n’est plus autorisée.".into()
        }
        "remote desktop authentication failed" => {
            "L’identité de cette machine n’a pas pu être vérifiée.".into()
        }
        "remote desktop resource limit reached" => {
            "La limite de machines ou de demandes a été atteinte.".into()
        }
        text => text.to_owned(),
    }
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn log_event(data: &mut StoredDirectory, level: &str, scope: &str, message: impl Into<String>) {
    let message = message.into();
    let at = now().saturating_mul(1000);
    if data.debug_logs.last().is_some_and(|entry| {
        entry.message == message && entry.scope == scope && at.saturating_sub(entry.at) < 30_000
    }) {
        return;
    }
    if data.debug_logs.len() >= 200 {
        data.debug_logs.remove(0);
    }
    data.debug_logs.push(DirectoryLog {
        at,
        level: level.into(),
        scope: scope.into(),
        message,
    });
}
fn machine_key(id: [u8; 16]) -> String {
    uuid::Uuid::from_bytes(id).simple().to_string()
}
fn same_device(left: &Principal, right: &Principal) -> bool {
    left.github_user_id == right.github_user_id && left.machine_id == right.machine_id
}
fn mailbox_uses_replaced_keys(sealed: &Sealed, sender: &Principal, recipient: &Principal) -> bool {
    (&sealed.sender != sender || &sealed.recipient != recipient)
        && same_device(&sealed.sender, sender)
        && same_device(&sealed.recipient, recipient)
}

fn answer_is_actionable(
    answer: &AccessDecision,
    peer: &Principal,
    owner: &Principal,
    outgoing: &[AccessRequest],
    time: u64,
) -> Result<bool> {
    if &answer.request.host != peer
        || &answer.request.guest != owner
        || answer.permissions.intersect(answer.request.permissions) != answer.permissions
    {
        return Err("Réponse sans demande correspondante.".into());
    }
    Ok(answer.request.expires_at > time
        && outgoing.iter().any(|request| request == &answer.request))
}

pub struct RemoteDirectory {
    operation: Mutex<()>,
    runtime_id: [u8; 16],
}

impl Default for RemoteDirectory {
    fn default() -> Self {
        Self {
            operation: Mutex::new(()),
            runtime_id: *uuid::Uuid::new_v4().as_bytes(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Machine {
    pub principal: Principal,
    pub login: String,
    pub avatar_url: String,
    pub repository: String,
    pub repository_id: u64,
    pub name: String,
    pub platform: String,
    pub identity_verified: bool,
    pub host_enabled: bool,
    pub permissions: Permissions,
    pub unattended: bool,
    pub last_seen: u64,
    pub received_sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserGrantPolicy {
    pub github_user_id: u64,
    pub permissions: Permissions,
    pub unattended: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryLog {
    pub at: u64,
    pub level: String,
    pub scope: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredDirectory {
    #[serde(default)]
    runtime_id: Option<[u8; 16]>,
    #[serde(default)]
    scanned_accounts: Vec<u64>,
    #[serde(default)]
    last_scan: u64,
    #[serde(default)]
    last_publish: u64,
    #[serde(default)]
    published_revision: u64,
    #[serde(default)]
    published_machine_count: usize,
    registered: bool,
    #[serde(default)]
    registered_principal: Option<Principal>,
    sequence: u64,
    book: AccessBook,
    #[serde(default)]
    user_grants: Vec<UserGrantPolicy>,
    machines: Vec<Machine>,
    outgoing: Vec<AccessRequest>,
    answers: Vec<AccessDecision>,
    #[serde(default)]
    session_hellos: Vec<GuestHello>,
    #[serde(default)]
    session_offers: Vec<HostOffer>,
    #[serde(default)]
    handled_sessions: Vec<[u8; 32]>,
    #[serde(default)]
    debug_logs: Vec<DirectoryLog>,
    #[serde(default)]
    firewall_executable: Option<String>,
    #[serde(default)]
    firewall_attempted_executable: Option<String>,
    #[serde(default)]
    firewall_last_check: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryView {
    owner: Principal,
    registered: bool,
    host_enabled: bool,
    machines: Vec<Machine>,
    grants: Vec<Grant>,
    user_grants: Vec<UserGrantPolicy>,
    incoming: Vec<AccessDecision>,
    outgoing: Vec<AccessRequest>,
    answers: Vec<AccessDecision>,
    streaming_ready: bool,
    connecting: bool,
    sync_errors: Vec<String>,
    logs: Vec<DirectoryLog>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mailbox {
    version: u8,
    sequence: u64,
    created_at: u64,
    expires_at: u64,
    name: String,
    platform: String,
    host_enabled: bool,
    permissions: Permissions,
    unattended: bool,
    request: Option<AccessRequest>,
    answer: Option<AccessDecision>,
    #[serde(default)]
    session_hello: Option<GuestHello>,
    #[serde(default)]
    session_offer: Option<HostOffer>,
}

impl Mailbox {
    fn validate(&self, previous: u64, time: u64) -> Result<()> {
        if self.version != 1
            || self.sequence <= previous
            || self.created_at > time.saturating_add(30)
            || self.expires_at <= time
            || self.expires_at <= self.created_at
            || self.expires_at - self.created_at > 120
            || self.name.trim().is_empty()
            || self.name.chars().count() > 64
            || self.name.chars().any(char::is_control)
            || !matches!(self.platform.as_str(), "windows" | "linux" | "macos")
        {
            return Err("Annonce de machine invalide ou expirée.".into());
        }
        Ok(())
    }
}

fn account(state: &AppState, repository: u64) -> Result<String> {
    state
        .profile
        .shared_account(&format!("remote-directory-{repository}"))
        .map_err(error)
}
fn load(state: &AppState, repository: u64) -> Result<StoredDirectory> {
    let value = state
        .blobs
        .load(&account(state, repository)?)
        .map_err(error)?;
    let mut stored = match value {
        Some(value) => {
            serde_json::from_str::<StoredDirectory>(value.expose_secret()).map_err(error)?
        }
        None => StoredDirectory::default(),
    };
    if stored.machines.len() > 100
        || stored.book.grants.len() > 100
        || stored.user_grants.len() > 100
        || stored.outgoing.len() > 100
        || stored.book.requests.len() > 100
        || stored.answers.len() > 100
        || stored.session_hellos.len() > 16
        || stored.debug_logs.len() > 200
        || stored.session_offers.len() > 16
        || stored.handled_sessions.len() > 100
    {
        return Err("Annuaire trop volumineux.".into());
    }
    let time = now();
    stored.outgoing.retain(|r| r.expires_at > time);
    stored.answers.retain(|r| r.request.expires_at > time);
    stored.book.requests.retain(|r| r.request.expires_at > time);
    stored
        .session_hellos
        .retain(|hello| hello.expires_at > time);
    stored
        .session_offers
        .retain(|offer| offer.expires_at > time);
    if stored.handled_sessions.len() > 100 {
        let drain = stored.handled_sessions.len() - 100;
        stored.handled_sessions.drain(..drain);
    }
    Ok(stored)
}

fn load_for_runtime(
    state: &AppState,
    repository: u64,
    directory: &RemoteDirectory,
) -> Result<StoredDirectory> {
    let mut data = load(state, repository)?;
    enter_runtime(&mut data, directory.runtime_id);
    Ok(data)
}

fn enter_runtime(data: &mut StoredDirectory, runtime_id: [u8; 16]) {
    if data.runtime_id == Some(runtime_id) {
        return;
    }
    data.debug_logs.clear();
    data.runtime_id = Some(runtime_id);
    data.firewall_attempted_executable = None;
    data.firewall_last_check = 0;
    retire_ephemeral(
        data,
        "Ancienne négociation locale abandonnée après le redémarrage.",
    );
    log_event(
        data,
        "info",
        "app",
        "Nouvelle exécution Noosphere ; journal précédent effacé.",
    );
}

fn retire_ephemeral(data: &mut StoredDirectory, message: &str) {
    if data.session_hellos.is_empty() && data.session_offers.is_empty() {
        return;
    }
    let ids: Vec<_> = data
        .session_hellos
        .iter()
        .map(|hello| hello.session_id)
        .chain(data.session_offers.iter().map(|offer| offer.session_id))
        .collect();
    for id in ids {
        if !data.handled_sessions.contains(&id) {
            data.handled_sessions.push(id);
        }
    }
    if data.handled_sessions.len() > 100 {
        let drain = data.handled_sessions.len() - 100;
        data.handled_sessions.drain(..drain);
    }
    data.session_hellos.clear();
    data.session_offers.clear();
    data.last_publish = 0;
    log_event(data, "info", "session", message);
}

fn reconcile_session(data: &mut StoredDirectory, state: &noosphere_remote::live::State) {
    if matches!(
        state,
        noosphere_remote::live::State::Idle | noosphere_remote::live::State::Failed { .. }
    ) {
        retire_ephemeral(
            data,
            "Négociation précédente terminée ; une nouvelle connexion peut démarrer.",
        );
    }
}

async fn ensure_firewall(data: &mut StoredDirectory, retry: bool) -> Option<String> {
    if cfg!(test) || std::env::var_os("NOOSPHERE_SMOKE_TEST").is_some() {
        return None;
    }
    let executable = crate::remote_access::firewall_executable()?;
    if data.firewall_executable.as_deref() == Some(&executable)
        && now().saturating_sub(data.firewall_last_check) < 300
    {
        return None;
    }
    let configured = crate::remote_access::firewall_is_configured(executable.clone()).await;
    data.firewall_last_check = now();
    if configured {
        data.firewall_executable = Some(executable);
        data.firewall_attempted_executable = None;
        return None;
    }
    data.firewall_executable = None;
    if !retry && data.firewall_attempted_executable.as_deref() == Some(&executable) {
        return None;
    }
    data.firewall_attempted_executable = Some(executable.clone());
    match crate::remote_access::configure_firewall(executable.clone()).await {
        Ok(()) if crate::remote_access::firewall_is_configured(executable.clone()).await => {
            data.firewall_executable = Some(executable);
            data.firewall_last_check = now();
            log_event(
                data,
                "success",
                "firewall",
                "Pare-feu Windows configuré pour le transport UDP de Noosphere.",
            );
            None
        }
        Ok(()) => {
            let failure =
                "Windows n’a pas conservé la règle UDP de Noosphere après l’autorisation."
                    .to_owned();
            log_event(data, "error", "firewall", &failure);
            Some(failure)
        }
        Err(failure) => {
            log_event(data, "error", "firewall", &failure);
            Some(failure)
        }
    }
}

fn save(state: &AppState, repository: u64, data: &StoredDirectory) -> Result<()> {
    let session = state.session.try_read().map_err(error)?;
    if !session
        .as_ref()
        .is_some_and(|s| s.viewer.repository.id == repository)
    {
        return Err("Le compte a changé pendant la synchronisation.".into());
    }
    state
        .blobs
        .save(
            &account(state, repository)?,
            SecretString::from(serde_json::to_string(data).map_err(error)?),
        )
        .map_err(error)
}
async fn context(state: &AppState) -> Result<(GitHubViewer, SignalIdentity, Principal)> {
    let viewer = commands::session_viewer(state).await.map_err(error)?;
    let machine = crate::remote_access::machine_id(state)?;
    let guard = state.identity.read().await;
    let identity = crate::protocol::remote_identity(
        &guard
            .as_ref()
            .ok_or("Identité Noosphere en cours de chargement.")?
            .secret,
    )
    .map_err(error)?;
    let owner = identity.principal(*machine.as_bytes()).map_err(error)?;
    if owner.github_user_id != viewer.id {
        return Err("Le compte a changé.".into());
    }
    Ok((viewer, identity, owner))
}
fn view(data: StoredDirectory, owner: Principal, sync_errors: Vec<String>) -> DirectoryView {
    DirectoryView {
        owner,
        registered: data.registered,
        host_enabled: data.book.enabled,
        machines: data.machines,
        grants: data.book.grants,
        user_grants: data.user_grants,
        incoming: data.book.requests,
        outgoing: data.outgoing,
        answers: data.answers,
        streaming_ready: cfg!(any(windows, target_os = "linux")),
        connecting: data
            .session_hellos
            .iter()
            .any(|hello| !data.handled_sessions.contains(&hello.session_id)),
        sync_errors,
        logs: data.debug_logs,
    }
}
fn registration_is_current(data: &StoredDirectory, owner: &Principal) -> bool {
    data.registered && data.registered_principal.as_ref() == Some(owner)
}
async fn write<T: Serialize>(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
    path: &str,
    value: &T,
) -> Result<()> {
    commands::write_repository_file(
        &state.github,
        token,
        &commands::viewer_identity(viewer),
        &commands::repository_identity(viewer),
        path,
        &serde_json::to_vec(value).map_err(error)?,
        "Synchroniser les machines Noosphere",
        true,
    )
    .await
    .map_err(error)?;
    Ok(())
}

async fn publish(
    state: &AppState,
    viewer: &GitHubViewer,
    identity: &SignalIdentity,
    owner: &Principal,
    data: &mut StoredDirectory,
    only: Option<&Principal>,
) -> Result<()> {
    if commands::session_viewer(state)
        .await
        .map_err(error)?
        .repository
        .id
        != viewer.repository.id
    {
        return Err("Le compte a changé pendant la synchronisation.".into());
    }
    let token = commands::oauth_token(state).await.map_err(error)?;
    if !registration_is_current(data, owner) {
        let registration = Registration::create(identity, owner.machine_id, viewer.repository.id)
            .map_err(error)?;
        write(
            state,
            &token,
            viewer,
            &format!("remote/devices/{}.json", machine_key(owner.machine_id)),
            &registration,
        )
        .await?;
        data.registered = true;
        data.registered_principal = Some(owner.clone());
        save(state, viewer.repository.id, data)?;
    }
    data.sequence = data
        .sequence
        .checked_add(1)
        .ok_or("Compteur de signalisation épuisé.")?;
    // Commit the counter before publishing, so a restart cannot reuse a revision.
    save(state, viewer.repository.id, data)?;
    let settings = crate::remote_access::load_settings(state, viewer.repository.id)?;
    let time = now();
    for peer in &data.machines {
        if only.is_some_and(|target| target != &peer.principal) {
            continue;
        }
        let grant = data.book.grant(&peer.principal, owner.machine_id);
        let permissions = if data.book.enabled {
            grant.map(|g| g.permissions).unwrap_or_default()
        } else {
            Permissions::default()
        };
        let mailbox = Mailbox {
            version: 1,
            sequence: data.sequence,
            created_at: time,
            expires_at: time + 120,
            name: settings.machine_name.clone(),
            platform: std::env::consts::OS.into(),
            host_enabled: data.book.enabled,
            permissions,
            unattended: data.book.enabled && grant.is_some_and(|g| g.unattended),
            request: data
                .outgoing
                .iter()
                .find(|r| r.host == peer.principal)
                .cloned(),
            answer: data
                .book
                .requests
                .iter()
                .rev()
                .find(|r| r.request.guest == peer.principal)
                .cloned(),
            session_hello: data
                .session_hellos
                .iter()
                .find(|hello| hello.host == peer.principal)
                .cloned(),
            session_offer: data
                .session_offers
                .iter()
                .find(|offer| offer.guest == peer.principal)
                .cloned(),
        };
        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&mailbox).map_err(error)?);
        let sealed = Sealed::seal(identity, owner.machine_id, peer.principal.clone(), &bytes)
            .map_err(error)?;
        write(
            state,
            &token,
            viewer,
            &format!(
                "remote/mail/{}/{}.json",
                machine_key(owner.machine_id),
                machine_key(peer.principal.machine_id)
            ),
            &sealed,
        )
        .await?;
    }
    if only.is_none() {
        data.last_publish = now();
        data.published_revision = data.book.revision;
        data.published_machine_count = data.machines.len();
        save(state, viewer.repository.id, data)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn remote_directory(
    state: State<'_, AppState>,
    directory: State<'_, RemoteDirectory>,
) -> Result<DirectoryView> {
    let _operation = directory.operation.lock().await;
    let (viewer, _, owner) = context(&state).await?;
    let data = load_for_runtime(&state, viewer.repository.id, directory.inner())?;
    save(&state, viewer.repository.id, &data)?;
    Ok(view(data, owner, Vec::new()))
}

#[tauri::command]
pub async fn remote_sync_directory(
    state: State<'_, AppState>,
    directory: State<'_, RemoteDirectory>,
    remote: State<'_, crate::remote_access::RemoteAccess>,
    refresh: bool,
) -> Result<DirectoryView> {
    let _operation = directory.operation.lock().await;
    let (viewer, identity, owner) = context(&state).await?;
    let mut data = load_for_runtime(&state, viewer.repository.id, directory.inner())?;
    if let Ok(status) = remote.status(&state).await {
        reconcile_session(&mut data, &status.session);
    }
    let token = commands::oauth_token(&state).await.map_err(error)?;
    let mut sources = vec![(
        viewer.id,
        viewer.login.clone(),
        viewer.avatar_url.clone(),
        viewer.repository.name.clone(),
        viewer.repository.id,
    )];
    for conversation in &state.social.read().await.conversations {
        let peer = &conversation.peer;
        if !sources.iter().any(|s| s.0 == peer.user.id) {
            sources.push((
                peer.user.id,
                peer.user.login.clone(),
                peer.user.avatar_url.clone(),
                peer.user.repository.clone(),
                peer.repository_id,
            ));
        }
    }
    // Removing a friend revokes its grants, pending requests and discovery state.
    let removed: Vec<_> = data
        .machines
        .iter()
        .filter(|m| !sources.iter().any(|s| s.0 == m.principal.github_user_id))
        .map(|m| m.principal.clone())
        .collect();
    for principal in removed {
        data.book.revoke(&principal);
    }
    data.machines
        .retain(|m| sources.iter().any(|s| s.0 == m.principal.github_user_id));
    data.user_grants
        .retain(|grant| sources.iter().any(|s| s.0 == grant.github_user_id));
    let mut failures = Vec::new();
    if data.book.enabled
        && let Some(failure) = ensure_firewall(&mut data, false).await
    {
        failures.push(failure);
    }
    let mut source_ids: Vec<_> = sources.iter().map(|source| source.0).collect();
    source_ids.sort_unstable();
    if refresh || source_ids != data.scanned_accounts || now().saturating_sub(data.last_scan) >= 60
    {
        for (github_id, login, avatar_url, repository, repository_id) in sources {
            let discovered = async {
                // Resolve immutable repository and owner IDs before trusting a login path.
                let response = state
                    .github
                    .api_json::<serde_json::Value>(
                        reqwest::Method::GET,
                        &format!("/repos/{login}/{repository}"),
                        Some(token.access_token.expose_secret()),
                        None,
                        None,
                        &[],
                    )
                    .await
                    .map_err(error)?;
                let repository_value = response.data.ok_or("Dépôt indisponible.")?;
                let verified =
                    crate::github::normalize_repository(&repository_value, github_id, &repository)
                        .map_err(error)?;
                if verified.id != repository_id {
                    return Err("L’identité du dépôt a changé.".into());
                }
                let names = commands::list_repository_directory(
                    &state,
                    &token,
                    &login,
                    &repository,
                    "remote/devices",
                    32,
                )
                .await
                .map_err(error)?;
                for filename in names {
                    let Some(id) = filename
                        .strip_suffix(".json")
                        .and_then(|s| uuid::Uuid::parse_str(s).ok())
                    else {
                        continue;
                    };
                    if id.as_bytes() == &owner.machine_id {
                        continue;
                    }
                    let value = commands::read_repository_json(
                        &state.github,
                        &token,
                        &login,
                        &repository,
                        &format!("remote/devices/{filename}"),
                        4096,
                    )
                    .await
                    .map_err(error)?;
                    let Some(value) = value else {
                        continue;
                    };
                    let registration: Registration =
                        serde_json::from_value(value).map_err(error)?;
                    registration
                        .verify(github_id, repository_id)
                        .map_err(error)?;
                    if registration.principal.machine_id != *id.as_bytes() {
                        return Err("Identifiant de machine incohérent.".into());
                    }
                    // The immutable repository owner is the GitHub identity link;
                    // the signed registration binds that account and repository to
                    // this device's own Noosphere key and machine ID.
                    let verified = true;
                    if let Some(existing) = data
                        .machines
                        .iter_mut()
                        .find(|m| m.principal == registration.principal)
                    {
                        existing.identity_verified = verified;
                        existing.login = login.clone();
                        existing.avatar_url = avatar_url.clone();
                    } else if data.machines.len() < 100 {
                        // A changed key never inherits a former machine's permissions.
                        let changed: Vec<_> = data
                            .machines
                            .iter()
                            .filter(|m| {
                                m.principal.machine_id == registration.principal.machine_id
                                    && m.principal.github_user_id == github_id
                            })
                            .map(|m| m.principal.clone())
                            .collect();
                        for old in changed {
                            data.book.revoke(&old);
                            data.machines.retain(|m| m.principal != old);
                        }
                        data.machines.push(Machine {
                            principal: registration.principal,
                            login: login.clone(),
                            avatar_url: avatar_url.clone(),
                            repository: repository.clone(),
                            repository_id,
                            name: format!("Ordinateur · {}", &id.simple().to_string()[..6]),
                            platform: "unknown".into(),
                            identity_verified: verified,
                            host_enabled: false,
                            permissions: Permissions::default(),
                            unattended: false,
                            last_seen: 0,
                            received_sequence: 0,
                        });
                        log_event(
                            &mut data,
                            "info",
                            "discovery",
                            format!(
                                "Machine {} détectée pour @{}.",
                                &machine_key(id.into_bytes())[..6],
                                login
                            ),
                        );
                    }
                }
                Ok::<_, String>(())
            }
            .await;
            if let Err(failure) = discovered {
                failures.push(format!("@{login} : {failure}"));
            }
        }
        if failures.is_empty() {
            data.last_scan = now();
            data.scanned_accounts = source_ids;
        }
    }
    apply_user_policies(&mut data, &owner)?;
    for index in 0..data.machines.len() {
        let peer = data.machines[index].clone();
        let received = async {
            let path = format!(
                "remote/mail/{}/{}.json",
                machine_key(peer.principal.machine_id),
                machine_key(owner.machine_id)
            );
            let value = commands::read_repository_json_conditional(
                &state,
                &token,
                &peer.login,
                &peer.repository,
                &path,
                64 * 1024,
            )
            .await
            .map_err(error)?;
            let Some(value) = value else {
                return Ok::<_, String>(());
            };
            let sealed: Sealed = serde_json::from_value(value).map_err(error)?;
            // A mailbox written before a device key was republished is obsolete.
            // Ignore it until the sender replaces it instead of reporting the
            // expected key migration as an authentication failure.
            if mailbox_uses_replaced_keys(&sealed, &peer.principal, &owner) {
                return Ok(());
            }
            let bytes = match sealed.open(&identity, owner.machine_id, &peer.principal) {
                Ok(bytes) => bytes,
                // A mailbox cannot grant anything before authentication. An old,
                // corrupted or forged value is therefore safe to quarantine while
                // waiting for the peer's next signed publication.
                Err(RemoteError::Authentication) => return Ok(()),
                Err(failure) => return Err(error(failure)),
            };
            let mailbox: Mailbox = serde_json::from_slice(&bytes).map_err(error)?;
            if mailbox.sequence <= peer.received_sequence || mailbox.expires_at <= now() {
                return Ok(());
            }
            mailbox.validate(peer.received_sequence, now())?;
            let machine = &mut data.machines[index];
            machine.name = mailbox.name;
            machine.platform = mailbox.platform;
            machine.last_seen = mailbox.created_at;
            machine.received_sequence = mailbox.sequence;
            machine.host_enabled = mailbox.host_enabled;
            machine.permissions = mailbox.permissions;
            machine.unattended = mailbox.unattended;
            if let Some(request) = mailbox.request {
                if request.guest != peer.principal || request.host != owner {
                    return Err("Demande destinée à une autre machine.".into());
                }
                if data.book.enabled && request.expires_at > now() {
                    let new_request = !data
                        .book
                        .requests
                        .iter()
                        .any(|current| current.request.id == request.id);
                    data.book.receive(request, &owner, now()).map_err(error)?;
                    if new_request {
                        log_event(
                            &mut data,
                            "info",
                            "signaling",
                            format!("Demande d’accès reçue de {}.", peer.name),
                        );
                    }
                }
            } else {
                data.book.cancel(&peer.principal);
            }
            if let Some(answer) = mailbox.answer {
                // GitHub keeps only the latest mailbox value. A refusal from an
                // earlier request may therefore still be present after the guest
                // starts a new unattended session. It is authenticated but no
                // longer actionable, so it must not poison the new negotiation.
                if answer_is_actionable(&answer, &peer.principal, &owner, &data.outgoing, now())? {
                    let new_answer = !data
                        .answers
                        .iter()
                        .any(|current| current.request.id == answer.request.id);
                    let decision = answer.decision;
                    let answer_permissions = answer.permissions;
                    data.answers.retain(|r| r.request.id != answer.request.id);
                    data.answers.push(answer);
                    if new_answer {
                        log_event(
                            &mut data,
                            if decision == Decision::Approved {
                                "success"
                            } else {
                                "info"
                            },
                            "signaling",
                            format!(
                                "Réponse reçue de {} : {}.",
                                peer.name,
                                if decision == Decision::Approved {
                                    "accès autorisé"
                                } else {
                                    "accès refusé"
                                }
                            ),
                        );
                        if decision == Decision::Approved
                            && !data
                                .session_hellos
                                .iter()
                                .any(|hello| hello.host == peer.principal)
                        {
                            let hello = crate::remote_access::prepare_guest(
                                &state,
                                &remote,
                                peer.principal.clone(),
                                answer_permissions,
                            )
                            .await?;
                            data.session_hellos.push(hello);
                            data.last_publish = 0;
                            log_event(
                                &mut data,
                                "info",
                                "signaling",
                                format!(
                                    "Accès accepté par {} ; connexion automatique lancée.",
                                    peer.name
                                ),
                            );
                        }
                    }
                }
            }
            if let Some(hello) = mailbox.session_hello {
                hello.validate(now()).map_err(error)?;
                if hello.guest != peer.principal || hello.host != owner {
                    return Err("Session destinée à une autre machine.".into());
                }
                let approved = data.book.requests.iter().find(|decision| {
                    decision.request.guest == hello.guest
                        && decision.request.host == owner
                        && decision.request.expires_at > now()
                        && decision.decision == Decision::Approved
                });
                let allowed = approved.map(|decision| decision.permissions).or_else(|| {
                    data.book
                        .grant(&hello.guest, owner.machine_id)
                        .filter(|grant| grant.unattended)
                        .map(|grant| grant.permissions)
                });
                let permissions = allowed
                    .map(|allowed| allowed.intersect(hello.permissions))
                    .filter(|permissions| permissions.screen)
                    .ok_or("Cette session n’est pas autorisée.")?;
                if !data
                    .session_offers
                    .iter()
                    .any(|offer| offer.session_id == hello.session_id)
                    && !data.handled_sessions.contains(&hello.session_id)
                {
                    if let Some(failure) = ensure_firewall(&mut data, true).await {
                        return Err(failure);
                    }
                    let offer =
                        crate::remote_access::start_live_host(&state, &remote, hello, permissions)
                            .await?;
                    data.session_offers
                        .retain(|current| current.guest != offer.guest);
                    data.session_offers.push(offer);
                    log_event(
                        &mut data,
                        "success",
                        "signaling",
                        format!("Offre QUIC préparée pour {}.", peer.name),
                    );
                    data.last_publish = 0;
                }
            }
            if let Some(offer) = mailbox.session_offer {
                if data.handled_sessions.contains(&offer.session_id) {
                    return Ok(());
                }
                let hello = data
                    .session_hellos
                    .iter()
                    .find(|hello| hello.session_id == offer.session_id)
                    .cloned()
                    .ok_or("Offre de session inattendue.")?;
                offer.validate(&hello, now()).map_err(error)?;
                if offer.host != peer.principal || offer.guest != owner {
                    return Err("Offre destinée à une autre machine.".into());
                }
                crate::remote_access::connect_live_guest(&state, &remote, offer.clone()).await?;
                data.handled_sessions.push(offer.session_id);
                data.session_hellos
                    .retain(|current| current.session_id != offer.session_id);
                log_event(
                    &mut data,
                    "success",
                    "session",
                    format!("Offre reçue de {} ; viewer natif lancé.", peer.name),
                );
                data.last_publish = 0;
            }
            Ok(())
        }
        .await;
        if let Err(failure) = received {
            failures.push(format!("{} : {failure}", peer.name));
        }
    }
    save(&state, viewer.repository.id, &data)?;
    let needs_publish = !data.registered
        || refresh
        || now().saturating_sub(data.last_publish) >= 45
        || data.published_revision != data.book.revision
        || data.published_machine_count != data.machines.len();
    if needs_publish
        && let Err(failure) = publish(&state, &viewer, &identity, &owner, &mut data, None).await
    {
        failures.push(failure);
    }
    for failure in &failures {
        log_event(&mut data, "error", "sync", failure);
    }
    save(&state, viewer.repository.id, &data)?;
    Ok(view(data, owner, failures))
}

#[tauri::command]
pub async fn remote_set_host(
    state: State<'_, AppState>,
    directory: State<'_, RemoteDirectory>,
    remote: State<'_, crate::remote_access::RemoteAccess>,
    enabled: bool,
) -> Result<DirectoryView> {
    let _operation = directory.operation.lock().await;
    let (viewer, identity, owner) = context(&state).await?;
    let mut data = load_for_runtime(&state, viewer.repository.id, directory.inner())?;
    data.book.set_enabled(enabled);
    log_event(
        &mut data,
        "info",
        "host",
        if enabled {
            "Hébergement activé."
        } else {
            "Hébergement désactivé."
        },
    );
    let mut failures = Vec::new();
    if enabled && let Some(failure) = ensure_firewall(&mut data, true).await {
        failures.push(failure);
    }
    save(&state, viewer.repository.id, &data)?;
    if !enabled && let Err(failure) = crate::remote_access::stop_live_if_running(&remote).await {
        failures.push(failure);
    }
    if let Err(failure) = publish(&state, &viewer, &identity, &owner, &mut data, None).await {
        failures.push(failure);
    }
    Ok(view(data, owner, failures))
}

#[tauri::command]
pub async fn remote_set_grant(
    state: State<'_, AppState>,
    directory: State<'_, RemoteDirectory>,
    remote: State<'_, crate::remote_access::RemoteAccess>,
    principal: Principal,
    permissions: Permissions,
    unattended: bool,
    revoke: bool,
) -> Result<DirectoryView> {
    let _operation = directory.operation.lock().await;
    let (viewer, identity, owner) = context(&state).await?;
    let mut data = load_for_runtime(&state, viewer.repository.id, directory.inner())?;
    if !revoke
        && principal.github_user_id != owner.github_user_id
        && !state
            .social
            .read()
            .await
            .conversations
            .iter()
            .any(|conversation| conversation.peer.user.id == principal.github_user_id)
    {
        return Err("Cet utilisateur ne fait plus partie de tes amis.".into());
    }
    update_grant(
        &mut data,
        &owner,
        &principal,
        permissions,
        unattended,
        revoke,
    )?;
    log_event(
        &mut data,
        "info",
        "permissions",
        if revoke {
            format!(
                "Accès révoqué pour la machine {}.",
                &machine_key(principal.machine_id)[..6]
            )
        } else {
            format!(
                "Accès enregistré pour la machine {}{}.",
                &machine_key(principal.machine_id)[..6],
                if unattended { " sans confirmation" } else { "" }
            )
        },
    );
    let mut failures = Vec::new();
    if data.book.enabled
        && !revoke
        && permissions.screen
        && let Some(failure) = ensure_firewall(&mut data, true).await
    {
        failures.push(failure);
    }
    save(&state, viewer.repository.id, &data)?;
    if let Err(failure) = crate::remote_access::stop_live_if_running(&remote).await {
        failures.push(failure);
    }
    if let Err(failure) = publish(
        &state,
        &viewer,
        &identity,
        &owner,
        &mut data,
        Some(&principal),
    )
    .await
    {
        failures.push(failure);
    }
    Ok(view(data, owner, failures))
}

#[tauri::command]
pub async fn remote_set_user_grant(
    state: State<'_, AppState>,
    directory: State<'_, RemoteDirectory>,
    remote: State<'_, crate::remote_access::RemoteAccess>,
    github_user_id: u64,
    permissions: Permissions,
    unattended: bool,
    revoke: bool,
) -> Result<DirectoryView> {
    let _operation = directory.operation.lock().await;
    let (viewer, identity, owner) = context(&state).await?;
    if github_user_id != owner.github_user_id
        && !state
            .social
            .read()
            .await
            .conversations
            .iter()
            .any(|conversation| conversation.peer.user.id == github_user_id)
    {
        return Err("Cet utilisateur ne fait plus partie de tes amis.".into());
    }
    let mut data = load_for_runtime(&state, viewer.repository.id, directory.inner())?;
    update_user_policy(
        &mut data,
        &owner,
        github_user_id,
        permissions,
        unattended,
        revoke,
    )?;
    log_event(
        &mut data,
        "info",
        "permissions",
        if revoke {
            format!("Accès utilisateur {github_user_id} révoqué.")
        } else {
            format!(
                "Accès utilisateur {github_user_id} enregistré{}.",
                if unattended { " sans confirmation" } else { "" }
            )
        },
    );
    let mut failures = Vec::new();
    if data.book.enabled
        && let Some(failure) = ensure_firewall(&mut data, true).await
    {
        failures.push(failure);
    }
    save(&state, viewer.repository.id, &data)?;
    if let Err(failure) = crate::remote_access::stop_live_if_running(&remote).await {
        failures.push(failure);
    }
    if let Err(failure) = publish(&state, &viewer, &identity, &owner, &mut data, None).await {
        failures.push(failure);
    }
    Ok(view(data, owner, failures))
}

fn update_user_policy(
    data: &mut StoredDirectory,
    owner: &Principal,
    github_user_id: u64,
    permissions: Permissions,
    unattended: bool,
    revoke: bool,
) -> Result<()> {
    if github_user_id == 0 || (unattended && !permissions.screen) {
        return Err("Autorisation invalide.".into());
    }
    data.user_grants
        .retain(|grant| grant.github_user_id != github_user_id);
    let principals: Vec<_> = data
        .machines
        .iter()
        .filter(|machine| machine.principal.github_user_id == github_user_id)
        .map(|machine| machine.principal.clone())
        .collect();
    if revoke || !permissions.screen {
        for principal in principals {
            data.book.revoke(&principal);
        }
        return Ok(());
    }
    if data.user_grants.len() >= 100 {
        return Err("La limite d’autorisations a été atteinte.".into());
    }
    data.user_grants.push(UserGrantPolicy {
        github_user_id,
        permissions,
        unattended,
    });
    data.book.set_enabled(true);
    apply_user_policies(data, owner)
}

fn apply_user_policies(data: &mut StoredDirectory, owner: &Principal) -> Result<()> {
    let policies = data.user_grants.clone();
    for policy in policies {
        let principals: Vec<_> = data
            .machines
            .iter()
            .filter(|machine| {
                machine.principal.github_user_id == policy.github_user_id
                    && machine.identity_verified
            })
            .map(|machine| machine.principal.clone())
            .collect();
        for principal in principals {
            let current = data.book.grant(&principal, owner.machine_id);
            if current.is_some_and(|grant| {
                grant.permissions == policy.permissions && grant.unattended == policy.unattended
            }) {
                continue;
            }
            update_grant(
                data,
                owner,
                &principal,
                policy.permissions,
                policy.unattended,
                false,
            )?;
        }
    }
    Ok(())
}

fn update_grant(
    data: &mut StoredDirectory,
    owner: &Principal,
    principal: &Principal,
    permissions: Permissions,
    unattended: bool,
    revoke: bool,
) -> Result<()> {
    // A guest does not need to host a desktop or be online to receive permissions.
    if !data.machines.iter().any(|m| &m.principal == principal) {
        return Err("Machine inconnue : actualise la liste.".into());
    }
    if revoke {
        data.book.revoke(principal);
    } else {
        data.book
            .set_grant(
                Grant {
                    principal: principal.clone(),
                    host_machine_id: owner.machine_id,
                    permissions,
                    unattended,
                },
                owner,
            )
            .map_err(error)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn remote_request_access(
    state: State<'_, AppState>,
    directory: State<'_, RemoteDirectory>,
    principal: Principal,
    permissions: Permissions,
) -> Result<DirectoryView> {
    let _operation = directory.operation.lock().await;
    let (viewer, identity, owner) = context(&state).await?;
    let mut data = load_for_runtime(&state, viewer.repository.id, directory.inner())?;
    if !data.machines.iter().any(|m| {
        m.principal == principal && m.host_enabled && m.last_seen.saturating_add(120) > now()
    }) {
        return Err("Cette machine n’accepte pas de demandes actuellement.".into());
    }
    let request = AccessRequest {
        id: noosphere_remote::signaling::random_id().map_err(error)?,
        guest: owner.clone(),
        host: principal.clone(),
        permissions,
        created_at: now(),
        expires_at: now() + 120,
    };
    request.validate(now()).map_err(error)?;
    data.outgoing.retain(|r| r.host != principal);
    data.outgoing.push(request);
    log_event(
        &mut data,
        "info",
        "signaling",
        format!(
            "Demande d’accès créée pour la machine {}.",
            &machine_key(principal.machine_id)[..6]
        ),
    );
    save(&state, viewer.repository.id, &data)?;
    let publish_result = publish(
        &state,
        &viewer,
        &identity,
        &owner,
        &mut data,
        Some(&principal),
    )
    .await;
    let failures = publish_result.err().into_iter().collect::<Vec<_>>();
    if failures.is_empty() {
        log_event(&mut data, "success", "github", "Demande d’accès publiée.");
    } else {
        for failure in &failures {
            log_event(&mut data, "error", "github", failure);
        }
    }
    save(&state, viewer.repository.id, &data)?;
    Ok(view(data, owner, failures))
}

#[tauri::command]
pub async fn remote_connect_machine(
    state: State<'_, AppState>,
    directory: State<'_, RemoteDirectory>,
    remote: State<'_, crate::remote_access::RemoteAccess>,
    principal: Principal,
) -> Result<DirectoryView> {
    let _operation = directory.operation.lock().await;
    let (viewer, identity, owner) = context(&state).await?;
    let mut data = load_for_runtime(&state, viewer.repository.id, directory.inner())?;
    if let Ok(status) = remote.status(&state).await {
        reconcile_session(&mut data, &status.session);
    }
    let machine = data
        .machines
        .iter()
        .find(|machine| machine.principal == principal)
        .ok_or("Machine inconnue : actualise la liste.")?;
    if !machine.identity_verified
        || !machine.host_enabled
        || machine.last_seen.saturating_add(120) <= now()
    {
        return Err("Cette machine n’est plus disponible.".into());
    }
    let approved = data
        .answers
        .iter()
        .rev()
        .find(|answer| {
            answer.request.guest == owner
                && answer.request.host == principal
                && answer.request.expires_at > now()
                && answer.decision == Decision::Approved
        })
        .map(|answer| answer.permissions);
    let permissions = approved
        .or_else(|| {
            (machine.unattended && machine.permissions.screen).then_some(machine.permissions)
        })
        .filter(|permissions| permissions.screen)
        .ok_or("L’accès à l’écran n’est pas autorisé.")?;
    if data.session_hellos.iter().any(|hello| {
        hello.host == principal
            && hello.expires_at > now()
            && !data.handled_sessions.contains(&hello.session_id)
    }) {
        log_event(
            &mut data,
            "info",
            "signaling",
            "Une connexion vers cette machine est déjà en préparation.",
        );
        save(&state, viewer.repository.id, &data)?;
        return Ok(view(data, owner, Vec::new()));
    }
    let hello =
        crate::remote_access::prepare_guest(&state, &remote, principal.clone(), permissions)
            .await?;
    data.session_hellos
        .retain(|current| current.host != principal);
    data.session_hellos.push(hello);
    log_event(
        &mut data,
        "info",
        "signaling",
        format!(
            "Connexion demandée à la machine {}.",
            &machine_key(principal.machine_id)[..6]
        ),
    );
    save(&state, viewer.repository.id, &data)?;
    let publish_result = publish(
        &state,
        &viewer,
        &identity,
        &owner,
        &mut data,
        Some(&principal),
    )
    .await;
    let failures = publish_result.err().into_iter().collect::<Vec<_>>();
    if failures.is_empty() {
        log_event(
            &mut data,
            "success",
            "github",
            "Signal de connexion publié.",
        );
    } else {
        for failure in &failures {
            log_event(&mut data, "error", "github", failure);
        }
    }
    save(&state, viewer.repository.id, &data)?;
    Ok(view(data, owner, failures))
}

#[tauri::command]
pub async fn remote_stop_session(
    state: State<'_, AppState>,
    remote: State<'_, crate::remote_access::RemoteAccess>,
) -> Result<()> {
    crate::remote_access::stop_live(&state, &remote).await
}

#[tauri::command]
pub async fn remote_decide_access(
    state: State<'_, AppState>,
    directory: State<'_, RemoteDirectory>,
    request_id: [u8; 32],
    permissions: Permissions,
    remember: bool,
    unattended: bool,
) -> Result<DirectoryView> {
    let _operation = directory.operation.lock().await;
    let (viewer, identity, owner) = context(&state).await?;
    let mut data = load_for_runtime(&state, viewer.repository.id, directory.inner())?;
    data.book
        .decide(request_id, permissions, remember, unattended, &owner, now())
        .map_err(error)?;
    let peer = data
        .book
        .requests
        .iter()
        .find(|r| r.request.id == request_id)
        .ok_or("Demande expirée.")?
        .request
        .guest
        .clone();
    log_event(
        &mut data,
        "info",
        "permissions",
        if permissions.screen {
            format!(
                "Demande autorisée pour la machine {}.",
                &machine_key(peer.machine_id)[..6]
            )
        } else {
            format!(
                "Demande refusée pour la machine {}.",
                &machine_key(peer.machine_id)[..6]
            )
        },
    );
    let mut failures = Vec::new();
    if permissions.screen
        && let Some(failure) = ensure_firewall(&mut data, true).await
    {
        failures.push(failure);
    }
    save(&state, viewer.repository.id, &data)?;
    if let Err(failure) = publish(&state, &viewer, &identity, &owner, &mut data, Some(&peer)).await
    {
        failures.push(failure);
    }
    Ok(view(data, owner, failures))
}

#[tauri::command]
pub async fn remote_cancel_access(
    state: State<'_, AppState>,
    directory: State<'_, RemoteDirectory>,
    remote: State<'_, crate::remote_access::RemoteAccess>,
    principal: Principal,
) -> Result<DirectoryView> {
    let _operation = directory.operation.lock().await;
    let (viewer, identity, owner) = context(&state).await?;
    let mut data = load_for_runtime(&state, viewer.repository.id, directory.inner())?;
    data.outgoing.retain(|r| r.host != principal);
    data.answers.retain(|r| r.request.host != principal);
    data.session_hellos.retain(|hello| hello.host != principal);
    data.session_offers.retain(|offer| offer.host != principal);
    log_event(
        &mut data,
        "info",
        "session",
        format!(
            "Demande et session fermées pour la machine {}.",
            &machine_key(principal.machine_id)[..6]
        ),
    );
    save(&state, viewer.repository.id, &data)?;
    let mut failures = Vec::new();
    if let Err(failure) = crate::remote_access::stop_live_if_running(&remote).await {
        failures.push(failure);
    }
    if let Err(failure) = publish(
        &state,
        &viewer,
        &identity,
        &owner,
        &mut data,
        Some(&principal),
    )
    .await
    {
        failures.push(failure);
    }
    Ok(view(data, owner, failures))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guest_permissions_persist_without_hosting_and_stay_bound_to_the_device() {
        let host = SignalIdentity::from_private_key(42, &[1; 32]).unwrap();
        let guest = SignalIdentity::from_private_key(84, &[2; 32]).unwrap();
        let owner = host.principal([1; 16]).unwrap();
        let principal = guest.principal([2; 16]).unwrap();
        let mut stored = StoredDirectory {
            machines: vec![Machine {
                principal: principal.clone(),
                login: "friend".into(),
                avatar_url: String::new(),
                repository: "noosphere".into(),
                repository_id: 840,
                name: "Ordinateur".into(),
                platform: "linux".into(),
                identity_verified: true,
                host_enabled: false,
                permissions: Permissions::default(),
                unattended: false,
                last_seen: 0,
                received_sequence: 0,
            }],
            ..Default::default()
        };
        let permissions = Permissions {
            screen: true,
            mouse: true,
            ..Default::default()
        };
        update_grant(&mut stored, &owner, &principal, permissions, true, false).unwrap();
        let mut restored: StoredDirectory =
            serde_json::from_slice(&serde_json::to_vec(&stored).unwrap()).unwrap();
        assert!(!restored.book.enabled);
        assert!(!restored.machines[0].host_enabled);
        let grant = restored.book.grant(&principal, owner.machine_id).unwrap();
        assert_eq!(grant.permissions, permissions);
        assert!(grant.unattended);
        assert!(!grant.permissions.keyboard);
        for changed in [
            guest.principal([3; 16]).unwrap(),
            SignalIdentity::from_private_key(84, &[3; 32])
                .unwrap()
                .principal([2; 16])
                .unwrap(),
        ] {
            assert!(
                update_grant(&mut restored, &owner, &changed, permissions, true, false).is_err()
            );
            assert!(restored.book.grant(&changed, owner.machine_id).is_none());
        }
        update_grant(&mut restored, &owner, &principal, permissions, false, true).unwrap();
        assert!(restored.book.grants.is_empty());
    }

    #[test]
    fn an_old_refusal_cannot_block_a_new_unattended_session() {
        let host = SignalIdentity::from_private_key(42, &[1; 32])
            .unwrap()
            .principal([1; 16])
            .unwrap();
        let guest = SignalIdentity::from_private_key(42, &[2; 32])
            .unwrap()
            .principal([2; 16])
            .unwrap();
        let request = AccessRequest {
            id: [7; 32],
            guest: guest.clone(),
            host: host.clone(),
            permissions: Permissions {
                screen: true,
                ..Default::default()
            },
            created_at: 100,
            expires_at: 220,
        };
        let refusal = AccessDecision {
            request: request.clone(),
            decision: Decision::Declined,
            permissions: Permissions::default(),
            automatic: false,
        };
        assert!(answer_is_actionable(&refusal, &host, &guest, &[request], 101).unwrap());
        assert!(!answer_is_actionable(&refusal, &host, &guest, &[], 101).unwrap());
    }

    #[test]
    fn user_policy_covers_known_and_later_discovered_devices() {
        let host = SignalIdentity::from_private_key(42, &[1; 32]).unwrap();
        let guest = SignalIdentity::from_private_key(84, &[2; 32]).unwrap();
        let owner = host.principal([1; 16]).unwrap();
        let machines = [[2; 16], [3; 16]].map(|machine_id| Machine {
            principal: guest.principal(machine_id).unwrap(),
            login: "friend".into(),
            avatar_url: String::new(),
            repository: "noosphere".into(),
            repository_id: 840,
            name: "Ordinateur".into(),
            platform: "windows".into(),
            identity_verified: true,
            host_enabled: false,
            permissions: Permissions::default(),
            unattended: false,
            last_seen: 0,
            received_sequence: 0,
        });
        let mut stored = StoredDirectory {
            machines: machines.to_vec(),
            ..Default::default()
        };
        let permissions = Permissions {
            screen: true,
            keyboard: true,
            ..Default::default()
        };
        update_user_policy(&mut stored, &owner, 84, permissions, true, false).unwrap();
        assert!(stored.book.enabled);
        assert_eq!(stored.user_grants.len(), 1);
        assert_eq!(stored.book.grants.len(), 2);
        assert!(stored.book.grants.iter().all(|grant| {
            grant.principal.github_user_id == 84
                && grant.permissions == permissions
                && grant.unattended
        }));
        let future = guest.principal([4; 16]).unwrap();
        stored.machines.push(Machine {
            principal: future.clone(),
            login: "friend".into(),
            avatar_url: String::new(),
            repository: "noosphere".into(),
            repository_id: 840,
            name: "Nouvel ordinateur".into(),
            platform: "linux".into(),
            identity_verified: true,
            host_enabled: false,
            permissions: Permissions::default(),
            unattended: false,
            last_seen: 0,
            received_sequence: 0,
        });
        apply_user_policies(&mut stored, &owner).unwrap();
        assert!(stored.book.grant(&future, owner.machine_id).is_some());
        update_user_policy(&mut stored, &owner, 84, Permissions::default(), false, true).unwrap();
        assert!(stored.book.grants.is_empty());
        assert!(stored.user_grants.is_empty());
    }

    #[test]
    fn user_policy_can_be_saved_before_a_machine_exists() {
        let host = SignalIdentity::from_private_key(42, &[1; 32]).unwrap();
        let owner = host.principal([1; 16]).unwrap();
        let mut stored = StoredDirectory::default();
        let permissions = Permissions {
            screen: true,
            mouse: true,
            ..Default::default()
        };
        update_user_policy(&mut stored, &owner, 84, permissions, false, false).unwrap();
        assert_eq!(stored.user_grants.len(), 1);
        assert!(stored.book.grants.is_empty());
        assert!(stored.book.enabled);
    }

    #[test]
    fn a_legacy_registration_is_republished_for_the_current_device_identity() {
        let old = SignalIdentity::from_private_key(42, &[1; 32]).unwrap();
        let current = SignalIdentity::from_private_key(42, &[2; 32]).unwrap();
        let old_principal = old.principal([1; 16]).unwrap();
        let current_principal = current.principal([1; 16]).unwrap();
        let mut stored = StoredDirectory {
            registered: true,
            ..Default::default()
        };
        assert!(!registration_is_current(&stored, &current_principal));
        stored.registered_principal = Some(old_principal);
        assert!(!registration_is_current(&stored, &current_principal));
        stored.registered_principal = Some(current_principal.clone());
        assert!(registration_is_current(&stored, &current_principal));
    }

    #[test]
    fn a_mailbox_for_an_old_key_of_the_same_device_is_ignored() {
        let old_sender = SignalIdentity::from_private_key(42, &[1; 32]).unwrap();
        let current_sender = SignalIdentity::from_private_key(42, &[2; 32]).unwrap();
        let recipient = SignalIdentity::from_private_key(84, &[3; 32]).unwrap();
        let recipient_principal = recipient.principal([8; 16]).unwrap();
        let sealed = Sealed::seal(
            &old_sender,
            [7; 16],
            recipient_principal.clone(),
            b"ancienne annonce",
        )
        .unwrap();
        assert!(mailbox_uses_replaced_keys(
            &sealed,
            &current_sender.principal([7; 16]).unwrap(),
            &recipient_principal,
        ));
        assert!(!mailbox_uses_replaced_keys(
            &sealed,
            &old_sender.principal([7; 16]).unwrap(),
            &recipient_principal,
        ));
    }

    #[test]
    fn mailbox_replay_and_expiry_survive_serialized_counters() {
        let value = Mailbox {
            version: 1,
            sequence: 5,
            created_at: 100,
            expires_at: 220,
            name: "Bureau".into(),
            platform: "windows".into(),
            host_enabled: true,
            permissions: Permissions::default(),
            unattended: false,
            request: None,
            answer: None,
            session_hello: None,
            session_offer: None,
        };
        value.validate(4, 101).unwrap();
        assert!(value.validate(5, 101).is_err());
        assert!(value.validate(4, 220).is_err());
        let stored = StoredDirectory {
            sequence: 5,
            ..Default::default()
        };
        let restored: StoredDirectory =
            serde_json::from_slice(&serde_json::to_vec(&stored).unwrap()).unwrap();
        assert_eq!(restored.sequence, 5);
    }

    #[test]
    fn a_restart_or_failed_worker_retires_ephemeral_negotiations() {
        let host = SignalIdentity::from_private_key(42, &[1; 32])
            .unwrap()
            .principal([1; 16])
            .unwrap();
        let guest = SignalIdentity::from_private_key(42, &[2; 32])
            .unwrap()
            .principal([2; 16])
            .unwrap();
        let session_id = [9; 32];
        let hello = GuestHello {
            version: 1,
            session_id,
            guest,
            host,
            guest_certificate_der: vec![1; 256],
            permissions: Permissions {
                screen: true,
                ..Default::default()
            },
            created_at: 1,
            expires_at: 121,
        };
        let mut stored = StoredDirectory {
            runtime_id: Some([3; 16]),
            last_publish: 50,
            session_hellos: vec![hello.clone()],
            ..Default::default()
        };
        enter_runtime(&mut stored, [3; 16]);
        assert_eq!(stored.session_hellos.len(), 1);
        enter_runtime(&mut stored, [4; 16]);
        assert!(stored.session_hellos.is_empty());
        assert!(stored.handled_sessions.contains(&session_id));
        assert_eq!(stored.last_publish, 0);

        let failed_session_id = [10; 32];
        stored.session_hellos.push(GuestHello {
            session_id: failed_session_id,
            ..hello
        });
        reconcile_session(
            &mut stored,
            &noosphere_remote::live::State::Failed {
                error: "test".into(),
            },
        );
        assert!(stored.session_hellos.is_empty());
        assert!(stored.handled_sessions.contains(&failed_session_id));
    }

    #[test]
    fn stored_directory_accepts_fields_from_a_newer_schema() {
        let mut value = serde_json::to_value(StoredDirectory::default()).unwrap();
        value["futureField"] = serde_json::json!({ "version": 2 });
        let restored: StoredDirectory = serde_json::from_value(value).unwrap();
        assert!(restored.machines.is_empty());
    }
}

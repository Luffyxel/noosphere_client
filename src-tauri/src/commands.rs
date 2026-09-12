use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::Method;
use secrecy::ExposeSecret as _;
use serde_json::Value;
use tauri::{Emitter as _, State};
use tauri_plugin_notification::NotificationExt as _;
use tauri_plugin_opener::OpenerExt as _;
use time::{Duration as TimeDuration, OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::time::{Instant, sleep};

use crate::{
    DesktopAppHandle,
    error::{Error, Result},
    github::{
        ApiResponse, GitHubClient, OAuthPoll, OAuthToken, RepositoryIdentity, ViewerIdentity,
    },
    models::{
        CallSignal, Conversation, FriendRemoved, FriendRequest, FriendRequestDeclined,
        GitHubViewer, LookupUser, Message, NoosphereUser, NotificationResult, ProvisioningConsent,
        Published, RealtimeSignal, RepositorySummary, SignedOut, SmokeConfig, SmokeResult,
        SocialState, UserLookup, WakeSignals,
    },
    repository_content,
    state::{AppState, ConversationState, LocalMessageState, OutgoingRequestState, PeerState},
    validation,
};

type CommandResult<T> = std::result::Result<T, String>;

const MAX_FRIENDS: usize = 100;
const MAX_REQUESTS_PER_USER: usize = 100;
const MAX_MESSAGES_PER_CONVERSATION: usize = 500;

fn command_error(error: Error) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn github_restore(state: State<'_, AppState>) -> CommandResult<Option<GitHubViewer>> {
    state.restore_session().await.map_err(command_error)
}

#[tauri::command]
pub async fn github_validate_session(
    state: State<'_, AppState>,
) -> CommandResult<Option<GitHubViewer>> {
    let saved = {
        let session = state.session.read().await;
        session.as_ref().map(|session| session.viewer.clone())
    };
    let saved = match saved {
        Some(viewer) => viewer,
        None => {
            let Some(viewer) = state.restore_session().await.map_err(command_error)? else {
                return Ok(None);
            };
            viewer
        }
    };
    match validate_restored_session(&state, saved).await {
        Ok(viewer) => Ok(Some(viewer)),
        Err(error @ (Error::Network | Error::GitHubDenied)) => Err(command_error(error)),
        Err(_) => {
            state.clear_session().await.map_err(command_error)?;
            Ok(None)
        }
    }
}

async fn validate_restored_session(state: &AppState, saved: GitHubViewer) -> Result<GitHubViewer> {
    let access_token = state.access_token().await?;
    let token = OAuthToken {
        access_token,
        refresh_token: None,
        expires_in: None,
        refresh_expires_in: None,
    };
    let remote_viewer = fetch_viewer(&state.github, &token).await?;
    if remote_viewer.id != saved.id || !remote_viewer.login.eq_ignore_ascii_case(&saved.login) {
        return Err(Error::InvalidGitHubResponse);
    }
    let endpoint = format!("/repos/{}/{}", remote_viewer.login, saved.repository.name);
    let repository = state
        .github
        .api_json::<Value>(
            Method::GET,
            &endpoint,
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[],
        )
        .await?
        .data
        .ok_or(Error::GitHubNotFound)?;
    let repository =
        crate::github::normalize_repository(&repository, remote_viewer.id, &saved.repository.name)?;
    if repository.id != saved.repository.id
        || !installation_is_valid(&state.github, &token, &remote_viewer, &repository).await?
    {
        return Err(Error::InvalidInstallation);
    }
    initialize_repository(state, &token, &remote_viewer, &repository).await?;
    state
        .update_viewer(GitHubViewer {
            id: remote_viewer.id,
            login: remote_viewer.login,
            avatar_url: remote_viewer.avatar_url,
            name: remote_viewer.name,
            repository: RepositorySummary {
                id: repository.id,
                name: repository.name,
                url: repository.url,
                created: false,
            },
        })
        .await
}

#[tauri::command]
pub async fn github_connect(
    app: DesktopAppHandle,
    state: State<'_, AppState>,
    consent: ProvisioningConsent,
) -> CommandResult<GitHubViewer> {
    if !consent.validate() {
        return Err(command_error(Error::InvalidData));
    }
    let _setup_guard = state
        .setup
        .try_lock()
        .map_err(|_| command_error(Error::SetupInProgress))?;
    let result = connect_github(&app, &state).await.map_err(command_error);
    let _ = app.emit("github-device-code", Value::Null);
    let _ = app.emit("github-setup-status", Value::Null);
    result
}

async fn connect_github(app: &DesktopAppHandle, state: &AppState) -> Result<GitHubViewer> {
    emit_setup_status(app, "authorization", None)?;
    let device = state.github.request_device_code().await?;
    app.emit(
        "github-device-code",
        serde_json::json!({
            "userCode": device.user_code,
            "verificationUrl": device.verification_uri,
        }),
    )
    .map_err(|_| Error::Local)?;
    let _ = open_github_url(app, &device.verification_uri).await;
    let token = wait_for_device_token(&state.github, device).await?;
    let viewer = fetch_viewer(&state.github, &token).await?;
    let selection = select_repository(&state.github, &token, &viewer).await?;
    let created = selection.1.is_none();
    emit_setup_status(app, "repository", Some(&selection.0))?;
    let repository = match selection.1 {
        Some(repository) => repository,
        None => {
            let url = crate::github::repository_creation_url(&viewer.login, &selection.0)?;
            open_github_url(app, &url).await?;
            wait_for_repository(&state.github, &token, &viewer, &selection.0).await?
        }
    };
    if !installation_is_valid(&state.github, &token, &viewer, &repository).await? {
        emit_setup_status(app, "installation", Some(&repository.name))?;
        let url = crate::github::installation_url(viewer.id, repository.id)?;
        open_github_url(app, &url).await?;
        wait_for_installation(&state.github, &token, &viewer, &repository).await?;
    }
    emit_setup_status(app, "initialization", Some(&repository.name))?;
    initialize_repository(state, &token, &viewer, &repository).await?;
    let viewer = GitHubViewer {
        id: viewer.id,
        login: viewer.login,
        avatar_url: viewer.avatar_url,
        name: viewer.name,
        repository: RepositorySummary {
            id: repository.id,
            name: repository.name,
            url: repository.url,
            created,
        },
    };
    state.install_session(token, viewer).await
}

#[tauri::command]
pub async fn github_open_device_page(app: DesktopAppHandle) -> CommandResult<()> {
    let url = url::Url::parse("https://github.com/login/device")
        .map_err(|_| command_error(Error::Local))?;
    open_github_url(&app, &url).await.map_err(command_error)
}

async fn open_github_url(app: &DesktopAppHandle, url: &url::Url) -> Result<()> {
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::InvalidData);
    }
    #[cfg(target_os = "linux")]
    if linux_open_url(url).await {
        return Ok(());
    }
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|_| Error::Local)
}

#[cfg(target_os = "linux")]
async fn linux_open_url(url: &url::Url) -> bool {
    use ashpd::desktop::open_uri::OpenFileRequest;
    use std::process::{Command, Stdio};

    if let Ok(Ok(request)) = tokio::time::timeout(Duration::from_secs(5), async {
        let uri = ashpd::Uri::parse(url.as_str())?;
        OpenFileRequest::default().send_uri(&uri).await
    })
    .await
        && request.response().is_ok()
    {
        return true;
    }

    let candidates: [(&str, &[&str]); 12] = [
        (
            "systemd-run",
            &[
                "--user",
                "--quiet",
                "--collect",
                "--wait",
                "--service-type=exec",
                "xdg-open",
            ],
        ),
        (
            "/run/current-system/sw/bin/systemd-run",
            &[
                "--user",
                "--quiet",
                "--collect",
                "--wait",
                "--service-type=exec",
                "xdg-open",
            ],
        ),
        ("xdg-open", &[]),
        ("/run/current-system/sw/bin/xdg-open", &[]),
        ("gio", &["open"]),
        ("/run/current-system/sw/bin/gio", &["open"]),
        ("sensible-browser", &[]),
        ("firefox", &["--new-tab"]),
        ("chromium", &["--new-tab"]),
        ("chromium-browser", &["--new-tab"]),
        ("google-chrome", &["--new-tab"]),
        ("brave", &["--new-tab"]),
    ];
    for (program, arguments) in candidates {
        let mut command = Command::new(program);
        command
            .args(arguments)
            .arg(url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        sanitize_linux_external_command(&mut command);
        let Ok(mut child) = command.spawn() else {
            continue;
        };
        for _ in 0..50 {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return true,
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => sleep(Duration::from_millis(100)).await,
            }
        }
        if child.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = std::thread::spawn(move || {
                let _ = child.wait();
            });
            return true;
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn sanitize_linux_external_command(command: &mut std::process::Command) {
    let Some(app_dir) = std::env::var_os("APPDIR").map(PathBuf::from) else {
        return;
    };
    sanitize_linux_external_command_for_appimage(command, &app_dir);
}

#[cfg(target_os = "linux")]
fn sanitize_linux_external_command_for_appimage(
    command: &mut std::process::Command,
    app_dir: &std::path::Path,
) {
    for name in [
        "APPDIR",
        "APPIMAGE",
        "ARGV0",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "GDK_PIXBUF_MODULE_FILE",
        "GIO_EXTRA_MODULES",
        "GI_TYPELIB_PATH",
        "GSETTINGS_SCHEMA_DIR",
        "GST_PLUGIN_PATH",
        "GST_PLUGIN_PATH_1_0",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "GST_PLUGIN_SCANNER",
        "GTK_PATH",
        "PIPEWIRE_CONFIG_DIR",
        "PIPEWIRE_MODULE_DIR",
        "SPA_PLUGIN_DIR",
        "XKB_CONFIG_ROOT",
    ] {
        command.env_remove(name);
    }

    for name in ["PATH", "XDG_DATA_DIRS"] {
        let Some(value) = std::env::var_os(name) else {
            continue;
        };
        let filtered = filter_appimage_paths(&value, app_dir);
        if filtered.is_empty() {
            command.env_remove(name);
        } else {
            command.env(name, filtered);
        }
    }
}

#[cfg(target_os = "linux")]
fn filter_appimage_paths(value: &std::ffi::OsStr, app_dir: &std::path::Path) -> std::ffi::OsString {
    let paths = std::env::split_paths(value).filter(|path| !path.starts_with(app_dir));
    std::env::join_paths(paths).unwrap_or_default()
}

#[cfg(all(test, target_os = "linux"))]
mod linux_browser_tests {
    use super::*;

    #[test]
    fn appimage_paths_are_removed_from_external_launcher_paths() {
        let app_dir = std::path::Path::new("/tmp/.mount_noosphere");
        let paths = std::env::join_paths([
            app_dir.join("usr/bin"),
            PathBuf::from("/run/current-system/sw/bin"),
            app_dir.join("usr/share"),
            PathBuf::from("/etc/profiles/per-user/test/bin"),
        ])
        .unwrap();

        let filtered = filter_appimage_paths(&paths, app_dir);
        let paths = std::env::split_paths(&filtered).collect::<Vec<_>>();

        assert_eq!(
            paths,
            [
                PathBuf::from("/run/current-system/sw/bin"),
                PathBuf::from("/etc/profiles/per-user/test/bin"),
            ]
        );
    }

    #[test]
    fn appimage_library_overrides_are_removed_from_external_launchers() {
        let mut command = std::process::Command::new("true");
        command.env("LD_LIBRARY_PATH", "/tmp/.mount_noosphere/usr/lib");
        command.env(
            "GST_PLUGIN_SCANNER",
            "/tmp/.mount_noosphere/usr/lib/scanner",
        );

        sanitize_linux_external_command_for_appimage(
            &mut command,
            std::path::Path::new("/tmp/.mount_noosphere"),
        );

        let removed = command
            .get_envs()
            .filter(|(name, _)| {
                *name == std::ffi::OsStr::new("LD_LIBRARY_PATH")
                    || *name == std::ffi::OsStr::new("GST_PLUGIN_SCANNER")
            })
            .all(|(_, value)| value.is_none());
        assert!(removed);
    }
}

fn emit_setup_status(
    app: &DesktopAppHandle,
    stage: &str,
    repository_name: Option<&str>,
) -> Result<()> {
    let mut value = serde_json::json!({ "stage": stage });
    if let Some(name) = repository_name {
        value["repositoryName"] = Value::String(name.to_owned());
    }
    app.emit("github-setup-status", value)
        .map_err(|_| Error::Local)
}

async fn wait_for_device_token(
    client: &GitHubClient,
    device: crate::github::DeviceCode,
) -> Result<OAuthToken> {
    let deadline = Instant::now() + device.expires_in;
    let mut interval = device.interval;
    while Instant::now() < deadline {
        sleep(interval).await;
        match client.poll_device_token(&device.secret, None).await? {
            OAuthPoll::Granted(token) => return Ok(token),
            OAuthPoll::Pending => {}
            OAuthPoll::SlowDown => {
                interval = (interval + Duration::from_secs(5)).min(Duration::from_secs(60))
            }
            OAuthPoll::Denied => return Err(Error::AuthorizationDenied),
            OAuthPoll::Expired => return Err(Error::AuthorizationExpired),
        }
    }
    Err(Error::AuthorizationExpired)
}

async fn fetch_viewer(client: &GitHubClient, token: &OAuthToken) -> Result<ViewerIdentity> {
    let response = client
        .api_json::<Value>(
            Method::GET,
            "/user",
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[],
        )
        .await?;
    crate::github::normalize_viewer(response.data.as_ref().ok_or(Error::InvalidGitHubResponse)?)
}

async fn select_repository(
    client: &GitHubClient,
    token: &OAuthToken,
    viewer: &ViewerIdentity,
) -> Result<(String, Option<RepositoryIdentity>)> {
    let mut available = None;
    for attempt in 0..=16 {
        let name = validation::repository_name(&viewer.login, viewer.id, attempt)?;
        let endpoint = format!("/repos/{}/{name}", viewer.login);
        let response = client
            .api_json::<Value>(
                Method::GET,
                &endpoint,
                Some(token.access_token.expose_secret()),
                None,
                None,
                &[],
            )
            .await?;
        if response.status == 404 {
            available.get_or_insert(name);
            continue;
        }
        let repository = crate::github::normalize_repository(
            response.data.as_ref().ok_or(Error::InvalidGitHubResponse)?,
            viewer.id,
            &name,
        )?;
        if repository.description.as_deref() == Some(crate::github::REPOSITORY_DESCRIPTION) {
            return Ok((name, Some(repository)));
        }
    }
    available
        .map(|name| (name, None))
        .ok_or(Error::GitHubConflict)
}

async fn wait_for_repository(
    client: &GitHubClient,
    token: &OAuthToken,
    viewer: &ViewerIdentity,
    name: &str,
) -> Result<RepositoryIdentity> {
    let endpoint = format!("/repos/{}/{name}", viewer.login);
    for _ in 0..300 {
        let response = client
            .api_json::<Value>(
                Method::GET,
                &endpoint,
                Some(token.access_token.expose_secret()),
                None,
                None,
                &[],
            )
            .await?;
        if let Some(data) = response.data {
            let repository = crate::github::normalize_repository(&data, viewer.id, name)?;
            if repository.description.as_deref() != Some(crate::github::REPOSITORY_DESCRIPTION) {
                return Err(Error::GitHubConflict);
            }
            return Ok(repository);
        }
        sleep(Duration::from_secs(2)).await;
    }
    Err(Error::AuthorizationExpired)
}

async fn installation_is_valid(
    client: &GitHubClient,
    token: &OAuthToken,
    viewer: &ViewerIdentity,
    repository: &RepositoryIdentity,
) -> Result<bool> {
    let Some(installation) = find_installation(client, token, viewer.id).await? else {
        return Ok(false);
    };
    let installation_id = installation
        .get("id")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or(Error::InvalidInstallation)?;
    let endpoint = format!("/user/installations/{installation_id}/repositories?per_page=2");
    let repositories = client
        .api_json::<Value>(
            Method::GET,
            &endpoint,
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[],
        )
        .await?
        .data
        .ok_or(Error::InvalidGitHubResponse)?;
    crate::github::validate_installation(&installation, &repositories, viewer.id, repository.id)?;
    Ok(true)
}

async fn find_installation(
    client: &GitHubClient,
    token: &OAuthToken,
    account_id: u64,
) -> Result<Option<Value>> {
    let data = client
        .api_json::<Value>(
            Method::GET,
            "/user/installations?per_page=100",
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[],
        )
        .await?
        .data
        .ok_or(Error::InvalidGitHubResponse)?;
    let installations = data
        .get("installations")
        .and_then(Value::as_array)
        .filter(|entries| entries.len() <= 100)
        .ok_or(Error::InvalidGitHubResponse)?;
    Ok(installations
        .iter()
        .find(|installation| {
            installation.get("app_slug").and_then(Value::as_str) == Some(crate::github::APP_SLUG)
                && installation
                    .get("account")
                    .and_then(|account| account.get("id"))
                    .and_then(Value::as_u64)
                    == Some(account_id)
        })
        .cloned())
}

async fn wait_for_installation(
    client: &GitHubClient,
    token: &OAuthToken,
    viewer: &ViewerIdentity,
    repository: &RepositoryIdentity,
) -> Result<()> {
    for _ in 0..300 {
        match installation_is_valid(client, token, viewer, repository).await {
            Ok(true) => return Ok(()),
            Ok(false) => sleep(Duration::from_secs(3)).await,
            Err(Error::InvalidInstallation) => return Err(Error::InvalidInstallation),
            Err(error) => return Err(error),
        }
    }
    Err(Error::AuthorizationExpired)
}

async fn initialize_repository(
    state: &AppState,
    token: &OAuthToken,
    viewer: &ViewerIdentity,
    repository: &RepositoryIdentity,
) -> Result<()> {
    let published_profile = read_repository_json(
        &state.github,
        token,
        &viewer.login,
        &repository.name,
        ".noosphere.json",
        64 * 1024,
    )
    .await?
    .and_then(|value| serde_json::from_value(value).ok())
    .filter(|profile| {
        crate::protocol::validate_public_profile(profile, viewer.id, repository.id).is_ok()
    });
    let local_profile = state
        .load_or_create_identity(viewer.id, repository.id, published_profile.as_ref())
        .await?;
    // A second computer has its own protected keys. Never replace the primary
    // messaging identity merely because that computer has just signed in.
    let profile = published_profile.unwrap_or(local_profile);
    let profile = format!(
        "{}\n",
        serde_json::to_string_pretty(&profile).map_err(|_| Error::Local)?
    );
    write_repository_file(
        &state.github,
        token,
        viewer,
        repository,
        ".noosphere.json",
        profile.as_bytes(),
        "Initialiser Noosphere",
        true,
    )
    .await?;
    write_repository_file(
        &state.github,
        token,
        viewer,
        repository,
        "conv/.keep",
        b"Conversations Noosphere\n",
        "Créer la structure des conversations",
        false,
    )
    .await?;
    write_repository_file(
        &state.github,
        token,
        viewer,
        repository,
        "friends/outgoing/.keep",
        "Demandes d’amis Noosphere\n".as_bytes(),
        "Créer la structure des contacts",
        false,
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn write_repository_file(
    client: &GitHubClient,
    token: &OAuthToken,
    viewer: &ViewerIdentity,
    repository: &RepositoryIdentity,
    file_path: &str,
    content: &[u8],
    commit_message: &str,
    update_existing: bool,
) -> Result<bool> {
    if content.len() > 512 * 1024
        || file_path.is_empty()
        || file_path.len() > 240
        || file_path.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        return Err(Error::InvalidData);
    }
    let endpoint = format!(
        "/repos/{}/{}/contents/{file_path}",
        viewer.login, repository.name
    );
    for attempt in 0..4 {
        let existing: ApiResponse<Value> = client
            .api_json(
                Method::GET,
                &endpoint,
                Some(token.access_token.expose_secret()),
                None,
                None,
                &[],
            )
            .await?;
        let mut body = serde_json::json!({
            "message": commit_message,
            "content": STANDARD.encode(content),
        });
        if let Some(existing) = existing.data {
            let bytes = repository_content::decode_file_bytes(&existing, 512 * 1024, true)?;
            if bytes == content {
                return Ok(false);
            }
            if !update_existing {
                return Err(Error::GitHubConflict);
            }
            let sha = existing
                .get("sha")
                .and_then(Value::as_str)
                .filter(|value| {
                    matches!(value.len(), 40 | 64)
                        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .ok_or(Error::InvalidGitHubResponse)?;
            body["sha"] = Value::String(sha.to_owned());
        }
        let created: ApiResponse<Value> = client
            .api_json(
                Method::PUT,
                &endpoint,
                Some(token.access_token.expose_secret()),
                None,
                Some(&body),
                &[409, 422],
            )
            .await?;
        if !matches!(created.status, 409 | 422) {
            return Ok(true);
        }
        if attempt == 3 {
            let raced: ApiResponse<Value> = client
                .api_json(
                    Method::GET,
                    &endpoint,
                    Some(token.access_token.expose_secret()),
                    None,
                    None,
                    &[],
                )
                .await?;
            if raced.data.is_some_and(|value| {
                repository_content::decode_file_bytes(&value, 512 * 1024, true)
                    .is_ok_and(|bytes| bytes == content)
            }) {
                return Ok(false);
            }
            return Err(Error::GitHubConflict);
        }
        let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]);
        sleep(Duration::from_millis(150 * (1_u64 << attempt) + jitter)).await;
    }
    Err(Error::GitHubConflict)
}

#[tauri::command]
pub async fn github_sign_out(state: State<'_, AppState>) -> CommandResult<SignedOut> {
    state.clear_session().await.map_err(command_error)?;
    Ok(SignedOut { signed_out: true })
}

#[tauri::command]
pub async fn noosphere_lookup_user(
    state: State<'_, AppState>,
    login: String,
) -> CommandResult<UserLookup> {
    let login = validation::github_login(&login)
        .map_err(command_error)?
        .to_owned();
    let token = state.access_token().await.map_err(command_error)?;
    let token = OAuthToken {
        access_token: token,
        refresh_token: None,
        expires_in: None,
        refresh_expires_in: None,
    };
    let Some(user) = resolve_user(&state.github, &token, &login)
        .await
        .map_err(command_error)?
    else {
        return Ok(UserLookup::Missing { found: false });
    };
    Ok(UserLookup::Found {
        found: true,
        registered: user.repository.is_some(),
        user: LookupUser {
            id: user.identity.id,
            login: user.identity.login,
            name: user.identity.name,
            avatar_url: user.identity.avatar_url,
            repository: user.repository.map(|repository| repository.name),
        },
    })
}

struct ResolvedUser {
    identity: ViewerIdentity,
    repository: Option<RepositoryIdentity>,
    profile: Option<crate::protocol::PublicProfile>,
}

impl ResolvedUser {
    fn registered(self) -> Result<RegisteredUser> {
        Ok(RegisteredUser {
            identity: self.identity,
            repository: self.repository.ok_or(Error::GitHubNotFound)?,
            profile: self.profile.ok_or(Error::GitHubNotFound)?,
        })
    }
}

struct RegisteredUser {
    identity: ViewerIdentity,
    repository: RepositoryIdentity,
    profile: crate::protocol::PublicProfile,
}

fn peer_state(peer: RegisteredUser) -> PeerState {
    PeerState {
        user: NoosphereUser {
            id: peer.identity.id,
            login: peer.identity.login,
            name: peer.identity.name,
            avatar_url: peer.identity.avatar_url,
            repository: peer.repository.name,
        },
        repository_id: peer.repository.id,
        profile: peer.profile,
    }
}

async fn resolve_user(
    client: &GitHubClient,
    token: &OAuthToken,
    login: &str,
) -> Result<Option<ResolvedUser>> {
    let login = validation::github_login(login)?;
    let endpoint = format!("/users/{login}");
    let response = client
        .api_json::<Value>(
            Method::GET,
            &endpoint,
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[],
        )
        .await?;
    let Some(data) = response.data else {
        return Ok(None);
    };
    let identity = crate::github::normalize_viewer(&data)?;
    for attempt in 0..=16 {
        let repository_name = validation::repository_name(&identity.login, identity.id, attempt)?;
        let endpoint = format!("/repos/{}/{repository_name}", identity.login);
        let repository = client
            .api_json::<Value>(
                Method::GET,
                &endpoint,
                Some(token.access_token.expose_secret()),
                None,
                None,
                &[],
            )
            .await?;
        let Some(repository) = repository.data else {
            continue;
        };
        let repository =
            crate::github::normalize_repository(&repository, identity.id, &repository_name)?;
        let marker = read_repository_json(
            client,
            token,
            &identity.login,
            &repository.name,
            ".noosphere.json",
            64 * 1024,
        )
        .await;
        let Ok(Some(marker)) = marker else {
            continue;
        };
        let profile: crate::protocol::PublicProfile =
            serde_json::from_value(marker).map_err(|_| Error::InvalidGitHubResponse)?;
        if crate::protocol::validate_public_profile(&profile, identity.id, repository.id).is_ok() {
            return Ok(Some(ResolvedUser {
                identity,
                repository: Some(repository),
                profile: Some(profile),
            }));
        }
    }
    Ok(Some(ResolvedUser {
        identity,
        repository: None,
        profile: None,
    }))
}

pub(crate) async fn read_repository_json(
    client: &GitHubClient,
    token: &OAuthToken,
    owner: &str,
    repository: &str,
    file_path: &str,
    maximum_bytes: usize,
) -> Result<Option<Value>> {
    validation::github_login(owner)?;
    validation::repository_name_exact(repository)?;
    let endpoint = format!("/repos/{owner}/{repository}/contents/{file_path}");
    let response = client
        .api_json::<Value>(
            Method::GET,
            &endpoint,
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[],
        )
        .await?;
    let Some(data) = response.data else {
        return Ok(None);
    };
    Ok(Some(Value::Object(repository_content::decode_json_object(
        &data,
        maximum_bytes,
    )?)))
}

pub(crate) async fn read_repository_json_conditional(
    state: &AppState,
    token: &OAuthToken,
    owner: &str,
    repository: &str,
    file_path: &str,
    maximum_bytes: usize,
) -> Result<Option<Value>> {
    validation::github_login(owner)?;
    validation::repository_name_exact(repository)?;
    let endpoint = format!("/repos/{owner}/{repository}/contents/{file_path}");
    let etag = state.etag(&endpoint).await;
    let response = state
        .github
        .api_json::<Value>(
            Method::GET,
            &endpoint,
            Some(token.access_token.expose_secret()),
            etag.as_deref(),
            None,
            &[],
        )
        .await?;
    if response.status == 304 {
        return Ok(None);
    }
    state.remember_etag(endpoint, response.etag).await;
    let Some(data) = response.data else {
        return Ok(None);
    };
    Ok(Some(Value::Object(repository_content::decode_json_object(
        &data,
        maximum_bytes,
    )?)))
}

pub(crate) async fn list_repository_directory(
    state: &AppState,
    token: &OAuthToken,
    owner: &str,
    repository: &str,
    directory: &str,
    maximum_entries: usize,
) -> Result<Vec<String>> {
    validation::github_login(owner)?;
    validation::repository_name_exact(repository)?;
    if directory.is_empty()
        || directory.len() > 200
        || directory.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
        || maximum_entries == 0
        || maximum_entries > 1_000
    {
        return Err(Error::InvalidData);
    }
    let endpoint = format!("/repos/{owner}/{repository}/contents/{directory}");
    let cached = state.cached_directory(&endpoint).await;
    let response = state
        .github
        .api_json::<Value>(
            Method::GET,
            &endpoint,
            Some(token.access_token.expose_secret()),
            cached.as_ref().map(|value| value.etag.as_str()),
            None,
            &[],
        )
        .await?;
    if response.status == 304 {
        return cached
            .map(|value| value.names)
            .ok_or(Error::InvalidGitHubResponse);
    }
    let Some(value) = response.data else {
        state.forget_directory(&endpoint).await;
        return Ok(Vec::new());
    };
    let entries = value.as_array().ok_or(Error::InvalidGitHubResponse)?;
    if entries.len() > maximum_entries {
        return Err(Error::ResponseTooLarge);
    }
    let mut names = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(object) = entry.as_object() else {
            return Err(Error::InvalidGitHubResponse);
        };
        if object.get("type").and_then(Value::as_str) != Some("file") {
            continue;
        }
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 255
                    && !name.contains('/')
                    && !name.contains('\\')
                    && name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                    })
            })
            .ok_or(Error::InvalidGitHubResponse)?;
        names.push(name.to_owned());
    }
    names.sort_unstable();
    names.dedup();
    state
        .remember_directory(endpoint, response.etag, names.clone())
        .await;
    Ok(names)
}

#[tauri::command]
pub async fn noosphere_send_friend_request(
    state: State<'_, AppState>,
    login: String,
) -> CommandResult<FriendRequest> {
    let login = validation::github_login(&login)
        .map_err(command_error)?
        .to_owned();
    let _operation = state.operations.lock().await;
    send_friend_request(&state, &login, true)
        .await
        .map_err(command_error)
}

async fn send_friend_request(
    state: &AppState,
    login: &str,
    repeat_signal: bool,
) -> Result<FriendRequest> {
    let viewer = session_viewer(state).await?;
    let token = oauth_token(state).await?;
    let peer = resolve_user(&state.github, &token, login)
        .await?
        .ok_or(Error::GitHubNotFound)?
        .registered()?;
    if peer.identity.id == viewer.id {
        return Err(Error::InvalidData);
    }
    let existing = {
        let social = state.social.read().await;
        if social
            .conversations
            .iter()
            .any(|conversation| conversation.peer.user.id == peer.identity.id)
        {
            return Err(Error::GitHubConflict);
        }
        social
            .outgoing
            .iter()
            .find(|request| request.peer.user.id == peer.identity.id)
            .cloned()
    };
    let mut secret = state
        .identity
        .read()
        .await
        .as_ref()
        .ok_or(Error::Crypto)?
        .secret
        .clone();
    if let Some(existing) = existing.as_ref() {
        let own_profile = crate::protocol::profile_from_secret(&secret)?;
        if outgoing_invitation_is_current(
            existing,
            &own_profile,
            &peer.profile,
            viewer.id,
            viewer.repository.id,
        ) {
            publish_outgoing_request(state, &token, &viewer, existing).await?;
            ensure_temporary_star(state, &token, &viewer, &existing.peer, repeat_signal).await?;
            return Ok(friend_request(existing));
        }
        secret = crate::protocol::forget_peer_owned(secret, peer.identity.id)?;
    }

    let id = existing
        .as_ref()
        .map(|request| request.id.clone())
        .unwrap_or_else(|| format!("dm-{}", uuid::Uuid::new_v4().simple()));
    let created_at = crate::protocol::current_timestamp()?;
    let peer_state = PeerState {
        user: NoosphereUser {
            id: peer.identity.id,
            login: peer.identity.login,
            name: peer.identity.name,
            avatar_url: peer.identity.avatar_url,
            repository: peer.repository.name,
        },
        repository_id: peer.repository.id,
        profile: peer.profile,
    };
    let handshake = crate::protocol::OwnedHandshake {
        peer_profile: peer_state.profile.clone(),
        peer_github_user_id: peer_state.user.id,
        peer_repository_id: peer_state.repository_id,
        conversation_id: id.clone(),
        created_at: created_at.clone(),
    };
    let (secret, envelope) = tokio::task::spawn_blocking(move || {
        crate::protocol::create_invitation_owned(secret, handshake)
    })
    .await
    .map_err(|_| Error::Local)??;
    state
        .identity
        .write()
        .await
        .as_mut()
        .ok_or(Error::Crypto)?
        .secret = secret;
    state.persist_identity(viewer.repository.id).await?;
    let request = OutgoingRequestState {
        id,
        created_at,
        peer: peer_state,
        envelope,
    };
    {
        let mut social = state.social.write().await;
        social
            .outgoing
            .retain(|existing| existing.peer.user.id != request.peer.user.id);
        social.outgoing.push(request.clone());
    }
    state.persist_social(viewer.repository.id).await?;
    publish_outgoing_request(state, &token, &viewer, &request).await?;
    ensure_temporary_star(state, &token, &viewer, &request.peer, true).await?;
    Ok(friend_request(&request))
}

fn outgoing_invitation_is_current(
    request: &OutgoingRequestState,
    own_profile: &crate::protocol::PublicProfile,
    peer_profile: &crate::protocol::PublicProfile,
    viewer_id: u64,
    repository_id: u64,
) -> bool {
    crate::protocol::same_public_profile(&request.peer.profile, peer_profile)
        && crate::protocol::validate_invitation_envelope(
            own_profile,
            viewer_id,
            repository_id,
            &request.id,
            &request.envelope,
        )
        .is_ok()
}

fn friend_request(request: &OutgoingRequestState) -> FriendRequest {
    FriendRequest {
        version: 1,
        id: request.id.clone(),
        direction: "outgoing".to_owned(),
        user: request.peer.user.clone(),
    }
}

async fn publish_outgoing_request(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
    request: &OutgoingRequestState,
) -> Result<()> {
    let content = format!(
        "{}\n",
        serde_json::to_string(&request.envelope).map_err(|_| Error::Local)?
    );
    write_repository_file(
        &state.github,
        token,
        &viewer_identity(viewer),
        &repository_identity(viewer),
        &format!(
            "friends/outgoing/{}/{}.enc.json",
            request.peer.user.id, request.id
        ),
        content.as_bytes(),
        "Envoyer une demande d’ami chiffrée",
        true,
    )
    .await?;
    Ok(())
}

async fn ensure_temporary_star(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
    peer: &PeerState,
    refresh: bool,
) -> Result<()> {
    let endpoint = format!("/user/starred/{}/{}", peer.user.login, peer.user.repository);
    let starred: ApiResponse<Value> = state
        .github
        .api_json(
            Method::GET,
            &endpoint,
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[],
        )
        .await?;
    if starred.status != 404 && !refresh {
        return Ok(());
    }
    if starred.status != 404 {
        state
            .github
            .api_json::<Value>(
                Method::DELETE,
                &endpoint,
                Some(token.access_token.expose_secret()),
                None,
                None,
                &[404],
            )
            .await?;
        sleep(Duration::from_secs(1)).await;
    }
    state
        .github
        .api_json::<Value>(
            Method::PUT,
            &endpoint,
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[],
        )
        .await?;
    if starred.status == 404 {
        let mut social = state.social.write().await;
        if !social.temporary_stars.contains(&peer.repository_id) {
            social.temporary_stars.push(peer.repository_id);
        }
    }
    state.persist_social(viewer.repository.id).await
}

pub(crate) async fn session_viewer(state: &AppState) -> Result<GitHubViewer> {
    state
        .session
        .read()
        .await
        .as_ref()
        .map(|session| session.viewer.clone())
        .ok_or(Error::SessionExpired)
}

pub(crate) async fn oauth_token(state: &AppState) -> Result<OAuthToken> {
    Ok(OAuthToken {
        access_token: state.access_token().await?,
        refresh_token: None,
        expires_in: None,
        refresh_expires_in: None,
    })
}

pub(crate) fn viewer_identity(viewer: &GitHubViewer) -> ViewerIdentity {
    ViewerIdentity {
        id: viewer.id,
        login: viewer.login.clone(),
        avatar_url: viewer.avatar_url.clone(),
        name: viewer.name.clone(),
    }
}

pub(crate) fn repository_identity(viewer: &GitHubViewer) -> RepositoryIdentity {
    RepositoryIdentity {
        id: viewer.repository.id,
        name: viewer.repository.name.clone(),
        url: viewer.repository.url.clone(),
        description: Some(crate::github::REPOSITORY_DESCRIPTION.to_owned()),
    }
}

#[tauri::command]
pub async fn noosphere_cached_state(state: State<'_, AppState>) -> CommandResult<SocialState> {
    let social = state.social.read().await;
    let mut conversations = social
        .conversations
        .iter()
        .map(conversation_model)
        .collect::<Vec<_>>();
    let mut outgoing = social
        .outgoing
        .iter()
        .map(friend_request)
        .collect::<Vec<_>>();
    conversations.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    outgoing.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(SocialState {
        conversations,
        incoming: Vec::new(),
        outgoing,
    })
}

#[tauri::command]
pub async fn noosphere_sync_state(state: State<'_, AppState>) -> CommandResult<SocialState> {
    let _operation = state.operations.lock().await;
    sync_social_state(&state).await.map_err(command_error)
}

#[tauri::command]
pub async fn noosphere_sync_requests(state: State<'_, AppState>) -> CommandResult<SocialState> {
    let _operation = state.operations.lock().await;
    sync_friend_requests(&state).await.map_err(command_error)
}

#[tauri::command]
pub async fn noosphere_poll_wake_signals(state: State<'_, AppState>) -> CommandResult<WakeSignals> {
    poll_wake_signals(&state).await.map_err(command_error)
}

fn normalize_stargazer(value: &Value) -> Result<(ViewerIdentity, String)> {
    let object = value.as_object().ok_or(Error::InvalidGitHubResponse)?;
    let user = object
        .get("user")
        .ok_or(Error::InvalidGitHubResponse)
        .and_then(crate::github::normalize_viewer)?;
    let starred_at = match object.get("starred_at") {
        Some(Value::String(value)) => {
            crate::protocol::validate_public_timestamp(value)
                .map_err(|_| Error::InvalidGitHubResponse)?;
            value.clone()
        }
        Some(Value::Null) | None => String::new(),
        _ => return Err(Error::InvalidGitHubResponse),
    };
    Ok((user, starred_at))
}

async fn poll_wake_signals(state: &AppState) -> Result<WakeSignals> {
    let viewer = session_viewer(state).await?;
    let token = oauth_token(state).await?;
    let endpoint = format!(
        "/repos/{}/{}/stargazers?per_page={MAX_FRIENDS}",
        viewer.login, viewer.repository.name
    );
    let etag_key = format!("wake:{endpoint}");
    let etag = state.etag(&etag_key).await;
    let response: ApiResponse<Value> = state
        .github
        .api_stargazers(
            &endpoint,
            token.access_token.expose_secret(),
            etag.as_deref(),
            &[403],
        )
        .await?;
    if response.status == 304 || response.status == 403 {
        return Ok(WakeSignals::default());
    }
    state.remember_etag(etag_key, response.etag).await;
    let candidates = response
        .data
        .ok_or(Error::InvalidGitHubResponse)?
        .as_array()
        .cloned()
        .ok_or(Error::InvalidGitHubResponse)?;
    if candidates.len() > MAX_FRIENDS {
        return Err(Error::InvalidGitHubResponse);
    }
    let mut current = BTreeMap::new();
    for candidate in &candidates {
        if let Ok((candidate, starred_at)) = normalize_stargazer(candidate)
            && candidate.id != viewer.id
        {
            current.insert(candidate.id, starred_at);
        }
    }

    let previous = {
        let mut snapshot = state.wake_stargazers.lock().await;
        snapshot.replace(current.clone())
    };
    let social = state.social.read().await;
    let conversation_peers = social
        .conversations
        .iter()
        .map(|conversation| (conversation.peer.user.id, conversation.id.clone()))
        .collect::<BTreeMap<_, _>>();
    let previous = match previous {
        Some(previous) => previous,
        None => {
            let cutoff = OffsetDateTime::now_utc() - TimeDuration::minutes(2);
            let recent = current
                .into_iter()
                .filter(|(_, starred_at)| {
                    parse_realtime_timestamp(starred_at).is_ok_and(|value| value >= cutoff)
                })
                .collect();
            return Ok(classify_wake_changes(
                &BTreeMap::new(),
                &recent,
                &conversation_peers,
            ));
        }
    };
    Ok(classify_wake_changes(
        &previous,
        &current,
        &conversation_peers,
    ))
}

fn classify_wake_changes(
    previous: &BTreeMap<u64, String>,
    current: &BTreeMap<u64, String>,
    conversation_peers: &BTreeMap<u64, String>,
) -> WakeSignals {
    let changed = previous
        .keys()
        .chain(current.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|user_id| previous.get(user_id) != current.get(user_id));
    let mut conversation_ids = BTreeSet::new();
    let mut social_changed = false;
    for user_id in changed {
        if let Some(conversation_id) = conversation_peers.get(&user_id) {
            conversation_ids.insert(conversation_id.clone());
        } else {
            social_changed = true;
        }
    }
    WakeSignals {
        conversation_ids: conversation_ids.into_iter().collect(),
        social_changed,
    }
}

#[cfg(test)]
mod wake_signal_tests {
    use super::*;

    #[test]
    fn github_stargazer_timestamps_are_preserved() {
        let value = serde_json::json!({
            "starred_at": "2026-09-10T08:00:04Z",
            "user": {
                "id": 42,
                "login": "octocat",
                "avatar_url": "https://avatars.githubusercontent.com/u/42?v=4",
                "name": null
            }
        });

        let (viewer, starred_at) = normalize_stargazer(&value).unwrap();
        assert_eq!(viewer.id, 42);
        assert_eq!(starred_at, "2026-09-10T08:00:04Z");
    }

    #[test]
    fn star_additions_and_removals_wake_known_conversations() {
        let previous = BTreeMap::from([(42, "2026-09-10T08:00:00Z".to_owned())]);
        let current = BTreeMap::from([(99, "2026-09-10T08:00:03Z".to_owned())]);
        let conversations = BTreeMap::from([
            (42, "dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()),
            (99, "dm-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()),
        ]);
        let wake = classify_wake_changes(&previous, &current, &conversations);
        assert_eq!(
            wake.conversation_ids,
            vec![
                "dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "dm-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            ]
        );
        assert!(!wake.social_changed);
    }

    #[test]
    fn an_unknown_stargazer_requests_a_social_refresh() {
        let wake = classify_wake_changes(
            &BTreeMap::new(),
            &BTreeMap::from([(123, "2026-09-10T08:00:00Z".to_owned())]),
            &BTreeMap::new(),
        );
        assert!(wake.conversation_ids.is_empty());
        assert!(wake.social_changed);
    }

    #[test]
    fn restarring_a_known_contact_wakes_their_conversation() {
        let previous = BTreeMap::from([(42, "2026-09-10T08:00:00Z".to_owned())]);
        let current = BTreeMap::from([(42, "2026-09-10T08:00:04Z".to_owned())]);
        let conversations =
            BTreeMap::from([(42, "dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned())]);

        let wake = classify_wake_changes(&previous, &current, &conversations);
        assert_eq!(
            wake.conversation_ids,
            vec!["dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
        );
        assert!(!wake.social_changed);
    }

    #[test]
    fn call_signals_are_bounded_and_strict() {
        let signal = CallSignal {
            version: 1,
            call_id: "call-0123456789abcdef0123456789abcdef".to_owned(),
            created_at: "2026-09-10T08:00:00Z".to_owned(),
            expires_at: "2026-09-10T08:01:00Z".to_owned(),
        };
        assert!(validate_call_signal(&signal, false).is_ok());
        assert!(
            validate_call_signal(
                &CallSignal {
                    expires_at: "2026-09-10T08:03:00Z".to_owned(),
                    ..signal.clone()
                },
                false,
            )
            .is_err()
        );
        assert!(
            validate_call_signal(
                &CallSignal {
                    call_id: "call-invalid".to_owned(),
                    ..signal
                },
                false,
            )
            .is_err()
        );
    }
}

#[derive(Clone)]
struct IncomingRequestRecord {
    id: String,
    peer: PeerState,
}

async fn sync_social_state(state: &AppState) -> Result<SocialState> {
    let viewer = session_viewer(state).await?;
    let token = oauth_token(state).await?;
    recover_local_publications(state, &token, &viewer).await?;
    synchronize_accepted_requests(state, &token, &viewer).await?;
    collect_social_state(state, &token, &viewer).await
}

async fn sync_friend_requests(state: &AppState) -> Result<SocialState> {
    let viewer = session_viewer(state).await?;
    let token = oauth_token(state).await?;
    let incoming = collect_social_state(state, &token, &viewer).await?;
    if !incoming.incoming.is_empty() || incoming.outgoing.is_empty() {
        return Ok(incoming);
    }
    synchronize_accepted_requests(state, &token, &viewer).await?;
    collect_social_state(state, &token, &viewer).await
}

async fn collect_social_state(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
) -> Result<SocialState> {
    let conversations = state.social.read().await.conversations.clone();
    let conversation_ids = conversations
        .iter()
        .map(|conversation| conversation.id.clone())
        .collect::<BTreeSet<_>>();
    let conversation_peer_ids = conversations
        .iter()
        .map(|conversation| conversation.peer.user.id)
        .collect::<BTreeSet<_>>();
    let declined_request_ids = state
        .social
        .read()
        .await
        .declined_request_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let outgoing_requests = state.social.read().await.outgoing.clone();
    let mut incoming = list_incoming_requests(
        state,
        token,
        viewer,
        &conversation_ids,
        &conversation_peer_ids,
        &declined_request_ids,
        &outgoing_requests,
    )
    .await?;

    reconcile_crossed_requests(state, &mut incoming).await?;
    let social = state.social.read().await;
    let conversation_ids = social
        .conversations
        .iter()
        .map(|conversation| conversation.id.as_str())
        .collect::<BTreeSet<_>>();
    let conversation_peer_ids = social
        .conversations
        .iter()
        .map(|conversation| conversation.peer.user.id)
        .collect::<BTreeSet<_>>();
    incoming.retain(|request| {
        !conversation_ids.contains(request.id.as_str())
            && !conversation_peer_ids.contains(&request.peer.user.id)
            && !social
                .outgoing
                .iter()
                .any(|outgoing| outgoing.peer.user.id == request.peer.user.id)
    });
    let mut conversations = social
        .conversations
        .iter()
        .map(conversation_model)
        .collect::<Vec<_>>();
    let mut outgoing = social
        .outgoing
        .iter()
        .filter(|request| {
            !conversation_ids.contains(request.id.as_str())
                && !conversation_peer_ids.contains(&request.peer.user.id)
        })
        .map(friend_request)
        .collect::<Vec<_>>();
    drop(social);

    conversations.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    outgoing.sort_by(|left, right| left.id.cmp(&right.id));
    incoming.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(SocialState {
        conversations,
        incoming: incoming
            .into_iter()
            .map(|request| FriendRequest {
                version: 1,
                id: request.id,
                direction: "incoming".to_owned(),
                user: request.peer.user,
            })
            .collect(),
        outgoing,
    })
}

async fn recover_local_publications(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
) -> Result<()> {
    let (conversations, outgoing, pending_messages) = {
        let social = state.social.read().await;
        (
            social.conversations.clone(),
            social.outgoing.clone(),
            social
                .messages
                .iter()
                .filter(|record| !record.published)
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    for conversation in conversations {
        publish_conversation_marker(state, token, viewer, &conversation).await?;
        remove_temporary_star(state, token, viewer, &conversation.peer).await?;
    }
    for request in outgoing {
        send_friend_request(state, &request.peer.user.login, false).await?;
    }
    for record in pending_messages {
        publish_message(state, token, viewer, &record.message, &record.envelope).await?;
        mark_message_published(state, viewer.repository.id, &record.message.id).await?;
    }
    Ok(())
}

async fn synchronize_accepted_requests(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
) -> Result<()> {
    let outgoing = state.social.read().await.outgoing.clone();
    for request in outgoing {
        if state
            .social
            .read()
            .await
            .conversations
            .iter()
            .any(|conversation| conversation.peer.user.id == request.peer.user.id)
        {
            state
                .social
                .write()
                .await
                .outgoing
                .retain(|candidate| candidate.id != request.id);
            state.persist_social(viewer.repository.id).await?;
            remove_temporary_star(state, token, viewer, &request.peer).await?;
            continue;
        }
        let marker = read_repository_json(
            &state.github,
            token,
            &request.peer.user.login,
            &request.peer.user.repository,
            &format!("conv/{}/conversation.enc.json", request.id),
            512 * 1024,
        )
        .await?;
        let Some(marker) = marker else {
            continue;
        };
        let envelope: crate::protocol::RatchetEnvelope = match serde_json::from_value(marker) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if crate::protocol::validate_acceptance_envelope(
            &request.peer.profile,
            request.peer.user.id,
            request.peer.repository_id,
            &request.id,
            &envelope,
        )
        .is_err()
        {
            continue;
        }
        let secret = state
            .identity
            .read()
            .await
            .as_ref()
            .ok_or(Error::Crypto)?
            .secret
            .clone();
        let handshake = crate::protocol::OwnedHandshake {
            peer_profile: request.peer.profile.clone(),
            peer_github_user_id: request.peer.user.id,
            peer_repository_id: request.peer.repository_id,
            conversation_id: request.id.clone(),
            created_at: request.created_at.clone(),
        };
        let decrypted = tokio::task::spawn_blocking(move || {
            crate::protocol::decrypt_acceptance_owned(secret, handshake, envelope)
        })
        .await
        .map_err(|_| Error::Local)?;
        let (secret, acceptance) = match decrypted {
            Ok(value) => value,
            Err(Error::Crypto | Error::InvalidData) => continue,
            Err(error) => return Err(error),
        };
        let conversation = ConversationState {
            id: request.id.clone(),
            created_at: acceptance.created_at,
            peer: request.peer.clone(),
            handshake_envelope: request.envelope.clone(),
        };
        state
            .identity
            .write()
            .await
            .as_mut()
            .ok_or(Error::Crypto)?
            .secret = secret;
        {
            let mut social = state.social.write().await;
            social
                .outgoing
                .retain(|candidate| candidate.peer.user.id != request.peer.user.id);
            if !social
                .conversations
                .iter()
                .any(|candidate| candidate.peer.user.id == request.peer.user.id)
            {
                social.conversations.push(conversation.clone());
            }
        }
        state.persist_social(viewer.repository.id).await?;
        state.persist_identity(viewer.repository.id).await?;
        publish_conversation_marker(state, token, viewer, &conversation).await?;
        remove_temporary_star(state, token, viewer, &conversation.peer).await?;
    }
    Ok(())
}

async fn list_incoming_requests(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
    existing_ids: &BTreeSet<String>,
    existing_peer_ids: &BTreeSet<u64>,
    declined_request_ids: &BTreeSet<String>,
    outgoing_requests: &[OutgoingRequestState],
) -> Result<Vec<IncomingRequestRecord>> {
    let endpoint = format!(
        "/repos/{}/{}/stargazers?per_page={MAX_FRIENDS}",
        viewer.login, viewer.repository.name
    );
    let response: ApiResponse<Value> = state
        .github
        .api_stargazers(&endpoint, token.access_token.expose_secret(), None, &[403])
        .await?;
    let mut peers = known_outgoing_candidates(outgoing_requests, existing_peer_ids);
    let candidates: &[Value] = match response.data.as_ref() {
        Some(data) => data
            .as_array()
            .ok_or(Error::InvalidGitHubResponse)?
            .as_slice(),
        None => &[],
    };
    if candidates.len() > MAX_FRIENDS {
        return Err(Error::InvalidGitHubResponse);
    }
    for candidate in candidates {
        let candidate = match normalize_stargazer(candidate).map(|value| value.0) {
            Ok(value) if value.id != viewer.id && !existing_peer_ids.contains(&value.id) => value,
            _ => continue,
        };
        let peer = match resolve_user(&state.github, token, &candidate.login).await {
            Ok(Some(value)) => match value.registered() {
                Ok(value) => peer_state(value),
                Err(_) => continue,
            },
            Ok(None) | Err(Error::InvalidGitHubResponse | Error::GitHubNotFound) => continue,
            Err(error) => return Err(error),
        };
        peers.insert(peer.user.id, peer);
    }
    let mut requests = BTreeMap::new();
    for peer in peers.into_values() {
        let path = format!("friends/outgoing/{}", viewer.id);
        let entries = list_repository_directory(
            state,
            token,
            &peer.user.login,
            &peer.user.repository,
            &path,
            MAX_REQUESTS_PER_USER,
        )
        .await?;
        for name in entries {
            let Some(id) = name.strip_suffix(".enc.json") else {
                continue;
            };
            if validation::conversation_id(id).is_err()
                || existing_ids.contains(id)
                || declined_request_ids.contains(id)
            {
                continue;
            }
            let path = format!("friends/outgoing/{}/{}", viewer.id, name);
            let value = match read_repository_json(
                &state.github,
                token,
                &peer.user.login,
                &peer.user.repository,
                &path,
                512 * 1024,
            )
            .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(Error::InvalidGitHubResponse | Error::ResponseTooLarge) => continue,
                Err(error) => return Err(error),
            };
            let envelope: crate::protocol::RatchetEnvelope = match serde_json::from_value(value) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if crate::protocol::validate_invitation_envelope(
                &peer.profile,
                peer.user.id,
                peer.repository_id,
                id,
                &envelope,
            )
            .is_err()
            {
                continue;
            }
            requests
                .entry(id.to_owned())
                .or_insert_with(|| IncomingRequestRecord {
                    id: id.to_owned(),
                    peer: peer.clone(),
                });
        }
    }
    Ok(requests.into_values().take(MAX_FRIENDS).collect())
}

fn known_outgoing_candidates(
    outgoing_requests: &[OutgoingRequestState],
    existing_peer_ids: &BTreeSet<u64>,
) -> BTreeMap<u64, PeerState> {
    outgoing_requests
        .iter()
        .filter(|request| !existing_peer_ids.contains(&request.peer.user.id))
        .map(|request| (request.peer.user.id, request.peer.clone()))
        .collect()
}

async fn reconcile_crossed_requests(
    state: &AppState,
    incoming: &mut Vec<IncomingRequestRecord>,
) -> Result<()> {
    let viewer = session_viewer(state).await?;
    let outgoing = state.social.read().await.outgoing.clone();
    let mut accepted_peers = BTreeSet::new();
    for request in &outgoing {
        let peer_incoming = incoming
            .iter()
            .filter(|candidate| candidate.peer.user.id == request.peer.user.id)
            .min_by(|left, right| {
                request_order(
                    (&left.id, left.peer.user.id),
                    (&right.id, right.peer.user.id),
                )
            });
        let Some(peer_incoming) = peer_incoming else {
            continue;
        };
        if should_accept_crossed_request(viewer.id, request, peer_incoming) {
            match accept_friend_request(state, &peer_incoming.peer.user.login, &peer_incoming.id)
                .await
            {
                Ok(_) => {
                    accepted_peers.insert(peer_incoming.peer.user.id);
                }
                Err(Error::Crypto | Error::InvalidData | Error::InvalidGitHubResponse) => {}
                Err(error) => return Err(error),
            }
        }
    }
    incoming.retain(|request| !accepted_peers.contains(&request.peer.user.id));
    Ok(())
}

fn request_order(left: (&str, u64), right: (&str, u64)) -> std::cmp::Ordering {
    left.0.cmp(right.0).then_with(|| left.1.cmp(&right.1))
}

fn should_accept_crossed_request(
    viewer_id: u64,
    outgoing: &OutgoingRequestState,
    incoming: &IncomingRequestRecord,
) -> bool {
    !crate::protocol::same_public_profile(&outgoing.peer.profile, &incoming.peer.profile)
        || request_order(
            (&incoming.id, incoming.peer.user.id),
            (&outgoing.id, viewer_id),
        )
        .is_lt()
}

#[cfg(test)]
mod crossed_request_tests {
    use super::*;

    fn peer(profile: crate::protocol::PublicProfile) -> PeerState {
        PeerState {
            user: NoosphereUser {
                id: 99,
                login: "peer".to_owned(),
                name: None,
                avatar_url: "https://avatars.githubusercontent.com/u/99?v=4".to_owned(),
                repository: "noosphere_user_peer".to_owned(),
            },
            repository_id: 990,
            profile,
        }
    }

    fn outgoing(id: &str, profile: crate::protocol::PublicProfile) -> OutgoingRequestState {
        let (viewer, _) = crate::protocol::create_identity(42, 420).unwrap();
        let handshake = crate::protocol::OwnedHandshake {
            peer_profile: profile.clone(),
            peer_github_user_id: 99,
            peer_repository_id: 990,
            conversation_id: id.to_owned(),
            created_at: "2026-09-04T09:00:00Z".to_owned(),
        };
        let (_, envelope) = crate::protocol::create_invitation_owned(viewer, handshake).unwrap();
        OutgoingRequestState {
            id: id.to_owned(),
            created_at: "2026-09-04T09:00:00Z".to_owned(),
            peer: peer(profile),
            envelope,
        }
    }

    #[test]
    fn crossed_requests_use_a_single_stable_conversation() {
        let (_, profile) = crate::protocol::create_identity(99, 990).unwrap();
        let outgoing = outgoing("dm-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", profile.clone());
        let incoming = IncomingRequestRecord {
            id: "dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            peer: peer(profile),
        };

        assert!(should_accept_crossed_request(42, &outgoing, &incoming));
    }

    #[test]
    fn a_current_peer_identity_takes_precedence_over_request_order() {
        let (_, old_profile) = crate::protocol::create_identity(99, 990).unwrap();
        let (_, current_profile) = crate::protocol::create_identity(99, 990).unwrap();
        let outgoing = outgoing("dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", old_profile);
        let incoming = IncomingRequestRecord {
            id: "dm-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
            peer: peer(current_profile),
        };

        assert!(should_accept_crossed_request(42, &outgoing, &incoming));
    }

    #[test]
    fn crossed_requests_are_checked_without_a_stargazer_listing() {
        let (_, profile) = crate::protocol::create_identity(99, 990).unwrap();
        let outgoing = outgoing("dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", profile);
        let candidates = known_outgoing_candidates(&[outgoing], &BTreeSet::new());

        assert!(candidates.contains_key(&99));
    }

    #[test]
    fn stale_outgoing_invitations_are_detected_after_an_identity_change() {
        let (viewer, viewer_profile) = crate::protocol::create_identity(42, 420).unwrap();
        let (_, peer_profile) = crate::protocol::create_identity(99, 990).unwrap();
        let request = outgoing("dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", peer_profile.clone());
        let handshake = crate::protocol::OwnedHandshake {
            peer_profile: peer_profile.clone(),
            peer_github_user_id: 99,
            peer_repository_id: 990,
            conversation_id: request.id.clone(),
            created_at: request.created_at.clone(),
        };
        let (_, envelope) = crate::protocol::create_invitation_owned(viewer, handshake).unwrap();
        let current = OutgoingRequestState {
            envelope,
            ..request
        };
        assert!(outgoing_invitation_is_current(
            &current,
            &viewer_profile,
            &peer_profile,
            42,
            420
        ));

        let (_, replacement_profile) = crate::protocol::create_identity(42, 420).unwrap();
        assert!(!outgoing_invitation_is_current(
            &current,
            &replacement_profile,
            &peer_profile,
            42,
            420
        ));
    }
}

#[tauri::command]
pub async fn noosphere_accept_friend_request(
    state: State<'_, AppState>,
    login: String,
    conversation_id: String,
) -> CommandResult<Conversation> {
    let login = validation::github_login(&login)
        .map_err(command_error)?
        .to_owned();
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    accept_friend_request(&state, &login, &conversation_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn noosphere_decline_friend_request(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CommandResult<FriendRequestDeclined> {
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    let viewer = session_viewer(&state).await.map_err(command_error)?;
    {
        let mut social = state.social.write().await;
        remember_declined_request(&mut social, conversation_id);
    }
    state
        .persist_social(viewer.repository.id)
        .await
        .map_err(command_error)?;
    Ok(FriendRequestDeclined { declined: true })
}

fn remember_declined_request(social: &mut crate::state::LocalSocialState, conversation_id: String) {
    if social
        .declined_request_ids
        .iter()
        .any(|request_id| request_id == &conversation_id)
    {
        return;
    }
    social.declined_request_ids.push(conversation_id);
    if social.declined_request_ids.len() > 2_000 {
        let remove = social.declined_request_ids.len() - 2_000;
        social.declined_request_ids.drain(..remove);
    }
}

#[cfg(test)]
mod declined_request_tests {
    use super::*;

    #[test]
    fn declined_requests_are_kept_once_and_bounded() {
        let mut social = crate::state::LocalSocialState::default();
        for index in 0..2_001 {
            remember_declined_request(&mut social, format!("dm-{index:032x}"));
        }
        remember_declined_request(
            &mut social,
            "dm-000000000000000000000000000007d0".to_owned(),
        );

        assert_eq!(social.declined_request_ids.len(), 2_000);
        assert_eq!(
            social.declined_request_ids.first().map(String::as_str),
            Some("dm-00000000000000000000000000000001")
        );
        assert_eq!(
            social.declined_request_ids.last().map(String::as_str),
            Some("dm-000000000000000000000000000007d0")
        );
    }
}

async fn accept_friend_request(
    state: &AppState,
    login: &str,
    conversation_id: &str,
) -> Result<Conversation> {
    let viewer = session_viewer(state).await?;
    let token = oauth_token(state).await?;
    if let Some(existing) = state
        .social
        .read()
        .await
        .conversations
        .iter()
        .find(|conversation| conversation.id == conversation_id)
        .cloned()
    {
        publish_conversation_marker(state, &token, &viewer, &existing).await?;
        return Ok(conversation_model(&existing));
    }
    let peer = resolve_user(&state.github, &token, login)
        .await?
        .ok_or(Error::GitHubNotFound)?
        .registered()?;
    if peer.identity.id == viewer.id {
        return Err(Error::InvalidData);
    }
    let path = format!(
        "friends/outgoing/{}/{}.enc.json",
        viewer.id, conversation_id
    );
    let invitation = read_repository_json(
        &state.github,
        &token,
        &peer.identity.login,
        &peer.repository.name,
        &path,
        512 * 1024,
    )
    .await?
    .ok_or(Error::GitHubNotFound)?;
    let invitation: crate::protocol::RatchetEnvelope =
        serde_json::from_value(invitation).map_err(|_| Error::InvalidGitHubResponse)?;
    let peer_state = PeerState {
        user: NoosphereUser {
            id: peer.identity.id,
            login: peer.identity.login,
            name: peer.identity.name,
            avatar_url: peer.identity.avatar_url,
            repository: peer.repository.name,
        },
        repository_id: peer.repository.id,
        profile: peer.profile,
    };
    let peer_identity_changed = state.social.read().await.outgoing.iter().any(|request| {
        request.peer.user.id == peer_state.user.id
            && !crate::protocol::same_public_profile(&request.peer.profile, &peer_state.profile)
    });
    let mut secret = state
        .identity
        .read()
        .await
        .as_ref()
        .ok_or(Error::Crypto)?
        .secret
        .clone();
    if peer_identity_changed {
        secret = crate::protocol::forget_peer_owned(secret, peer_state.user.id)?;
    }
    let handshake = crate::protocol::OwnedHandshake {
        peer_profile: peer_state.profile.clone(),
        peer_github_user_id: peer_state.user.id,
        peer_repository_id: peer_state.repository_id,
        conversation_id: conversation_id.to_owned(),
        created_at: crate::protocol::current_timestamp()?,
    };
    let (secret, opened, acceptance) = tokio::task::spawn_blocking(move || {
        crate::protocol::accept_invitation_owned(secret, handshake, invitation)
    })
    .await
    .map_err(|_| Error::Local)??;
    let conversation = ConversationState {
        id: conversation_id.to_owned(),
        created_at: opened.created_at,
        peer: peer_state,
        handshake_envelope: acceptance,
    };
    state
        .identity
        .write()
        .await
        .as_mut()
        .ok_or(Error::Crypto)?
        .secret = secret;
    {
        let mut social = state.social.write().await;
        social
            .outgoing
            .retain(|request| request.peer.user.id != conversation.peer.user.id);
        social.conversations.push(conversation.clone());
    }
    state.persist_identity(viewer.repository.id).await?;
    state.persist_social(viewer.repository.id).await?;
    publish_conversation_marker(state, &token, &viewer, &conversation).await?;
    remove_temporary_star(state, &token, &viewer, &conversation.peer).await?;
    ensure_temporary_star(state, &token, &viewer, &conversation.peer, true).await?;
    Ok(conversation_model(&conversation))
}

fn conversation_model(conversation: &ConversationState) -> Conversation {
    Conversation {
        version: 1,
        id: conversation.id.clone(),
        kind: "direct".to_owned(),
        created_at: conversation.created_at.clone(),
        peer: conversation.peer.user.clone(),
    }
}

async fn publish_conversation_marker(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
    conversation: &ConversationState,
) -> Result<()> {
    let content = format!(
        "{}\n",
        serde_json::to_string(&conversation.handshake_envelope).map_err(|_| Error::Local)?
    );
    write_repository_file(
        &state.github,
        token,
        &viewer_identity(viewer),
        &repository_identity(viewer),
        &format!("conv/{}/conversation.enc.json", conversation.id),
        content.as_bytes(),
        "Accepter une demande d’ami",
        false,
    )
    .await?;
    Ok(())
}

async fn remove_temporary_star(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
    peer: &PeerState,
) -> Result<()> {
    let temporary = state
        .social
        .read()
        .await
        .temporary_stars
        .contains(&peer.repository_id);
    if !temporary {
        return Ok(());
    }
    let endpoint = format!("/user/starred/{}/{}", peer.user.login, peer.user.repository);
    state
        .github
        .api_json::<Value>(
            Method::DELETE,
            &endpoint,
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[404],
        )
        .await?;
    state
        .social
        .write()
        .await
        .temporary_stars
        .retain(|value| *value != peer.repository_id);
    state.persist_social(viewer.repository.id).await
}

#[tauri::command]
pub async fn noosphere_remove_friend(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CommandResult<FriendRemoved> {
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    remove_friend(&state, &conversation_id)
        .await
        .map_err(command_error)
}

async fn remove_friend(state: &AppState, conversation_id: &str) -> Result<FriendRemoved> {
    let viewer = session_viewer(state).await?;
    let peer = {
        let mut social = state.social.write().await;
        remove_friend_records(&mut social, conversation_id).ok_or(Error::GitHubNotFound)?
    };
    let secret = state
        .identity
        .read()
        .await
        .as_ref()
        .ok_or(Error::Crypto)?
        .secret
        .clone();
    let secret = crate::protocol::forget_peer_owned(secret, peer.user.id)?;
    state
        .identity
        .write()
        .await
        .as_mut()
        .ok_or(Error::Crypto)?
        .secret = secret;
    state.persist_social(viewer.repository.id).await?;
    state.persist_identity(viewer.repository.id).await?;
    Ok(FriendRemoved { removed: true })
}

fn remove_friend_records(
    social: &mut crate::state::LocalSocialState,
    conversation_id: &str,
) -> Option<PeerState> {
    let peer = social
        .conversations
        .iter()
        .find(|conversation| conversation.id == conversation_id)?
        .peer
        .clone();
    let removed_message_ids = social
        .messages
        .iter()
        .filter(|record| record.message.conversation_id == conversation_id)
        .map(|record| record.message.id.clone())
        .collect::<BTreeSet<_>>();
    social
        .conversations
        .retain(|conversation| conversation.id != conversation_id);
    social
        .outgoing
        .retain(|request| request.peer.user.id != peer.user.id);
    social
        .messages
        .retain(|record| record.message.conversation_id != conversation_id);
    social
        .seen_message_ids
        .retain(|message_id| !removed_message_ids.contains(message_id));
    social
        .temporary_stars
        .retain(|repository_id| *repository_id != peer.repository_id);
    Some(peer)
}

#[cfg(test)]
mod friend_removal_tests {
    use super::*;

    #[test]
    fn removing_a_friend_clears_their_conversation_messages_and_request() {
        let (viewer, _) = crate::protocol::create_identity(42, 420).unwrap();
        let (_, peer_profile) = crate::protocol::create_identity(99, 990).unwrap();
        let conversation_id = "dm-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let handshake = crate::protocol::OwnedHandshake {
            peer_profile: peer_profile.clone(),
            peer_github_user_id: 99,
            peer_repository_id: 990,
            conversation_id: conversation_id.to_owned(),
            created_at: "2026-09-08T12:00:00Z".to_owned(),
        };
        let (_, envelope) = crate::protocol::create_invitation_owned(viewer, handshake).unwrap();
        let peer = PeerState {
            user: NoosphereUser {
                id: 99,
                login: "friend".to_owned(),
                name: Some("Friend".to_owned()),
                avatar_url: "https://avatars.githubusercontent.com/u/99?v=4".to_owned(),
                repository: "noosphere_user_friend".to_owned(),
            },
            repository_id: 990,
            profile: peer_profile,
        };
        let mut social = crate::state::LocalSocialState::default();
        social.conversations.push(ConversationState {
            id: conversation_id.to_owned(),
            created_at: "2026-09-08T12:00:00Z".to_owned(),
            peer: peer.clone(),
            handshake_envelope: envelope.clone(),
        });
        social.outgoing.push(OutgoingRequestState {
            id: conversation_id.to_owned(),
            created_at: "2026-09-08T12:00:00Z".to_owned(),
            peer: peer.clone(),
            envelope: envelope.clone(),
        });
        social.messages.push(LocalMessageState {
            message: Message {
                version: 1,
                id: "msg-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
                conversation_id: conversation_id.to_owned(),
                sent_at: "2026-09-08T12:01:00Z".to_owned(),
                text: "hello".to_owned(),
                sender_id: 99,
                own: false,
            },
            envelope,
            published: true,
        });
        social
            .seen_message_ids
            .push("msg-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned());
        social.temporary_stars.push(peer.repository_id);

        let removed = remove_friend_records(&mut social, conversation_id).unwrap();

        assert_eq!(removed.user.id, peer.user.id);
        assert!(social.conversations.is_empty());
        assert!(social.outgoing.is_empty());
        assert!(social.messages.is_empty());
        assert!(social.seen_message_ids.is_empty());
        assert!(social.temporary_stars.is_empty());
    }
}

#[tauri::command]
pub async fn noosphere_send_message(
    state: State<'_, AppState>,
    conversation_id: String,
    text: String,
) -> CommandResult<Message> {
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    validation::message_text(&text).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    send_message(&state, conversation_id, text)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn noosphere_list_messages(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CommandResult<Vec<Message>> {
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    list_messages(&state, &conversation_id)
        .await
        .map_err(command_error)
}

async fn send_message(state: &AppState, conversation_id: String, text: String) -> Result<Message> {
    let viewer = session_viewer(state).await?;
    let token = oauth_token(state).await?;
    let conversation = find_conversation(state, &conversation_id).await?;
    let id = format!("msg-{}", uuid::Uuid::new_v4().simple());
    let sent_at = crate::protocol::current_timestamp()?;
    let secret = state
        .identity
        .read()
        .await
        .as_ref()
        .ok_or(Error::Crypto)?
        .secret
        .clone();
    let owned = crate::protocol::OwnedMessage {
        peer_profile: conversation.peer.profile.clone(),
        peer_github_user_id: conversation.peer.user.id,
        peer_repository_id: conversation.peer.repository_id,
        conversation_id: conversation_id.clone(),
        message_id: id.clone(),
        sent_at: sent_at.clone(),
        text: text.clone(),
    };
    let (secret, envelope) =
        tokio::task::spawn_blocking(move || crate::protocol::encrypt_message_owned(secret, owned))
            .await
            .map_err(|_| Error::Local)??;
    let message = Message {
        version: 1,
        id,
        conversation_id,
        sent_at,
        text,
        sender_id: viewer.id,
        own: true,
    };
    state
        .identity
        .write()
        .await
        .as_mut()
        .ok_or(Error::Crypto)?
        .secret = secret;
    state
        .record_message(LocalMessageState {
            message: message.clone(),
            envelope: envelope.clone(),
            published: false,
        })
        .await?;
    // Persist the plaintext queue before advancing the durable ratchet. A crash can
    // then replay the encrypted publication without losing the user's message.
    state.persist_social(viewer.repository.id).await?;
    state.persist_identity(viewer.repository.id).await?;
    publish_message(state, &token, &viewer, &message, &envelope).await?;
    mark_message_published(state, viewer.repository.id, &message.id).await?;
    Ok(message)
}

#[tauri::command]
pub async fn noosphere_signal_wake(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CommandResult<Published> {
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    let token = oauth_token(&state).await.map_err(command_error)?;
    let conversation = find_conversation(&state, &conversation_id)
        .await
        .map_err(command_error)?;
    refresh_peer_wake_signal(&state, &token, &conversation.peer)
        .await
        .map_err(command_error)?;
    Ok(Published { published: true })
}

async fn refresh_peer_wake_signal(
    state: &AppState,
    token: &OAuthToken,
    peer: &PeerState,
) -> Result<()> {
    let endpoint = format!("/user/starred/{}/{}", peer.user.login, peer.user.repository);
    let starred: ApiResponse<Value> = state
        .github
        .api_json(
            Method::GET,
            &endpoint,
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[404],
        )
        .await?;
    if starred.status != 404 {
        state
            .github
            .api_json::<Value>(
                Method::DELETE,
                &endpoint,
                Some(token.access_token.expose_secret()),
                None,
                None,
                &[404],
            )
            .await?;
        sleep(Duration::from_secs(1)).await;
    }
    state
        .github
        .api_json::<Value>(
            Method::PUT,
            &endpoint,
            Some(token.access_token.expose_secret()),
            None,
            None,
            &[],
        )
        .await?;
    Ok(())
}

async fn list_messages(state: &AppState, conversation_id: &str) -> Result<Vec<Message>> {
    let viewer = session_viewer(state).await?;
    let token = oauth_token(state).await?;
    let conversation = find_conversation(state, conversation_id).await?;
    publish_pending_messages(state, &token, &viewer, conversation_id).await?;

    let path = format!("conv/{conversation_id}/messages");
    let entries = list_repository_directory(
        state,
        &token,
        &conversation.peer.user.login,
        &conversation.peer.user.repository,
        &path,
        MAX_MESSAGES_PER_CONVERSATION,
    )
    .await?;
    let mut known = state
        .social
        .read()
        .await
        .seen_message_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for name in entries {
        let Some(id) = name.strip_suffix(".enc.json") else {
            continue;
        };
        if validation::message_id(id).is_err() || known.contains(id) {
            continue;
        }
        let value = match read_repository_json(
            &state.github,
            &token,
            &conversation.peer.user.login,
            &conversation.peer.user.repository,
            &format!("{path}/{name}"),
            512 * 1024,
        )
        .await
        {
            Ok(Some(value)) => value,
            Ok(None) | Err(Error::InvalidGitHubResponse | Error::ResponseTooLarge) => continue,
            Err(error) => return Err(error),
        };
        let envelope: crate::protocol::RatchetEnvelope = match serde_json::from_value(value) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if crate::protocol::validate_ratchet_envelope(
            &conversation.peer.profile,
            conversation.peer.user.id,
            conversation.peer.repository_id,
            conversation_id,
            id,
            &envelope,
        )
        .is_err()
        {
            continue;
        }
        let secret = state
            .identity
            .read()
            .await
            .as_ref()
            .ok_or(Error::Crypto)?
            .secret
            .clone();
        let owned = crate::protocol::OwnedDecryptMessage {
            peer_profile: conversation.peer.profile.clone(),
            peer_github_user_id: conversation.peer.user.id,
            peer_repository_id: conversation.peer.repository_id,
            conversation_id: conversation_id.to_owned(),
            message_id: id.to_owned(),
        };
        let decrypted = tokio::task::spawn_blocking({
            let envelope = envelope.clone();
            move || crate::protocol::decrypt_message_owned(secret, owned, envelope)
        })
        .await
        .map_err(|_| Error::Local)?;
        let (secret, decrypted) = match decrypted {
            Ok(value) => value,
            Err(Error::Crypto | Error::InvalidData) => continue,
            Err(error) => return Err(error),
        };
        let message = Message {
            version: 1,
            id: id.to_owned(),
            conversation_id: conversation_id.to_owned(),
            sent_at: decrypted.sent_at,
            text: decrypted.text,
            sender_id: conversation.peer.user.id,
            own: false,
        };
        state
            .identity
            .write()
            .await
            .as_mut()
            .ok_or(Error::Crypto)?
            .secret = secret;
        state
            .record_message(LocalMessageState {
                message: message.clone(),
                envelope: envelope.clone(),
                published: false,
            })
            .await?;
        state.persist_social(viewer.repository.id).await?;
        state.persist_identity(viewer.repository.id).await?;
        publish_message(state, &token, &viewer, &message, &envelope).await?;
        mark_message_published(state, viewer.repository.id, id).await?;
        known.insert(id.to_owned());
    }

    let mut messages = state
        .social
        .read()
        .await
        .messages
        .iter()
        .filter(|record| record.message.conversation_id == conversation_id)
        .map(|record| record.message.clone())
        .collect::<Vec<_>>();
    messages.sort_by(|left, right| {
        left.sent_at
            .cmp(&right.sent_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(messages)
}

async fn find_conversation(state: &AppState, conversation_id: &str) -> Result<ConversationState> {
    state
        .social
        .read()
        .await
        .conversations
        .iter()
        .find(|conversation| conversation.id == conversation_id)
        .cloned()
        .ok_or(Error::GitHubNotFound)
}

async fn publish_pending_messages(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
    conversation_id: &str,
) -> Result<()> {
    let pending = state
        .social
        .read()
        .await
        .messages
        .iter()
        .filter(|record| record.message.conversation_id == conversation_id && !record.published)
        .cloned()
        .collect::<Vec<_>>();
    for record in pending {
        publish_message(state, token, viewer, &record.message, &record.envelope).await?;
        mark_message_published(state, viewer.repository.id, &record.message.id).await?;
    }
    Ok(())
}

async fn publish_message(
    state: &AppState,
    token: &OAuthToken,
    viewer: &GitHubViewer,
    message: &Message,
    envelope: &crate::protocol::RatchetEnvelope,
) -> Result<()> {
    let content = format!(
        "{}\n",
        serde_json::to_string(envelope).map_err(|_| Error::Local)?
    );
    write_repository_file(
        &state.github,
        token,
        &viewer_identity(viewer),
        &repository_identity(viewer),
        &format!(
            "conv/{}/messages/{}.enc.json",
            message.conversation_id, message.id
        ),
        content.as_bytes(),
        if message.own {
            "Envoyer un message chiffré"
        } else {
            "Synchroniser un message chiffré"
        },
        false,
    )
    .await?;
    Ok(())
}

async fn mark_message_published(
    state: &AppState,
    repository_id: u64,
    message_id: &str,
) -> Result<()> {
    if let Some(record) = state
        .social
        .write()
        .await
        .messages
        .iter_mut()
        .find(|record| record.message.id == message_id)
    {
        record.published = true;
    }
    state.persist_social(repository_id).await
}

#[tauri::command]
pub async fn noosphere_signal_call(
    state: State<'_, AppState>,
    conversation_id: String,
    call_id: String,
) -> CommandResult<CallSignal> {
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    validation::call_id(&call_id).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    let now = OffsetDateTime::now_utc();
    let signal = CallSignal {
        version: 1,
        call_id,
        created_at: now
            .format(&Rfc3339)
            .map_err(|_| command_error(Error::Local))?,
        expires_at: (now + TimeDuration::seconds(60))
            .format(&Rfc3339)
            .map_err(|_| command_error(Error::Local))?,
    };
    publish_call_signal(&state, &conversation_id, &signal)
        .await
        .map_err(command_error)?;
    let token = oauth_token(&state).await.map_err(command_error)?;
    let conversation = find_conversation(&state, &conversation_id)
        .await
        .map_err(command_error)?;
    refresh_peer_wake_signal(&state, &token, &conversation.peer)
        .await
        .map_err(command_error)?;
    Ok(signal)
}

#[tauri::command]
pub async fn noosphere_read_call_signal(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CommandResult<Option<CallSignal>> {
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    read_call_signal(&state, &conversation_id)
        .await
        .map_err(command_error)
}

async fn publish_call_signal(
    state: &AppState,
    conversation_id: &str,
    signal: &CallSignal,
) -> Result<Published> {
    validate_call_signal(signal, true)?;
    let viewer = session_viewer(state).await?;
    let message_id = crate::protocol::call_signal_message_id(conversation_id, viewer.id)?;
    publish_ephemeral_payload(
        state,
        conversation_id,
        message_id,
        &format!("conv/{conversation_id}/realtime/call.enc.json"),
        &signal.created_at,
        serde_json::to_string(signal).map_err(|_| Error::Local)?,
        "Signaler un appel",
    )
    .await
}

async fn read_call_signal(state: &AppState, conversation_id: &str) -> Result<Option<CallSignal>> {
    let conversation = find_conversation(state, conversation_id).await?;
    let message_id =
        crate::protocol::call_signal_message_id(conversation_id, conversation.peer.user.id)?;
    let Some(serialized) = read_ephemeral_payload(
        state,
        conversation_id,
        &conversation,
        &format!("conv/{conversation_id}/realtime/call.enc.json"),
        message_id,
    )
    .await?
    else {
        return Ok(None);
    };
    let signal: CallSignal =
        serde_json::from_str(&serialized).map_err(|_| Error::InvalidGitHubResponse)?;
    validate_call_signal(&signal, false)?;
    Ok(is_live_signal(&signal.created_at, &signal.expires_at)?.then_some(signal))
}

#[tauri::command]
pub async fn noosphere_publish_realtime_signal(
    state: State<'_, AppState>,
    conversation_id: String,
    signal: RealtimeSignal,
) -> CommandResult<Published> {
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    validate_realtime_signal(&signal, true).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    publish_realtime_signal(&state, &conversation_id, signal)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn noosphere_read_realtime_signal(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CommandResult<Option<RealtimeSignal>> {
    validation::conversation_id(&conversation_id).map_err(command_error)?;
    let _operation = state.operations.lock().await;
    read_realtime_signal(&state, &conversation_id)
        .await
        .map_err(command_error)
}

async fn publish_realtime_signal(
    state: &AppState,
    conversation_id: &str,
    signal: RealtimeSignal,
) -> Result<Published> {
    let viewer = session_viewer(state).await?;
    let message_id = crate::protocol::realtime_message_id(conversation_id, viewer.id)?;
    publish_ephemeral_payload(
        state,
        conversation_id,
        message_id,
        &format!("conv/{conversation_id}/realtime/state.enc.json"),
        &signal.created_at,
        serde_json::to_string(&signal).map_err(|_| Error::Local)?,
        "Actualiser la liaison directe",
    )
    .await
}

async fn read_realtime_signal(
    state: &AppState,
    conversation_id: &str,
) -> Result<Option<RealtimeSignal>> {
    let conversation = find_conversation(state, conversation_id).await?;
    let message_id =
        crate::protocol::realtime_message_id(conversation_id, conversation.peer.user.id)?;
    let Some(serialized) = read_ephemeral_payload(
        state,
        conversation_id,
        &conversation,
        &format!("conv/{conversation_id}/realtime/state.enc.json"),
        message_id,
    )
    .await?
    else {
        return Ok(None);
    };
    let signal: RealtimeSignal =
        serde_json::from_str(&serialized).map_err(|_| Error::InvalidGitHubResponse)?;
    validate_realtime_signal(&signal, false)?;
    Ok(is_live_signal(&signal.created_at, &signal.expires_at)?.then_some(signal))
}

async fn publish_ephemeral_payload(
    state: &AppState,
    conversation_id: &str,
    message_id: String,
    path: &str,
    sent_at: &str,
    text: String,
    commit_message: &str,
) -> Result<Published> {
    let viewer = session_viewer(state).await?;
    let token = oauth_token(state).await?;
    let conversation = find_conversation(state, conversation_id).await?;
    let secret = state
        .identity
        .read()
        .await
        .as_ref()
        .ok_or(Error::Crypto)?
        .secret
        .clone();
    let owned = crate::protocol::OwnedMessage {
        peer_profile: conversation.peer.profile.clone(),
        peer_github_user_id: conversation.peer.user.id,
        peer_repository_id: conversation.peer.repository_id,
        conversation_id: conversation_id.to_owned(),
        message_id,
        sent_at: sent_at.to_owned(),
        text,
    };
    let (secret, envelope) =
        tokio::task::spawn_blocking(move || crate::protocol::encrypt_realtime_owned(secret, owned))
            .await
            .map_err(|_| Error::Local)??;
    state
        .identity
        .write()
        .await
        .as_mut()
        .ok_or(Error::Crypto)?
        .secret = secret;
    state.persist_identity(viewer.repository.id).await?;
    let content = format!(
        "{}\n",
        serde_json::to_string(&envelope).map_err(|_| Error::Local)?
    );
    write_repository_file(
        &state.github,
        &token,
        &viewer_identity(&viewer),
        &repository_identity(&viewer),
        path,
        content.as_bytes(),
        commit_message,
        true,
    )
    .await?;
    Ok(Published { published: true })
}

async fn read_ephemeral_payload(
    state: &AppState,
    conversation_id: &str,
    conversation: &ConversationState,
    path: &str,
    message_id: String,
) -> Result<Option<String>> {
    let viewer = session_viewer(state).await?;
    let token = oauth_token(state).await?;
    let value = read_repository_json_conditional(
        state,
        &token,
        &conversation.peer.user.login,
        &conversation.peer.user.repository,
        path,
        512 * 1024,
    )
    .await?;
    let Some(value) = value else {
        return Ok(None);
    };
    let envelope: crate::protocol::RatchetEnvelope =
        serde_json::from_value(value).map_err(|_| Error::InvalidGitHubResponse)?;
    let secret = state
        .identity
        .read()
        .await
        .as_ref()
        .ok_or(Error::Crypto)?
        .secret
        .clone();
    let owned = crate::protocol::OwnedDecryptMessage {
        peer_profile: conversation.peer.profile.clone(),
        peer_github_user_id: conversation.peer.user.id,
        peer_repository_id: conversation.peer.repository_id,
        conversation_id: conversation_id.to_owned(),
        message_id,
    };
    let decrypted = tokio::task::spawn_blocking(move || {
        crate::protocol::decrypt_realtime_owned(secret, owned, envelope)
    })
    .await
    .map_err(|_| Error::Local)?;
    let (secret, decrypted) = match decrypted {
        Ok(value) => value,
        Err(Error::Crypto | Error::InvalidData) => return Ok(None),
        Err(error) => return Err(error),
    };
    state
        .identity
        .write()
        .await
        .as_mut()
        .ok_or(Error::Crypto)?
        .secret = secret;
    state.persist_identity(viewer.repository.id).await?;
    Ok(Some(decrypted.text))
}

fn is_live_signal(created_at: &str, expires_at: &str) -> Result<bool> {
    let now = OffsetDateTime::now_utc();
    let created = parse_realtime_timestamp(created_at)?;
    let expires = parse_realtime_timestamp(expires_at)?;
    Ok(expires > now && created <= now + TimeDuration::seconds(30))
}

fn validate_realtime_signal(signal: &RealtimeSignal, require_live: bool) -> Result<()> {
    validation::realtime_session_id(&signal.session_id)?;
    crate::protocol::validate_public_timestamp(&signal.created_at)?;
    crate::protocol::validate_public_timestamp(&signal.expires_at)?;
    if signal.version != 1
        || !matches!(signal.kind.as_str(), "offer" | "answer")
        || signal.sdp.trim().is_empty()
        || signal.sdp.len() > 12 * 1024
        || signal.sdp.contains('\0')
    {
        return Err(Error::InvalidData);
    }
    let created = parse_realtime_timestamp(&signal.created_at)?;
    let expires = parse_realtime_timestamp(&signal.expires_at)?;
    if expires <= created || expires - created > TimeDuration::minutes(5) {
        return Err(Error::InvalidData);
    }
    if require_live {
        let now = OffsetDateTime::now_utc();
        if created < now - TimeDuration::minutes(5)
            || created > now + TimeDuration::seconds(30)
            || expires <= now
        {
            return Err(Error::InvalidData);
        }
    }
    Ok(())
}

fn validate_call_signal(signal: &CallSignal, require_live: bool) -> Result<()> {
    validation::call_id(&signal.call_id)?;
    crate::protocol::validate_public_timestamp(&signal.created_at)?;
    crate::protocol::validate_public_timestamp(&signal.expires_at)?;
    if signal.version != 1 {
        return Err(Error::InvalidData);
    }
    let created = parse_realtime_timestamp(&signal.created_at)?;
    let expires = parse_realtime_timestamp(&signal.expires_at)?;
    if expires <= created || expires - created > TimeDuration::minutes(2) {
        return Err(Error::InvalidData);
    }
    if require_live && !is_live_signal(&signal.created_at, &signal.expires_at)? {
        return Err(Error::InvalidData);
    }
    Ok(())
}

fn parse_realtime_timestamp(value: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| Error::InvalidData)
}

#[tauri::command]
pub fn system_notify(
    app: DesktopAppHandle,
    title: String,
    body: String,
) -> CommandResult<NotificationResult> {
    validation::notification_text(&title).map_err(command_error)?;
    validation::notification_text(&body).map_err(command_error)?;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|_| command_error(Error::Local))?;
    Ok(NotificationResult { shown: true })
}

#[tauri::command]
pub fn system_smoke_config(state: State<'_, AppState>) -> SmokeConfig {
    let enabled = std::env::var("NOOSPHERE_SMOKE_TEST").as_deref() == Ok("1");
    SmokeConfig {
        enabled,
        synthetic_media: enabled
            && std::env::var("NOOSPHERE_SMOKE_SYNTHETIC_MEDIA").as_deref() == Ok("1"),
        instance_profile_slot: enabled.then(|| state.profile.slot()),
    }
}

#[tauri::command]
pub fn system_smoke_complete(
    app: DesktopAppHandle,
    state: State<'_, AppState>,
    result: SmokeResult,
) -> CommandResult<Published> {
    if std::env::var("NOOSPHERE_SMOKE_TEST").as_deref() != Ok("1")
        || result.version != 1
        || result.instance_profile_slot != state.profile.slot()
    {
        return Err(command_error(Error::InvalidData));
    }
    let result_path = smoke_result_path().map_err(command_error)?;
    let serialized = serde_json::to_vec(&serde_json::json!({
        "version": result.version,
        "nativeBridge": result.native_bridge,
        "socialBridge": result.social_bridge,
        "webRtcDataChannel": result.web_rtc_data_channel,
        "webRtcMedia": result.web_rtc_media,
        "mediaPermission": result.media_permission,
        "brandAssets": result.brand_assets,
        "diagnostics": result.diagnostics,
        "instanceProfileSlot": result.instance_profile_slot,
    }))
    .map_err(|_| command_error(Error::Local))?;
    let temporary = result_path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, serialized).map_err(|_| command_error(Error::Local))?;
    std::fs::rename(&temporary, &result_path).map_err(|_| command_error(Error::Local))?;
    let hold = std::env::var("NOOSPHERE_SMOKE_HOLD_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
        .min(60_000);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(hold));
        app.exit(0);
    });
    Ok(Published { published: true })
}

fn smoke_result_path() -> Result<PathBuf> {
    let result =
        PathBuf::from(std::env::var_os("NOOSPHERE_SMOKE_RESULT_PATH").ok_or(Error::InvalidData)?);
    let data =
        PathBuf::from(std::env::var_os("NOOSPHERE_SMOKE_USER_DATA").ok_or(Error::InvalidData)?);
    if !result.is_absolute() || !data.is_absolute() {
        return Err(Error::InvalidData);
    }
    let result_parent = result.parent().ok_or(Error::InvalidData)?;
    let data_parent = data.parent().ok_or(Error::InvalidData)?;
    if std::fs::canonicalize(result_parent).map_err(|_| Error::InvalidData)?
        != std::fs::canonicalize(data_parent).map_err(|_| Error::InvalidData)?
    {
        return Err(Error::InvalidData);
    }
    let name = result
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(Error::InvalidData)?;
    if !name.starts_with("smoke-result-") || !name.ends_with(".json") || name.len() > 80 {
        return Err(Error::InvalidData);
    }
    Ok(result)
}

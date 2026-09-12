use crate::state::AppState;
use noosphere_remote::{
    daemon::{self, Bootstrap, Settings, Status},
    live::{GuestHello, HostOffer},
    permissions::{Permissions, Principal},
    signaling::random_id,
};
use secrecy::{ExposeSecret as _, SecretString};
use std::{
    io::Write as _,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tauri::State;
use tokio::sync::Mutex;
use zeroize::Zeroize as _;

#[cfg(windows)]
const FIREWALL_INSTALL_ARGUMENT: &str = "--remote-firewall-install";

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn system_netsh() -> Result<std::path::PathBuf, String> {
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    let mut buffer = vec![0_u16; 32_768];
    // SAFETY: `buffer` is writable for the length passed to the Win32 API.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length as usize >= buffer.len() {
        return Err("Le dossier système Windows est inaccessible.".into());
    }
    buffer.truncate(length as usize);
    use std::os::windows::ffi::OsStringExt as _;
    Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer)).join("netsh.exe"))
}

#[cfg(windows)]
fn firewall_rule_name(executable: &str) -> String {
    use sha2::{Digest as _, Sha256};

    let normalized = executable.replace('/', "\\").to_lowercase();
    let digest = hex::encode(Sha256::digest(normalized.as_bytes()));
    format!("Noosphere Remote Desktop [{}]", &digest[..12])
}

#[cfg(windows)]
fn run_netsh(arguments: &[String]) -> Result<bool, String> {
    let mut command = Command::new(system_netsh()?);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    use std::os::windows::process::CommandExt as _;
    command.creation_flags(0x08000000);
    command
        .status()
        .map(|status| status.success())
        .map_err(|error| {
            format!("Windows n’a pas pu lancer son gestionnaire de pare-feu : {error}")
        })
}

/// Runs only from the fixed, self-elevated command-line mode handled by main.rs.
/// No path or command is accepted from the caller: the rule always targets this
/// exact Noosphere executable.
#[cfg(windows)]
pub(crate) fn install_firewall_for_current_executable() -> i32 {
    let Some(executable) = firewall_executable() else {
        return 10;
    };
    let rule_name = firewall_rule_name(&executable);
    let netsh = match system_netsh() {
        Ok(path) => path,
        Err(_) => return 11,
    };
    let command = |arguments: &[String]| {
        let mut child = Command::new(&netsh);
        child
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        use std::os::windows::process::CommandExt as _;
        child.creation_flags(0x08000000);
        child.status().is_ok_and(|status| status.success())
    };

    // Remove the fixed-name rule left by the 0.1.15 PowerShell implementation.
    // Deletion is intentionally idempotent.
    let _ = command(&[
        "advfirewall".into(),
        "firewall".into(),
        "delete".into(),
        "rule".into(),
        "name=Noosphere Remote Desktop".into(),
    ]);

    let installed = command(&[
        "advfirewall".into(),
        "firewall".into(),
        "add".into(),
        "rule".into(),
        format!("name={rule_name}"),
        "dir=in".into(),
        "action=allow".into(),
        format!("program={executable}"),
        "enable=yes".into(),
        "profile=any".into(),
        "protocol=UDP".into(),
        "description=Noosphere native QUIC remote desktop".into(),
    ]);
    if installed { 0 } else { 12 }
}

#[cfg(windows)]
fn configure_windows_firewall(executable: &str) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{GetExitCodeProcess, WaitForSingleObject},
        UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
    };

    let current = firewall_executable()
        .ok_or_else(|| "L’exécutable Noosphere est introuvable.".to_owned())?;
    if !current.eq_ignore_ascii_case(executable) {
        return Err("Le chemin de Noosphere a changé pendant la configuration.".into());
    }
    let parameters = wide(FIREWALL_INSTALL_ARGUMENT);
    let verb = wide("runas");
    let file = wide(&current);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: 0,
        ..Default::default()
    };
    // SAFETY: all UTF-16 buffers stay alive until ShellExecuteExW returns and
    // hProcess is closed below. The structure is initialized to the documented
    // zero defaults for fields that are not used.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let failure = std::io::Error::last_os_error();
        if failure.raw_os_error() == Some(1223) {
            return Err(
                "Autorisation administrateur annulée. Relance l’hébergement pour réessayer.".into(),
            );
        }
        return Err(format!(
            "Windows n’a pas pu ouvrir Noosphere en administrateur : {failure}"
        ));
    }
    if info.hProcess.is_null() {
        return Err("Windows n’a pas démarré la configuration du pare-feu.".into());
    }
    // SAFETY: ShellExecuteExW returned an owned process handle because
    // SEE_MASK_NOCLOSEPROCESS was requested.
    let wait = unsafe { WaitForSingleObject(info.hProcess, 120_000) };
    let mut exit_code = u32::MAX;
    let read_exit = unsafe { GetExitCodeProcess(info.hProcess, &mut exit_code) };
    let _ = unsafe { CloseHandle(info.hProcess) };
    if wait != WAIT_OBJECT_0 {
        return Err("La configuration du pare-feu Windows a expiré.".into());
    }
    if read_exit == 0 {
        return Err("Windows n’a pas renvoyé le résultat de la configuration.".into());
    }
    if exit_code != 0 {
        return Err(format!(
            "Windows a refusé la règle UDP de Noosphere (code {exit_code})."
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn windows_firewall_is_configured(executable: &str) -> bool {
    let rule_name = firewall_rule_name(executable);
    run_netsh(&[
        "advfirewall".into(),
        "firewall".into(),
        "show".into(),
        "rule".into(),
        format!("name={rule_name}"),
    ])
    .unwrap_or(false)
}

#[cfg(windows)]
pub(crate) fn firewall_executable() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(not(windows))]
pub(crate) fn firewall_executable() -> Option<String> {
    None
}

#[cfg(windows)]
pub(crate) async fn configure_firewall(executable: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || configure_windows_firewall(&executable))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(windows)]
pub(crate) async fn firewall_is_configured(executable: String) -> bool {
    tokio::task::spawn_blocking(move || windows_firewall_is_configured(&executable))
        .await
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub(crate) async fn configure_firewall(_executable: String) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub(crate) async fn firewall_is_configured(_executable: String) -> bool {
    true
}

#[derive(Default)]
pub struct RemoteAccess {
    process: Mutex<Option<Host>>,
}
struct Host {
    child: Child,
    endpoint: String,
    key: [u8; 32],
    repository_id: u64,
}

impl Drop for Host {
    fn drop(&mut self) {
        // Closing the UI leaves the bounded diagnostic alive. The daemon owns its
        // input cleanup and exits after its idle timeout; no signing key is saved.
        self.key.zeroize();
    }
}

impl Host {
    async fn stop(&mut self) -> Result<(), String> {
        if self.child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Ok(());
        }
        daemon::ipc::request::<bool>(&self.endpoint, &self.key, daemon::Command::Stop)
            .await
            .map_err(|e| e.to_string())?;
        for _ in 0..50 {
            if self.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Err("L’hôte termine la session. Réessaie dans un instant.".into())
    }
}

async fn repository_id(state: &AppState) -> Result<u64, String> {
    state
        .session
        .read()
        .await
        .as_ref()
        .map(|session| session.viewer.repository.id)
        .ok_or_else(|| "Connecte-toi pour ouvrir les réglages.".into())
}

pub(crate) fn load_settings(state: &AppState, repository_id: u64) -> Result<Settings, String> {
    let account = state
        .profile
        .shared_account(&format!("remote-{repository_id}"))
        .map_err(|e| e.to_string())?;
    let settings = state
        .blobs
        .load(&account)
        .map_err(|e| e.to_string())?
        .map(|value| {
            serde_json::from_str::<Settings>(value.expose_secret()).map_err(|e| e.to_string())
        })
        .transpose()?
        .unwrap_or_default();
    settings.validate().map_err(|e| e.to_string())?;
    Ok(settings)
}

fn start_host(state: &AppState, repository_id: u64, settings: Settings) -> Result<Host, String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    #[cfg(windows)]
    let endpoint = format!(r"\\.\pipe\noosphere-remote-{id}");
    #[cfg(not(windows))]
    let endpoint = state
        .profile
        .base_directory()
        .join(format!("remote-{id}.sock"))
        .to_string_lossy()
        .into_owned();
    #[cfg(windows)]
    let _ = state;
    let key = random_id().map_err(|e| e.to_string())?;
    let bootstrap = Bootstrap {
        endpoint: endpoint.clone(),
        key,
        settings,
    };
    let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&bootstrap).map_err(|e| e.to_string())?);
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .arg("--remote-host")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x08000000);
    }
    let child = command.spawn().map_err(|e| e.to_string())?;
    let mut host = Host {
        child,
        endpoint,
        key,
        repository_id,
    };
    let mut input = host
        .child
        .stdin
        .take()
        .ok_or("Impossible de démarrer l’hôte.")?;
    input
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| input.write_all(&bytes))
        .map_err(|e| e.to_string())?;
    Ok(host)
}

impl RemoteAccess {
    pub(crate) async fn status(&self, state: &AppState) -> Result<Status, String> {
        let repository_id = repository_id(state).await?;
        let mut guard = self.process.lock().await;
        let restart = match guard.as_mut() {
            Some(host) => {
                host.repository_id != repository_id
                    || host.child.try_wait().map_err(|e| e.to_string())?.is_some()
            }
            None => true,
        };
        if restart {
            if let Some(host) = guard.as_mut() {
                host.stop().await?;
            }
            *guard = None;
            *guard = Some(start_host(
                state,
                repository_id,
                load_settings(state, repository_id)?,
            )?);
        }
        let host = guard.as_ref().ok_or("Hôte indisponible.")?;
        let mut last_error = String::new();
        for _ in 0..30 {
            match daemon::ipc::request(&host.endpoint, &host.key, daemon::Command::Status).await {
                Ok(status) => return Ok(status),
                Err(error) => last_error = error.to_string(),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        *guard = None;
        Err(last_error)
    }
}

async fn credentials(
    state: &AppState,
) -> Result<noosphere_remote::diagnostic::Credentials, String> {
    let machine_id = machine_id(state)?;
    let identity = state.identity.read().await;
    let identity = identity
        .as_ref()
        .ok_or("Identité Noosphere indisponible.")?;
    crate::protocol::remote_credentials(&identity.secret, *machine_id.as_bytes())
        .map_err(|e| e.to_string())
}

pub(crate) async fn prepare_guest(
    state: &AppState,
    remote: &RemoteAccess,
    host: Principal,
    permissions: Permissions,
) -> Result<GuestHello, String> {
    remote.status(state).await?;
    let credentials = credentials(state).await?;
    let guard = remote.process.lock().await;
    let daemon = guard.as_ref().ok_or("Hôte local indisponible.")?;
    let result: Result<GuestHello, String> = daemon::ipc::request(
        &daemon.endpoint,
        &daemon.key,
        daemon::Command::PrepareGuest {
            credentials,
            host,
            permissions,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    result
}

pub(crate) async fn start_live_host(
    state: &AppState,
    remote: &RemoteAccess,
    hello: GuestHello,
    permissions: Permissions,
) -> Result<HostOffer, String> {
    let status = remote.status(state).await?;
    if !status.streaming_ready {
        return Err(status
            .unavailable
            .first()
            .cloned()
            .unwrap_or_else(|| "Capture vidéo indisponible.".into()));
    }
    let credentials = credentials(state).await?;
    let guard = remote.process.lock().await;
    let daemon = guard.as_ref().ok_or("Hôte local indisponible.")?;
    let result: Result<HostOffer, String> = daemon::ipc::request(
        &daemon.endpoint,
        &daemon.key,
        daemon::Command::StartHost {
            settings: status.settings,
            credentials,
            hello,
            permissions,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    result
}

pub(crate) async fn connect_live_guest(
    state: &AppState,
    remote: &RemoteAccess,
    offer: HostOffer,
) -> Result<(), String> {
    remote.status(state).await?;
    let guard = remote.process.lock().await;
    let daemon = guard.as_ref().ok_or("Hôte local indisponible.")?;
    let result: Result<(), String> = daemon::ipc::request(
        &daemon.endpoint,
        &daemon.key,
        daemon::Command::ConnectGuest { offer },
    )
    .await
    .map_err(|e| e.to_string())?;
    result
}

pub(crate) async fn stop_live(state: &AppState, remote: &RemoteAccess) -> Result<(), String> {
    remote.status(state).await?;
    let guard = remote.process.lock().await;
    let daemon = guard.as_ref().ok_or("Hôte local indisponible.")?;
    daemon::ipc::request::<bool>(&daemon.endpoint, &daemon.key, daemon::Command::StopSession)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn stop_live_if_running(remote: &RemoteAccess) -> Result<(), String> {
    let guard = remote.process.lock().await;
    let Some(daemon) = guard.as_ref() else {
        return Ok(());
    };
    daemon::ipc::request::<bool>(&daemon.endpoint, &daemon.key, daemon::Command::StopSession)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn remote_status(
    state: State<'_, AppState>,
    remote: State<'_, RemoteAccess>,
) -> Result<Status, String> {
    let mut status = remote.status(&state).await?;
    let machine_id = machine_id(&state)?;
    if let Some(identity) = state.identity.read().await.as_ref() {
        status.owner = Some(
            crate::protocol::remote_identity(&identity.secret)
                .map_err(|e| e.to_string())?
                .principal(*machine_id.as_bytes())
                .map_err(|e| e.to_string())?,
        );
    }
    Ok(status)
}

pub(crate) fn machine_id(state: &AppState) -> Result<uuid::Uuid, String> {
    // Separate from account locks: several signed-in accounts share this device.
    let lock_path = state.profile.base_directory().join("remote-machine.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    fs2::FileExt::lock_exclusive(&lock).map_err(|e| e.to_string())?;
    let account = state
        .profile
        .shared_account("remote-machine")
        .map_err(|e| e.to_string())?;
    let machine_id = match state.blobs.load(&account).map_err(|e| e.to_string())? {
        Some(value) => uuid::Uuid::parse_str(value.expose_secret()).map_err(|e| e.to_string())?,
        None => {
            let id = uuid::Uuid::new_v4();
            state
                .blobs
                .save(&account, SecretString::from(id.to_string()))
                .map_err(|e| e.to_string())?;
            id
        }
    };
    Ok(machine_id)
}

#[tauri::command]
pub async fn remote_save_settings(
    state: State<'_, AppState>,
    remote: State<'_, RemoteAccess>,
    settings: Settings,
) -> Result<Status, String> {
    settings.validate().map_err(|e| e.to_string())?;
    let id = repository_id(&state).await?;
    let account = state
        .profile
        .shared_account(&format!("remote-{id}"))
        .map_err(|e| e.to_string())?;
    state
        .blobs
        .save(
            &account,
            SecretString::from(serde_json::to_string(&settings).map_err(|e| e.to_string())?),
        )
        .map_err(|e| e.to_string())?;
    {
        let mut guard = remote.process.lock().await;
        if let Some(host) = guard.as_mut() {
            host.stop().await?;
        }
        *guard = None;
    }
    remote.status(&state).await
}

#[tauri::command]
pub async fn remote_start_local_test(
    state: State<'_, AppState>,
    remote: State<'_, RemoteAccess>,
    settings: Settings,
) -> Result<(), String> {
    settings.validate().map_err(|e| e.to_string())?;
    let _operation = state.operations.lock().await;
    let status = remote_status(state.clone(), remote.clone()).await?;
    let owner = status.owner.ok_or("Identité Noosphere indisponible.")?;
    let identity = state.identity.read().await;
    let identity = identity
        .as_ref()
        .ok_or("Identité Noosphere indisponible.")?;
    let credentials = crate::protocol::remote_credentials(&identity.secret, owner.machine_id)
        .map_err(|e| e.to_string())?;
    let guard = remote.process.lock().await;
    let host = guard.as_ref().ok_or("Hôte indisponible.")?;
    let result: Result<(), String> = daemon::ipc::request(
        &host.endpoint,
        &host.key,
        daemon::Command::StartLocalTest {
            settings,
            credentials,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    result
}

#[tauri::command]
pub async fn remote_stop_local_test(
    state: State<'_, AppState>,
    remote: State<'_, RemoteAccess>,
) -> Result<(), String> {
    let id = repository_id(&state).await?;
    let guard = remote.process.lock().await;
    let host = guard
        .as_ref()
        .filter(|host| host.repository_id == id)
        .ok_or("Hôte indisponible.")?;
    daemon::ipc::request::<bool>(&host.endpoint, &host.key, daemon::Command::StopLocalTest)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(all(test, windows))]
mod windows_firewall_tests {
    use super::firewall_rule_name;

    #[test]
    fn firewall_rule_is_stable_for_windows_path_variants() {
        assert_eq!(
            firewall_rule_name(r"C:\Users\Alice\Noosphere.exe"),
            firewall_rule_name("c:/users/alice/noosphere.exe")
        );
    }

    #[test]
    fn portable_locations_have_distinct_rules() {
        assert_ne!(
            firewall_rule_name(r"C:\Apps\Noosphere.exe"),
            firewall_rule_name(r"D:\Portable\Noosphere.exe")
        );
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use std::path::{Path, PathBuf};
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use std::process::{Command, Stdio};
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use tauri::Manager as _;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use tauri_plugin_updater::UpdaterExt as _;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const PACKAGE_KIND_FILE: &str = "noosphere-package-kind";

#[tauri::command]
pub fn system_update_target(_app: crate::DesktopAppHandle) -> Result<Option<String>, String> {
    if cfg!(debug_assertions)
        || std::env::var("NOOSPHERE_SMOKE_TEST").as_deref() == Ok("1")
        || std::env::var("NOOSPHERE_DISABLE_UPDATES").as_deref() == Ok("1")
    {
        return Ok(None);
    }

    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    if std::env::var_os("APPIMAGE").is_some() && prepare_writable_appimage(&_app)? {
        return Ok(None);
    }

    Ok(update_target())
}

#[derive(Clone, serde::Serialize)]
#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux")),
    allow(dead_code)
)]
#[serde(rename_all = "camelCase", tag = "event")]
pub enum AppImageUpdateEvent {
    Found {
        version: String,
    },
    Started {
        version: String,
        content_length: Option<u64>,
    },
    Progress {
        version: String,
        chunk_length: usize,
    },
    Installing {
        version: String,
    },
}

#[tauri::command]
pub async fn system_install_appimage_update(
    _app: crate::DesktopAppHandle,
    _target: String,
    _on_event: tauri::ipc::Channel<AppImageUpdateEvent>,
) -> Result<Option<String>, String> {
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        if _target != "linux-x86_64-appimage" {
            return Err("Unsupported AppImage update target".to_owned());
        }
        let source = appimage_source()?;
        eprintln!(
            "[updater] checking AppImage update for {}",
            source.display()
        );
        let updater = _app
            .updater_builder()
            .target(_target)
            .executable_path(source.clone())
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|error| update_failure("configuration", error))?;
        let Some(mut update) = updater
            .check()
            .await
            .map_err(|error| update_failure("metadata", error))?
        else {
            eprintln!("[updater] AppImage is current");
            return Ok(None);
        };
        update.timeout = Some(std::time::Duration::from_secs(30 * 60));

        let version = update.version.clone();
        eprintln!("[updater] downloading AppImage {version}");
        _on_event
            .send(AppImageUpdateEvent::Found {
                version: version.clone(),
            })
            .map_err(|error| error.to_string())?;

        let progress_events = _on_event.clone();
        let progress_version = version.clone();
        let mut started = false;
        let mut pending_chunk_length = 0usize;
        let mut last_progress = std::time::Instant::now();
        let bytes = update
            .download(
                move |chunk_length, content_length| {
                    if !started {
                        started = true;
                        let _ = progress_events.send(AppImageUpdateEvent::Started {
                            version: progress_version.clone(),
                            content_length,
                        });
                    }
                    pending_chunk_length = pending_chunk_length.saturating_add(chunk_length);
                    if last_progress.elapsed() >= std::time::Duration::from_millis(500) {
                        let _ = progress_events.send(AppImageUpdateEvent::Progress {
                            version: progress_version.clone(),
                            chunk_length: pending_chunk_length,
                        });
                        pending_chunk_length = 0;
                        last_progress = std::time::Instant::now();
                    }
                },
                move || {
                    eprintln!("[updater] AppImage download complete");
                },
            )
            .await
            .map_err(|error| update_failure("download", error))?;
        _on_event
            .send(AppImageUpdateEvent::Installing {
                version: version.clone(),
            })
            .map_err(|error| error.to_string())?;
        install_appimage_update(&source, &bytes)
            .map_err(|error| update_failure("installation", error))?;
        eprintln!("[updater] AppImage {version} installed");
        return Ok(Some(version));
    }

    #[allow(unreachable_code)]
    Err("AppImage updates are unavailable on this platform".to_owned())
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn update_failure(stage: &str, error: impl std::fmt::Display) -> String {
    let message = error.to_string();
    eprintln!("[updater] {stage} failed: {message}");
    message
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn install_appimage_update(destination: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;

    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "AppImage destination has no parent directory",
        )
    })?;
    let temporary = parent.join(format!(".Noosphere-update-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        output.write_all(bytes)?;
        output.sync_all()?;

        let mut permissions = std::fs::metadata(destination)?.permissions();
        permissions.set_mode(permissions.mode() | 0o100);
        std::fs::set_permissions(&temporary, permissions)?;
        std::fs::rename(&temporary, destination)?;
        std::fs::File::open(parent)?.sync_all()
    })();
    let _ = std::fs::remove_file(temporary);
    result
}

#[tauri::command]
pub fn system_relaunch_after_update(_app: crate::DesktopAppHandle) -> Result<bool, String> {
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        let app_dir = std::env::var_os("APPDIR");
        if !uses_appimage_run(app_dir.as_deref()) {
            return Ok(false);
        }

        let app_image = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.is_file())
            .ok_or_else(|| "AppImage source unavailable".to_owned())?;
        let runner = find_appimage_run().ok_or_else(|| "appimage-run unavailable".to_owned())?;
        launch_appimage(&runner, &app_image)?;
        _app.exit(0);
        return Ok(true);
    }

    #[allow(unreachable_code)]
    Ok(false)
}

#[cfg(all(target_arch = "x86_64", windows))]
fn update_target() -> Option<String> {
    Some("windows-x86_64-nsis".into())
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn prepare_writable_appimage(app: &crate::DesktopAppHandle) -> Result<bool, String> {
    let source = appimage_source()?;

    if appimage_directory_is_writable(&source) {
        return Ok(false);
    }

    let managed_directory = app
        .path()
        .app_data_dir()
        .map_err(|_| "AppImage user directory unavailable".to_owned())?;
    std::fs::create_dir_all(&managed_directory)
        .map_err(|_| "AppImage user directory could not be created".to_owned())?;
    let managed =
        managed_directory.join(format!("Noosphere-{}.AppImage", env!("CARGO_PKG_VERSION")));
    if !managed.is_file() {
        install_managed_copy(&source, &managed)?;
    }

    let runner = find_appimage_run().ok_or_else(|| "appimage-run unavailable".to_owned())?;
    launch_appimage(&runner, &managed)?;
    app.exit(0);
    Ok(true)
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn appimage_source() -> Result<PathBuf, String> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .ok_or_else(|| "AppImage source unavailable".to_owned())
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn launch_appimage(runner: &Path, app_image: &Path) -> Result<(), String> {
    let unit = format!("noosphere-update-{}", uuid::Uuid::new_v4());
    let mut detached = Command::new("systemd-run");
    detached
        .args(["--user", "--quiet", "--collect", "--on-active=1s", "--unit"])
        .arg(unit);
    for variable in [
        "DBUS_SESSION_BUS_ADDRESS",
        "DISPLAY",
        "HOME",
        "PATH",
        "WAYLAND_DISPLAY",
        "XDG_CURRENT_DESKTOP",
        "XDG_RUNTIME_DIR",
    ] {
        if let Some(value) = std::env::var_os(variable) {
            detached
                .arg("--setenv")
                .arg(format!("{variable}={}", value.to_string_lossy()));
        }
    }
    let detached_started = detached
        .arg(runner)
        .arg(app_image)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if detached_started {
        return Ok(());
    }

    Command::new(runner)
        .arg(app_image)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| "appimage-run could not restart Noosphere".to_owned())
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn appimage_directory_is_writable(app_image: &Path) -> bool {
    use std::io::Write as _;

    let Some(parent) = app_image.parent() else {
        return false;
    };
    let probe = parent.join(format!(".noosphere-update-{}", uuid::Uuid::new_v4()));
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .and_then(|mut file| file.write_all(b"update"));
    let _ = std::fs::remove_file(probe);
    result.is_ok()
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn install_managed_copy(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;

    let parent = destination
        .parent()
        .ok_or_else(|| "AppImage user directory unavailable".to_owned())?;
    let temporary = parent.join(format!(".Noosphere.AppImage-{}", std::process::id()));
    let copy_result = (|| {
        std::fs::copy(source, &temporary)?;
        let mut permissions = std::fs::metadata(&temporary)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&temporary, permissions)?;
        std::fs::rename(&temporary, destination)
    })();
    let _ = std::fs::remove_file(&temporary);
    copy_result.map_err(|_| "AppImage could not be installed in the user directory".to_owned())
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn update_target() -> Option<String> {
    if std::env::var_os("APPIMAGE").is_some() {
        return Some("linux-x86_64-appimage".into());
    }

    package_kind_paths()
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|kind| linux_target_for_kind(kind.trim()))
        .map(str::to_owned)
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn package_kind_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(app_dir) = std::env::var_os("APPDIR") {
        paths.push(
            PathBuf::from(app_dir)
                .join("usr/lib/Noosphere")
                .join(PACKAGE_KIND_FILE),
        );
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(binary_directory) = executable.parent()
    {
        paths.push(
            binary_directory
                .join("../lib/Noosphere")
                .join(PACKAGE_KIND_FILE),
        );
    }
    paths.push(Path::new("/usr/lib/Noosphere").join(PACKAGE_KIND_FILE));
    paths
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn linux_target_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "appimage" => Some("linux-x86_64-appimage"),
        "deb" => Some("linux-x86_64-deb"),
        "rpm" => Some("linux-x86_64-rpm"),
        _ => None,
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn uses_appimage_run(app_dir: Option<&std::ffi::OsStr>) -> bool {
    app_dir.is_some_and(|value| {
        Path::new(value)
            .components()
            .any(|component| component.as_os_str() == "appimage-run")
    })
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn find_appimage_run() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;

    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join("appimage-run"))
        .find(|candidate| {
            candidate.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
}

#[cfg(not(any(
    all(target_arch = "x86_64", windows),
    all(target_arch = "x86_64", target_os = "linux")
)))]
fn update_target() -> Option<String> {
    None
}

#[cfg(all(test, target_arch = "x86_64", target_os = "linux"))]
mod tests {
    use std::ffi::OsStr;

    use super::{
        appimage_directory_is_writable, install_appimage_update, install_managed_copy,
        linux_target_for_kind, uses_appimage_run,
    };

    #[test]
    fn maps_each_linux_package_to_its_release_target() {
        assert_eq!(
            linux_target_for_kind("appimage"),
            Some("linux-x86_64-appimage")
        );
        assert_eq!(linux_target_for_kind("deb"), Some("linux-x86_64-deb"));
        assert_eq!(linux_target_for_kind("rpm"), Some("linux-x86_64-rpm"));
        assert_eq!(linux_target_for_kind("unknown"), None);
    }

    #[test]
    fn detects_the_nixos_appimage_runner_cache() {
        assert!(uses_appimage_run(Some(OsStr::new(
            "/home/user/.cache/appimage-run/0123456789"
        ))));
        assert!(!uses_appimage_run(Some(OsStr::new(
            "/tmp/.mount_Noosphere/usr"
        ))));
        assert!(!uses_appimage_run(None));
    }

    #[test]
    fn prepares_a_writable_managed_appimage() {
        let writable = tempfile::tempdir().unwrap();
        let source = writable.path().join("source.AppImage");
        let managed = writable.path().join("managed").join("Noosphere.AppImage");
        std::fs::write(&source, b"appimage").unwrap();
        std::fs::create_dir_all(managed.parent().unwrap()).unwrap();

        install_managed_copy(&source, &managed).unwrap();

        assert_eq!(std::fs::read(&managed).unwrap(), b"appimage");
        assert!(appimage_directory_is_writable(&managed));
    }

    #[test]
    fn replaces_an_appimage_beside_its_temporary_file() {
        use std::os::unix::fs::PermissionsExt as _;

        let writable = tempfile::tempdir().unwrap();
        let appimage = writable.path().join("Noosphere.AppImage");
        std::fs::write(&appimage, b"old version").unwrap();
        std::fs::set_permissions(&appimage, std::fs::Permissions::from_mode(0o755)).unwrap();

        install_appimage_update(&appimage, b"new version").unwrap();

        assert_eq!(std::fs::read(&appimage).unwrap(), b"new version");
        assert_ne!(
            std::fs::metadata(&appimage).unwrap().permissions().mode() & 0o100,
            0
        );
        assert_eq!(std::fs::read_dir(writable.path()).unwrap().count(), 1);
    }
}

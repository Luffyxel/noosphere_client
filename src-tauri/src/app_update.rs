#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use std::path::{Path, PathBuf};
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use std::process::{Command, Stdio};

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const PACKAGE_KIND_FILE: &str = "noosphere-package-kind";

#[tauri::command]
pub fn system_update_target() -> Option<String> {
    if cfg!(debug_assertions)
        || std::env::var("NOOSPHERE_SMOKE_TEST").as_deref() == Ok("1")
        || std::env::var("NOOSPHERE_DISABLE_UPDATES").as_deref() == Ok("1")
    {
        return None;
    }

    update_target()
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
        Command::new(runner)
            .arg(app_image)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "appimage-run could not restart Noosphere".to_owned())?;
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

    use super::{linux_target_for_kind, uses_appimage_run};

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
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use std::path::{Path, PathBuf};

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

#[cfg(not(any(
    all(target_arch = "x86_64", windows),
    all(target_arch = "x86_64", target_os = "linux")
)))]
fn update_target() -> Option<String> {
    None
}

#[cfg(all(test, target_arch = "x86_64", target_os = "linux"))]
mod tests {
    use super::linux_target_for_kind;

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
}

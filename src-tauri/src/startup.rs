#[cfg(any(target_os = "linux", test))]
use std::collections::BTreeSet;

pub fn version() -> String {
    format!(
        "Noosphere {} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("NOOSPHERE_BUILD_COMMIT").unwrap_or("unknown")
    )
}

pub fn verbose() -> bool {
    std::env::var("NOOSPHERE_VERBOSE").as_deref() == Ok("1")
}

pub fn log() {
    if std::env::args().any(|argument| argument == "--verbose") {
        unsafe { std::env::set_var("NOOSPHERE_VERBOSE", "1") };
    }
    let subprocess =
        std::env::args().find_map(|argument| argument.strip_prefix("--type=").map(str::to_owned));
    if subprocess.is_some() && !verbose() {
        return;
    }
    eprintln!(
        "[startup] {}",
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "commit": option_env!("NOOSPHERE_BUILD_COMMIT").unwrap_or("unknown"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "pid": std::process::id(),
            "process": subprocess.as_deref().unwrap_or("desktop"),
            "debug": cfg!(debug_assertions),
            "verbose": verbose(),
            "executable": std::env::current_exe().ok().map(|path| path.display().to_string()),
            "appImage": std::env::var_os("APPIMAGE").map(|path| path.to_string_lossy().into_owned()),
            "appDir": std::env::var_os("APPDIR").map(|path| path.to_string_lossy().into_owned()),
            "session": std::env::var("XDG_SESSION_TYPE").ok(),
            "desktop": std::env::var("XDG_CURRENT_DESKTOP").ok(),
            "engine": if cfg!(target_os = "linux") { "CEF/Chromium" } else { "WebView" },
        })
    );
    if !verbose() {
        return;
    }
    for name in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "GDK_BACKEND",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "__GLX_VENDOR_LIBRARY_NAME",
        "__EGL_VENDOR_LIBRARY_DIRS",
        "__EGL_VENDOR_LIBRARY_FILENAMES",
        "LIBGL_ALWAYS_SOFTWARE",
        "LIBGL_DRIVERS_PATH",
        "LIBVA_DRIVERS_PATH",
        "DRI_PRIME",
        "MESA_LOADER_DRIVER_OVERRIDE",
        "EGL_PLATFORM",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "GST_PLUGIN_SCANNER",
        "XKB_CONFIG_ROOT",
    ] {
        if let Ok(value) = std::env::var(name) {
            eprintln!("[startup] inheritedEnvironment {name}={value:?}");
        }
    }
    #[cfg(target_os = "linux")]
    if let Ok(kernel) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        eprintln!("[startup] kernel {}", kernel.trim());
    }
    #[cfg(target_os = "linux")]
    if let Ok(cards) = std::fs::read_dir("/sys/class/drm") {
        for card in cards.flatten() {
            let name = card.file_name().to_string_lossy().into_owned();
            if !name.strip_prefix("card").is_some_and(|index| {
                !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
            }) {
                continue;
            }
            let device = card.path().join("device");
            let identifier = |file| {
                std::fs::read_to_string(device.join(file))
                    .ok()
                    .map(|value| value.trim().to_owned())
            };
            let driver = std::fs::read_link(device.join("driver"))
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                });
            eprintln!(
                "[startup] gpu {}",
                serde_json::json!({"card": name, "vendor": identifier("vendor"), "device": identifier("device"), "driver": driver})
            );
        }
    }
    #[cfg(target_os = "linux")]
    if let Ok(distribution) = std::fs::read_to_string("/etc/os-release")
        && let Some(line) = distribution
            .lines()
            .find(|line| line.starts_with("PRETTY_NAME="))
    {
        eprintln!("[startup] distribution {}", &line[12..]);
    }
    #[cfg(target_os = "linux")]
    if let Ok(maps) = std::fs::read_to_string("/proc/self/maps") {
        for library in loaded_libraries(&maps) {
            eprintln!("[startup] loadedLibrary {library:?}");
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn loaded_libraries(maps: &str) -> BTreeSet<String> {
    maps.lines()
        .filter_map(|line| {
            let path = line
                .split_whitespace()
                .skip(5)
                .collect::<Vec<_>>()
                .join(" ");
            let name = path.rsplit('/').next()?;
            [
                "libcef.so",
                "libstdc++.so",
                "libgcc_s.so",
                "libGL",
                "libEGL",
                "libgbm.so",
                "libfontconfig.so",
                "libfreetype.so",
                "libxkbcommon",
                "libssl.so",
                "libcrypto.so",
                "liblzma.so",
                "libX11.so",
                "libwayland",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix))
            .then_some(path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_unique_library_paths_including_deleted_mounts() {
        let maps = "7f00-7f01 r--p 0000 00:01 42 /tmp/.mount_old/usr/lib/libstdc++.so.6 (deleted)\n7f01-7f02 r-xp 0000 00:01 42 /tmp/.mount_old/usr/lib/libstdc++.so.6 (deleted)\n7f02-7f03 r-xp 0000 00:02 43 /usr/lib/libfontconfig.so.1\n7f03-7f04 rw-p 0000 00:00 0 [heap]\n";
        assert_eq!(
            loaded_libraries(maps),
            BTreeSet::from([
                "/tmp/.mount_old/usr/lib/libstdc++.so.6 (deleted)".to_owned(),
                "/usr/lib/libfontconfig.so.1".to_owned(),
            ])
        );
    }
}

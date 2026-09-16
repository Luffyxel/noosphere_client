use std::sync::atomic::{AtomicBool, Ordering};

pub const SERVICE_ARGUMENT: &str = "--remote-service";

pub struct BackgroundHost {
    keep_alive: AtomicBool,
}

impl BackgroundHost {
    pub fn new(service_mode: bool) -> Self {
        Self {
            keep_alive: AtomicBool::new(service_mode),
        }
    }

    pub fn enabled(&self) -> bool {
        self.keep_alive.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.keep_alive.store(enabled, Ordering::Release);
    }
}

pub fn service_mode() -> bool {
    std::env::args().any(|argument| argument == SERVICE_ARGUMENT)
}

pub fn configure(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    return windows::configure(enabled);
    #[cfg(target_os = "linux")]
    return linux::configure(enabled);
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = enabled;
        Err("Le démarrage automatique n’est pas disponible sur cette plateforme.".into())
    }
}

fn startup_executable() -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let path = std::path::PathBuf::from(appimage);
        if path.is_absolute() && path.is_file() {
            return Ok(path);
        }
    }
    std::env::current_exe().map_err(|error| error.to_string())
}

#[cfg(windows)]
mod windows {
    use std::process::{Command, Stdio};

    use super::{SERVICE_ARGUMENT, startup_executable};

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "Noosphere Remote";

    pub fn configure(enabled: bool) -> Result<(), String> {
        let system = system_directory()?;
        let reg = system.join("reg.exe");
        let mut command = Command::new(reg);
        if enabled {
            let executable = startup_executable()?;
            let value = windows_command(&executable)?;
            command.args([
                "add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &value, "/f",
            ]);
        } else {
            command.args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"]);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x08000000);
        let status = command.status().map_err(|error| error.to_string())?;
        if status.success() || !enabled && status.code() == Some(1) {
            Ok(())
        } else {
            Err("Windows n’a pas pu modifier le démarrage automatique de Noosphere.".into())
        }
    }

    fn windows_command(executable: &std::path::Path) -> Result<String, String> {
        let executable = executable
            .to_str()
            .filter(|value| !value.contains(['\r', '\n', '"']))
            .ok_or("Le chemin de Noosphere ne peut pas être enregistré.")?;
        Ok(format!("\"{executable}\" {SERVICE_ARGUMENT}"))
    }

    fn system_directory() -> Result<std::path::PathBuf, String> {
        use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
        let mut buffer = vec![0_u16; 32_768];
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 || length as usize >= buffer.len() {
            return Err("Le dossier système Windows est inaccessible.".into());
        }
        buffer.truncate(length as usize);
        use std::os::windows::ffi::OsStringExt as _;
        Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
            &buffer,
        )))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn startup_command_preserves_spaces() {
            assert_eq!(
                windows_command(std::path::Path::new(
                    r"C:\Program Files\Noosphere\Noosphere.exe"
                ))
                .unwrap(),
                r#""C:\Program Files\Noosphere\Noosphere.exe" --remote-service"#
            );
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::{Command, Stdio},
    };

    use super::{SERVICE_ARGUMENT, startup_executable};

    const UNIT_NAME: &str = "noosphere-remote.service";

    pub fn configure(enabled: bool) -> Result<(), String> {
        let unit = unit_path()?;
        let systemctl = command_path("systemctl")
            .ok_or("systemd utilisateur est indisponible sur cette session.")?;
        if enabled {
            let executable = startup_executable()?;
            let launcher = is_nixos()
                .then(|| command_path("appimage-run"))
                .flatten()
                .filter(|_| std::env::var_os("APPIMAGE").is_some());
            write_unit(&unit, &service_unit(&executable, launcher.as_deref())?)?;
            run(&systemctl, &["--user", "daemon-reload"])?;
            import_session_environment(&systemctl);
            run(&systemctl, &["--user", "enable", UNIT_NAME])?;
        } else {
            let _ = run(&systemctl, &["--user", "disable", UNIT_NAME]);
            if unit.exists() {
                fs::remove_file(&unit).map_err(|error| error.to_string())?;
            }
            run(&systemctl, &["--user", "daemon-reload"])?;
        }
        Ok(())
    }

    fn unit_path() -> Result<PathBuf, String> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .map(|home| home.join(".config"))
            })
            .ok_or("Le dossier de configuration utilisateur est introuvable.")?;
        Ok(config.join("systemd/user").join(UNIT_NAME))
    }

    fn service_unit(executable: &Path, launcher: Option<&Path>) -> Result<String, String> {
        let executable = executable
            .to_str()
            .filter(|value| !value.contains(['\r', '\n']))
            .ok_or("Le chemin de Noosphere ne peut pas être enregistré.")?;
        let command = match launcher {
            Some(launcher) => {
                let launcher = launcher
                    .to_str()
                    .filter(|value| !value.contains(['\r', '\n']))
                    .ok_or("Le lanceur AppImage ne peut pas être enregistré.")?;
                format!("{} {}", systemd_quote(launcher), systemd_quote(executable))
            }
            None => systemd_quote(executable),
        };
        Ok(format!(
            "[Unit]\nDescription=Noosphere Remote host\nAfter=graphical-session.target network-online.target\n\n[Service]\nType=simple\nExecStart={} {}\nRestart=on-failure\nRestartSec=3\n\n[Install]\nWantedBy=default.target\n",
            command, SERVICE_ARGUMENT
        ))
    }

    fn is_nixos() -> bool {
        fs::read_to_string("/etc/os-release")
            .ok()
            .is_some_and(|contents| {
                contents
                    .lines()
                    .any(|line| line.trim().eq_ignore_ascii_case("ID=nixos"))
            })
    }

    fn systemd_quote(value: &str) -> String {
        format!(
            "\"{}\"",
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('%', "%%")
        )
    }

    fn write_unit(path: &Path, contents: &str) -> Result<(), String> {
        let parent = path.parent().ok_or("Chemin systemd invalide.")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = path.with_extension("service.tmp");
        fs::write(&temporary, contents).map_err(|error| error.to_string())?;
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
        fs::rename(temporary, path).map_err(|error| error.to_string())
    }

    fn import_session_environment(systemctl: &Path) {
        let names = [
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_CURRENT_DESKTOP",
            "HYPRLAND_INSTANCE_SIGNATURE",
        ];
        let present = names
            .into_iter()
            .filter(|name| std::env::var_os(name).is_some())
            .collect::<Vec<_>>();
        if present.is_empty() {
            return;
        }
        let mut arguments = vec!["--user", "import-environment"];
        arguments.extend(present);
        let _ = run(systemctl, &arguments);
    }

    fn run(program: &Path, arguments: &[&str]) -> Result<(), String> {
        let status = Command::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("systemd n’a pas pu modifier le service utilisateur Noosphere.".into())
        }
    }

    fn command_path(name: &str) -> Option<PathBuf> {
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .chain([
                PathBuf::from("/run/current-system/sw/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ])
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn appimage_path_is_quoted_and_specifiers_are_escaped() {
            let unit =
                service_unit(Path::new("/home/alice/Apps/Noosphere 100%.AppImage"), None).unwrap();
            assert!(unit.contains(
                "ExecStart=\"/home/alice/Apps/Noosphere 100%%.AppImage\" --remote-service"
            ));
            assert!(unit.contains("WantedBy=default.target"));
        }

        #[test]
        fn nixos_service_uses_the_appimage_runner() {
            let unit = service_unit(
                Path::new("/home/alice/.local/share/noosphere/Noosphere.AppImage"),
                Some(Path::new("/run/current-system/sw/bin/appimage-run")),
            )
            .unwrap();
            assert!(unit.contains(
                "ExecStart=\"/run/current-system/sw/bin/appimage-run\" \"/home/alice/.local/share/noosphere/Noosphere.AppImage\" --remote-service"
            ));
        }
    }
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    if let Some(status) = noosphere_core::appimage_relaunch_helper_status() {
        std::process::exit(status);
    }
    #[cfg(target_os = "linux")]
    noosphere_core::prepare_linux_media_runtime();
    let mode = std::env::args().nth(1);
    #[cfg(target_os = "linux")]
    reexec_linux_media_probe_with_host_libraries(mode.as_deref());
    #[cfg(windows)]
    if mode.as_deref() == Some("--remote-firewall-install") {
        std::process::exit(noosphere_core::run_firewall_installer());
    }
    if mode.as_deref() == Some("--remote-host") {
        if noosphere_remote::daemon::run_from_parent().is_err() {
            std::process::exit(1);
        }
        return;
    }
    #[cfg(target_os = "linux")]
    if mode.as_deref() == Some("--remote-linux-media-probe") {
        let probe = noosphere_remote::capture::linux::probe();
        println!(
            "{}",
            serde_json::json!({
                "ready": probe.ready,
                "encoder": probe.encoder,
                "hardware": probe.hardware,
                "unavailable": probe.unavailable,
                "libvaDriversPath": std::env::var("LIBVA_DRIVERS_PATH").unwrap_or_default(),
                "gstRegistry": std::env::var("GST_REGISTRY").unwrap_or_default(),
            })
        );
        if !probe.ready {
            std::process::exit(1);
        }
        return;
    }
    #[cfg(target_os = "linux")]
    if mode.as_deref() == Some("--remote-linux-capture-probe") {
        match noosphere_remote::diagnostic::linux_live_capture_probe_json() {
            Ok(report) => println!("{report}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    noosphere_core::run();
}

#[cfg(target_os = "linux")]
fn reexec_linux_media_probe_with_host_libraries(mode: Option<&str>) {
    use std::os::unix::process::CommandExt as _;

    if !matches!(
        mode,
        Some("--remote-linux-media-probe" | "--remote-linux-capture-probe")
    ) || std::env::var_os("NOOSPHERE_REMOTE_PRELOAD_READY").is_some()
    {
        return;
    }
    let Some(preload) = std::env::var_os("NOOSPHERE_REMOTE_LD_PRELOAD") else {
        return;
    };
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let error = std::process::Command::new(executable)
        .args(std::env::args_os().skip(1))
        .env("LD_PRELOAD", preload)
        .env("NOOSPHERE_REMOTE_PRELOAD_READY", "1")
        .exec();
    eprintln!("Linux media diagnostic restart failed: {error}");
}

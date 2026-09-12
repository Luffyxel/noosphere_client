#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    noosphere_core::prepare_linux_media_runtime();
    let mode = std::env::args().nth(1);
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

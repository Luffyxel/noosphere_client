#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if noosphere_remote::daemon::run_from_parent().is_err() {
        std::process::exit(1);
    }
}

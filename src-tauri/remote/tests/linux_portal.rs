#![cfg(target_os = "linux")]

use ashpd::desktop::screencast::{Screencast, SourceType};

#[tokio::test]
async fn active_wayland_session_exposes_a_monitor_screencast_portal() {
    if std::env::var_os("NOOSPHERE_REMOTE_REQUIRE_PORTAL").is_none() {
        return;
    }
    let portal = Screencast::new().await.unwrap();
    let sources = portal.available_source_types().await.unwrap();
    assert!(sources.contains(SourceType::Monitor));
    assert!(portal.version() >= 1);
}

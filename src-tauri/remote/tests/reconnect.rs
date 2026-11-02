#![cfg(target_os = "linux")]

use noosphere_remote::{
    daemon::Settings,
    diagnostic::Credentials,
    identity::SignalIdentity,
    live::{State, Worker},
    permissions::Permissions,
};
use rand::{TryRngCore as _, rngs::OsRng};
use std::time::{Duration, Instant};

fn credentials(machine: u8) -> Credentials {
    let pair = libsignal_protocol::KeyPair::generate(&mut OsRng.unwrap_err());
    Credentials {
        github_user_id: 42,
        private_key: pair.private_key.serialize().to_vec(),
        machine_id: [machine; 16],
    }
}

fn wait_for(worker: &Worker, predicate: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = worker.state();
        if predicate(&state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "session did not settle: {state:?}"
        );
        if let State::Failed { error } = state {
            panic!("session failed: {error}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_same_host_and_guest_can_disconnect_and_reconnect_repeatedly() {
    unsafe {
        std::env::set_var("NOOSPHERE_REMOTE_TEST_SOURCE", "1");
        std::env::set_var("NOOSPHERE_REMOTE_TEST_SINK", "1");
    }
    let host_credentials = credentials(1);
    let host_identity =
        SignalIdentity::from_private_key(42, &host_credentials.private_key).unwrap();
    let host_principal = host_identity.principal([1; 16]).unwrap();
    let host_key = host_credentials.private_key.clone();
    let guest_credentials = credentials(2);
    let guest_key = guest_credentials.private_key.clone();
    let permissions = Permissions {
        screen: true,
        keyboard: true,
        mouse: true,
        ..Permissions::default()
    };
    let mut settings = Settings::default();
    settings.capture_output = Some("eDP-1".into());
    settings.video.width = 640;
    settings.video.height = 360;
    settings.video.bitrate = 2_000_000;
    let mut host = Worker::default();
    let mut guest = Worker::default();
    for cycle in 0..4 {
        let hello = guest
            .prepare_guest(
                Credentials {
                    github_user_id: 42,
                    private_key: guest_key.clone(),
                    machine_id: [2; 16],
                },
                host_principal.clone(),
                permissions,
            )
            .unwrap();
        let offer = host
            .start_host(
                settings.clone(),
                Credentials {
                    github_user_id: 42,
                    private_key: host_key.clone(),
                    machine_id: [1; 16],
                },
                hello,
                permissions,
            )
            .unwrap();
        assert!(offer.settings.capture_output.is_none());
        guest.connect_guest(offer).unwrap();
        wait_for(
            &guest,
            |state| matches!(state, State::Streaming { frames, .. } if *frames >= 3),
        );
        if cycle % 2 == 0 {
            guest.stop();
        } else {
            host.stop();
        }
        wait_for(&guest, |state| {
            matches!(state, State::Idle | State::Failed { .. })
        });
        wait_for(&host, |state| {
            matches!(state, State::Idle | State::Failed { .. })
        });
    }
}

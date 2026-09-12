#![cfg(target_os = "linux")]

use noosphere_remote::{
    capture::linux::PortalEncoder,
    encoder::{Codec, VideoSettings},
    input::{Event, linux::DesktopSession},
    permissions::Permissions,
    viewer::linux::Viewer,
};
use x11rb::{connection::Connection as _, protocol::xproto::ConnectionExt as _};

#[tokio::test]
async fn x11_fallback_captures_decodes_and_injects_input() {
    if std::env::var_os("NOOSPHERE_REMOTE_REQUIRE_X11").is_none() {
        return;
    }
    assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
    let permissions = Permissions {
        keyboard: true,
        mouse: true,
        ..Permissions::default()
    };
    let (mut session, source) = DesktopSession::open(permissions).await.unwrap();
    let settings = VideoSettings {
        width: 640,
        height: 360,
        fps: 30,
        bitrate: 2_000_000,
        codec: Codec::H264,
    };
    let encoder = PortalEncoder::open(source, &settings).unwrap();
    let mut viewer = Viewer::open_headless(&settings).unwrap();
    encoder.force_keyframe();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut encoded = 0;
    while encoded < 12 && std::time::Instant::now() < deadline {
        if let Some(frame) = encoder.acquire(100).unwrap() {
            viewer.present(frame.bytes).unwrap();
            assert!(viewer.pump().unwrap());
            encoded += 1;
        }
    }
    assert_eq!(encoded, 12);
    let decode_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while viewer.decoded_frames() == 0 && std::time::Instant::now() < decode_deadline {
        assert!(viewer.pump().unwrap());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(viewer.decoded_frames() > 0);

    session
        .inject(&Event::Key {
            code: 0x41,
            pressed: true,
        })
        .await
        .unwrap();
    let (connection, _) = x11rb::connect(None).unwrap();
    let keys = connection.query_keymap().unwrap().reply().unwrap().keys;
    let keycode = 38_usize;
    assert_ne!(keys[keycode / 8] & (1 << (keycode % 8)), 0);
    session
        .inject(&Event::Key {
            code: 0x41,
            pressed: false,
        })
        .await
        .unwrap();
    session
        .inject(&Event::Pointer { x: 32767, y: 32767 })
        .await
        .unwrap();
    let root = connection.setup().roots[0].root;
    let pointer = connection.query_pointer(root).unwrap().reply().unwrap();
    assert!((315..=325).contains(&pointer.root_x));
    assert!((175..=185).contains(&pointer.root_y));
}

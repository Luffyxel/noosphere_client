#![cfg(target_os = "linux")]

use noosphere_remote::{
    capture::linux::{PortalEncoder, Source},
    encoder::{Codec, VideoSettings},
    viewer::linux::Viewer,
};

#[test]
fn synthetic_linux_pipeline_encodes_and_decodes_low_latency_frames() {
    let settings = VideoSettings {
        width: 640,
        height: 360,
        fps: 30,
        bitrate: 2_000_000,
        codec: Codec::H264,
    };
    let encoder = PortalEncoder::open(Source::test(), &settings).unwrap();
    let mut viewer = Viewer::open_headless(&settings).unwrap();
    encoder.force_keyframe();
    let mut encoded = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
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
}

#[test]
fn a_single_initial_frame_is_presented_when_the_desktop_stays_idle() {
    let settings = VideoSettings {
        width: 640,
        height: 360,
        fps: 30,
        bitrate: 2_000_000,
        codec: Codec::H264,
    };
    let encoder = PortalEncoder::open(Source::test(), &settings).unwrap();
    let mut viewer = Viewer::open_headless(&settings).unwrap();
    encoder.force_keyframe();
    let capture_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let first_frame = loop {
        if let Some(frame) = encoder.acquire(100).unwrap() {
            break frame;
        }
        assert!(std::time::Instant::now() < capture_deadline);
    };
    drop(encoder);

    viewer.present(first_frame.bytes).unwrap();
    let decode_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while viewer.decoded_frames() == 0 && std::time::Instant::now() < decode_deadline {
        assert!(viewer.pump().unwrap());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(viewer.decoded_frames() > 0);
}

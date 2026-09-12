use bytes::Bytes;
use noosphere_remote::{
    congestion::{Feedback, LowLatency, Pacer, RateController},
    transport::{assembler::Assembler, packet::packetize},
};
use serde::Serialize;

#[derive(Clone, Copy)]
struct Profile {
    name: &'static str,
    rtt_us: u64,
    jitter_us: u64,
    loss_per_10000: u64,
    initial_bps: u64,
    reduced_bps: u64,
}

#[derive(Serialize)]
struct Report {
    profile: &'static str,
    mode: &'static str,
    seed: u64,
    frames: u64,
    delivered_frames: u64,
    dropped_frames: u64,
    network_latency_p50_us: u64,
    network_latency_p95_us: u64,
    final_bitrate_bps: u64,
    encode_latency_us: Option<u64>,
    decode_latency_us: Option<u64>,
    input_to_photon_us: Option<u64>,
}

fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn run(profile: Profile, seed: u64, adaptive: bool) -> Report {
    let mut rng = seed;
    let mut controller = LowLatency::new(500_000, 12_000_000, 60_000_000).unwrap();
    let mut pacer = Pacer::default();
    let mut assembler = Assembler::default();
    let mut link_available: u64 = 0;
    let mut sequence = 0;
    let mut delivered = 0;
    let mut latencies = Vec::new();
    let mut pending = Vec::new();
    let mut sent_packets = 0;
    let mut lost_packets = 0;
    let mut delivered_bytes = 0;
    let mut last_feedback = 0;
    let mut bitrate = 12_000_000;
    const FRAMES: u64 = 1800;
    for id in 0..FRAMES {
        let now = id * 1_000_000 / 60;
        let bandwidth = if (600..1200).contains(&id) {
            profile.reduced_bps
        } else {
            profile.initial_bps
        };
        let mut arriving: Vec<(u64, noosphere_remote::transport::packet::Packet)> = Vec::new();
        pending.retain(
            |(arrival, packet): &(u64, noosphere_remote::transport::packet::Packet)| {
                if *arrival <= now {
                    arriving.push((*arrival, packet.clone()));
                    false
                } else {
                    true
                }
            },
        );
        arriving.sort_by_key(|(arrival, _)| *arrival);
        for (arrival, packet) in arriving {
            delivered_bytes += packet.payload.len() as u64;
            if let Some(frame) = assembler.receive(packet, arrival).unwrap() {
                delivered += 1;
                latencies.push(arrival - frame.captured_us);
            }
        }
        assembler.expire(now);
        if now - last_feedback >= 100_000 && sent_packets > 0 {
            let feedback = Feedback {
                rtt_us: profile.rtt_us + 2 * link_available.saturating_sub(now),
                delivered_bytes,
                interval_us: now - last_feedback,
                sent_packets,
                lost_packets,
                app_limited: false,
            };
            let next = controller.update(&feedback).unwrap();
            if adaptive {
                bitrate = next;
            }
            sent_packets = 0;
            lost_packets = 0;
            delivered_bytes = 0;
            last_feedback = now;
        }
        let frame = Bytes::from(vec![0; (bitrate / 8 / 60) as usize]);
        let deadline = now + profile.rtt_us / 2 + 35_000;
        let packets = packetize(
            frame,
            id,
            sequence,
            now,
            (deadline - now) as u32,
            true,
            1200,
        )
        .unwrap();
        sequence += packets.len() as u64;
        for packet in packets {
            sent_packets += 1;
            let size = packet.payload.len() + 44;
            let Some(send) = pacer.schedule(now, size, bitrate * 11 / 10, deadline) else {
                lost_packets += 1;
                continue;
            };
            let serialization = (size as u64 * 8_000_000).div_ceil(bandwidth);
            link_available = link_available.max(send) + serialization;
            // Bound the simulated router queue; congestion losses and independent random losses differ.
            if link_available > send + 100_000 {
                link_available -= serialization;
                lost_packets += 1;
                continue;
            }
            if random(&mut rng) % 10_000 < profile.loss_per_10000 {
                lost_packets += 1;
                continue;
            }
            let jitter = random(&mut rng) % (profile.jitter_us + 1);
            let reorder = if random(&mut rng).is_multiple_of(100) {
                10_000
            } else {
                0
            };
            let arrival = link_available + profile.rtt_us / 2 + jitter + reorder;
            pending.push((arrival, packet));
        }
    }
    pending.sort_by_key(|(arrival, _)| *arrival);
    for (arrival, packet) in pending {
        if let Some(frame) = assembler.receive(packet, arrival).unwrap() {
            delivered += 1;
            latencies.push(arrival - frame.captured_us);
        }
    }
    latencies.sort_unstable();
    let percentile = |p: usize| {
        latencies
            .get(latencies.len().saturating_sub(1) * p / 100)
            .copied()
            .unwrap_or(0)
    };
    Report {
        profile: profile.name,
        mode: if adaptive { "adaptive" } else { "fixed" },
        seed,
        frames: FRAMES,
        delivered_frames: delivered,
        dropped_frames: FRAMES - delivered,
        network_latency_p50_us: percentile(50),
        network_latency_p95_us: percentile(95),
        final_bitrate_bps: bitrate,
        encode_latency_us: None,
        decode_latency_us: None,
        input_to_photon_us: None,
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--linux-activate") {
        if let Err(error) = noosphere_remote::input::linux::probe_wlr_activate() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        println!("{{\"kind\":\"noosphere-linux-keyboard-probe\",\"activated\":true}}");
        return;
    }
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--linux-pointer-click") {
        let mut values = std::env::args().skip(2);
        let x = values.next().and_then(|value| value.parse().ok());
        let y = values.next().and_then(|value| value.parse().ok());
        let coordinates = x.zip(y).filter(|_| values.next().is_none());
        let Some((x, y)) = coordinates else {
            eprintln!("usage: --linux-pointer-click <x:0..65535> <y:0..65535>");
            std::process::exit(2);
        };
        if let Err(error) = noosphere_remote::input::linux::probe_wlr_click(x, y) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        println!(
            "{{\"kind\":\"noosphere-linux-pointer-probe\",\"clicked\":true,\"x\":{x},\"y\":{y}}}"
        );
        return;
    }
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--linux-capture-probe") {
        if let Err(error) = linux_capture_probe() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--linux-probe") {
        if let Err(error) = linux_probe() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--session") {
        use rand::{TryRngCore as _, rngs::OsRng};
        let key = libsignal_protocol::KeyPair::generate(&mut OsRng.unwrap_err());
        let identity = noosphere_remote::identity::SignalIdentity::from_private_key(
            1,
            &key.private_key.serialize(),
        )
        .unwrap();
        let mut settings = noosphere_remote::daemon::Settings::default();
        settings.video.width = 1280;
        settings.video.height = 720;
        settings.video.bitrate = 12_000_000;
        match noosphere_remote::diagnostic::run(settings, identity, [1; 16], 5, Default::default())
        {
            Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--media") {
        if let Err(error) = media_benchmark() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--capture") {
        if let Err(error) = capture_benchmark() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let profiles = [
        Profile {
            name: "fiber",
            rtt_us: 12_000,
            jitter_us: 1000,
            loss_per_10000: 1,
            initial_bps: 80_000_000,
            reduced_bps: 20_000_000,
        },
        Profile {
            name: "wifi",
            rtt_us: 25_000,
            jitter_us: 8000,
            loss_per_10000: 30,
            initial_bps: 35_000_000,
            reduced_bps: 8_000_000,
        },
        Profile {
            name: "mobile",
            rtt_us: 65_000,
            jitter_us: 20_000,
            loss_per_10000: 100,
            initial_bps: 20_000_000,
            reduced_bps: 4_000_000,
        },
    ];
    let results: Vec<_> = profiles
        .into_iter()
        .flat_map(|p| [false, true].map(|adaptive| run(p, 20260910, adaptive)))
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({ "kind": "deterministic-network-simulation", "results": results })
        )
        .unwrap()
    );
}

#[cfg(target_os = "linux")]
fn linux_capture_probe() -> noosphere_remote::Result<()> {
    println!(
        "{}",
        noosphere_remote::diagnostic::linux_live_capture_probe_json()?
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_probe() -> noosphere_remote::Result<()> {
    use ashpd::desktop::{
        remote_desktop::{DeviceType, RemoteDesktop},
        screencast::{Screencast, SourceType},
    };
    use rand::{TryRngCore as _, rngs::OsRng};

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let (portal_version, monitor, window, virtual_source, remote_input) =
        runtime.block_on(async {
            let portal = Screencast::new().await.map_err(|error| {
                noosphere_remote::Error::Unavailable(format!(
                    "XDG ScreenCast portal unavailable: {error}"
                ))
            })?;
            let sources = portal.available_source_types().await.map_err(|error| {
                noosphere_remote::Error::Unavailable(format!(
                    "XDG ScreenCast source query failed: {error}"
                ))
            })?;
            let remote_input = match RemoteDesktop::new().await {
                Ok(remote) => remote.available_device_types().await.is_ok_and(|devices| {
                    devices.contains(DeviceType::Keyboard) && devices.contains(DeviceType::Pointer)
                }),
                Err(_) => false,
            };
            Ok::<_, noosphere_remote::Error>((
                portal.version(),
                sources.contains(SourceType::Monitor),
                sources.contains(SourceType::Window),
                sources.contains(SourceType::Virtual),
                remote_input,
            ))
        })?;
    if !monitor {
        return Err(noosphere_remote::Error::Unavailable(
            "XDG ScreenCast portal does not expose monitor capture".into(),
        ));
    }
    let input_backend = if remote_input {
        "xdg-remote-desktop"
    } else {
        noosphere_remote::input::linux::probe_wlr_input()?;
        "wayland-virtual-keyboard+wlr-virtual-pointer"
    };

    let media = noosphere_remote::capture::linux::probe();
    let key = libsignal_protocol::KeyPair::generate(&mut OsRng.unwrap_err());
    let identity = noosphere_remote::identity::SignalIdentity::from_private_key(
        1,
        &key.private_key.serialize(),
    )?;
    let mut settings = noosphere_remote::daemon::Settings::default();
    settings.video.width = 640;
    settings.video.height = 360;
    settings.video.fps = 30;
    settings.video.bitrate = 4_000_000;
    let report =
        noosphere_remote::diagnostic::run(settings, identity, [0x4e; 16], 3, Default::default())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "kind": "noosphere-linux-session-probe",
            "desktop": std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            "waylandDisplay": std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
            "portal": {
                "screenCastVersion": portal_version,
                "monitor": monitor,
                "window": window,
                "virtual": virtual_source
            },
            "inputBackend": input_backend,
            "media": {
                "ready": media.ready,
                "encoder": media.encoder,
                "hardware": media.hardware,
                "unavailable": media.unavailable
            },
            "diagnostic": report
        }))
        .map_err(|_| noosphere_remote::Error::InvalidPacket)?
    );
    Ok(())
}

#[cfg(windows)]
fn media_benchmark() -> noosphere_remote::Result<()> {
    use noosphere_remote::{
        capture::{Capture as _, dxgi},
        decoder::windows::HardwareDecoder,
        encoder::{
            Codec, VideoSettings,
            windows::{HardwareEncoder, Runtime, media_error},
        },
    };
    use std::time::{Duration, Instant};
    let _runtime = Runtime::start().map_err(media_error)?;
    let displays = dxgi::displays()?;
    let display = displays
        .first()
        .ok_or(noosphere_remote::Error::Unavailable("No display".into()))?;
    let mut capture = dxgi::DesktopDuplication::open(display.id)?;
    let settings = VideoSettings {
        width: 1280,
        height: 720,
        fps: 60,
        bitrate: 12_000_000,
        codec: Codec::H264,
    };
    let mut encoder =
        HardwareEncoder::open(&capture.device, (display.width, display.height), &settings)?;
    eprintln!("Hardware encoder: {}", encoder.name);
    let mut decoder = HardwareDecoder::open(&capture.device, &settings)?;
    let start = Instant::now();
    let mut encodes = Vec::new();
    let mut decodes = Vec::new();
    let mut size = 0;
    while start.elapsed() < Duration::from_secs(5) {
        let Some(frame) = capture.acquire(16)? else {
            continue;
        };
        let before = Instant::now();
        let encoded = encoder
            .encode(
                &frame.texture,
                start.elapsed().as_micros() as i64 * 10,
                encodes.is_empty(),
            )
            .map_err(|e| {
                noosphere_remote::Error::Unavailable(format!("encode frame {}: {e}", encodes.len()))
            })?;
        encodes.push(before.elapsed().as_micros() as u64);
        size += encoded.bytes.len();
        drop(frame);
        let before = Instant::now();
        if decoder
            .decode(&encoded.bytes, encoded.timestamp)
            .map_err(|e| {
                noosphere_remote::Error::Unavailable(format!("decode frame {}: {e}", encodes.len()))
            })?
            .is_some()
        {
            decodes.push(before.elapsed().as_micros() as u64);
        }
    }
    if decodes.is_empty() {
        return Err(noosphere_remote::Error::Unavailable(
            "Hardware decoder produced no frames".into(),
        ));
    }
    encodes.sort_unstable();
    decodes.sort_unstable();
    println!(
        "{}",
        serde_json::json!({"kind": "native-gpu-media", "encoder": encoder.name, "encoded_frames": encodes.len(), "decoded_gpu_frames": decodes.len(), "encoded_bytes": size,
        "encode_p50_us": encodes[encodes.len()/2], "encode_p95_us": encodes[(encodes.len()-1)*95/100],
        "decode_p50_us": decodes[decodes.len()/2], "decode_p95_us": decodes[(decodes.len()-1)*95/100], "input_to_photon_us": null})
    );
    Ok(())
}

#[cfg(not(windows))]
fn media_benchmark() -> noosphere_remote::Result<()> {
    Err(noosphere_remote::Error::Unavailable(
        "native media benchmark requires Windows".into(),
    ))
}

#[cfg(windows)]
fn capture_benchmark() -> noosphere_remote::Result<()> {
    use noosphere_remote::capture::{Capture as _, dxgi};
    use std::time::{Duration, Instant};
    let displays = dxgi::displays()?;
    let mut capture = dxgi::DesktopDuplication::open(0)?;
    let started = Instant::now();
    let mut acquisition = Vec::new();
    while started.elapsed() < Duration::from_secs(3) {
        let before = Instant::now();
        if let Some(frame) = capture.acquire(16)? {
            acquisition.push(before.elapsed().as_micros() as u64);
            drop(frame);
        }
    }
    if acquisition.is_empty() {
        return Err(noosphere_remote::Error::Unavailable(
            "DXGI produced no frames".into(),
        ));
    }
    acquisition.sort_unstable();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "kind": "native-dxgi-capture", "duration_ms": started.elapsed().as_millis(),
            "frames": acquisition.len(), "acquire_wait_p50_us": acquisition[acquisition.len() / 2],
            "acquire_wait_p95_us": acquisition[(acquisition.len() - 1) * 95 / 100],
            "displays": displays,
            "encode_latency_us": null, "decode_latency_us": null, "input_to_photon_us": null
        }))
        .map_err(|_| noosphere_remote::Error::InvalidPacket)?
    );
    Ok(())
}

#[cfg(not(windows))]
fn capture_benchmark() -> noosphere_remote::Result<()> {
    Err(noosphere_remote::Error::Unavailable(
        "native capture benchmark requires Windows".into(),
    ))
}

//! End-to-end local diagnostic used by the app and the hardware smoke command.
use crate::{Error, Result, daemon::Settings, identity::SignalIdentity};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Credentials {
    pub github_user_id: u64,
    pub private_key: Vec<u8>,
    pub machine_id: [u8; 16],
}
impl Drop for Credentials {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.private_key.zeroize();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub encoder: String,
    pub width: u32,
    pub height: u32,
    pub encoded_frames: u64,
    pub delivered_frames: u64,
    pub presented_frames: u64,
    pub dropped_frames: u64,
    pub encode_p50_us: u64,
    pub encode_p95_us: u64,
    pub network_p50_us: u64,
    pub network_p95_us: u64,
    pub decode_submit_p50_us: u64,
    pub decode_submit_p95_us: u64,
    pub rtt_us: u64,
    pub duration_ms: u64,
    pub input_events_received: u64,
    pub input_key_observed: bool,
    pub input_mouse_observed: bool,
    pub revocation_verified: bool,
    pub input_to_photon_us: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum State {
    #[default]
    Idle,
    Running,
    Complete {
        report: Report,
    },
    Failed {
        error: String,
    },
}

#[derive(Default)]
pub struct Worker {
    state: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub fn state(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn start(&mut self, settings: Settings, credentials: Credentials) -> Result<()> {
        settings.validate()?;
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
        {
            return Err(Error::Unavailable("Un test est déjà en cours.".into()));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let identity =
            SignalIdentity::from_private_key(credentials.github_user_id, &credentials.private_key)?;
        let machine_id = credentials.machine_id;
        identity.principal(machine_id)?;
        self.cancel.store(false, Ordering::Relaxed);
        *self
            .state
            .lock()
            .map_err(|_| Error::Unavailable("diagnostic lock".into()))? = State::Running;
        let state = self.state.clone();
        let cancel = self.cancel.clone();
        self.thread = Some(std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(settings, identity, machine_id, 15, cancel)
            }));
            let next = match result {
                Ok(Ok(report)) => State::Complete { report },
                Ok(Err(error)) => State::Failed {
                    error: error.to_string(),
                },
                Err(_) => State::Failed {
                    error: "Le moteur vidéo s’est arrêté.".into(),
                },
            };
            *state.lock().unwrap_or_else(|e| e.into_inner()) = next;
        }));
        Ok(())
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(windows)]
pub fn run(
    settings: Settings,
    identity: SignalIdentity,
    machine_id: [u8; 16],
    seconds: u64,
    cancel: Arc<AtomicBool>,
) -> Result<Report> {
    settings.validate()?;
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(windows::run(
            settings,
            identity,
            machine_id,
            seconds.clamp(1, 30),
            cancel,
        ))
}
#[cfg(target_os = "linux")]
pub fn run(
    mut settings: Settings,
    identity: SignalIdentity,
    machine_id: [u8; 16],
    seconds: u64,
    cancel: Arc<AtomicBool>,
) -> Result<Report> {
    settings.video.codec = crate::encoder::Codec::H264;
    settings.validate()?;
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(linux::run(
            settings,
            identity,
            machine_id,
            seconds.clamp(1, 30),
            cancel,
        ))
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn run(
    _settings: Settings,
    _identity: SignalIdentity,
    _machine_id: [u8; 16],
    _seconds: u64,
    _cancel: Arc<AtomicBool>,
) -> Result<Report> {
    Err(Error::Unavailable(
        "Test vidéo indisponible sur cette plateforme.".into(),
    ))
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::{
        capture::{Capture as _, dxgi},
        congestion::{Feedback, LowLatency, RateController as _},
        decoder::windows::HardwareDecoder,
        encoder::{
            DecodeGate,
            windows::{HardwareEncoder, Runtime, media_error},
        },
        input::{Event, InputController, windows::TestInput},
        permissions::Permissions,
        transport::{
            assembler::Assembler,
            loopback::Loopback,
            packet::{Kind, Packet, packetize},
        },
        viewer::windows::Viewer,
    };
    use bytes::Bytes;
    use std::time::{Duration, Instant};

    fn percentile(values: &mut [u64], percent: usize) -> u64 {
        values.sort_unstable();
        values
            .get(values.len().saturating_sub(1) * percent / 100)
            .copied()
            .unwrap_or(0)
    }

    pub async fn run(
        settings: Settings,
        identity: SignalIdentity,
        machine_id: [u8; 16],
        seconds: u64,
        cancel: Arc<AtomicBool>,
    ) -> Result<Report> {
        let _runtime = Runtime::start().map_err(media_error)?;
        let display = dxgi::displays()?
            .into_iter()
            .find(|d| d.id == settings.display)
            .ok_or(Error::Unavailable("Écran introuvable.".into()))?;
        let mut capture = dxgi::DesktopDuplication::open(display.id)?;
        let mut encoder = HardwareEncoder::open(
            &capture.device,
            (display.width, display.height),
            &settings.video,
        )?;
        let mut decoder = HardwareDecoder::open(&capture.device, &settings.video)?;
        let mut viewer = Viewer::open(
            &capture.device,
            settings.video.width,
            settings.video.height,
            settings.video.fps,
        )
        .map_err(media_error)?;
        let mut link = Loopback::open(&identity, machine_id).await?;
        let start = Instant::now();
        let clock = || start.elapsed().as_micros() as u64;
        let mut assembler = Assembler::default();
        let mut gate = DecodeGate::default();
        let mut sequence = 0;
        let mut id = 0;
        let mut delivered = 0;
        let mut presented = 0;
        let mut encodes = Vec::new();
        let mut networks = Vec::new();
        let mut decodes = Vec::new();
        let mut received_inputs = 0;
        let mut revoked = false;
        let mut force_idr = true;
        let mut controller =
            LowLatency::new(500_000, settings.video.bitrate, settings.video.bitrate)?;
        let mut feedback_at = clock();
        let mut interval_bytes = 0;
        let mut interval_sent = 0;
        let mut interval_received = 0;
        let mut next_frame = Instant::now();
        while start.elapsed() < Duration::from_secs(seconds)
            && !cancel.load(Ordering::Relaxed)
            && viewer.pump()
        {
            tokio::time::sleep_until(next_frame.into()).await;
            next_frame = (next_frame
                + Duration::from_micros(1_000_000 / settings.video.fps as u64))
            .max(Instant::now());
            let Some(frame) = capture.acquire(16)? else {
                continue;
            };
            if frame.dimensions() != (display.width, display.height) {
                return Err(Error::Unavailable("Les dimensions physiques de l’écran ont changé ou sa rotation n’est pas prise en charge.".into()));
            }
            let captured_us = clock();
            let before = Instant::now();
            let encoded = encoder.encode(&frame.texture, captured_us as i64 * 10, force_idr)?;
            encodes.push(before.elapsed().as_micros() as u64);
            drop(frame);
            let packets = packetize(
                encoded.bytes,
                id,
                sequence,
                captured_us,
                50_000,
                encoded.keyframe,
                link.host.max_datagram_size()?,
            )?;
            interval_sent += packets.len() as u64;
            sequence += packets.len() as u64;
            let sent = Instant::now();
            let host = &link.host;
            let guest = &mut link.guest;
            let (send_result, receive_result) = tokio::join!(
                async {
                    for packet in packets {
                        if !host.send_before_deadline(packet, clock).await? {
                            break;
                        }
                    }
                    Ok::<_, Error>(())
                },
                tokio::time::timeout(Duration::from_millis(50), async {
                    loop {
                        if let Some(packet) = guest.receive(clock).await? {
                            interval_received += 1;
                            interval_bytes += packet.payload.len() as u64;
                            if let Some(frame) = assembler.receive(packet, clock())? {
                                return Ok::<_, Error>(frame);
                            }
                        }
                    }
                })
            );
            send_result?;
            force_idr = false;
            if let Ok(result) = receive_result {
                let frame = result?;
                networks.push(sent.elapsed().as_micros() as u64);
                delivered += 1;
                if gate.admit(frame.id, frame.keyframe) {
                    let before = Instant::now();
                    if let Some(decoded) =
                        decoder.decode(&frame.data, frame.captured_us as i64 * 10)?
                    {
                        decodes.push(before.elapsed().as_micros() as u64);
                        if viewer.present(&decoded).map_err(media_error)? {
                            presented += 1;
                        }
                    }
                }
            } else {
                force_idr = true;
            }
            force_idr |= gate.needs_keyframe();
            id += 1;
            let interval_us = clock().saturating_sub(feedback_at);
            if interval_us >= 250_000 && interval_sent > 0 {
                let previous = controller.bitrate();
                let bitrate = controller.update(&Feedback {
                    rtt_us: (link.host.rtt().as_micros() as u64).max(1),
                    delivered_bytes: interval_bytes,
                    interval_us,
                    sent_packets: interval_sent,
                    lost_packets: interval_sent.saturating_sub(interval_received),
                    app_limited: interval_bytes.saturating_mul(8_000_000) / interval_us
                        < previous * 9 / 10,
                })?;
                if bitrate != previous {
                    encoder.set_bitrate(bitrate)?;
                }
                feedback_at = clock();
                interval_bytes = 0;
                interval_sent = 0;
                interval_received = 0;
            }
            if id == 30 {
                // The test key is F24, scoped to the native viewer while it has focus.
                let mut controller = InputController::new(
                    Permissions {
                        keyboard: true,
                        mouse: true,
                        ..Permissions::default()
                    },
                    TestInput::new(viewer.test_target()),
                );
                for (sequence, event) in [
                    Event::Key {
                        code: 135,
                        pressed: true,
                    },
                    Event::Key {
                        code: 135,
                        pressed: false,
                    },
                    Event::Pointer { x: 32767, y: 32767 },
                    Event::Button {
                        button: 0,
                        pressed: true,
                    },
                    Event::Button {
                        button: 0,
                        pressed: false,
                    },
                ]
                .into_iter()
                .enumerate()
                {
                    let payload =
                        Bytes::from(serde_json::to_vec(&event).map_err(|_| Error::InvalidPacket)?);
                    let packet = Packet {
                        kind: Kind::Input,
                        keyframe: false,
                        shard: 0,
                        shards: 1,
                        frame: sequence as u64,
                        sequence: sequence as u64,
                        captured_us: clock(),
                        lifetime_us: 50_000,
                        frame_len: payload.len() as u32,
                        payload,
                    };
                    link.guest.send_before_deadline(packet, clock).await?;
                    if let Ok(Ok(Some(packet))) =
                        tokio::time::timeout(Duration::from_millis(50), link.host.receive(clock))
                            .await
                    {
                        let event = serde_json::from_slice(&packet.payload)
                            .map_err(|_| Error::InvalidPacket)?;
                        received_inputs += 1;
                        let _ = controller.event(packet.sequence, event, clock());
                    }
                }
                controller.set_permissions(Permissions::default())?;
                revoked = controller
                    .event(
                        5,
                        Event::Key {
                            code: 135,
                            pressed: true,
                        },
                        clock(),
                    )
                    .is_err();
            }
            if id % 30 == 0 {
                viewer
                    .title(&format!(
                        "Noosphere · Test local · {} images · QUIC {:.1} ms · Échap pour fermer",
                        presented,
                        link.host.rtt().as_secs_f64() * 1000.0
                    ))
                    .map_err(media_error)?;
            }
        }
        viewer.pump();
        if presented == 0 {
            return Err(Error::Unavailable(
                "Aucune image n’a été affichée par le décodeur GPU.".into(),
            ));
        }
        Ok(Report {
            encoder: encoder.name.clone(),
            width: settings.video.width,
            height: settings.video.height,
            encoded_frames: id,
            delivered_frames: delivered,
            presented_frames: presented,
            dropped_frames: id - presented,
            encode_p50_us: percentile(&mut encodes, 50),
            encode_p95_us: percentile(&mut encodes, 95),
            network_p50_us: percentile(&mut networks, 50),
            network_p95_us: percentile(&mut networks, 95),
            decode_submit_p50_us: percentile(&mut decodes, 50),
            decode_submit_p95_us: percentile(&mut decodes, 95),
            rtt_us: link.host.rtt().as_micros() as u64,
            duration_ms: start.elapsed().as_millis() as u64,
            input_events_received: received_inputs,
            input_key_observed: viewer.observed_test_keys > 0,
            input_mouse_observed: viewer.observed_test_buttons > 0,
            revocation_verified: revoked,
            input_to_photon_us: None,
        })
    }
}

#[cfg(target_os = "linux")]
#[doc(hidden)]
pub fn linux_live_capture_probe_json() -> Result<String> {
    use crate::{
        capture::linux::PortalEncoder, input::linux::DesktopSession, permissions::Permissions,
        viewer::linux::Viewer,
    };
    use std::time::{Duration, Instant};

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let permissions = Permissions {
            screen: true,
            ..Permissions::default()
        };
        let (_desktop, source) = DesktopSession::open(permissions).await?;
        let mut settings = Settings::default();
        settings.video.width = 640;
        settings.video.height = 360;
        settings.video.fps = 30;
        settings.video.bitrate = 4_000_000;
        let encoder = PortalEncoder::open(source, &settings.video)?;
        let encoder_name = encoder.encoder_name();
        let mut viewer = Viewer::open_headless(&settings.video)?;
        let started = Instant::now();
        let mut encoded_frames = 0_u64;
        let mut keyframes = 0_u64;
        let mut encoded_bytes = 0_u64;
        while started.elapsed() < Duration::from_secs(3) {
            let Some(frame) = encoder.acquire(100)? else {
                continue;
            };
            encoded_frames += 1;
            keyframes += u64::from(frame.keyframe);
            encoded_bytes = encoded_bytes.saturating_add(frame.bytes.len() as u64);
            let _ = viewer.present(frame.bytes)?;
            if !viewer.pump()? {
                break;
            }
        }
        for _ in 0..20 {
            if viewer.decoded_frames() >= encoded_frames {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            if !viewer.pump()? {
                break;
            }
        }
        let decoded_frames = viewer.decoded_frames();
        if encoded_frames == 0 || decoded_frames == 0 {
            return Err(Error::Unavailable(format!(
                "Linux live capture produced {encoded_frames} encoded frame(s), {encoded_bytes} byte(s), and {decoded_frames} decoded frame(s)"
            )));
        }
        serde_json::to_string_pretty(&serde_json::json!({
            "kind": "noosphere-linux-live-capture-probe",
            "desktop": std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            "waylandDisplay": std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
            "encoder": encoder_name,
            "encodedFrames": encoded_frames,
            "decodedFrames": decoded_frames,
            "keyframes": keyframes,
            "encodedBytes": encoded_bytes,
            "durationMs": started.elapsed().as_millis()
        }))
        .map_err(|_| Error::InvalidPacket)
    })
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::{
        capture::linux::{PortalEncoder, Source},
        encoder::DecodeGate,
        input::{Event, InputGuard},
        permissions::Permissions,
        transport::{
            assembler::Assembler,
            loopback::Loopback,
            packet::{Kind, Packet, packetize},
        },
        viewer::linux::Viewer,
    };
    use bytes::Bytes;
    use std::time::{Duration, Instant};

    fn percentile(values: &mut [u64], percent: usize) -> u64 {
        values.sort_unstable();
        values
            .get(values.len().saturating_sub(1) * percent / 100)
            .copied()
            .unwrap_or(0)
    }

    pub async fn run(
        settings: Settings,
        identity: SignalIdentity,
        machine_id: [u8; 16],
        seconds: u64,
        cancel: Arc<AtomicBool>,
    ) -> Result<Report> {
        let encoder = PortalEncoder::open(Source::test(), &settings.video)?;
        let mut viewer = Viewer::open_headless(&settings.video)?;
        let mut link = Loopback::open(&identity, machine_id).await?;
        let start = Instant::now();
        let clock = || start.elapsed().as_micros() as u64;
        let mut assembler = Assembler::default();
        let mut gate = DecodeGate::default();
        let mut sequence = 0_u64;
        let mut frame_id = 0_u64;
        let mut delivered = 0_u64;
        let mut presented = 0_u64;
        let mut encodes = Vec::new();
        let mut networks = Vec::new();
        let mut decodes = Vec::new();
        let mut force_idr = true;
        while start.elapsed() < Duration::from_secs(seconds) && !cancel.load(Ordering::Relaxed) {
            if force_idr {
                encoder.force_keyframe();
            }
            let before = Instant::now();
            let Some(encoded) = encoder.acquire(100)? else {
                continue;
            };
            encodes.push(before.elapsed().as_micros() as u64);
            let captured = clock();
            let packets = packetize(
                encoded.bytes,
                frame_id,
                sequence,
                captured,
                100_000,
                encoded.keyframe,
                link.host.max_datagram_size()?,
            )?;
            sequence += packets.len() as u64;
            let sent = Instant::now();
            let host = &link.host;
            let guest = &mut link.guest;
            let (sent_result, received_result) = tokio::join!(
                async {
                    for packet in packets {
                        let _ = host.send_before_deadline(packet, clock).await?;
                    }
                    Ok::<_, Error>(())
                },
                tokio::time::timeout(Duration::from_millis(100), async {
                    loop {
                        if let Some(packet) = guest.receive(clock).await?
                            && let Some(frame) = assembler.receive(packet, clock())?
                        {
                            return Ok::<_, Error>(frame);
                        }
                    }
                })
            );
            sent_result?;
            if let Ok(frame) = received_result {
                let frame = frame?;
                networks.push(sent.elapsed().as_micros() as u64);
                delivered += 1;
                if gate.admit(frame.id, frame.keyframe) {
                    let before = Instant::now();
                    if viewer.present(frame.data)? {
                        decodes.push(before.elapsed().as_micros() as u64);
                        presented += 1;
                    }
                }
                force_idr = gate.needs_keyframe();
            } else {
                force_idr = true;
            }
            frame_id += 1;
            if !viewer.pump()? {
                break;
            }
        }
        if presented == 0 {
            return Err(Error::Unavailable(
                "Le test Linux n’a présenté aucune image.".into(),
            ));
        }

        let input_permissions = Permissions {
            keyboard: true,
            mouse: true,
            ..Permissions::default()
        };
        let mut input = InputGuard::new(input_permissions);
        let test_events = [
            Event::Key {
                code: 0x41,
                pressed: true,
            },
            Event::Key {
                code: 0x41,
                pressed: false,
            },
            Event::Pointer { x: 32767, y: 32767 },
        ];
        let mut received_inputs = 0_u64;
        for (index, event) in test_events.into_iter().enumerate() {
            let payload =
                Bytes::from(serde_json::to_vec(&event).map_err(|_| Error::InvalidPacket)?);
            let packet = Packet {
                kind: Kind::Input,
                keyframe: false,
                shard: 0,
                shards: 1,
                frame: index as u64,
                sequence: index as u64,
                captured_us: clock(),
                lifetime_us: 50_000,
                frame_len: payload.len() as u32,
                payload,
            };
            link.guest.send_before_deadline(packet, clock).await?;
            if let Ok(Ok(Some(packet))) =
                tokio::time::timeout(Duration::from_millis(50), link.host.receive(clock)).await
            {
                let event =
                    serde_json::from_slice(&packet.payload).map_err(|_| Error::InvalidPacket)?;
                input.accept(packet.sequence, event)?;
                received_inputs += 1;
            }
        }
        input.set_permissions(Permissions::default());
        let revoked = input
            .accept(
                10,
                Event::Key {
                    code: 0x42,
                    pressed: true,
                },
            )
            .is_err();
        Ok(Report {
            encoder: encoder.encoder_name().into(),
            width: settings.video.width,
            height: settings.video.height,
            encoded_frames: frame_id,
            delivered_frames: delivered,
            presented_frames: presented,
            dropped_frames: frame_id.saturating_sub(presented),
            encode_p50_us: percentile(&mut encodes, 50),
            encode_p95_us: percentile(&mut encodes, 95),
            network_p50_us: percentile(&mut networks, 50),
            network_p95_us: percentile(&mut networks, 95),
            decode_submit_p50_us: percentile(&mut decodes, 50),
            decode_submit_p95_us: percentile(&mut decodes, 95),
            rtt_us: link.host.rtt().as_micros() as u64,
            duration_ms: start.elapsed().as_millis() as u64,
            input_events_received: received_inputs,
            input_key_observed: received_inputs >= 2,
            input_mouse_observed: received_inputs >= 3,
            revocation_verified: revoked,
            input_to_photon_us: None,
        })
    }
}

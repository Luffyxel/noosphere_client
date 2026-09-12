//! Native host/client sessions. Signaling objects are carried inside the existing
//! encrypted Noosphere machine mailboxes; media and input never cross the WebView.
use crate::{
    Error, Result,
    daemon::Settings,
    diagnostic::Credentials,
    permissions::{Permissions, Principal},
    signaling::{SessionBinding, random_id},
    transport::{self, CongestionControl, session::AuthenticatedTransport},
};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuestHello {
    pub version: u8,
    pub session_id: [u8; 32],
    pub guest: Principal,
    pub host: Principal,
    pub guest_certificate_der: Vec<u8>,
    pub permissions: Permissions,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostOffer {
    pub version: u8,
    pub session_id: [u8; 32],
    pub guest: Principal,
    pub host: Principal,
    pub host_certificate_der: Vec<u8>,
    pub addresses: Vec<String>,
    pub permissions: Permissions,
    pub settings: Settings,
    pub created_at: u64,
    pub expires_at: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn certificate_hash(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .expect("SHA-256 length")
}

impl GuestHello {
    pub fn validate(&self, time: u64) -> Result<()> {
        self.guest.validate()?;
        self.host.validate()?;
        if self.version != 1
            || self.session_id == [0; 32]
            || self.guest == self.host
            || self.guest.machine_id == self.host.machine_id
            || self.guest_certificate_der.len() > 4096
            || self.guest_certificate_der.len() < 128
            || !self.permissions.screen
            || self.created_at > time.saturating_add(30)
            || self.expires_at <= time
            || self.expires_at <= self.created_at
            || self.expires_at - self.created_at > 120
        {
            return Err(Error::Authentication);
        }
        Ok(())
    }
}

impl HostOffer {
    pub fn validate(&self, hello: &GuestHello, time: u64) -> Result<()> {
        hello.validate(time)?;
        self.settings.validate()?;
        if self.version != 1
            || self.session_id != hello.session_id
            || self.guest != hello.guest
            || self.host != hello.host
            || self.host_certificate_der.len() > 4096
            || self.host_certificate_der.len() < 128
            || self.addresses.is_empty()
            || self.addresses.len() > 8
            || self.permissions != self.permissions.intersect(hello.permissions)
            || !self.permissions.screen
            || self.created_at < hello.created_at
            || self.created_at > time.saturating_add(30)
            || self.expires_at != hello.expires_at
            || self
                .addresses
                .iter()
                .any(|address| address.len() > 64 || address.parse::<SocketAddr>().is_err())
        {
            return Err(Error::Authentication);
        }
        Ok(())
    }

    fn binding(&self, guest_certificate: &[u8]) -> SessionBinding {
        SessionBinding {
            session_id: self.session_id,
            host: self.host.clone(),
            guest: self.guest.clone(),
            host_certificate_sha256: certificate_hash(&self.host_certificate_der),
            guest_certificate_sha256: certificate_hash(guest_certificate),
            expires_at: self.expires_at,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum State {
    #[default]
    Idle,
    Preparing,
    Waiting,
    Connecting,
    Streaming {
        peer: Principal,
        frames: u64,
        rtt_us: u64,
    },
    Failed {
        error: String,
    },
}

struct PendingGuest {
    hello: GuestHello,
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
    credentials: Credentials,
}

#[derive(Default)]
pub struct Worker {
    state: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    guest: Option<PendingGuest>,
}

impl Worker {
    pub fn state(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn reap(&mut self) -> Result<()> {
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
        {
            let thread = self.thread.take().expect("checked thread");
            thread
                .join()
                .map_err(|_| Error::Unavailable("remote session worker panicked".into()))?;
        }
        if self.thread.is_some() {
            return Err(Error::Unavailable(
                "Une session distante est déjà active.".into(),
            ));
        }
        Ok(())
    }

    fn stop_and_reap(&mut self) -> Result<()> {
        self.stop();
        for _ in 0..100 {
            if self
                .thread
                .as_ref()
                .is_none_or(std::thread::JoinHandle::is_finished)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.reap()
    }

    pub fn prepare_guest(
        &mut self,
        credentials: Credentials,
        host: Principal,
        permissions: Permissions,
    ) -> Result<GuestHello> {
        self.stop_and_reap()?;
        let identity = crate::identity::SignalIdentity::from_private_key(
            credentials.github_user_id,
            &credentials.private_key,
        )?;
        let guest = identity.principal(credentials.machine_id)?;
        if guest == host || guest.machine_id == host.machine_id || !permissions.screen {
            return Err(Error::PermissionDenied);
        }
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .map_err(|_| Error::Authentication)?;
        let certificate_der: CertificateDer<'static> = certificate.cert.into();
        let created_at = now();
        let hello = GuestHello {
            version: 1,
            session_id: random_id()?,
            guest,
            host,
            guest_certificate_der: certificate_der.to_vec(),
            permissions,
            created_at,
            expires_at: created_at + 120,
        };
        hello.validate(created_at)?;
        self.guest = Some(PendingGuest {
            hello: hello.clone(),
            certificate_der: certificate_der.to_vec(),
            private_key_der: certificate.signing_key.serialize_der(),
            credentials,
        });
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = State::Preparing;
        Ok(hello)
    }

    pub fn start_host(
        &mut self,
        mut settings: Settings,
        credentials: Credentials,
        hello: GuestHello,
        permissions: Permissions,
    ) -> Result<HostOffer> {
        if matches!(self.state(), State::Streaming { .. }) {
            return Err(Error::Unavailable(
                "Une session distante est déjà active.".into(),
            ));
        }
        self.stop_and_reap()?;
        hello.validate(now())?;
        #[cfg(any(windows, target_os = "linux"))]
        {
            // H.264 is the universally available Windows hardware path in this
            // revision. A saved HEVC/AV1 preference therefore negotiates down
            // instead of making an otherwise compatible machine unreachable.
            settings.video.codec = crate::encoder::Codec::H264;
        }
        settings.validate()?;
        let identity = crate::identity::SignalIdentity::from_private_key(
            credentials.github_user_id,
            &credentials.private_key,
        )?;
        if identity.principal(credentials.machine_id)? != hello.host {
            return Err(Error::Authentication);
        }
        let permissions = permissions.intersect(hello.permissions);
        if !permissions.screen {
            return Err(Error::PermissionDenied);
        }
        let state = self.state.clone();
        let cancel = self.cancel.clone();
        cancel.store(false, Ordering::Relaxed);
        *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Waiting;
        let (offer_tx, offer_rx) = mpsc::sync_channel(1);
        self.thread = Some(std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()?;
                runtime.block_on(host_session(
                    settings,
                    identity,
                    hello,
                    permissions,
                    cancel,
                    state.clone(),
                    offer_tx,
                ))
            }));
            if let Err(error) = result
                .unwrap_or_else(|_| Err(Error::Unavailable("Le moteur hôte s’est arrêté.".into())))
            {
                *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Failed {
                    error: error.to_string(),
                };
            } else {
                *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Idle;
            }
        }));
        offer_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| Error::Unavailable("Impossible d’ouvrir le port QUIC.".into()))?
    }

    pub fn connect_guest(&mut self, offer: HostOffer) -> Result<()> {
        self.reap()?;
        let pending = self.guest.take().ok_or(Error::Authentication)?;
        offer.validate(&pending.hello, now())?;
        let identity = crate::identity::SignalIdentity::from_private_key(
            pending.credentials.github_user_id,
            &pending.credentials.private_key,
        )?;
        let state = self.state.clone();
        let cancel = self.cancel.clone();
        cancel.store(false, Ordering::Relaxed);
        *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Connecting;
        self.thread = Some(std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()?;
                runtime.block_on(guest_session(
                    pending,
                    offer,
                    identity,
                    cancel,
                    state.clone(),
                ))
            }));
            if let Err(error) = result.unwrap_or_else(|_| {
                Err(Error::Unavailable("Le viewer distant s’est arrêté.".into()))
            }) {
                *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Failed {
                    error: error.to_string(),
                };
            } else {
                *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Idle;
            }
        }));
        Ok(())
    }

    pub fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.guest = None;
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

fn push_candidate(addresses: &mut Vec<String>, address: SocketAddr) {
    if address.ip().is_unspecified()
        || address.ip().is_multicast()
        || address.ip().is_ipv6()
        || matches!(address.ip(), IpAddr::V4(ip) if ip.is_link_local())
    {
        return;
    }
    let address = address.to_string();
    if !addresses.contains(&address) && addresses.len() < 8 {
        addresses.push(address);
    }
}

fn local_addresses(port: u16, mapped: Option<SocketAddr>) -> Vec<String> {
    use std::net::ToSocketAddrs as _;

    let mut addresses = Vec::with_capacity(8);
    // Put the address selected by the operating-system route first. This is the
    // candidate that normally reaches another machine on the current LAN and
    // avoids wasting a QUIC timeout on loopback or a virtual adapter.
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0")
        && socket.connect("1.1.1.1:53").is_ok()
        && let Ok(address) = socket.local_addr()
        && !address.ip().is_unspecified()
    {
        push_candidate(&mut addresses, SocketAddr::new(address.ip(), port));
    }
    // getaddrinfo exposes the other active interfaces without coupling the
    // transport to a platform-specific adapter API. Virtual adapters remain
    // useful for local labs, while link-local and IPv6 candidates are excluded
    // because this endpoint currently listens on IPv4.
    let hostname = std::env::var_os("COMPUTERNAME").or_else(|| std::env::var_os("HOSTNAME"));
    if let Some(hostname) = hostname.and_then(|value| value.into_string().ok())
        && let Ok(resolved) = (hostname.as_str(), 0).to_socket_addrs()
    {
        for address in resolved {
            if addresses.len() >= 6 {
                break;
            }
            push_candidate(&mut addresses, SocketAddr::new(address.ip(), port));
        }
    }
    if let Some(mapped) = mapped {
        push_candidate(&mut addresses, mapped);
    }
    // Loopback is last: it supports two local processes but can never reach a
    // second physical machine.
    push_candidate(
        &mut addresses,
        SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
    );
    addresses
}

fn stun_address(socket: &UdpSocket) -> Option<SocketAddr> {
    let server = ("stun.cloudflare.com", 3478)
        .to_socket_addrs()
        .ok()?
        .find(SocketAddr::is_ipv4)?;
    let id = random_id().ok()?;
    let transaction: [u8; 12] = id[..12].try_into().ok()?;
    socket
        .set_read_timeout(Some(Duration::from_millis(800)))
        .ok()?;
    socket
        .set_write_timeout(Some(Duration::from_millis(800)))
        .ok()?;
    socket
        .send_to(&crate::nat::binding_request(transaction), server)
        .ok()?;
    let mut response = [0; 2048];
    let (length, source) = socket.recv_from(&mut response).ok()?;
    if source.ip() != server.ip() {
        return None;
    }
    crate::nat::mapped_address(&response[..length], transaction).ok()
}

async fn host_session(
    settings: Settings,
    identity: crate::identity::SignalIdentity,
    hello: GuestHello,
    permissions: Permissions,
    cancel: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
    offer_tx: mpsc::SyncSender<Result<HostOffer>>,
) -> Result<()> {
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .map_err(|_| Error::Authentication)?;
    let certificate_der: CertificateDer<'static> = certificate.cert.into();
    let config = transport::server_config(
        certificate_der.clone(),
        PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der()).into(),
        CertificateDer::from(hello.guest_certificate_der.clone()),
        CongestionControl::Cubic,
    )?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let port = socket.local_addr()?.port();
    let mapped = stun_address(&socket);
    socket.set_nonblocking(true)?;
    let runtime = quinn::default_runtime()
        .ok_or_else(|| Error::Unavailable("Runtime réseau QUIC indisponible.".into()))?;
    let endpoint = quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(config),
        socket,
        runtime,
    )?;
    let offer = HostOffer {
        version: 1,
        session_id: hello.session_id,
        guest: hello.guest.clone(),
        host: hello.host.clone(),
        host_certificate_der: certificate_der.to_vec(),
        addresses: local_addresses(port, mapped),
        permissions,
        settings: settings.clone(),
        created_at: now(),
        expires_at: hello.expires_at,
    };
    offer.validate(&hello, now())?;
    let binding = offer.binding(&hello.guest_certificate_der);
    offer_tx
        .send(Ok(offer))
        .map_err(|_| Error::Unavailable("session offer receiver closed".into()))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    let incoming = loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(Error::Unavailable(
                "La demande de connexion a expiré.".into(),
            ));
        }
        match tokio::time::timeout(Duration::from_millis(100), endpoint.accept()).await {
            Ok(Some(incoming)) => break incoming,
            Ok(None) => return Err(Error::Unavailable("Le port QUIC est fermé.".into())),
            Err(_) => {}
        }
    };
    let incoming = incoming
        .await
        .map_err(|error| Error::Unavailable(format!("Connexion QUIC : {error}")))?;
    let session =
        AuthenticatedTransport::establish(incoming, &binding, true, &identity, permissions).await?;
    *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Streaming {
        peer: hello.guest.clone(),
        frames: 0,
        rtt_us: session.rtt().as_micros() as u64,
    };
    platform::host(settings, session, permissions, cancel, state, hello.guest).await
}

async fn guest_session(
    pending: PendingGuest,
    offer: HostOffer,
    identity: crate::identity::SignalIdentity,
    cancel: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
) -> Result<()> {
    let config = transport::client_config(
        CertificateDer::from(pending.certificate_der.clone()),
        PrivatePkcs8KeyDer::from(pending.private_key_der).into(),
        CertificateDer::from(offer.host_certificate_der.clone()),
        CongestionControl::Cubic,
    )?;
    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())?;
    endpoint.set_default_client_config(config);
    let mut attempts = tokio::task::JoinSet::new();
    for address in &offer.addresses {
        let candidate: SocketAddr = address.parse().map_err(|_| Error::InvalidPacket)?;
        if let Ok(attempt) = endpoint.connect(candidate, "localhost") {
            attempts.spawn(async move { attempt.await.ok() });
        }
    }
    // Race all ICE-style candidates. Virtual adapters and an unreachable public
    // mapping must never delay a working LAN route by several seconds each.
    let connection = tokio::time::timeout(Duration::from_secs(6), async {
        while let Some(result) = attempts.join_next().await {
            if let Ok(Some(connection)) = result {
                return Some(connection);
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    attempts.abort_all();
    let connection = connection.ok_or_else(|| {
        Error::Unavailable("Aucun chemin UDP direct n’a répondu. Un relais sera nécessaire.".into())
    })?;
    let binding = offer.binding(&pending.certificate_der);
    let session = AuthenticatedTransport::establish(
        connection,
        &binding,
        false,
        &identity,
        offer.permissions,
    )
    .await?;
    *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Streaming {
        peer: offer.host.clone(),
        frames: 0,
        rtt_us: session.rtt().as_micros() as u64,
    };
    platform::guest(
        offer.settings,
        session,
        offer.permissions,
        cancel,
        state,
        offer.host,
    )
    .await
}

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::{
        capture::{Capture as _, dxgi},
        congestion::{Feedback, LowLatency, RateController as _},
        decoder::windows::HardwareDecoder,
        encoder::{
            Codec, DecodeGate,
            windows::{HardwareEncoder, Runtime, media_error},
        },
        fec,
        input::{Event, InputController, windows::NativeInput},
        transport::{
            assembler::Assembler,
            packet::{Kind, Packet, packetize},
        },
        viewer::windows::Viewer,
    };
    use bytes::Bytes;
    use std::time::{Duration, Instant};

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct NetworkFeedback {
        interval_us: u64,
        delivered_bytes: u64,
        received_packets: u64,
        lost_packets: u64,
        request_keyframe: bool,
    }

    impl NetworkFeedback {
        fn validate(&self) -> Result<()> {
            // A busy GPU or a temporarily suspended window can delay one report.
            // Keep the same bounded interval accepted by the controller instead of
            // terminating an otherwise healthy session after a two-second stall.
            if !(10_000..=10_000_000).contains(&self.interval_us)
                || self.received_packets == 0
                || self.lost_packets > 1_000_000
                || self.received_packets.saturating_add(self.lost_packets) > 1_000_000
                || self.delivered_bytes > 256 * 1024 * 1024
            {
                return Err(Error::InvalidPacket);
            }
            Ok(())
        }
    }

    fn clock() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }

    fn progress(
        state: &Arc<Mutex<State>>,
        peer: &Principal,
        frames: u64,
        session: &AuthenticatedTransport,
    ) {
        *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Streaming {
            peer: peer.clone(),
            frames,
            rtt_us: session.rtt().as_micros() as u64,
        };
    }

    pub async fn host(
        settings: Settings,
        mut session: AuthenticatedTransport,
        permissions: Permissions,
        cancel: Arc<AtomicBool>,
        state: Arc<Mutex<State>>,
        peer: Principal,
    ) -> Result<()> {
        if settings.video.codec != Codec::H264 {
            return Err(Error::Unavailable(
                "La session Windows requiert H.264.".into(),
            ));
        }
        let _runtime = Runtime::start().map_err(media_error)?;
        let display = dxgi::displays()?
            .into_iter()
            .find(|display| display.id == settings.display)
            .ok_or_else(|| Error::Unavailable("Écran introuvable.".into()))?;
        let mut capture = dxgi::DesktopDuplication::open(display.id)?;
        let mut encoder = HardwareEncoder::open(
            &capture.device,
            (display.width, display.height),
            &settings.video,
        )?;
        let mut input = InputController::new(permissions, NativeInput);
        let mut sequence = 0_u64;
        let mut frame_id = 0_u64;
        let mut force_idr = true;
        let mut controller =
            LowLatency::new(500_000, settings.video.bitrate, settings.video.bitrate)?;
        let mut fec_group = None;
        let frame_time = Duration::from_micros(1_000_000 / settings.video.fps as u64);
        let mut next_frame = Instant::now();
        while !cancel.load(Ordering::Relaxed) {
            tokio::time::sleep_until(next_frame.into()).await;
            next_frame = (next_frame + frame_time).max(Instant::now());
            if let Some(frame) = capture.acquire(16)? {
                let captured = clock();
                let encoded = encoder.encode(
                    &frame.texture,
                    captured as i64 * 10,
                    force_idr || frame_id.is_multiple_of(settings.video.fps as u64),
                )?;
                force_idr = false;
                drop(frame);
                let mtu = if fec_group.is_some() {
                    session.max_datagram_size()?.min(1144)
                } else {
                    session.max_datagram_size()?
                };
                let mut packets = packetize(
                    encoded.bytes,
                    frame_id,
                    sequence,
                    captured,
                    120_000,
                    encoded.keyframe,
                    mtu,
                )?;
                let video_packets = packets.len();
                if let Some(group) = fec_group {
                    for start in (0..video_packets).step_by(group) {
                        let end = (start + group).min(video_packets);
                        let payload = fec::parity(
                            &packets[start..end]
                                .iter()
                                .map(|packet| packet.payload.clone())
                                .collect::<Vec<_>>(),
                        )?;
                        packets.push(Packet {
                            kind: Kind::Parity,
                            keyframe: encoded.keyframe,
                            shard: start as u16,
                            shards: video_packets as u16,
                            frame: frame_id,
                            sequence: sequence + packets.len() as u64,
                            captured_us: captured,
                            lifetime_us: 120_000,
                            frame_len: packets[0].frame_len,
                            payload,
                        });
                    }
                }
                sequence += packets.len() as u64;
                for packet in packets {
                    if !session.send_before_deadline(packet, clock).await? {
                        if session.is_closed() {
                            return Ok(());
                        }
                        break;
                    }
                }
                frame_id += 1;
                if frame_id.is_multiple_of(30) {
                    progress(&state, &peer, frame_id, &session);
                }
            }
            for _ in 0..32 {
                match tokio::time::timeout(Duration::from_millis(1), session.receive(clock)).await {
                    Ok(Ok(Some(packet))) if packet.kind == Kind::Input => {
                        let event: Event = serde_json::from_slice(&packet.payload)
                            .map_err(|_| Error::InvalidPacket)?;
                        input.event(packet.sequence, event, clock())?;
                    }
                    Ok(Ok(Some(packet))) if packet.kind == Kind::Feedback => {
                        let feedback: NetworkFeedback = serde_json::from_slice(&packet.payload)
                            .map_err(|_| Error::InvalidPacket)?;
                        feedback.validate().map_err(|error| {
                            Error::Unavailable(format!("retour réseau distant invalide : {error}"))
                        })?;
                        let previous = controller.bitrate();
                        let bitrate = controller.update(&Feedback {
                            rtt_us: (session.rtt().as_micros() as u64).max(1),
                            delivered_bytes: feedback.delivered_bytes,
                            interval_us: feedback.interval_us,
                            sent_packets: feedback
                                .received_packets
                                .saturating_add(feedback.lost_packets),
                            lost_packets: feedback.lost_packets,
                            app_limited: feedback.delivered_bytes.saturating_mul(8_000_000)
                                / feedback.interval_us
                                < previous * 9 / 10,
                        })?;
                        if bitrate != previous {
                            encoder.set_bitrate(bitrate)?;
                        }
                        let estimate = controller.estimate();
                        fec_group = fec::group_size(
                            estimate.loss,
                            estimate.rtt_us > estimate.min_rtt_us.saturating_add(8_000),
                        );
                        force_idr |= feedback.request_keyframe;
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => return Err(error),
                    Err(_) => break,
                }
            }
            input.tick(clock())?;
        }
        session.close();
        Ok(())
    }

    fn event_allowed(event: &Event, permissions: Permissions) -> bool {
        match event {
            Event::Key { .. } => permissions.keyboard,
            Event::Pointer { .. } | Event::Button { .. } => permissions.mouse,
            Event::Gamepad { .. } => permissions.gamepad,
            Event::Clipboard(_) => permissions.clipboard,
        }
    }

    pub async fn guest(
        settings: Settings,
        mut session: AuthenticatedTransport,
        permissions: Permissions,
        cancel: Arc<AtomicBool>,
        state: Arc<Mutex<State>>,
        peer: Principal,
    ) -> Result<()> {
        if settings.video.codec != Codec::H264 {
            return Err(Error::Unavailable(
                "Le viewer Windows requiert H.264.".into(),
            ));
        }
        let _runtime = Runtime::start().map_err(media_error)?;
        let (device, _) = dxgi::device()?;
        let mut decoder = HardwareDecoder::open(&device, &settings.video)?;
        let mut viewer = Viewer::open(
            &device,
            settings.video.width,
            settings.video.height,
            settings.video.fps,
        )
        .map_err(media_error)?;
        viewer
            .title("Noosphere · Bureau distant · Échap pour fermer")
            .map_err(media_error)?;
        let mut assembler = Assembler::default();
        let mut gate = DecodeGate::default();
        let mut outgoing_sequence = 1_u64 << 63;
        let mut presented = 0_u64;
        let mut first_media_sequence = None;
        let mut highest_media_sequence = 0_u64;
        let mut received_total = 0_u64;
        let mut reported_received = 0_u64;
        let mut reported_lost = 0_u64;
        let mut interval_bytes = 0_u64;
        let mut feedback_at = clock();
        while !cancel.load(Ordering::Relaxed) && viewer.pump() {
            if let Ok(received) =
                tokio::time::timeout(Duration::from_millis(20), session.receive(clock)).await
            {
                let packet = received.map_err(|error| {
                    Error::Unavailable(format!(
                        "viewer QUIC après {presented} images et {} abandon(s) : {error}",
                        assembler.dropped
                    ))
                })?;
                if let Some(packet) = packet {
                    if matches!(packet.kind, Kind::Video | Kind::Parity) {
                        first_media_sequence.get_or_insert(packet.sequence);
                        highest_media_sequence = highest_media_sequence.max(packet.sequence);
                        received_total = received_total.saturating_add(1);
                        interval_bytes = interval_bytes.saturating_add(packet.payload.len() as u64);
                    }
                    if matches!(packet.kind, Kind::Video | Kind::Parity) {
                        let packet_frame = packet.frame;
                        let assembled = assembler
                            .receive(packet, session.peer_clock(clock()))
                            .map_err(|error| {
                                Error::Unavailable(format!(
                                    "assemblage de l’image distante {packet_frame} : {error}"
                                ))
                            })?;
                        if let Some(frame) = assembled
                            && gate.admit(frame.id, frame.keyframe)
                            && let Some(decoded) =
                                decoder.decode(&frame.data, frame.captured_us as i64 * 10)?
                            && viewer.present(&decoded).map_err(media_error)?
                        {
                            presented += 1;
                            progress(&state, &peer, presented, &session);
                        }
                    }
                }
            }
            for event in viewer
                .take_input()
                .into_iter()
                .filter(|event| event_allowed(event, permissions))
            {
                let payload =
                    Bytes::from(serde_json::to_vec(&event).map_err(|_| Error::InvalidPacket)?);
                let packet = Packet {
                    kind: Kind::Input,
                    keyframe: false,
                    shard: 0,
                    shards: 1,
                    frame: outgoing_sequence,
                    sequence: outgoing_sequence,
                    captured_us: clock(),
                    lifetime_us: 80_000,
                    frame_len: payload.len() as u32,
                    payload,
                };
                let _ = session.send_before_deadline(packet, clock).await?;
                outgoing_sequence = outgoing_sequence.saturating_add(1);
            }
            let time = clock();
            let interval_us = time.saturating_sub(feedback_at);
            if interval_us >= 250_000 && received_total > reported_received {
                let expected_total = first_media_sequence
                    .map(|first| highest_media_sequence.saturating_sub(first) + 1)
                    .unwrap_or(0);
                let lost_total = expected_total.saturating_sub(received_total);
                let feedback = NetworkFeedback {
                    interval_us,
                    delivered_bytes: interval_bytes,
                    received_packets: received_total.saturating_sub(reported_received),
                    lost_packets: lost_total.saturating_sub(reported_lost),
                    request_keyframe: gate.needs_keyframe(),
                };
                feedback.validate().map_err(|error| {
                    Error::Unavailable(format!("calcul du retour réseau local : {error}"))
                })?;
                let payload =
                    Bytes::from(serde_json::to_vec(&feedback).map_err(|_| Error::InvalidPacket)?);
                let packet = Packet {
                    kind: Kind::Feedback,
                    keyframe: false,
                    shard: 0,
                    shards: 1,
                    frame: outgoing_sequence,
                    sequence: outgoing_sequence,
                    captured_us: time,
                    lifetime_us: 200_000,
                    frame_len: payload.len() as u32,
                    payload,
                };
                let _ = session.send_before_deadline(packet, clock).await?;
                outgoing_sequence = outgoing_sequence.saturating_add(1);
                reported_received = received_total;
                reported_lost = lost_total;
                interval_bytes = 0;
                feedback_at = time;
            }
        }
        session.close();
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use crate::{
        capture::linux::PortalEncoder,
        congestion::{Feedback, LowLatency, RateController as _},
        encoder::{Codec, DecodeGate},
        fec,
        input::{Event, InputGuard, linux::DesktopSession},
        transport::{
            assembler::Assembler,
            packet::{Kind, Packet, packetize},
        },
        viewer::linux::Viewer,
    };
    use bytes::Bytes;
    use std::time::{Duration, Instant};

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct NetworkFeedback {
        interval_us: u64,
        delivered_bytes: u64,
        received_packets: u64,
        lost_packets: u64,
        request_keyframe: bool,
    }

    impl NetworkFeedback {
        fn validate(&self) -> Result<()> {
            if !(10_000..=10_000_000).contains(&self.interval_us)
                || self.received_packets == 0
                || self.lost_packets > 1_000_000
                || self.received_packets.saturating_add(self.lost_packets) > 1_000_000
                || self.delivered_bytes > 256 * 1024 * 1024
            {
                return Err(Error::InvalidPacket);
            }
            Ok(())
        }
    }

    fn clock() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }

    fn progress(
        state: &Arc<Mutex<State>>,
        peer: &Principal,
        frames: u64,
        session: &AuthenticatedTransport,
    ) {
        *state.lock().unwrap_or_else(|error| error.into_inner()) = State::Streaming {
            peer: peer.clone(),
            frames,
            rtt_us: session.rtt().as_micros() as u64,
        };
    }

    pub async fn host(
        settings: Settings,
        mut session: AuthenticatedTransport,
        permissions: Permissions,
        cancel: Arc<AtomicBool>,
        state: Arc<Mutex<State>>,
        peer: Principal,
    ) -> Result<()> {
        if settings.video.codec != Codec::H264 {
            return Err(Error::Unavailable(
                "La session Linux requiert H.264 pour cette version.".into(),
            ));
        }
        let (mut desktop, source) = DesktopSession::open(permissions).await?;
        let encoder = PortalEncoder::open(source, &settings.video)?;
        let mut input = InputGuard::new(permissions);
        let mut last_input = None;
        let mut sequence = 0_u64;
        let mut frame_id = 0_u64;
        let mut force_idr = true;
        let mut controller =
            LowLatency::new(500_000, settings.video.bitrate, settings.video.bitrate)?;
        let mut fec_group = None;
        let frame_time = Duration::from_micros(1_000_000 / u64::from(settings.video.fps));
        let mut next_frame = Instant::now();
        while !cancel.load(Ordering::Relaxed) {
            tokio::time::sleep_until(next_frame.into()).await;
            next_frame = (next_frame + frame_time).max(Instant::now());
            if force_idr || frame_id.is_multiple_of(u64::from(settings.video.fps)) {
                encoder.force_keyframe();
                force_idr = false;
            }
            if let Some(encoded) = encoder.acquire(16)? {
                let captured = clock();
                let mtu = if fec_group.is_some() {
                    session.max_datagram_size()?.min(1144)
                } else {
                    session.max_datagram_size()?
                };
                let mut packets = packetize(
                    encoded.bytes,
                    frame_id,
                    sequence,
                    captured,
                    120_000,
                    encoded.keyframe,
                    mtu,
                )?;
                let video_packets = packets.len();
                if let Some(group) = fec_group {
                    for start in (0..video_packets).step_by(group) {
                        let end = (start + group).min(video_packets);
                        let payload = fec::parity(
                            &packets[start..end]
                                .iter()
                                .map(|packet| packet.payload.clone())
                                .collect::<Vec<_>>(),
                        )?;
                        packets.push(Packet {
                            kind: Kind::Parity,
                            keyframe: encoded.keyframe,
                            shard: start as u16,
                            shards: video_packets as u16,
                            frame: frame_id,
                            sequence: sequence + packets.len() as u64,
                            captured_us: captured,
                            lifetime_us: 120_000,
                            frame_len: packets[0].frame_len,
                            payload,
                        });
                    }
                }
                sequence += packets.len() as u64;
                for packet in packets {
                    if !session.send_before_deadline(packet, clock).await? {
                        if session.is_closed() {
                            return Ok(());
                        }
                        break;
                    }
                }
                frame_id += 1;
                if frame_id.is_multiple_of(30) {
                    progress(&state, &peer, frame_id, &session);
                }
            }
            for _ in 0..32 {
                match tokio::time::timeout(Duration::from_millis(1), session.receive(clock)).await {
                    Ok(Ok(Some(packet))) if packet.kind == Kind::Input => {
                        let event: Event = serde_json::from_slice(&packet.payload)
                            .map_err(|_| Error::InvalidPacket)?;
                        let event = input.accept(packet.sequence, event)?;
                        desktop.inject(&event).await?;
                        last_input = Some(clock());
                    }
                    Ok(Ok(Some(packet))) if packet.kind == Kind::Feedback => {
                        let feedback: NetworkFeedback = serde_json::from_slice(&packet.payload)
                            .map_err(|_| Error::InvalidPacket)?;
                        feedback.validate()?;
                        let previous = controller.bitrate();
                        let bitrate = controller.update(&Feedback {
                            rtt_us: (session.rtt().as_micros() as u64).max(1),
                            delivered_bytes: feedback.delivered_bytes,
                            interval_us: feedback.interval_us,
                            sent_packets: feedback
                                .received_packets
                                .saturating_add(feedback.lost_packets),
                            lost_packets: feedback.lost_packets,
                            app_limited: feedback.delivered_bytes.saturating_mul(8_000_000)
                                / feedback.interval_us
                                < previous * 9 / 10,
                        })?;
                        if bitrate != previous {
                            encoder.set_bitrate(bitrate)?;
                        }
                        let estimate = controller.estimate();
                        fec_group = fec::group_size(
                            estimate.loss,
                            estimate.rtt_us > estimate.min_rtt_us.saturating_add(8_000),
                        );
                        force_idr |= feedback.request_keyframe;
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => return Err(error),
                    Err(_) => break,
                }
            }
            if last_input.is_some_and(|last| clock().saturating_sub(last) >= 250_000) {
                last_input = None;
                for event in input.release_all() {
                    desktop.inject(&event).await?;
                }
            }
        }
        for event in input.release_all() {
            let _ = desktop.inject(&event).await;
        }
        session.close();
        Ok(())
    }

    fn event_allowed(event: &Event, permissions: Permissions) -> bool {
        match event {
            Event::Key { .. } => permissions.keyboard,
            Event::Pointer { .. } | Event::Button { .. } => permissions.mouse,
            Event::Gamepad { .. } => permissions.gamepad,
            Event::Clipboard(_) => permissions.clipboard,
        }
    }

    pub async fn guest(
        settings: Settings,
        mut session: AuthenticatedTransport,
        permissions: Permissions,
        cancel: Arc<AtomicBool>,
        state: Arc<Mutex<State>>,
        peer: Principal,
    ) -> Result<()> {
        if settings.video.codec != Codec::H264 {
            return Err(Error::Unavailable(
                "Le viewer Linux requiert H.264 pour cette version.".into(),
            ));
        }
        let mut viewer = Viewer::open(&settings.video)?;
        let mut assembler = Assembler::default();
        let mut gate = DecodeGate::default();
        let mut outgoing_sequence = 1_u64 << 63;
        let mut presented = 0_u64;
        let mut first_media_sequence = None;
        let mut highest_media_sequence = 0_u64;
        let mut received_total = 0_u64;
        let mut reported_received = 0_u64;
        let mut reported_lost = 0_u64;
        let mut interval_bytes = 0_u64;
        let mut feedback_at = clock();
        while !cancel.load(Ordering::Relaxed) && viewer.pump()? {
            if let Ok(received) =
                tokio::time::timeout(Duration::from_millis(20), session.receive(clock)).await
            {
                let packet = received.map_err(|error| {
                    Error::Unavailable(format!(
                        "viewer QUIC après {presented} images et {} abandon(s) : {error}",
                        assembler.dropped
                    ))
                })?;
                if let Some(packet) = packet
                    && matches!(packet.kind, Kind::Video | Kind::Parity)
                {
                    first_media_sequence.get_or_insert(packet.sequence);
                    highest_media_sequence = highest_media_sequence.max(packet.sequence);
                    received_total = received_total.saturating_add(1);
                    interval_bytes = interval_bytes.saturating_add(packet.payload.len() as u64);
                    let frame_id = packet.frame;
                    if let Some(frame) = assembler
                        .receive(packet, session.peer_clock(clock()))
                        .map_err(|error| {
                            Error::Unavailable(format!(
                                "assemblage de l’image distante {frame_id} : {error}"
                            ))
                        })?
                        && gate.admit(frame.id, frame.keyframe)
                        && viewer.present(frame.data)?
                    {
                        presented += 1;
                        progress(&state, &peer, presented, &session);
                    }
                }
            }
            for event in viewer
                .take_input()
                .into_iter()
                .filter(|event| event_allowed(event, permissions))
            {
                let payload =
                    Bytes::from(serde_json::to_vec(&event).map_err(|_| Error::InvalidPacket)?);
                let packet = Packet {
                    kind: Kind::Input,
                    keyframe: false,
                    shard: 0,
                    shards: 1,
                    frame: outgoing_sequence,
                    sequence: outgoing_sequence,
                    captured_us: clock(),
                    lifetime_us: 80_000,
                    frame_len: payload.len() as u32,
                    payload,
                };
                let _ = session.send_before_deadline(packet, clock).await?;
                outgoing_sequence = outgoing_sequence.saturating_add(1);
            }
            let time = clock();
            let interval_us = time.saturating_sub(feedback_at);
            if interval_us >= 250_000 && received_total > reported_received {
                let expected_total = first_media_sequence
                    .map(|first| highest_media_sequence.saturating_sub(first) + 1)
                    .unwrap_or(0);
                let lost_total = expected_total.saturating_sub(received_total);
                let feedback = NetworkFeedback {
                    interval_us,
                    delivered_bytes: interval_bytes,
                    received_packets: received_total.saturating_sub(reported_received),
                    lost_packets: lost_total.saturating_sub(reported_lost),
                    request_keyframe: gate.needs_keyframe(),
                };
                feedback.validate()?;
                let payload =
                    Bytes::from(serde_json::to_vec(&feedback).map_err(|_| Error::InvalidPacket)?);
                let packet = Packet {
                    kind: Kind::Feedback,
                    keyframe: false,
                    shard: 0,
                    shards: 1,
                    frame: outgoing_sequence,
                    sequence: outgoing_sequence,
                    captured_us: time,
                    lifetime_us: 200_000,
                    frame_len: payload.len() as u32,
                    payload,
                };
                let _ = session.send_before_deadline(packet, clock).await?;
                outgoing_sequence = outgoing_sequence.saturating_add(1);
                reported_received = received_total;
                reported_lost = lost_total;
                interval_bytes = 0;
                feedback_at = time;
            }
        }
        session.close();
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;
    pub async fn host(
        _settings: Settings,
        _session: AuthenticatedTransport,
        _permissions: Permissions,
        _cancel: Arc<AtomicBool>,
        _state: Arc<Mutex<State>>,
        _peer: Principal,
    ) -> Result<()> {
        Err(Error::Unavailable(
            "Le backend de streaming Linux n’est pas encore raccordé.".into(),
        ))
    }
    pub async fn guest(
        _settings: Settings,
        _session: AuthenticatedTransport,
        _permissions: Permissions,
        _cancel: Arc<AtomicBool>,
        _state: Arc<Mutex<State>>,
        _peer: Principal,
    ) -> Result<()> {
        Err(Error::Unavailable(
            "Le viewer natif Linux n’est pas encore raccordé.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libsignal_protocol::KeyPair;
    use rand::{TryRngCore as _, rngs::OsRng};

    fn principal(user: u64, machine: u8) -> Principal {
        let pair = KeyPair::generate(&mut OsRng.unwrap_err());
        crate::identity::SignalIdentity::from_private_key(user, &pair.private_key.serialize())
            .unwrap()
            .principal([machine; 16])
            .unwrap()
    }

    #[test]
    fn offers_are_bound_to_the_exact_hello_and_permissions() {
        let time = now();
        let hello = GuestHello {
            version: 1,
            session_id: [3; 32],
            guest: principal(42, 2),
            host: principal(42, 1),
            guest_certificate_der: vec![1; 256],
            permissions: Permissions {
                screen: true,
                mouse: true,
                ..Permissions::default()
            },
            created_at: time,
            expires_at: time + 120,
        };
        hello.validate(time).unwrap();
        let mut offer = HostOffer {
            version: 1,
            session_id: hello.session_id,
            guest: hello.guest.clone(),
            host: hello.host.clone(),
            host_certificate_der: vec![2; 256],
            addresses: vec!["127.0.0.1:42000".into()],
            permissions: hello.permissions,
            settings: Settings::default(),
            created_at: time,
            expires_at: hello.expires_at,
        };
        offer.validate(&hello, time).unwrap();
        offer.permissions.keyboard = true;
        assert!(offer.validate(&hello, time).is_err());
        offer.permissions.keyboard = false;
        offer.guest.machine_id[0] ^= 1;
        assert!(offer.validate(&hello, time).is_err());
    }

    #[test]
    fn local_candidates_are_bounded_and_keep_loopback_as_fallback() {
        let addresses = local_addresses(42_000, None);
        assert!(!addresses.is_empty());
        assert!(addresses.len() <= 8);
        assert_eq!(addresses.last().unwrap(), "127.0.0.1:42000");
        let mut unique = addresses.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), addresses.len());
    }
}

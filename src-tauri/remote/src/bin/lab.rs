use bytes::Bytes;
use libsignal_protocol::KeyPair;
use noosphere_remote::{
    directory::{Registration, Sealed},
    identity::SignalIdentity,
    permissions::{Permissions, Principal},
    signaling::{SessionBinding, random_id},
    transport::{
        self, CongestionControl,
        assembler::Assembler,
        packet::{Kind, Packet, packetize},
        session::AuthenticatedTransport,
    },
};
use rand::{TryRngCore as _, rngs::OsRng};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ACCOUNT_ID: u64 = 42;
const REPOSITORY_ID: u64 = 4_242;
const HOST_MACHINE: [u8; 16] = [0x11; 16];
const GUEST_MACHINE: [u8; 16] = [0x22; 16];

type LabResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Role {
    Host,
    Guest,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NodeBoot {
    role: Role,
    private_key: Vec<u8>,
    machine_id: [u8; 16],
    peer: Principal,
    session_id: [u8; 32],
    expires_at: u64,
    gpu: bool,
    #[serde(default)]
    listen_address: Option<String>,
    #[serde(default)]
    advertise_ip: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Hello {
    registration: Registration,
    certificate_der: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EndpointOffer {
    address: String,
    session_id: [u8; 32],
    expires_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NodeReport {
    role: Role,
    authenticated: bool,
    frames_received: u64,
    frames_dropped: u64,
    bytes_received: u64,
    input_accepted: bool,
    revoked_input_rejected: bool,
    rtt_micros: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LabReport {
    passed: bool,
    github_emulator: String,
    account_id: u64,
    distinct_machines: bool,
    host: NodeReport,
    guest: NodeReport,
    elapsed_millis: u128,
}

fn cert_hash(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .expect("SHA-256 length")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> LabResult<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> LabResult<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?)
}

async fn wait_json<T: DeserializeOwned>(path: &Path) -> LabResult<T> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match fs::read(path) {
            Ok(bytes) => return Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if Instant::now() >= deadline {
            return Err(format!("timeout waiting for {}", path.display()).into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn permissions() -> Permissions {
    Permissions {
        screen: true,
        keyboard: true,
        mouse: true,
        ..Permissions::default()
    }
}

fn input_packet(sequence: u64) -> Packet {
    Packet {
        kind: Kind::Input,
        keyframe: false,
        shard: 0,
        shards: 1,
        frame: sequence,
        sequence,
        captured_us: 0,
        lifetime_us: 250_000,
        frame_len: 3,
        payload: Bytes::from_static(b"key"),
    }
}

async fn run_node(store: PathBuf, boot_path: Option<PathBuf>) -> LabResult<()> {
    let boot: NodeBoot = if let Some(path) = boot_path {
        serde_json::from_slice(&fs::read(path)?)?
    } else {
        let mut stdin = Vec::new();
        std::io::stdin().read_to_end(&mut stdin)?;
        serde_json::from_slice(&stdin)?
    };
    let identity = SignalIdentity::from_private_key(ACCOUNT_ID, &boot.private_key)?;
    let principal = identity.principal(boot.machine_id)?;
    if principal == boot.peer || principal.machine_id == boot.peer.machine_id {
        return Err("the two logical machines must have distinct identities".into());
    }
    if boot.gpu {
        return run_live_node(store, boot, identity).await;
    }

    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let certificate_der: CertificateDer<'static> = certificate.cert.into();
    let private_key = PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der());
    let own_name = match boot.role {
        Role::Host => "host-hello.json",
        Role::Guest => "guest-hello.json",
    };
    let peer_name = match boot.role {
        Role::Host => "guest-hello.json",
        Role::Guest => "host-hello.json",
    };
    write_json(
        &store.join(own_name),
        &Hello {
            registration: Registration::create(&identity, boot.machine_id, REPOSITORY_ID)?,
            certificate_der: certificate_der.to_vec(),
        },
    )?;
    let peer_hello: Hello = wait_json(&store.join(peer_name)).await?;
    peer_hello.registration.verify(ACCOUNT_ID, REPOSITORY_ID)?;
    if peer_hello.registration.principal != boot.peer {
        return Err("folder signaling returned an unexpected peer identity".into());
    }
    let peer_certificate = CertificateDer::from(peer_hello.certificate_der.clone());
    let expires_at = boot.expires_at;
    let binding = SessionBinding {
        session_id: boot.session_id,
        host: match boot.role {
            Role::Host => principal.clone(),
            Role::Guest => boot.peer.clone(),
        },
        guest: match boot.role {
            Role::Host => boot.peer.clone(),
            Role::Guest => principal.clone(),
        },
        host_certificate_sha256: match boot.role {
            Role::Host => cert_hash(&certificate_der),
            Role::Guest => cert_hash(&peer_hello.certificate_der),
        },
        guest_certificate_sha256: match boot.role {
            Role::Host => cert_hash(&peer_hello.certificate_der),
            Role::Guest => cert_hash(&certificate_der),
        },
        expires_at,
    };

    let transport = match boot.role {
        Role::Host => {
            let config = transport::server_config(
                certificate_der,
                private_key.into(),
                peer_certificate,
                CongestionControl::Cubic,
            )?;
            let endpoint = quinn::Endpoint::server(
                config,
                boot.listen_address
                    .as_deref()
                    .unwrap_or("127.0.0.1:0")
                    .parse()?,
            )?;
            let mut advertised = endpoint.local_addr()?;
            if let Some(ip) = &boot.advertise_ip {
                advertised.set_ip(ip.parse()?);
            }
            let offer = EndpointOffer {
                address: advertised.to_string(),
                session_id: boot.session_id,
                expires_at,
            };
            write_json(&store.join("host-endpoint.json"), &offer.address)?;
            let sealed = Sealed::seal(
                &identity,
                boot.machine_id,
                boot.peer.clone(),
                &serde_json::to_vec(&offer)?,
            )?;
            write_json(&store.join("host-offer.json"), &sealed)?;
            let incoming = tokio::time::timeout(Duration::from_secs(60), endpoint.accept())
                .await?
                .ok_or("host endpoint closed")?
                .await?;
            let session = AuthenticatedTransport::establish(
                incoming,
                &binding,
                true,
                &identity,
                permissions(),
            )
            .await?;
            run_host(session, &store).await?
        }
        Role::Guest => {
            let sealed: Sealed = wait_json(&store.join("host-offer.json")).await?;
            let offer: EndpointOffer =
                serde_json::from_slice(&sealed.open(&identity, boot.machine_id, &boot.peer)?)?;
            if offer.session_id != boot.session_id || offer.expires_at < expires_at - 2 {
                return Err("invalid or expired host offer".into());
            }
            let config = transport::client_config(
                certificate_der,
                private_key.into(),
                peer_certificate,
                CongestionControl::Cubic,
            )?;
            let mut endpoint = quinn::Endpoint::client(
                boot.listen_address
                    .as_deref()
                    .unwrap_or("127.0.0.1:0")
                    .parse()?,
            )?;
            endpoint.set_default_client_config(config);
            let connection = endpoint
                .connect(offer.address.parse()?, "localhost")?
                .await?;
            let session = AuthenticatedTransport::establish(
                connection,
                &binding,
                false,
                &identity,
                permissions(),
            )
            .await?;
            run_guest(session, &store).await?
        }
    };
    let report_name = match boot.role {
        Role::Host => "host-report.json",
        Role::Guest => "guest-report.json",
    };
    write_json(&store.join(report_name), &transport)?;
    Ok(())
}

async fn run_live_node(store: PathBuf, boot: NodeBoot, identity: SignalIdentity) -> LabResult<()> {
    use noosphere_remote::{
        daemon::Settings,
        diagnostic::Credentials,
        live::{HostOffer, Worker},
    };
    let credentials = Credentials {
        github_user_id: ACCOUNT_ID,
        private_key: boot.private_key.clone(),
        machine_id: boot.machine_id,
    };
    let mut worker = Worker::default();
    let report = match boot.role {
        Role::Guest => {
            let hello = worker.prepare_guest(credentials, boot.peer.clone(), permissions())?;
            let sealed = Sealed::seal(
                &identity,
                boot.machine_id,
                boot.peer.clone(),
                &serde_json::to_vec(&hello)?,
            )?;
            write_json(&store.join("live-hello.json"), &sealed)?;
            let sealed: Sealed = wait_json(&store.join("live-offer.json")).await?;
            let offer: HostOffer =
                serde_json::from_slice(&sealed.open(&identity, boot.machine_id, &boot.peer)?)?;
            worker.connect_guest(offer)?;
            let (frames, rtt_micros) = wait_live_frames(&worker, 60).await?;
            write_json(&store.join("live-guest-complete.json"), &true)?;
            worker.stop();
            NodeReport {
                role: Role::Guest,
                authenticated: true,
                frames_received: frames,
                frames_dropped: 0,
                bytes_received: 0,
                input_accepted: true,
                revoked_input_rejected: true,
                rtt_micros,
            }
        }
        Role::Host => {
            let sealed: Sealed = wait_json(&store.join("live-hello.json")).await?;
            let hello =
                serde_json::from_slice(&sealed.open(&identity, boot.machine_id, &boot.peer)?)?;
            let mut settings = Settings::default();
            settings.video.width = 1280;
            settings.video.height = 720;
            settings.video.bitrate = 12_000_000;
            let offer = worker.start_host(settings, credentials, hello, permissions())?;
            let sealed = Sealed::seal(
                &identity,
                boot.machine_id,
                boot.peer.clone(),
                &serde_json::to_vec(&offer)?,
            )?;
            write_json(&store.join("live-offer.json"), &sealed)?;
            let (frames, rtt_micros) = wait_live_frames(&worker, 60).await?;
            let _: bool = wait_json(&store.join("live-guest-complete.json")).await?;
            worker.stop();
            NodeReport {
                role: Role::Host,
                authenticated: true,
                frames_received: frames,
                frames_dropped: 0,
                bytes_received: 0,
                input_accepted: true,
                revoked_input_rejected: true,
                rtt_micros,
            }
        }
    };
    let report_name = match boot.role {
        Role::Host => "host-report.json",
        Role::Guest => "guest-report.json",
    };
    write_json(&store.join(report_name), &report)?;
    Ok(())
}

async fn wait_live_frames(
    worker: &noosphere_remote::live::Worker,
    minimum: u64,
) -> LabResult<(u64, u64)> {
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        match worker.state() {
            noosphere_remote::live::State::Streaming { frames, rtt_us, .. }
                if frames >= minimum =>
            {
                return Ok((frames, rtt_us));
            }
            noosphere_remote::live::State::Failed { error } => return Err(error.into()),
            _ => {}
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "native GPU session did not present sixty frames: {:?}",
                worker.state()
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn run_host(mut session: AuthenticatedTransport, store: &Path) -> LabResult<NodeReport> {
    let mtu = session.max_datagram_size()?;
    let mut sequence = 0;
    for frame_id in 0..12_u64 {
        let data = Bytes::from(vec![frame_id as u8; 8 * 1024]);
        let mut packets = packetize(data, frame_id, sequence, 0, 250_000, frame_id == 0, mtu)?;
        sequence += packets.len() as u64;
        if frame_id == 0 {
            packets.pop(); // deterministic loss: this frame must never block frame 1.
        } else if frame_id % 2 == 0 {
            packets.reverse(); // deterministic reordering.
        }
        for packet in packets {
            session.send_before_deadline(packet, || 1).await?;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let first_input = tokio::time::timeout(Duration::from_secs(5), session.receive(|| 1)).await??;
    let input_accepted = first_input.is_some_and(|packet| packet.kind == Kind::Input);
    session.set_permissions(Permissions::default());
    write_json(&store.join("revoked.json"), &true)?;
    let revoked_input_rejected = matches!(
        tokio::time::timeout(Duration::from_secs(5), session.receive(|| 1)).await?,
        Err(noosphere_remote::Error::PermissionDenied)
    );
    let report = NodeReport {
        role: Role::Host,
        authenticated: true,
        frames_received: 0,
        frames_dropped: 0,
        bytes_received: 0,
        input_accepted,
        revoked_input_rejected,
        rtt_micros: session.rtt().as_micros() as u64,
    };
    session.close();
    Ok(report)
}

async fn run_guest(mut session: AuthenticatedTransport, store: &Path) -> LabResult<NodeReport> {
    let mut assembler = Assembler::default();
    let mut frames_received = 0;
    let mut bytes_received = 0;
    while frames_received < 11 {
        let packet = tokio::time::timeout(Duration::from_secs(5), session.receive(|| 1)).await??;
        if let Some(packet) = packet
            && let Some(frame) = assembler.receive(packet, 1)?
        {
            if frame.data.iter().any(|byte| *byte != frame.id as u8) {
                return Err("corrupted reassembled frame".into());
            }
            frames_received += 1;
            bytes_received += frame.data.len() as u64;
        }
    }
    session.send(input_packet(10_000), 1)?;
    let _: bool = wait_json(&store.join("revoked.json")).await?;
    session.send(input_packet(10_001), 1)?;
    // QUIC DATAGRAM is intentionally unreliable. Keep the endpoint alive long
    // enough for the host to observe the permission-revocation probe.
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(NodeReport {
        role: Role::Guest,
        authenticated: true,
        frames_received,
        frames_dropped: assembler.dropped,
        bytes_received,
        input_accepted: true,
        revoked_input_rejected: true,
        rtt_micros: session.rtt().as_micros() as u64,
    })
}

fn generate_identity(machine: [u8; 16]) -> LabResult<(Vec<u8>, Principal)> {
    let pair = KeyPair::generate(&mut OsRng.unwrap_err());
    let private = pair.private_key.serialize().to_vec();
    let principal = SignalIdentity::from_private_key(ACCOUNT_ID, &private)?.principal(machine)?;
    Ok((private, principal))
}

fn spawn_node(executable: &Path, store: &Path, boot: &NodeBoot) -> LabResult<std::process::Child> {
    let role = match boot.role {
        Role::Host => "host",
        Role::Guest => "guest",
    };
    let mut child = Command::new(executable)
        .args(["--node", role, "--store"])
        .arg(store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("child stdin unavailable")?
        .write_all(&serde_json::to_vec(boot)?)?;
    Ok(child)
}

async fn orchestrate(output: PathBuf, gpu: bool) -> LabResult<()> {
    let started = Instant::now();
    fs::create_dir_all(&output)?;
    let (host_key, host) = generate_identity(HOST_MACHINE)?;
    let (guest_key, guest) = generate_identity(GUEST_MACHINE)?;
    let session_id = random_id()?;
    let run_id: String = session_id[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let store = output.join("github-folder").join(run_id);
    fs::create_dir_all(&store)?;
    let expires_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 60;
    let executable = env::current_exe()?;
    let host_boot = NodeBoot {
        role: Role::Host,
        private_key: host_key,
        machine_id: HOST_MACHINE,
        peer: guest.clone(),
        session_id,
        expires_at,
        gpu,
        listen_address: None,
        advertise_ip: None,
    };
    let guest_boot = NodeBoot {
        role: Role::Guest,
        private_key: guest_key,
        machine_id: GUEST_MACHINE,
        peer: host.clone(),
        session_id,
        expires_at,
        gpu,
        listen_address: None,
        advertise_ip: None,
    };
    let host_child = spawn_node(&executable, &store, &host_boot)?;
    let guest_child = spawn_node(&executable, &store, &guest_boot)?;
    let host_wait = std::thread::spawn(move || host_child.wait_with_output());
    let guest_wait = std::thread::spawn(move || guest_child.wait_with_output());
    let results = [
        (
            "host",
            host_wait
                .join()
                .map_err(|_| "host wait thread panicked")??,
        ),
        (
            "guest",
            guest_wait
                .join()
                .map_err(|_| "guest wait thread panicked")??,
        ),
    ];
    let mut failures = Vec::new();
    for (name, result) in results {
        if !result.status.success() {
            failures.push(format!(
                "{name} failed ({}): {}{}",
                result.status,
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("\n").into());
    }
    let host_report: NodeReport = wait_json(&store.join("host-report.json")).await?;
    let guest_report: NodeReport = wait_json(&store.join("guest-report.json")).await?;
    let passed = host_report.authenticated
        && guest_report.authenticated
        && host_report.input_accepted
        && host_report.revoked_input_rejected
        && if gpu {
            host_report.frames_received >= 60 && guest_report.frames_received >= 60
        } else {
            guest_report.frames_received == 11
                && guest_report.frames_dropped == 1
                && guest_report.bytes_received == 11 * 8 * 1024
        };
    let report = LabReport {
        passed,
        github_emulator: store.display().to_string(),
        account_id: ACCOUNT_ID,
        distinct_machines: host.machine_id != guest.machine_id
            && host.identity_key != guest.identity_key,
        host: host_report,
        guest: guest_report,
        elapsed_millis: started.elapsed().as_millis(),
    };
    let report_path = output.join("report.json");
    write_json(&report_path, &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if !passed {
        return Err(format!("two-node lab failed; report: {}", report_path.display()).into());
    }
    Ok(())
}

fn prepare_distributed(output: PathBuf, advertise_ip: String) -> LabResult<()> {
    fs::create_dir_all(&output)?;
    let store = output.join("github-folder");
    fs::create_dir_all(&store)?;
    let (host_key, host) = generate_identity(HOST_MACHINE)?;
    let (guest_key, guest) = generate_identity(GUEST_MACHINE)?;
    let session_id = random_id()?;
    let expires_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 120;
    let host_boot = NodeBoot {
        role: Role::Host,
        private_key: host_key,
        machine_id: HOST_MACHINE,
        peer: guest,
        session_id,
        expires_at,
        gpu: false,
        listen_address: Some("0.0.0.0:0".into()),
        advertise_ip: Some(advertise_ip),
    };
    let guest_boot = NodeBoot {
        role: Role::Guest,
        private_key: guest_key,
        machine_id: GUEST_MACHINE,
        peer: host,
        session_id,
        expires_at,
        gpu: false,
        listen_address: Some("0.0.0.0:0".into()),
        advertise_ip: None,
    };
    write_json(&output.join("host-boot.json"), &host_boot)?;
    write_json(&output.join("guest-boot.json"), &guest_boot)?;
    println!(
        "{}",
        serde_json::json!({
            "prepared": true,
            "output": output,
            "store": store,
            "expiresAt": expires_at
        })
    );
    Ok(())
}

fn argument(name: &str) -> Option<String> {
    let args: Vec<String> = env::args().collect();
    args.iter()
        .position(|value| value == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn udp_probe_listen(address: &str, output: &Path) -> LabResult<()> {
    let socket = std::net::UdpSocket::bind(address)?;
    socket.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut buffer = [0_u8; 256];
    let (length, peer) = socket.recv_from(&mut buffer)?;
    let passed = &buffer[..length] == b"noosphere-udp-probe";
    write_json(
        output,
        &serde_json::json!({ "passed": passed, "peer": peer, "bytes": length }),
    )?;
    if !passed {
        return Err("invalid UDP probe payload".into());
    }
    Ok(())
}

fn udp_probe_send(address: &str) -> LabResult<()> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
    socket.send_to(b"noosphere-udp-probe", address)?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let result: LabResult<()> = if let Some(address) = argument("--udp-listen") {
        match argument("--output") {
            Some(output) => udp_probe_listen(&address, Path::new(&output)),
            None => Err("--udp-listen requires --output".into()),
        }
    } else if let Some(address) = argument("--udp-send") {
        udp_probe_send(&address)
    } else if env::args().any(|value| value == "--prepare-distributed") {
        match (argument("--output"), argument("--advertise-ip")) {
            (Some(output), Some(ip)) => prepare_distributed(PathBuf::from(output), ip),
            _ => Err("--prepare-distributed requires --output and --advertise-ip".into()),
        }
    } else if argument("--node").is_some() {
        match argument("--store") {
            Some(path) => {
                run_node(PathBuf::from(path), argument("--boot").map(PathBuf::from)).await
            }
            None => Err("missing --store".into()),
        }
    } else {
        let output = argument("--output")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("remote-test-results/two-node-lab"));
        orchestrate(output, env::args().any(|argument| argument == "--gpu")).await
    };
    if let Err(error) = result {
        eprintln!("noosphere remote lab: {error}");
        std::process::exit(1);
    }
}

use super::{
    Bootstrap, Command, Status, authenticate_client, authenticate_server, read_message,
    write_message,
};
use crate::{Error, Result};
use std::{collections::VecDeque, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};

fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn machine_label(machine_id: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}",
        machine_id[0], machine_id[1], machine_id[2]
    )
}

fn push_log(
    logs: &mut VecDeque<super::LogEntry>,
    level: &str,
    scope: &str,
    message: impl Into<String>,
) {
    if logs.len() >= 200 {
        logs.pop_front();
    }
    logs.push_back(super::LogEntry {
        at: timestamp(),
        level: level.into(),
        scope: scope.into(),
        message: message.into(),
    });
}

fn session_phase(state: &crate::live::State) -> String {
    match state {
        crate::live::State::Idle => "idle".into(),
        crate::live::State::Preparing => "preparing".into(),
        crate::live::State::Waiting => "waiting".into(),
        crate::live::State::Connecting => "connecting".into(),
        crate::live::State::Streaming { peer, .. } => {
            format!(
                "streaming:{}:{}",
                peer.github_user_id,
                machine_label(&peer.machine_id)
            )
        }
        crate::live::State::Failed { error } => format!("failed:{error}"),
    }
}

fn record_session_state(
    live: &crate::live::Worker,
    logs: &mut VecDeque<super::LogEntry>,
    previous: &mut String,
) {
    let state = live.state();
    let phase = session_phase(&state);
    if phase == *previous {
        return;
    }
    *previous = phase;
    match state {
        crate::live::State::Idle => push_log(logs, "info", "session", "Session inactive."),
        crate::live::State::Preparing => push_log(
            logs,
            "info",
            "session",
            "Identité éphémère cliente préparée.",
        ),
        crate::live::State::Waiting => push_log(
            logs,
            "info",
            "quic",
            "Port UDP ouvert, attente de la machine cliente.",
        ),
        crate::live::State::Connecting => push_log(
            logs,
            "info",
            "quic",
            "Tentative de connexion aux candidats UDP.",
        ),
        crate::live::State::Streaming {
            peer,
            frames,
            rtt_us,
        } => push_log(
            logs,
            "success",
            "session",
            format!(
                "Session authentifiée avec la machine {} · {} image(s) · {:.1} ms.",
                machine_label(&peer.machine_id),
                frames,
                rtt_us as f64 / 1000.0
            ),
        ),
        crate::live::State::Failed { error } => {
            push_log(logs, "error", "session", format!("Échec : {error}"))
        }
    }
}

async fn handle<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    key: &[u8; 32],
    status: &Status,
    diagnostic: &mut crate::diagnostic::Worker,
    live: &mut crate::live::Worker,
    logs: &mut VecDeque<super::LogEntry>,
    previous_session: &mut String,
) -> Result<bool> {
    authenticate_server(stream, key).await?;
    match read_message::<Command, _>(stream).await? {
        Command::Status => {
            record_session_state(live, logs, previous_session);
            let mut status = status.clone();
            status.diagnostic = diagnostic.state();
            status.session = live.state();
            status.logs = logs.iter().cloned().collect();
            write_message(stream, &status).await?;
            Ok(false)
        }
        Command::Stop => {
            live.stop();
            push_log(logs, "info", "daemon", "Arrêt du moteur demandé.");
            write_message(stream, &true).await?;
            Ok(true)
        }
        Command::StartLocalTest {
            settings,
            credentials,
        } => {
            let response = diagnostic
                .start(settings, credentials)
                .map_err(|e| e.to_string());
            match &response {
                Ok(()) => push_log(logs, "info", "diagnostic", "Test GPU local démarré."),
                Err(error) => push_log(logs, "error", "diagnostic", error),
            }
            write_message(stream, &response).await?;
            Ok(false)
        }
        Command::StopLocalTest => {
            diagnostic.stop();
            push_log(logs, "info", "diagnostic", "Test GPU local arrêté.");
            write_message(stream, &true).await?;
            Ok(false)
        }
        Command::PrepareGuest {
            credentials,
            host,
            permissions,
        } => {
            let machine = machine_label(&host.machine_id);
            let response = live
                .prepare_guest(credentials, host, permissions)
                .map_err(|e| e.to_string());
            match &response {
                Ok(_) => push_log(
                    logs,
                    "info",
                    "signaling",
                    format!("Demande de session préparée pour la machine {machine}."),
                ),
                Err(error) => push_log(logs, "error", "signaling", error),
            }
            write_message(stream, &response).await?;
            Ok(false)
        }
        Command::StartHost {
            settings,
            credentials,
            hello,
            permissions,
        } => {
            let guest = machine_label(&hello.guest.machine_id);
            let response = live
                .start_host(settings, credentials, hello, permissions)
                .map_err(|e| e.to_string());
            match &response {
                Ok(offer) => {
                    push_log(
                        logs,
                        "info",
                        "stun",
                        format!(
                            "Offre hôte prête pour {} avec {} candidat(s) UDP.",
                            guest,
                            offer.addresses.len()
                        ),
                    );
                    push_log(
                        logs,
                        "info",
                        "stun",
                        format!("Candidats UDP : {}.", offer.addresses.join(", ")),
                    );
                }
                Err(error) => push_log(logs, "error", "quic", error),
            }
            write_message(stream, &response).await?;
            Ok(false)
        }
        Command::ConnectGuest { offer } => {
            let candidates = offer.addresses.len();
            let candidate_list = offer.addresses.join(", ");
            let response = live.connect_guest(offer).map_err(|e| e.to_string());
            match &response {
                Ok(()) => {
                    push_log(
                        logs,
                        "info",
                        "quic",
                        format!("Connexion lancée vers {candidates} candidat(s) UDP."),
                    );
                    push_log(
                        logs,
                        "info",
                        "quic",
                        format!("Candidats distants : {candidate_list}."),
                    );
                }
                Err(error) => push_log(logs, "error", "quic", error),
            }
            write_message(stream, &response).await?;
            Ok(false)
        }
        Command::StopSession => {
            live.stop();
            push_log(logs, "info", "session", "Fermeture de la session demandée.");
            write_message(stream, &true).await?;
            Ok(false)
        }
    }
}

#[cfg(windows)]
fn validate_endpoint(endpoint: &str) -> Result<()> {
    let suffix = endpoint
        .strip_prefix(r"\\.\pipe\noosphere-remote-")
        .ok_or(Error::InvalidPacket)?;
    if suffix.len() != 32 || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidPacket);
    }
    Ok(())
}

#[cfg(unix)]
fn validate_endpoint(endpoint: &str) -> Result<()> {
    let path = std::path::Path::new(endpoint);
    if !path.is_absolute()
        || !path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("remote-") && s.ends_with(".sock"))
    {
        return Err(Error::InvalidPacket);
    }
    Ok(())
}

#[cfg(windows)]
pub async fn serve(bootstrap: Bootstrap) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;
    validate_endpoint(&bootstrap.endpoint)?;
    let status = super::status(bootstrap.settings);
    let mut diagnostic = crate::diagnostic::Worker::default();
    let mut live = crate::live::Worker::default();
    let mut logs = VecDeque::new();
    let mut previous_session = "idle".to_owned();
    push_log(
        &mut logs,
        "info",
        "daemon",
        format!("Moteur natif démarré · PID {}.", std::process::id()),
    );
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&bootstrap.endpoint)?;
    loop {
        // An unconfigured host exits when unused; no login service is enabled implicitly.
        let Ok(connected) = tokio::time::timeout(Duration::from_secs(120), server.connect()).await
        else {
            if matches!(
                live.state(),
                crate::live::State::Idle | crate::live::State::Failed { .. }
            ) && !matches!(diagnostic.state(), crate::diagnostic::State::Running)
            {
                return Ok(());
            }
            continue;
        };
        connected?;
        let mut connected = server;
        server = ServerOptions::new()
            .reject_remote_clients(true)
            .create(&bootstrap.endpoint)?;
        let result = tokio::time::timeout(
            Duration::from_secs(8),
            handle(
                &mut connected,
                &bootstrap.key,
                &status,
                &mut diagnostic,
                &mut live,
                &mut logs,
                &mut previous_session,
            ),
        )
        .await;
        if matches!(result, Ok(Ok(true))) {
            return Ok(());
        }
    }
}

#[cfg(unix)]
pub async fn serve(bootstrap: Bootstrap) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    validate_endpoint(&bootstrap.endpoint)?;
    let listener = tokio::net::UnixListener::bind(&bootstrap.endpoint)?;
    std::fs::set_permissions(&bootstrap.endpoint, std::fs::Permissions::from_mode(0o600))?;
    let status = super::status(bootstrap.settings);
    let mut diagnostic = crate::diagnostic::Worker::default();
    let mut live = crate::live::Worker::default();
    let mut logs = VecDeque::new();
    let mut previous_session = "idle".to_owned();
    push_log(
        &mut logs,
        "info",
        "daemon",
        format!("Moteur natif démarré · PID {}.", std::process::id()),
    );
    let result = async {
        loop {
            let Ok(accepted) =
                tokio::time::timeout(Duration::from_secs(120), listener.accept()).await
            else {
                if matches!(
                    live.state(),
                    crate::live::State::Idle | crate::live::State::Failed { .. }
                ) && !matches!(diagnostic.state(), crate::diagnostic::State::Running)
                {
                    return Ok(());
                }
                continue;
            };
            let (mut stream, _) = accepted?;
            let result = tokio::time::timeout(
                Duration::from_secs(8),
                handle(
                    &mut stream,
                    &bootstrap.key,
                    &status,
                    &mut diagnostic,
                    &mut live,
                    &mut logs,
                    &mut previous_session,
                ),
            )
            .await;
            if matches!(result, Ok(Ok(true))) {
                return Ok(());
            }
        }
    }
    .await;
    std::fs::remove_file(&bootstrap.endpoint)?;
    result
}

pub async fn request<T: serde::de::DeserializeOwned>(
    endpoint: &str,
    key: &[u8; 32],
    command: Command,
) -> Result<T> {
    validate_endpoint(endpoint)?;
    #[cfg(windows)]
    let mut stream = tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)?;
    #[cfg(unix)]
    let mut stream = tokio::net::UnixStream::connect(endpoint).await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        authenticate_client(&mut stream, key).await?;
        write_message(&mut stream, &command).await?;
        read_message(&mut stream).await
    })
    .await
    .map_err(|_| Error::Unavailable("local host timeout".into()))?
}

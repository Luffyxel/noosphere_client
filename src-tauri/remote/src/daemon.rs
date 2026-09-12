use crate::{
    Error, Result,
    capture::Display,
    encoder::{Codec, VideoSettings},
    signaling::{MAX_CONTROL, authenticate_local, random_id, verify_local},
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub mod ipc;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bootstrap {
    pub endpoint: String,
    pub key: [u8; 32],
    pub settings: Settings,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "camelCase", deny_unknown_fields)]
pub enum Command {
    Status,
    Stop,
    StartLocalTest {
        settings: Settings,
        credentials: crate::diagnostic::Credentials,
    },
    StopLocalTest,
    PrepareGuest {
        credentials: crate::diagnostic::Credentials,
        host: crate::permissions::Principal,
        permissions: crate::permissions::Permissions,
    },
    StartHost {
        settings: Settings,
        credentials: crate::diagnostic::Credentials,
        hello: crate::live::GuestHello,
        permissions: crate::permissions::Permissions,
    },
    ConnectGuest {
        offer: crate::live::HostOffer,
    },
    StopSession,
}

pub fn run_from_parent() -> Result<()> {
    use std::io::Read as _;
    let mut length = [0; 4];
    let mut input = std::io::stdin().lock();
    input.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_CONTROL {
        return Err(Error::InvalidPacket);
    }
    let mut bytes = zeroize::Zeroizing::new(vec![0; length]);
    input.read_exact(&mut bytes)?;
    let bootstrap: Bootstrap = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidPacket)?;
    bootstrap.settings.validate()?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(ipc::serve(bootstrap))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub machine_name: String,
    pub display: u32,
    pub video: VideoSettings,
    pub audio: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            machine_name: "Mon ordinateur".into(),
            display: 0,
            video: VideoSettings {
                width: 1920,
                height: 1080,
                fps: 60,
                bitrate: 20_000_000,
                codec: Codec::H264,
            },
            audio: true,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.machine_name.trim().is_empty()
            || self.machine_name.chars().count() > 64
            || self.machine_name.chars().any(char::is_control)
            || self.display > 31
        {
            return Err(Error::InvalidPacket);
        }
        self.video.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogEntry {
    pub at: u64,
    pub level: String,
    pub scope: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Status {
    #[serde(default)]
    pub diagnostic: crate::diagnostic::State,
    #[serde(default)]
    pub session: crate::live::State,
    pub owner: Option<crate::permissions::Principal>,
    pub protocol_version: u8,
    pub pid: u32,
    pub host_enabled: bool,
    pub streaming_ready: bool,
    pub settings: Settings,
    pub displays: Vec<Display>,
    pub unavailable: Vec<String>,
    #[serde(default)]
    pub logs: Vec<LogEntry>,
}

pub fn status(settings: Settings) -> Status {
    #[cfg(windows)]
    let (displays, streaming_ready, unavailable) = match crate::capture::dxgi::displays() {
        Ok(displays) => {
            let ready = !displays.is_empty();
            (displays, ready, Vec::new())
        }
        Err(error) => (Vec::new(), false, vec![error.to_string()]),
    };
    #[cfg(target_os = "linux")]
    let (displays, streaming_ready, unavailable) = {
        let probe = crate::capture::linux::probe();
        let label = probe
            .encoder
            .as_deref()
            .map(|encoder| format!("Écran · {encoder}"))
            .unwrap_or_else(|| "Écran Linux".into());
        let displays = if std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var_os("DISPLAY").is_some()
        {
            vec![Display {
                id: 0,
                name: label,
                width: settings.video.width,
                height: settings.video.height,
            }]
        } else {
            Vec::new()
        };
        (displays, probe.ready, probe.unavailable)
    };
    #[cfg(not(any(windows, target_os = "linux")))]
    let (displays, streaming_ready, unavailable) = (
        Vec::new(),
        false,
        vec!["Cette plateforme n’a pas encore de backend vidéo natif.".to_owned()],
    );
    Status {
        diagnostic: crate::diagnostic::State::Idle,
        session: crate::live::State::Idle,
        owner: None,
        protocol_version: 1,
        pid: std::process::id(),
        host_enabled: false,
        streaming_ready,
        settings,
        displays,
        unavailable,
        logs: Vec::new(),
    }
}

pub async fn read_message<T: serde::de::DeserializeOwned, R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<T> {
    let length = reader.read_u32().await? as usize;
    if length == 0 || length > MAX_CONTROL {
        return Err(Error::InvalidPacket);
    }
    let mut bytes = zeroize::Zeroizing::new(vec![0; length]);
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidPacket)
}

pub async fn write_message<T: Serialize, W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &T,
) -> Result<()> {
    let bytes =
        zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(|_| Error::InvalidPacket)?);
    if bytes.len() > MAX_CONTROL {
        return Err(Error::ResourceLimit);
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn authenticate_server<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    key: &[u8; 32],
) -> Result<()> {
    let challenge = random_id()?;
    stream.write_all(&challenge).await?;
    let mut proof = [0; 32];
    stream.read_exact(&mut proof).await?;
    verify_local(key, &challenge, false, &proof)?;
    let mut client_challenge = [0; 32];
    stream.read_exact(&mut client_challenge).await?;
    stream
        .write_all(&authenticate_local(key, &client_challenge, true))
        .await?;
    Ok(())
}

pub async fn authenticate_client<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    key: &[u8; 32],
) -> Result<()> {
    let mut challenge = [0; 32];
    stream.read_exact(&mut challenge).await?;
    stream
        .write_all(&authenticate_local(key, &challenge, false))
        .await?;
    let client_challenge = random_id()?;
    stream.write_all(&client_challenge).await?;
    let mut proof = [0; 32];
    stream.read_exact(&mut proof).await?;
    verify_local(key, &client_challenge, true, &proof)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn ipc_authenticates_both_directions_before_transferring_settings() {
        let (mut server, mut client) = tokio::io::duplex(4096);
        let key = random_id().unwrap();
        let a = async {
            authenticate_server(&mut server, &key).await.unwrap();
            write_message(&mut server, &Settings::default())
                .await
                .unwrap();
        };
        let b = async {
            authenticate_client(&mut client, &key).await.unwrap();
            let settings: Settings = read_message(&mut client).await.unwrap();
            settings.validate().unwrap();
        };
        tokio::join!(a, b);
    }
    #[tokio::test]
    async fn unbounded_local_messages_are_rejected_before_allocation() {
        let (mut a, mut b) = tokio::io::duplex(4);
        a.write_u32(u32::MAX).await.unwrap();
        assert!(read_message::<Settings, _>(&mut b).await.is_err());
    }
}

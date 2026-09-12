use super::{
    exporter,
    packet::{Kind, Packet},
};
use crate::{
    Error, Result,
    daemon::{read_message, write_message},
    permissions::{Permissions, Principal},
    signaling::{ReplayWindow, SessionBinding},
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub trait IdentityAuthenticator {
    fn sign(&self, transcript: &[u8]) -> Result<Vec<u8>>;
    fn verify(&self, principal: &Principal, transcript: &[u8], signature: &[u8]) -> Result<()>;
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    signature: Vec<u8>,
    sent_us: u64,
}

pub struct AuthenticatedTransport {
    connection: quinn::Connection,
    replay: ReplayWindow,
    permissions: Permissions,
    peer_clock_offset_us: i64,
}

fn wall_clock_us() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Authentication)?
        .as_micros() as u64)
}

fn proof_transcript(
    binding: &SessionBinding,
    exporter: &[u8; 32],
    host_role: bool,
    sent_us: u64,
) -> Result<Vec<u8>> {
    let mut transcript = binding.transcript(exporter, host_role)?;
    transcript.extend_from_slice(&sent_us.to_be_bytes());
    Ok(transcript)
}

fn certificate_hash(connection: &quinn::Connection) -> Result<[u8; 32]> {
    let identity = connection.peer_identity().ok_or(Error::Authentication)?;
    let certificates = identity
        .downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>()
        .map_err(|_| Error::Authentication)?;
    let certificate = certificates.first().ok_or(Error::Authentication)?;
    ring::digest::digest(&ring::digest::SHA256, certificate)
        .as_ref()
        .try_into()
        .map_err(|_| Error::Authentication)
}

impl AuthenticatedTransport {
    pub async fn establish(
        connection: quinn::Connection,
        binding: &SessionBinding,
        host_role: bool,
        authenticator: &impl IdentityAuthenticator,
        permissions: Permissions,
    ) -> Result<Self> {
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::Authentication)?
                .as_secs();
            binding.validate(now)?;
            let expected = if host_role {
                binding.guest_certificate_sha256
            } else {
                binding.host_certificate_sha256
            };
            if certificate_hash(&connection)? != expected {
                return Err(Error::Authentication);
            }
            let exporter = exporter(&connection)?;
            let (mut send, mut receive) = if host_role {
                connection.accept_bi().await
            } else {
                connection.open_bi().await
            }
            .map_err(|_| Error::Authentication)?;
            let sent_us = wall_clock_us()?;
            let signature =
                authenticator.sign(&proof_transcript(binding, &exporter, host_role, sent_us)?)?;
            write_message(&mut send, &Proof { signature, sent_us }).await?;
            send.finish().map_err(|_| Error::Authentication)?;
            let proof: Proof = read_message(&mut receive).await?;
            if proof.signature.len() != 64 {
                return Err(Error::Authentication);
            }
            let peer = if host_role {
                &binding.guest
            } else {
                &binding.host
            };
            authenticator.verify(
                peer,
                &proof_transcript(binding, &exporter, !host_role, proof.sent_us)?,
                &proof.signature,
            )?;
            let received_us = wall_clock_us()?;
            let transit_us = connection.rtt().as_micros() as i128 / 2;
            let offset = i128::from(proof.sent_us) + transit_us - i128::from(received_us);
            let offset = i64::try_from(offset).map_err(|_| Error::Authentication)?;
            Ok(offset)
        })
        .await
        .unwrap_or(Err(Error::Authentication));
        let peer_clock_offset_us = match result {
            Ok(offset) => offset,
            Err(error) => {
                connection.close(1u32.into(), b"authentication failed");
                return Err(error);
            }
        };
        Ok(Self {
            connection,
            replay: ReplayWindow::default(),
            permissions,
            peer_clock_offset_us,
        })
    }

    pub fn set_permissions(&mut self, permissions: Permissions) {
        self.permissions = permissions;
    }

    pub fn send(&self, packet: Packet, session_us: u64) -> Result<bool> {
        self.authorize(packet.kind)?;
        if packet.expired(session_us) {
            return Ok(false);
        }
        let bytes = packet.encode()?;
        if self
            .connection
            .max_datagram_size()
            .is_none_or(|mtu| bytes.len() > mtu)
        {
            return Err(Error::ResourceLimit);
        }
        self.connection
            .send_datagram(bytes)
            .map_err(|_| Error::Unavailable("QUIC datagram send".into()))?;
        Ok(true)
    }

    /// Backpressure remains bounded by the media deadline; this never retransmits a datagram.
    pub async fn send_before_deadline(
        &self,
        packet: Packet,
        clock: impl Fn() -> u64,
    ) -> Result<bool> {
        self.authorize(packet.kind)?;
        if packet.expired(clock()) {
            return Ok(false);
        }
        let remaining = packet
            .captured_us
            .saturating_add(packet.lifetime_us as u64)
            .saturating_sub(clock());
        let bytes = packet.encode()?;
        if self
            .connection
            .max_datagram_size()
            .is_none_or(|mtu| bytes.len() > mtu)
        {
            // A PMTU reduction can race frame packetization. Retire the frame;
            // the next one will use the current path limit.
            return Ok(false);
        }
        match tokio::time::timeout(
            Duration::from_micros(remaining),
            self.connection.send_datagram_wait(bytes),
        )
        .await
        {
            Ok(Ok(())) => Ok(true),
            Ok(Err(quinn::SendDatagramError::TooLarge)) => Ok(false),
            Ok(Err(quinn::SendDatagramError::ConnectionLost(_))) => Ok(false),
            Ok(Err(error)) => Err(Error::Unavailable(format!("QUIC datagram send: {error}"))),
            Err(_) => Ok(false),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.connection.close_reason().is_some()
    }

    pub fn rtt(&self) -> Duration {
        self.connection.rtt()
    }

    pub fn max_datagram_size(&self) -> Result<usize> {
        self.connection
            .max_datagram_size()
            .filter(|&size| size > super::packet::HEADER_LEN)
            .map(|size| size.min(super::packet::MAX_DATAGRAM))
            .ok_or(Error::ResourceLimit)
    }

    pub async fn receive(&mut self, session_clock: impl Fn() -> u64) -> Result<Option<Packet>> {
        let bytes: Bytes = self
            .connection
            .read_datagram()
            .await
            .map_err(|_| Error::Unavailable("QUIC connection closed".into()))?;
        let packet = Packet::decode(bytes)?;
        self.authorize(packet.kind)?;
        if packet.expired(self.peer_clock(session_clock())) || !self.replay.accept(packet.sequence)
        {
            return Ok(None);
        }
        Ok(Some(packet))
    }

    pub fn peer_clock(&self, local_us: u64) -> u64 {
        if self.peer_clock_offset_us >= 0 {
            local_us.saturating_add(self.peer_clock_offset_us as u64)
        } else {
            local_us.saturating_sub(self.peer_clock_offset_us.unsigned_abs())
        }
    }

    fn authorize(&self, kind: Kind) -> Result<()> {
        match kind {
            Kind::Video | Kind::Parity if self.permissions.screen => Ok(()),
            Kind::Audio if self.permissions.audio => Ok(()),
            // Input packets require the per-device guard before OS injection.
            Kind::Input
                if self.permissions.keyboard
                    || self.permissions.mouse
                    || self.permissions.gamepad =>
            {
                Ok(())
            }
            Kind::Feedback => Ok(()),
            _ => Err(Error::PermissionDenied),
        }
    }

    pub fn close(&self) {
        self.connection.close(0u32.into(), b"session ended");
    }
}

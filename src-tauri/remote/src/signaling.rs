use crate::{Error, Result, permissions::Principal};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};

pub const ALPN: &[u8] = b"noosphere-remote/1";
pub const MAX_CONTROL: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBinding {
    pub session_id: [u8; 32],
    pub host: Principal,
    pub guest: Principal,
    pub host_certificate_sha256: [u8; 32],
    pub guest_certificate_sha256: [u8; 32],
    pub expires_at: u64,
}

impl SessionBinding {
    pub fn validate(&self, now: u64) -> Result<()> {
        self.host.validate()?;
        self.guest.validate()?;
        if self.session_id == [0; 32]
            || self.expires_at <= now
            || self.expires_at.saturating_sub(now) > 120
            || self.host_certificate_sha256 == [0; 32]
            || self.guest_certificate_sha256 == [0; 32]
        {
            return Err(Error::Authentication);
        }
        Ok(())
    }

    pub fn transcript(&self, exporter: &[u8; 32], host_role: bool) -> Result<Vec<u8>> {
        let mut bytes = b"noosphere/remote/auth/v1\0".to_vec();
        bytes.push(u8::from(host_role));
        bytes.extend_from_slice(exporter);
        bytes.extend_from_slice(&serde_json::to_vec(self).map_err(|_| Error::InvalidPacket)?);
        Ok(bytes)
    }
}

pub fn random_id() -> Result<[u8; 32]> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::Authentication)?;
    Ok(bytes)
}

pub fn authenticate_local(key: &[u8; 32], challenge: &[u8; 32], server: bool) -> Vec<u8> {
    let mut input = b"noosphere/remote/ipc/v1\0".to_vec();
    input.push(u8::from(server));
    input.extend_from_slice(challenge);
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), &input)
        .as_ref()
        .to_vec()
}

pub fn verify_local(key: &[u8; 32], challenge: &[u8; 32], server: bool, tag: &[u8]) -> Result<()> {
    let mut input = b"noosphere/remote/ipc/v1\0".to_vec();
    input.push(u8::from(server));
    input.extend_from_slice(challenge);
    hmac::verify(&hmac::Key::new(hmac::HMAC_SHA256, key), &input, tag)
        .map_err(|_| Error::Authentication)
}

#[derive(Default)]
pub struct ReplayWindow {
    highest: Option<u64>,
    seen: u128,
}
impl ReplayWindow {
    pub fn accept(&mut self, sequence: u64) -> bool {
        match self.highest {
            None => {
                self.highest = Some(sequence);
                self.seen = 1;
                true
            }
            Some(highest) if sequence > highest => {
                let shift = sequence - highest;
                self.seen = if shift >= 128 {
                    1
                } else {
                    (self.seen << shift) | 1
                };
                self.highest = Some(sequence);
                true
            }
            Some(highest) => {
                let shift = highest - sequence;
                if shift >= 128 || self.seen & (1 << shift) != 0 {
                    return false;
                }
                self.seen |= 1 << shift;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reordered_packets_are_accepted_once_and_old_packets_expire() {
        let mut window = ReplayWindow::default();
        assert!(window.accept(100));
        assert!(window.accept(98));
        assert!(!window.accept(98));
        assert!(window.accept(300));
        assert!(!window.accept(100));
    }
    #[test]
    fn ipc_tags_bind_direction_and_fresh_challenge() {
        let key = random_id().unwrap();
        let challenge = random_id().unwrap();
        let tag = authenticate_local(&key, &challenge, false);
        verify_local(&key, &challenge, false, &tag).unwrap();
        assert!(verify_local(&key, &challenge, true, &tag).is_err());
        assert!(verify_local(&key, &random_id().unwrap(), false, &tag).is_err());
    }
}

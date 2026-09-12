use crate::{Error, Result, permissions::Principal, transport::session::IdentityAuthenticator};
use libsignal_protocol::{PrivateKey, PublicKey};
use rand::{TryRngCore as _, rngs::OsRng};

pub struct SignalIdentity {
    github_user_id: u64,
    pub(crate) private: PrivateKey,
}

impl SignalIdentity {
    pub fn from_private_key(github_user_id: u64, private: &[u8]) -> Result<Self> {
        if github_user_id == 0 || private.len() != 32 {
            return Err(Error::Authentication);
        }
        Ok(Self {
            github_user_id,
            private: PrivateKey::deserialize(private).map_err(|_| Error::Authentication)?,
        })
    }
    pub fn principal(&self, machine_id: [u8; 16]) -> Result<Principal> {
        let principal = Principal {
            github_user_id: self.github_user_id,
            identity_key: self
                .private
                .public_key()
                .map_err(|_| Error::Authentication)?
                .serialize()
                .to_vec(),
            machine_id,
        };
        principal.validate()?;
        Ok(principal)
    }
}

fn validate_transcript(transcript: &[u8]) -> Result<()> {
    if !transcript.starts_with(b"noosphere/remote/auth/v1\0") || transcript.len() > 16 * 1024 {
        return Err(Error::InvalidPacket);
    }
    Ok(())
}

impl IdentityAuthenticator for SignalIdentity {
    fn sign(&self, transcript: &[u8]) -> Result<Vec<u8>> {
        validate_transcript(transcript)?;
        self.private
            .calculate_signature(transcript, &mut OsRng.unwrap_err())
            .map(|s| s.to_vec())
            .map_err(|_| Error::Authentication)
    }
    fn verify(&self, principal: &Principal, transcript: &[u8], signature: &[u8]) -> Result<()> {
        principal.validate()?;
        validate_transcript(transcript)?;
        if signature.len() != 64 {
            return Err(Error::Authentication);
        }
        let key =
            PublicKey::deserialize(&principal.identity_key).map_err(|_| Error::Authentication)?;
        if !key.verify_signature(transcript, signature) {
            return Err(Error::Authentication);
        }
        Ok(())
    }
}

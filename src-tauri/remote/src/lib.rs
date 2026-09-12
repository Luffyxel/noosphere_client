pub mod access;
pub mod audio;
pub mod capture;
pub mod congestion;
pub mod daemon;
pub mod decoder;
pub mod diagnostic;
pub mod directory;
pub mod encoder;
pub mod fec;
pub mod identity;
pub mod input;
pub mod live;
pub mod nat;
pub mod permissions;
pub mod signaling;
pub mod transport;
pub mod viewer;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid remote desktop packet")]
    InvalidPacket,
    #[error("remote desktop resource limit reached")]
    ResourceLimit,
    #[error("remote desktop permission denied")]
    PermissionDenied,
    #[error("remote desktop authentication failed")]
    Authentication,
    #[error("remote desktop backend unavailable: {0}")]
    Unavailable(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

use crate::{Error, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

#[cfg(windows)]
pub mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

#[derive(Clone, Debug)]
pub struct Capability {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub max_fps: u16,
    pub gpu: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u16,
    pub bitrate: u64,
    pub codec: Codec,
}

impl VideoSettings {
    pub fn validate(&self) -> Result<()> {
        if self.width < 320
            || self.width > 7680
            || self.height < 200
            || self.height > 4320
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
            || ![30, 60, 120, 144, 240].contains(&self.fps)
            || !(500_000..=200_000_000).contains(&self.bitrate)
        {
            return Err(Error::InvalidPacket);
        }
        Ok(())
    }
}

pub fn negotiate(
    host: &[Capability],
    guest: &[Capability],
    settings: &VideoSettings,
) -> Result<Codec> {
    settings.validate()?;
    for codec in [settings.codec, Codec::Av1, Codec::Hevc, Codec::H264] {
        let compatible = |caps: &[Capability]| {
            caps.iter().any(|c| {
                c.gpu
                    && c.codec == codec
                    && c.width >= settings.width
                    && c.height >= settings.height
                    && c.max_fps >= settings.fps
            })
        };
        if compatible(host) && compatible(guest) {
            return Ok(codec);
        }
    }
    Err(Error::Unavailable("no common hardware codec".into()))
}

pub trait Encoder {
    type Surface;
    fn encode(&mut self, surface: &Self::Surface, force_keyframe: bool) -> Result<Bytes>;
    fn set_bitrate(&mut self, bits_per_second: u64) -> Result<()>;
}

#[derive(Default)]
pub struct DecodeGate {
    previous: Option<u64>,
    waiting_for_keyframe: bool,
}
impl DecodeGate {
    pub fn admit(&mut self, id: u64, keyframe: bool) -> bool {
        if self.previous.is_some_and(|previous| id <= previous) {
            return false;
        }
        if !keyframe
            && (self.previous.is_none()
                || self.waiting_for_keyframe
                || self.previous.is_some_and(|p| p.checked_add(1) != Some(id)))
        {
            self.waiting_for_keyframe = true;
            return false;
        }
        self.waiting_for_keyframe = false;
        self.previous = Some(id);
        true
    }
    pub fn needs_keyframe(&self) -> bool {
        self.waiting_for_keyframe || self.previous.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_reference_requires_keyframe_before_decode() {
        let mut gate = DecodeGate::default();
        assert!(!gate.admit(0, false));
        assert!(gate.admit(1, true));
        assert!(gate.admit(2, false));
        assert!(!gate.admit(4, false));
        assert!(!gate.admit(5, false));
        assert!(gate.needs_keyframe());
        assert!(gate.admit(6, true));
        assert!(!gate.admit(5, true));
        assert!(gate.admit(7, false));
    }
}

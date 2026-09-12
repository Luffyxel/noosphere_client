use crate::{Error, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};

pub const HEADER_LEN: usize = 44;
pub const MAX_DATAGRAM: usize = 1200;
pub const MAX_FRAME: usize = 4 * 1024 * 1024;
pub const MAX_SHARDS: u16 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Video = 1,
    Audio = 2,
    Input = 3,
    Parity = 4,
    Feedback = 5,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub kind: Kind,
    pub keyframe: bool,
    pub shard: u16,
    pub shards: u16,
    pub frame: u64,
    pub sequence: u64,
    pub captured_us: u64,
    pub lifetime_us: u32,
    pub frame_len: u32,
    pub payload: Bytes,
}

impl Packet {
    pub fn validate(&self) -> Result<()> {
        if self.shards == 0
            || self.shards > MAX_SHARDS
            || self.shard >= self.shards
            || self.payload.is_empty()
            || self.payload.len() > MAX_DATAGRAM - HEADER_LEN
            || self.frame_len == 0
            || self.frame_len as usize > MAX_FRAME
            || self.lifetime_us == 0
            || self.lifetime_us > 250_000
            || self
                .captured_us
                .checked_add(u64::from(self.lifetime_us))
                .is_none()
        {
            return Err(Error::InvalidPacket);
        }
        Ok(())
    }

    pub fn expired(&self, session_us: u64) -> bool {
        session_us >= self.captured_us.saturating_add(u64::from(self.lifetime_us))
    }

    pub fn encode(&self) -> Result<Bytes> {
        self.validate()?;
        let mut out = BytesMut::with_capacity(HEADER_LEN + self.payload.len());
        out.extend_from_slice(b"NSR1");
        out.put_u8(self.kind as u8);
        out.put_u8(u8::from(self.keyframe));
        out.put_u16(self.shard);
        out.put_u16(self.shards);
        out.put_u16(0);
        out.put_u64(self.frame);
        out.put_u64(self.sequence);
        out.put_u64(self.captured_us);
        out.put_u32(self.lifetime_us);
        out.put_u32(self.frame_len);
        out.extend_from_slice(&self.payload);
        Ok(out.freeze())
    }

    pub fn decode(mut bytes: Bytes) -> Result<Self> {
        if bytes.len() <= HEADER_LEN || bytes.len() > MAX_DATAGRAM || &bytes[..4] != b"NSR1" {
            return Err(Error::InvalidPacket);
        }
        bytes.advance(4);
        let kind = match bytes.get_u8() {
            1 => Kind::Video,
            2 => Kind::Audio,
            3 => Kind::Input,
            4 => Kind::Parity,
            5 => Kind::Feedback,
            _ => return Err(Error::InvalidPacket),
        };
        let flags = bytes.get_u8();
        if flags > 1 {
            return Err(Error::InvalidPacket);
        }
        let shard = bytes.get_u16();
        let shards = bytes.get_u16();
        if bytes.get_u16() != 0 {
            return Err(Error::InvalidPacket);
        }
        let packet = Self {
            kind,
            keyframe: flags == 1,
            shard,
            shards,
            frame: bytes.get_u64(),
            sequence: bytes.get_u64(),
            captured_us: bytes.get_u64(),
            lifetime_us: bytes.get_u32(),
            frame_len: bytes.get_u32(),
            payload: bytes,
        };
        packet.validate()?;
        Ok(packet)
    }
}

pub fn packetize(
    frame: Bytes,
    id: u64,
    first_sequence: u64,
    captured_us: u64,
    lifetime_us: u32,
    keyframe: bool,
    mtu: usize,
) -> Result<Vec<Packet>> {
    if frame.is_empty()
        || frame.len() > MAX_FRAME
        || !(HEADER_LEN + 1..=MAX_DATAGRAM).contains(&mtu)
    {
        return Err(Error::InvalidPacket);
    }
    let size = mtu - HEADER_LEN;
    let count = frame.len().div_ceil(size);
    if count > usize::from(MAX_SHARDS) || first_sequence.checked_add(count as u64).is_none() {
        return Err(Error::ResourceLimit);
    }
    (0..count)
        .map(|index| {
            let packet = Packet {
                kind: Kind::Video,
                keyframe,
                shard: index as u16,
                shards: count as u16,
                frame: id,
                sequence: first_sequence + index as u64,
                captured_us,
                lifetime_us,
                frame_len: frame.len() as u32,
                payload: frame.slice(index * size..frame.len().min((index + 1) * size)),
            };
            packet.validate()?;
            Ok(packet)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packetization_respects_mtu_and_round_trips() {
        let frame = Bytes::from(vec![19; 5000]);
        let packets = packetize(frame.clone(), 9, 200, 1000, 30_000, true, 1200).unwrap();
        let mut decoded = Vec::new();
        for packet in packets {
            let bytes = packet.encode().unwrap();
            assert!(bytes.len() <= 1200);
            let restored = Packet::decode(bytes).unwrap();
            assert_eq!(restored, packet);
            decoded.extend_from_slice(&restored.payload);
        }
        assert_eq!(decoded, frame);
    }
    #[test]
    fn malformed_headers_and_overflow_are_rejected_without_panics() {
        for length in 0..=HEADER_LEN {
            assert!(Packet::decode(Bytes::from(vec![0; length])).is_err());
        }
        assert!(
            packetize(
                Bytes::from_static(b"frame"),
                1,
                u64::MAX,
                0,
                1000,
                false,
                1200
            )
            .is_err()
        );
        assert!(
            packetize(
                Bytes::from_static(b"frame"),
                1,
                1,
                u64::MAX,
                1000,
                false,
                1200
            )
            .is_err()
        );
    }
}

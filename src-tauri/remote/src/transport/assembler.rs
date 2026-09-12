use super::packet::{Kind, MAX_FRAME, Packet};
use crate::{Error, Result};
use bytes::Bytes;
use std::collections::BTreeMap;

struct PartialFrame {
    header: Packet,
    chunks: Vec<Option<Bytes>>,
    parity: BTreeMap<u16, Bytes>,
    received: usize,
    parity_bytes: usize,
}

pub struct Frame {
    pub id: u64,
    pub keyframe: bool,
    pub captured_us: u64,
    pub data: Bytes,
}

#[derive(Default)]
pub struct Assembler {
    pending: BTreeMap<u64, PartialFrame>,
    bytes: usize,
    last_delivered: Option<u64>,
    pub dropped: u64,
}

impl Assembler {
    pub fn expire(&mut self, session_us: u64) {
        self.pending.retain(|_, partial| {
            if partial.header.expired(session_us) {
                self.bytes -= partial.received + partial.parity_bytes;
                self.dropped += 1;
                false
            } else {
                true
            }
        });
    }

    // session_us is mapped to the sender's monotonic clock by clock synchronization.
    pub fn receive(&mut self, packet: Packet, session_us: u64) -> Result<Option<Frame>> {
        packet.validate()?;
        if !matches!(packet.kind, Kind::Video | Kind::Parity) {
            return Err(Error::InvalidPacket);
        }
        self.expire(session_us);
        if packet.expired(session_us) || self.last_delivered.is_some_and(|id| packet.frame <= id) {
            return Ok(None);
        }
        if packet.captured_us > session_us.saturating_add(250_000) {
            return Err(Error::InvalidPacket);
        }
        if !self.pending.contains_key(&packet.frame) {
            if self.pending.len() >= 3 {
                let (&oldest, _) = self.pending.first_key_value().ok_or(Error::ResourceLimit)?;
                if packet.frame < oldest {
                    return Ok(None);
                }
                let old = self.pending.remove(&oldest).ok_or(Error::ResourceLimit)?;
                self.bytes -= old.received + old.parity_bytes;
                self.dropped += 1;
            }
            self.pending.insert(
                packet.frame,
                PartialFrame {
                    header: packet.clone(),
                    chunks: vec![None; usize::from(packet.shards)],
                    parity: BTreeMap::new(),
                    received: 0,
                    parity_bytes: 0,
                },
            );
        }
        let current_bytes = self.bytes;
        let partial = self
            .pending
            .get_mut(&packet.frame)
            .ok_or(Error::InvalidPacket)?;
        let h = &partial.header;
        if h.shards != packet.shards
            || h.captured_us != packet.captured_us
            || h.lifetime_us != packet.lifetime_us
            || h.frame_len != packet.frame_len
            || h.keyframe != packet.keyframe
        {
            return Err(Error::InvalidPacket);
        }
        let mut added = 0;
        match packet.kind {
            Kind::Video => {
                let slot = &mut partial.chunks[usize::from(packet.shard)];
                if let Some(existing) = slot {
                    return if *existing == packet.payload {
                        Ok(None)
                    } else {
                        Err(Error::InvalidPacket)
                    };
                }
                if current_bytes + packet.payload.len() > 2 * MAX_FRAME
                    || partial.received + packet.payload.len() > packet.frame_len as usize
                {
                    return Err(Error::ResourceLimit);
                }
                partial.received += packet.payload.len();
                added += packet.payload.len();
                *slot = Some(packet.payload);
            }
            Kind::Parity => {
                let group = packet
                    .payload
                    .first()
                    .copied()
                    .map(usize::from)
                    .ok_or(Error::InvalidPacket)?;
                let start = usize::from(packet.shard);
                if group == 0 || group > 16 || start + group > usize::from(packet.shards) {
                    return Err(Error::InvalidPacket);
                }
                if let Some(existing) = partial.parity.get(&packet.shard) {
                    return if *existing == packet.payload {
                        Ok(None)
                    } else {
                        Err(Error::InvalidPacket)
                    };
                }
                if current_bytes + packet.payload.len() > 2 * MAX_FRAME {
                    return Err(Error::ResourceLimit);
                }
                partial.parity_bytes += packet.payload.len();
                added += packet.payload.len();
                partial.parity.insert(packet.shard, packet.payload);
            }
            _ => unreachable!("kind checked above"),
        }
        loop {
            let mut recovered = None;
            for (&start, parity) in &partial.parity {
                let group = usize::from(parity[0]);
                let start = usize::from(start);
                let shards = &partial.chunks[start..start + group];
                if let Some((index, bytes)) = crate::fec::recover(parity, shards)? {
                    recovered = Some((start + index, bytes));
                    break;
                }
            }
            let Some((index, bytes)) = recovered else {
                break;
            };
            if partial.received + bytes.len() > packet.frame_len as usize
                || current_bytes + added + bytes.len() > 2 * MAX_FRAME
            {
                return Err(Error::ResourceLimit);
            }
            partial.received += bytes.len();
            added += bytes.len();
            partial.chunks[index] = Some(bytes);
        }
        self.bytes += added;
        if partial.chunks.iter().any(Option::is_none) {
            return Ok(None);
        }
        let complete = self
            .pending
            .remove(&packet.frame)
            .ok_or(Error::InvalidPacket)?;
        self.bytes -= complete.received + complete.parity_bytes;
        if complete.received != packet.frame_len as usize {
            return Err(Error::InvalidPacket);
        }
        self.last_delivered = Some(packet.frame);
        self.pending.retain(|&id, partial| {
            if id < packet.frame {
                self.bytes -= partial.received + partial.parity_bytes;
                self.dropped += 1;
                false
            } else {
                true
            }
        });
        let mut data = Vec::with_capacity(complete.received);
        for chunk in complete.chunks.into_iter().flatten() {
            data.extend_from_slice(&chunk);
        }
        Ok(Some(Frame {
            id: packet.frame,
            keyframe: packet.keyframe,
            captured_us: packet.captured_us,
            data: data.into(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fec;
    use crate::transport::packet::packetize;
    #[test]
    fn loss_does_not_block_new_frames_and_late_frames_stay_discarded() {
        let frame = Bytes::from(vec![1; 4000]);
        let mut assembler = Assembler::default();
        let old = packetize(frame.clone(), 1, 0, 0, 20_000, true, 1200).unwrap();
        assembler.receive(old[0].clone(), 100).unwrap();
        let mut next = packetize(frame.clone(), 2, 4, 10_000, 20_000, true, 1200).unwrap();
        next.reverse();
        let mut complete = None;
        for packet in next {
            complete = assembler.receive(packet, 11_000).unwrap().or(complete);
        }
        assert_eq!(complete.unwrap().data, frame);
        assert_eq!(assembler.dropped, 1);
        for packet in old {
            assert!(assembler.receive(packet, 12_000).unwrap().is_none());
        }
        assert_eq!(assembler.bytes, 0);
    }
    #[test]
    fn incomplete_frames_expire_without_new_traffic() {
        let mut assembler = Assembler::default();
        let p = packetize(Bytes::from(vec![1; 4000]), 1, 0, 0, 20_000, true, 1200).unwrap();
        assembler.receive(p[0].clone(), 100).unwrap();
        assembler.expire(20_000);
        assert_eq!(assembler.bytes, 0);
        assert_eq!(assembler.dropped, 1);
    }
    #[test]
    fn parity_recovers_one_missing_video_datagram() {
        let frame = Bytes::from(vec![7; 4000]);
        let packets = packetize(frame.clone(), 4, 10, 1000, 20_000, true, 1144).unwrap();
        let parity = Packet {
            kind: Kind::Parity,
            keyframe: true,
            shard: 0,
            shards: packets.len() as u16,
            frame: 4,
            sequence: 10 + packets.len() as u64,
            captured_us: 1000,
            lifetime_us: 20_000,
            frame_len: frame.len() as u32,
            payload: fec::parity(
                &packets
                    .iter()
                    .map(|packet| packet.payload.clone())
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        };
        let mut assembler = Assembler::default();
        assembler.receive(parity, 1100).unwrap();
        let mut complete = None;
        for (index, packet) in packets.into_iter().enumerate() {
            if index != 2 {
                complete = assembler.receive(packet, 1200).unwrap().or(complete);
            }
        }
        assert_eq!(complete.unwrap().data, frame);
        assert_eq!(assembler.dropped, 0);
        assert_eq!(assembler.bytes, 0);
    }
}

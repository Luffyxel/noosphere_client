use crate::{Error, Result};
use bytes::Bytes;
use std::collections::BTreeMap;

pub struct AudioPacket {
    pub sequence: u64,
    pub playout_us: u64,
    pub payload: Bytes,
}
#[derive(Default)]
pub struct Playout {
    pending: BTreeMap<u64, AudioPacket>,
    last: Option<u64>,
}
impl Playout {
    pub fn push(&mut self, packet: AudioPacket, now_us: u64) -> Result<()> {
        if packet.payload.is_empty()
            || packet.payload.len() > 1100
            || packet.playout_us > now_us.saturating_add(60_000)
        {
            return Err(Error::InvalidPacket);
        }
        if packet.playout_us < now_us
            || self.last.is_some_and(|last| packet.sequence <= last)
            || self.pending.contains_key(&packet.sequence)
        {
            return Ok(());
        }
        if self.pending.len() >= 6 {
            self.pending.pop_first();
        }
        self.pending.entry(packet.sequence).or_insert(packet);
        Ok(())
    }
    pub fn take(&mut self, now_us: u64) -> Option<AudioPacket> {
        let sequence = self
            .pending
            .iter()
            .filter(|(_, p)| p.playout_us <= now_us)
            .map(|(&s, _)| s)
            .max()?;
        let packet = self.pending.remove(&sequence)?;
        self.pending.retain(|&s, _| s > sequence);
        self.last = Some(sequence);
        Some(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicates_cannot_evict_queued_audio_and_late_packets_are_ignored() {
        let packet = |sequence| AudioPacket {
            sequence,
            playout_us: 10_000,
            payload: Bytes::from_static(b"audio"),
        };
        let mut playout = Playout::default();
        for sequence in 0..6 {
            playout.push(packet(sequence), 0).unwrap();
        }
        playout.push(packet(5), 0).unwrap();
        assert_eq!(playout.pending.len(), 6);
        assert_eq!(playout.take(10_000).unwrap().sequence, 5);
        playout.push(packet(4), 10_000).unwrap();
        assert!(playout.take(10_000).is_none());
    }
}

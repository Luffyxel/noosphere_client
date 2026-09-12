use crate::{Error, Result};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEstimate {
    pub rtt_us: u64,
    pub min_rtt_us: u64,
    pub jitter_us: u64,
    pub loss: f64,
    pub delivery_bps: u64,
}

pub struct Feedback {
    pub rtt_us: u64,
    pub delivered_bytes: u64,
    pub interval_us: u64,
    pub sent_packets: u64,
    pub lost_packets: u64,
    pub app_limited: bool,
}

pub trait RateController: Send {
    fn update(&mut self, feedback: &Feedback) -> Result<u64>;
    fn bitrate(&self) -> u64;
    fn estimate(&self) -> NetworkEstimate;
}

pub struct LowLatency {
    estimate: NetworkEstimate,
    bitrate: u64,
    min: u64,
    max: u64,
    previous_rtt: Option<u64>,
}

impl LowLatency {
    pub fn new(min: u64, initial: u64, max: u64) -> Result<Self> {
        if min < 64_000 || min > initial || initial > max || max > 1_000_000_000 {
            return Err(Error::InvalidPacket);
        }
        Ok(Self {
            estimate: NetworkEstimate::default(),
            bitrate: initial,
            min,
            max,
            previous_rtt: None,
        })
    }
}

impl RateController for LowLatency {
    fn update(&mut self, feedback: &Feedback) -> Result<u64> {
        let f = feedback;
        if f.interval_us < 10_000
            || f.interval_us > 10_000_000
            || f.rtt_us == 0
            || f.rtt_us > 10_000_000
            || f.sent_packets == 0
            || f.lost_packets > f.sent_packets
            || f.delivered_bytes > 1_250_000_000
        {
            return Err(Error::InvalidPacket);
        }
        let e = &mut self.estimate;
        e.min_rtt_us = if e.min_rtt_us == 0 {
            f.rtt_us
        } else {
            e.min_rtt_us.min(f.rtt_us)
        };
        e.rtt_us = if e.rtt_us == 0 {
            f.rtt_us
        } else {
            (7 * e.rtt_us + f.rtt_us) / 8
        };
        if let Some(previous) = self.previous_rtt {
            e.jitter_us = (15 * e.jitter_us + previous.abs_diff(f.rtt_us)) / 16;
        }
        self.previous_rtt = Some(f.rtt_us);
        e.loss = 0.75 * e.loss + 0.25 * f.lost_packets as f64 / f.sent_packets as f64;
        let delivered = f.delivered_bytes.saturating_mul(8_000_000) / f.interval_us;
        e.delivery_bps = if e.delivery_bps == 0 {
            delivered
        } else {
            (3 * e.delivery_bps + delivered) / 4
        };
        let queue = f.rtt_us.saturating_sub(e.min_rtt_us);
        let elapsed = f.interval_us.min(1_000_000) as f64 / 1_000_000.0;
        if queue > 8_000 || f.lost_packets as f64 / f.sent_packets as f64 > 0.05 {
            self.bitrate = (self.bitrate as f64 * 0.45_f64.powf(elapsed)) as u64;
            if !f.app_limited && delivered > 0 {
                self.bitrate = self.bitrate.min(delivered.saturating_mul(9) / 10);
            }
        } else if !f.app_limited {
            self.bitrate = self
                .bitrate
                .saturating_add((self.bitrate as f64 * 0.12 * elapsed) as u64);
        }
        self.bitrate = self.bitrate.clamp(self.min, self.max);
        Ok(self.bitrate)
    }
    fn bitrate(&self) -> u64 {
        self.bitrate
    }
    fn estimate(&self) -> NetworkEstimate {
        self.estimate
    }
}

#[derive(Default)]
pub struct Pacer {
    next_us: u64,
}

impl Pacer {
    pub fn schedule(
        &mut self,
        now_us: u64,
        bytes: usize,
        bitrate: u64,
        deadline_us: u64,
    ) -> Option<u64> {
        if bitrate == 0 || bytes == 0 {
            return None;
        }
        let at = now_us.max(self.next_us);
        let duration = (bytes as u64).checked_mul(8_000_000)?.div_ceil(bitrate);
        let finish = at.checked_add(duration)?;
        if finish >= deadline_us {
            return None;
        }
        self.next_us = finish;
        Some(at)
    }
}

pub fn retransmission_useful(now_us: u64, deadline_us: u64, rtt_us: u64, jitter_us: u64) -> bool {
    now_us
        .saturating_add(rtt_us)
        .saturating_add(jitter_us.saturating_mul(2))
        .saturating_add(2000)
        < deadline_us
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bandwidth_collapse_reduces_encoder_budget_and_idle_does_not_grow_it() {
        let mut c = LowLatency::new(500_000, 20_000_000, 80_000_000).unwrap();
        let mut f = Feedback {
            rtt_us: 20_000,
            delivered_bytes: 250_000,
            interval_us: 100_000,
            sent_packets: 250,
            lost_packets: 0,
            app_limited: true,
        };
        assert_eq!(c.update(&f).unwrap(), 20_000_000);
        f.rtt_us = 80_000;
        f.delivered_bytes = 60_000;
        f.app_limited = false;
        assert!(c.update(&f).unwrap() < 5_000_000);
        assert_eq!(c.estimate().min_rtt_us, 20_000);
    }
    #[test]
    fn pacing_never_bursts_after_idle_or_sends_past_deadline() {
        let mut p = Pacer::default();
        assert_eq!(p.schedule(0, 1200, 9_600_000, 10_000), Some(0));
        assert_eq!(p.schedule(0, 1200, 9_600_000, 10_000), Some(1000));
        assert_eq!(p.schedule(20_000, 1200, 9_600_000, 21_000), None);
        assert_eq!(p.schedule(20_000, 1200, 9_600_000, 30_000), Some(20_000));
        assert!(!retransmission_useful(10_000, 30_000, 30_000, 1000));
    }
}

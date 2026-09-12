use crate::{Error, Result};
use bytes::Bytes;

// One XOR parity shard recovers one erasure per group. Headers remain authenticated by QUIC.
pub fn parity(shards: &[Bytes]) -> Result<Bytes> {
    if shards.is_empty()
        || shards.len() > 16
        || shards.iter().any(|s| s.is_empty() || s.len() > 1100)
    {
        return Err(Error::InvalidPacket);
    }
    let width = shards
        .iter()
        .map(Bytes::len)
        .max()
        .ok_or(Error::InvalidPacket)?;
    let header = 1 + 2 * shards.len();
    let mut out = vec![0; header + width];
    out[0] = shards.len() as u8;
    for (index, shard) in shards.iter().enumerate() {
        out[1 + 2 * index..3 + 2 * index].copy_from_slice(&(shard.len() as u16).to_be_bytes());
        for (offset, &byte) in shard.iter().enumerate() {
            out[header + offset] ^= byte;
        }
    }
    Ok(out.into())
}

pub fn recover(parity: &[u8], shards: &[Option<Bytes>]) -> Result<Option<(usize, Bytes)>> {
    if shards.is_empty() || shards.len() > 16 || parity.first().copied() != Some(shards.len() as u8)
    {
        return Err(Error::InvalidPacket);
    }
    let header = 1 + 2 * shards.len();
    if parity.len() <= header || parity.len() > header + 1100 {
        return Err(Error::InvalidPacket);
    }
    let lengths: Vec<_> = (0..shards.len())
        .map(|i| usize::from(u16::from_be_bytes([parity[1 + 2 * i], parity[2 + 2 * i]])))
        .collect();
    if lengths
        .iter()
        .any(|&len| len == 0 || len > parity.len() - header)
    {
        return Err(Error::InvalidPacket);
    }
    let missing: Vec<_> = shards
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_none())
        .map(|(i, _)| i)
        .collect();
    if missing.len() != 1 {
        return Ok(None);
    }
    let index = missing[0];
    let mut recovered = parity[header..].to_vec();
    for (i, shard) in shards.iter().enumerate() {
        if let Some(shard) = shard {
            if shard.len() != lengths[i] {
                return Err(Error::InvalidPacket);
            }
            for (j, &byte) in shard.iter().enumerate() {
                recovered[j] ^= byte;
            }
        }
    }
    recovered.truncate(lengths[index]);
    Ok(Some((index, recovered.into())))
}

pub fn group_size(loss: f64, congested: bool) -> Option<usize> {
    if congested || !loss.is_finite() || loss < 0.005 {
        None
    } else if loss < 0.02 {
        Some(16)
    } else if loss < 0.05 {
        Some(8)
    } else {
        Some(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_single_erasure_is_recovered_including_short_last_shard() {
        let data = vec![
            Bytes::from_static(b"abcdef"),
            Bytes::from_static(b"ghijkl"),
            Bytes::from_static(b"mn"),
        ];
        let encoded = parity(&data).unwrap();
        for missing in 0..data.len() {
            let mut received: Vec<_> = data.iter().cloned().map(Some).collect();
            received[missing] = None;
            assert_eq!(
                recover(&encoded, &received).unwrap(),
                Some((missing, data[missing].clone()))
            );
        }
        assert!(
            recover(&encoded, &[None, None, Some(data[2].clone())])
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn redundancy_does_not_amplify_congestion() {
        assert_eq!(group_size(0.2, true), None);
    }
}

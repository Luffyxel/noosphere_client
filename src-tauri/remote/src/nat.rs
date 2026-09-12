use crate::{Error, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const COOKIE: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];

pub fn binding_request(transaction: [u8; 12]) -> [u8; 20] {
    let mut out = [0; 20];
    out[1] = 1;
    out[4..8].copy_from_slice(&COOKIE);
    out[8..].copy_from_slice(&transaction);
    out
}

pub fn mapped_address(response: &[u8], transaction: [u8; 12]) -> Result<SocketAddr> {
    if response.len() < 20
        || response.len() > 2048
        || response[..2] != [1, 1]
        || response[4..8] != COOKIE
        || response[8..20] != transaction
    {
        return Err(Error::InvalidPacket);
    }
    let length = usize::from(u16::from_be_bytes([response[2], response[3]]));
    if length + 20 != response.len() || !length.is_multiple_of(4) {
        return Err(Error::InvalidPacket);
    }
    let mut offset = 20;
    while offset + 4 <= response.len() {
        let kind = u16::from_be_bytes([response[offset], response[offset + 1]]);
        let len = usize::from(u16::from_be_bytes([
            response[offset + 2],
            response[offset + 3],
        ]));
        offset += 4;
        let end = offset
            .checked_add(len)
            .filter(|&end| end <= response.len())
            .ok_or(Error::InvalidPacket)?;
        if kind == 0x0020 {
            let data = &response[offset..end];
            if data.len() < 4 || data[0] != 0 {
                return Err(Error::InvalidPacket);
            }
            let port =
                u16::from_be_bytes([data[2], data[3]]) ^ u16::from_be_bytes([COOKIE[0], COOKIE[1]]);
            let ip = match (data[1], data.len()) {
                (1, 8) => IpAddr::V4(Ipv4Addr::new(
                    data[4] ^ COOKIE[0],
                    data[5] ^ COOKIE[1],
                    data[6] ^ COOKIE[2],
                    data[7] ^ COOKIE[3],
                )),
                (2, 20) => {
                    let mut ip = [0; 16];
                    let mask: Vec<_> = COOKIE.into_iter().chain(transaction).collect();
                    for i in 0..16 {
                        ip[i] = data[i + 4] ^ mask[i];
                    }
                    IpAddr::V6(Ipv6Addr::from(ip))
                }
                _ => return Err(Error::InvalidPacket),
            };
            if port == 0 || ip.is_unspecified() || ip.is_multicast() {
                return Err(Error::InvalidPacket);
            }
            return Ok(SocketAddr::new(ip, port));
        }
        offset = offset
            .checked_add(len.div_ceil(4) * 4)
            .filter(|&n| n <= response.len())
            .ok_or(Error::InvalidPacket)?;
    }
    Err(Error::InvalidPacket)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stun_requires_matching_transaction_and_bounded_attributes() {
        let transaction = [9; 12];
        let mut response = binding_request(transaction).to_vec();
        response[0] = 1;
        response[3] = 12;
        response.extend_from_slice(&[0, 0x20, 0, 8, 0, 1, 0x32, 0x9a, 0xe1, 0x12, 0xa6, 0x43]);
        assert_eq!(
            mapped_address(&response, transaction).unwrap(),
            "192.0.2.1:5000".parse().unwrap()
        );
        assert!(mapped_address(&response, [8; 12]).is_err());
        response[23] = 255;
        assert!(mapped_address(&response, transaction).is_err());
    }
}

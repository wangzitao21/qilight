use crate::{config::Device, crypto::Codec, message, Error, Result};
use serde_json::{json, Value};
use std::{
    net::{SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

pub const HELLO: [u8; 32] = [
    0x21, 0x31, 0, 0x20, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
];

pub fn encode_packet(codec: &Codec, did: u32, timestamp: u32, value: &Value) -> Result<Vec<u8>> {
    let encrypted = codec.encrypt(&serde_json::to_vec(value)?);
    let length = u16::try_from(32 + encrypted.len()).map_err(|_| message("设备指令过长。"))?;
    let mut out = Vec::with_capacity(length as usize);
    out.extend_from_slice(&[0x21, 0x31]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&did.to_be_bytes());
    out.extend_from_slice(&timestamp.to_be_bytes());
    let digest = codec.checksum(&out, &encrypted);
    out.extend_from_slice(&digest);
    out.extend_from_slice(&encrypted);
    Ok(out)
}
pub fn decode_envelope(codec: &Codec, data: &[u8], did: u32) -> Result<Value> {
    if data.len() <= 32 || !(data.len() - 32).is_multiple_of(16) {
        return Err(message("设备回复长度异常。"));
    }
    if data[..2] != [0x21, 0x31]
        || u16::from_be_bytes([data[2], data[3]]) as usize != data.len()
        || u32::from_be_bytes(data[8..12].try_into().unwrap()) != did
    {
        return Err(message("设备回复身份校验失败，请检查 IP 或重新扫码。"));
    }
    if !codec.verify(&data[..16], &data[32..], &data[16..32]) {
        return Err(message("设备密钥校验失败，请点击“米家配对”。"));
    }
    let raw = codec.decrypt(&data[32..])?;
    Ok(serde_json::from_slice(
        raw.strip_suffix(&[0]).unwrap_or(&raw),
    )?)
}
pub fn result_from_envelope(value: Value, request_id: u32) -> Result<Value> {
    if value["id"].as_u64() != Some(request_id as u64) {
        return Err(message("设备回复序号不匹配。"));
    }
    if value.get("error").is_some() {
        return Err(Error::Device(value["error"]["code"].as_i64().unwrap_or(-1)));
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| message("设备回复缺少结果。"))
}

pub fn hello_identity(data: &[u8]) -> Result<(u32, u32)> {
    if data.len() != 32 || data[..4] != [0x21, 0x31, 0, 32] {
        return Err(message("设备握手回复异常。"));
    }
    let did = u32::from_be_bytes(data[8..12].try_into().unwrap());
    if did == 0 || did == u32::MAX {
        return Err(message("设备身份无效。"));
    }
    Ok((did, u32::from_be_bytes(data[12..16].try_into().unwrap())))
}
pub fn probe(ip: &str) -> Result<u32> {
    let address = crate::config::normalize_ip(ip)?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect((address.as_str(), 54321))?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    for _ in 0..2 {
        socket.send(&HELLO)?;
        let mut buf = [0; 1024];
        if let Ok(n) = socket.recv(&mut buf) {
            return Ok(hello_identity(&buf[..n])?.0);
        }
    }
    Err(message("连接超时，请检查灯具 IP、供电和局域网。"))
}

pub trait Rpc: Send {
    fn call(&mut self, method: &str, params: Value) -> Result<Value>;
}

pub struct Session {
    socket: UdpSocket,
    codec: Codec,
    did: u32,
    next_id: u32,
    clock: Option<(u32, Instant)>,
    timeout: Duration,
}
impl Session {
    pub fn new(device: &Device) -> Result<Self> {
        Self::at(
            device,
            format!("{}:54321", device.ip)
                .parse()
                .map_err(|_| message("IP 地址无效。"))?,
            Duration::from_millis(1800),
        )
    }
    pub fn at(device: &Device, address: SocketAddr, timeout: Duration) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect(address)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;
        Ok(Self {
            socket,
            codec: Codec::new(device.token_bytes()?),
            did: device.did.ok_or_else(|| message("请先扫码配对。"))?,
            next_id: rand::random::<u32>() % 900000 + 1000,
            clock: None,
            timeout,
        })
    }
    fn handshake(&mut self) -> Result<()> {
        if self
            .clock
            .is_some_and(|(_, when)| when.elapsed() < Duration::from_secs(30))
        {
            return Ok(());
        }
        self.socket.set_read_timeout(Some(self.timeout))?;
        self.socket.send(&HELLO)?;
        let deadline = Instant::now() + self.timeout;
        let mut buffer = [0; 8192];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(std::io::Error::from(std::io::ErrorKind::TimedOut).into());
            }
            self.socket.set_read_timeout(Some(remaining))?;
            let n = self.socket.recv(&mut buffer)?;
            if n != 32 {
                continue;
            }
            let (did, timestamp) = hello_identity(&buffer[..n])?;
            if did != self.did {
                return Err(message("此 IP 的设备已变化，请核对地址并重新扫码。"));
            }
            self.clock = Some((timestamp, Instant::now()));
            return Ok(());
        }
    }
    fn request(&mut self, method: &str, params: &Value, id: u32) -> Result<Value> {
        self.handshake()?;
        let (timestamp, when) = self.clock.unwrap();
        let packet = encode_packet(
            &self.codec,
            self.did,
            timestamp.wrapping_add(when.elapsed().as_secs() as u32),
            &json!({"id":id,"method":method,"params":params}),
        )?;
        self.socket.send(&packet)?;
        let deadline = Instant::now() + self.timeout;
        let mut buffer = [0; 8192];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(std::io::Error::from(std::io::ErrorKind::TimedOut).into());
            }
            self.socket.set_read_timeout(Some(remaining))?;
            let n = self.socket.recv(&mut buffer)?;
            if n == 32 {
                continue;
            }
            let response = decode_envelope(&self.codec, &buffer[..n], self.did)?;
            // A delayed response to a previous request must not fail a newer command.
            if response["id"].as_u64() != Some(id as u64) {
                continue;
            }
            return result_from_envelope(response, id);
        }
    }
}
impl Rpc for Session {
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id = self.next_id.wrapping_add(1);
        let id = self.next_id;
        for attempt in 0..2 {
            match self.request(method, &params, id) {
                Err(Error::Io(error)) => {
                    self.clock = None;
                    if attempt == 1 {
                        return Err(Error::Connection(error));
                    }
                }
                result => return result,
            }
        }
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packets_reject_corruption_identity_and_stale_id() {
        let codec = Codec::new([7; 16]);
        let mut packet =
            encode_packet(&codec, 1234, 5678, &json!({"id":9,"result":["ok"]})).unwrap();
        assert_eq!(
            result_from_envelope(decode_envelope(&codec, &packet, 1234).unwrap(), 9).unwrap(),
            json!(["ok"])
        );
        assert!(decode_envelope(&codec, &packet, 1235).is_err());
        assert!(result_from_envelope(decode_envelope(&codec, &packet, 1234).unwrap(), 8).is_err());
        packet[40] ^= 1;
        assert!(decode_envelope(&codec, &packet, 1234).is_err());
        for length in 0..48 {
            assert!(decode_envelope(&codec, &vec![0; length], 1234).is_err());
        }
    }
}

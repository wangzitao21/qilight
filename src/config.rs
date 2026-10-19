use crate::{crypto::protect, message, spec::Capabilities, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    net::Ipv4Addr,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    pub ip: String,
    pub name: String,
    #[serde(default)]
    pub did: Option<u32>,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub capabilities: Option<Capabilities>,
}
// Credentials must never be exposed by debug formatting or logs.
impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("ip", &self.ip)
            .field("name", &self.name)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}
impl Device {
    pub fn pending(ip: &str, name: &str) -> Self {
        Self {
            ip: ip.into(),
            name: name.into(),
            did: None,
            token: String::new(),
            model: String::new(),
            capabilities: None,
        }
    }
    pub fn paired(&self) -> bool {
        self.did.is_some() && !self.token.is_empty()
    }
    pub fn same_connection(&self, other: &Self) -> bool {
        self.ip == other.ip
            && self.token == other.token
            && self.did == other.did
            && self.caps() == other.caps()
    }
    pub fn caps(&self) -> Capabilities {
        // The bundled, verified definition is authoritative for 1S, including migrated files.
        if self.model == "yeelink.light.lamp22" {
            Capabilities::for_model(&self.model)
        } else {
            self.capabilities.clone().unwrap_or_default()
        }
    }
    pub fn token_bytes(&self) -> Result<[u8; 16]> {
        let mut out = [0; 16];
        hex::decode_to_slice(&self.token, &mut out)
            .map_err(|_| message("密钥格式错误，请重新扫码。"))?;
        Ok(out)
    }
}

pub fn normalize_ip(text: &str) -> Result<String> {
    let ip: Ipv4Addr = text
        .trim()
        .parse()
        .map_err(|_| message("请输入完整的局域网 IPv4 地址。"))?;
    if !ip.is_private() {
        return Err(message(
            "请使用 10.x、172.16–31.x 或 192.168.x 的局域网地址。",
        ));
    }
    Ok(ip.to_string())
}
pub fn parse_ips(text: &str) -> Result<Vec<String>> {
    let ips: Vec<String> = text
        .split(|c: char| c.is_whitespace() || ",;，；".contains(c))
        .filter(|s| !s.is_empty())
        .map(normalize_ip)
        .collect::<Result<_>>()?;
    if ips.is_empty() {
        return Err(message("请先输入灯具的局域网 IP。"));
    }
    if ips.iter().collect::<HashSet<_>>().len() != ips.len() {
        return Err(message("输入的 IP 有重复。"));
    }
    Ok(ips)
}
pub fn validate(mut devices: Vec<Device>) -> Result<Vec<Device>> {
    let mut ips = HashSet::new();
    let mut dids = HashSet::new();
    for device in &mut devices {
        device.ip = normalize_ip(&device.ip)?;
        if !ips.insert(device.ip.clone()) {
            return Err(message(format!("{} 已在列表中。", device.ip)));
        }
        device.name = device.name.trim().chars().take(80).collect();
        if device.name.is_empty() {
            device.name = format!("屏幕灯 {}", device.ip.rsplit('.').next().unwrap_or(""));
        }
        device.token = device.token.trim().to_lowercase();
        if device.token.is_empty() {
            device.did = None;
            device.model.clear();
            device.capabilities = None;
        } else {
            let token = device.token_bytes()?;
            if token == [0; 16] || token == [255; 16] {
                return Err(message("设备密钥缺失，请使用绑定灯具的米家账户扫码。"));
            }
            if !device.model.starts_with("yeelink.light.") || device.did.unwrap_or(0) == 0 {
                return Err(message(
                    "设备型号或身份无效，仅支持米家 1S 与兼容 Yeelight 灯具。",
                ));
            }
            if !dids.insert(device.did) {
                return Err(message("同一盏灯已登记在另一个 IP 下，请编辑原有地址。"));
            }
        }
    }
    Ok(devices)
}

pub fn load(path: &Path) -> Result<Vec<Device>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    if fs::metadata(path)?.len() > 4 * 1024 * 1024 {
        return Err(message("配置文件过大，请备份后重新配对。"));
    }
    let plain = Zeroizing::new(protect(&fs::read(path)?, true)?);
    validate(serde_json::from_slice(&plain)?)
}

pub fn save(path: &Path, devices: &[Device]) -> Result<()> {
    let clean = validate(devices.to_vec())?;
    let plain = Zeroizing::new(serde_json::to_vec(&clean)?);
    let encrypted = protect(&plain, false)?;
    let pending = path.with_file_name(format!(".devices-{:016x}.tmp", rand::random::<u64>()));
    let result = (|| {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)?;
        file.write_all(&encrypted)?;
        file.sync_all()?;
        drop(file);
        replace_file(&pending, path)
    })();
    if pending.exists() {
        let _ = fs::remove_file(&pending);
    }
    result
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> Result<()> {
    fs::rename(source, target)?;
    Ok(())
}

pub fn backup_and_reset(path: &Path) -> Result<Option<PathBuf>> {
    let backup = path.with_file_name(format!("devices-{:016x}.bin.bak", rand::random::<u64>()));
    let result = if path.exists() {
        fs::copy(path, &backup)?;
        Some(backup)
    } else {
        None
    };
    save(path, &[])?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn fixture() -> Device {
        Device {
            ip: "192.168.6.100".into(),
            name: "测试灯".into(),
            did: Some(1234),
            token: hex::encode((0u8..16).collect::<Vec<_>>()),
            model: "yeelink.light.lamp22".into(),
            capabilities: None,
        }
    }
    #[test]
    fn validate_addresses_and_identity() {
        assert_eq!(
            parse_ips("192.168.6.1，10.0.0.2\n172.16.1.3")
                .unwrap()
                .len(),
            3
        );
        for value in [
            "",
            "bad",
            "8.8.8.8",
            "127.0.0.1",
            "224.0.0.1",
            "192.168.1.2,192.168.1.2",
        ] {
            assert!(parse_ips(value).is_err());
        }
        assert!(validate(vec![fixture(), fixture()]).is_err());
        let mut b = fixture();
        b.token = "ff".repeat(16);
        assert!(validate(vec![b]).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn legacy_format_and_atomic_encrypted_roundtrip() {
        let dir =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let path = dir.path().join("data/devices.bin");
        let devices = vec![fixture(), Device::pending("192.168.6.101", "")];
        save(&path, &devices).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].token, devices[0].token);
        assert!(!String::from_utf8_lossy(&fs::read(&path).unwrap()).contains(&devices[0].token));
        save(&path, &[]).unwrap();
        assert!(load(&path).unwrap().is_empty());
        // Python v1 wrote the same DPAPI-protected JSON array without capabilities.
        let legacy = br#"[{"ip":"192.168.6.100","name":"Legacy","did":1234,"token":"000102030405060708090a0b0c0d0e0f","model":"yeelink.light.lamp22"}]"#;
        fs::write(&path, protect(legacy, false).unwrap()).unwrap();
        assert_eq!(
            load(&path).unwrap()[0].caps().temperature.unwrap().range,
            Some([2700, 6500, 1])
        );
        fs::write(&path, b"corrupt").unwrap();
        assert!(load(&path).is_err());
        let backup = backup_and_reset(&path).unwrap().unwrap();
        assert_eq!(fs::read(backup).unwrap(), b"corrupt");
        assert!(load(&path).unwrap().is_empty());
    }
}

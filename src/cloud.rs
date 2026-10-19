use crate::{
    config::{validate, Device},
    control::Lamp,
    crypto::rc4,
    message,
    protocol::{probe, Session},
    spec::Capabilities,
    Result,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::{
    blocking::{Client, Response},
    cookie::{CookieStore, Jar},
    redirect::Policy,
    Url,
};
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

pub const REGIONS: [(&str, &str); 7] = [
    ("中国大陆", "cn"),
    ("新加坡", "sg"),
    ("德国", "de"),
    ("美国", "us"),
    ("中国台湾", "tw"),
    ("俄罗斯", "ru"),
    ("印度", "in"),
];
pub fn trusted_xiaomi_url(text: &str) -> Result<Url> {
    let url = Url::parse(text).map_err(|_| message("小米登录地址无效。"))?;
    let trusted = url.host_str().is_some_and(|host| {
        ["xiaomi.com", "mi.com"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    });
    if !trusted
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|p| p != 443)
    {
        return Err(message("登录服务地址校验失败，请重试。"));
    }
    Ok(url)
}
fn cloud_json(response: Response) -> Result<Value> {
    let text = response.error_for_status()?.text()?;
    Ok(serde_json::from_str(
        text.strip_prefix("&&&START&&&").unwrap_or(&text),
    )?)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn decode(text: &str) -> Result<Vec<u8>> {
    BASE64
        .decode(text)
        .map_err(|_| message("小米登录会话数据无效，请重新扫码。"))
}

pub struct QrCode {
    pub bytes: Vec<u8>,
    pub expires_in: u64,
}
pub struct CloudSetup {
    client: Client,
    jar: Arc<Jar>,
    region: String,
    pub cancelled: Arc<AtomicBool>,
    qr: Value,
    deadline: Option<Instant>,
    security: Zeroizing<String>,
    service_token: Zeroizing<String>,
    user_id: String,
}
impl CloudSetup {
    pub fn new(region: &str, cancelled: Arc<AtomicBool>) -> Result<Self> {
        if !REGIONS.iter().any(|(_, r)| *r == region) {
            return Err(message("米家地区无效。"));
        }
        let jar = Arc::new(Jar::default());
        let client = Client::builder()
            .cookie_provider(jar.clone())
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .user_agent(concat!(
                "QiLight/",
                env!("CARGO_PKG_VERSION"),
                " APP/com.xiaomi.mihome APPV/10.5.201"
            ))
            .redirect(Policy::custom(|attempt| {
                if attempt.previous().len() >= 10
                    || trusted_xiaomi_url(attempt.url().as_str()).is_err()
                {
                    attempt.error("untrusted Xiaomi redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        Ok(Self {
            client,
            jar,
            region: region.into(),
            cancelled,
            qr: Value::Null,
            deadline: None,
            security: Zeroizing::new(String::new()),
            service_token: Zeroizing::new(String::new()),
            user_id: String::new(),
        })
    }
    fn check_cancel(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(message("扫码已取消。"))
        } else {
            Ok(())
        }
    }
    pub fn create_qr(&mut self) -> Result<QrCode> {
        self.check_cancel()?;
        self.qr = cloud_json(
            self.client
                .get("https://account.xiaomi.com/longPolling/loginUrl")
                .query(&[
                    ("_qrsize", "480"),
                    ("qs", "%3Fsid%3Dxiaomiio%26_json%3Dtrue"),
                    ("callback", "https://sts.api.io.mi.com/sts"),
                    ("_hasLogo", "false"),
                    ("sid", "xiaomiio"),
                    ("serviceParam", ""),
                    ("_locale", "zh_CN"),
                    ("_dc", &(now() * 1000).to_string()),
                ])
                .send()?,
        )?;
        let image_url = trusted_xiaomi_url(
            self.qr["qr"]
                .as_str()
                .ok_or_else(|| message("小米暂未生成二维码，请重试。"))?,
        )?;
        trusted_xiaomi_url(
            self.qr["lp"]
                .as_str()
                .ok_or_else(|| message("小米登录服务缺少轮询地址。"))?,
        )?;
        let expires_in = self.qr["timeout"].as_u64().unwrap_or(180).clamp(1, 300);
        self.deadline = Some(Instant::now() + Duration::from_secs(expires_in));
        let bytes = self
            .client
            .get(image_url)
            .send()?
            .error_for_status()?
            .bytes()?
            .to_vec();
        self.check_cancel()?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(message("二维码图片异常，请重新生成。"));
        }
        Ok(QrCode { bytes, expires_in })
    }
    pub fn await_login(&mut self) -> Result<()> {
        let url = trusted_xiaomi_url(
            self.qr["lp"]
                .as_str()
                .ok_or_else(|| message("请先生成二维码。"))?,
        )?;
        while self.deadline.is_some_and(|d| Instant::now() < d) {
            self.check_cancel()?;
            let response = match self
                .client
                .get(url.clone())
                .timeout(Duration::from_secs(12))
                .send()
            {
                Ok(r) => r,
                Err(e) if e.is_timeout() => continue,
                Err(e) => return Err(e.into()),
            };
            self.check_cancel()?;
            if response.status().as_u16() != 200 {
                thread::sleep(Duration::from_millis(500));
                continue;
            }
            let reply = cloud_json(response)?;
            if let Some(security) = reply["ssecurity"].as_str() {
                *self.security = security.into();
                self.user_id = string_id(&reply["userId"]);
                let location = trusted_xiaomi_url(
                    reply["location"]
                        .as_str()
                        .ok_or_else(|| message("登录确认地址缺失。"))?,
                )?;
                let response = self
                    .client
                    .get(location.clone())
                    .send()?
                    .error_for_status()?;
                let final_url = response.url().clone();
                drop(response);
                for url in [
                    location,
                    final_url,
                    Url::parse("https://api.io.mi.com/").unwrap(),
                    Url::parse("https://sts.api.io.mi.com/").unwrap(),
                ] {
                    if let Some(cookies) = self.jar.cookies(&url) {
                        if let Ok(text) = cookies.to_str() {
                            if let Some(value) = text
                                .split(';')
                                .find_map(|c| c.trim().strip_prefix("serviceToken="))
                            {
                                *self.service_token = value.into();
                                break;
                            }
                        }
                    }
                }
                self.check_cancel()?;
                if self.service_token.is_empty() || self.user_id.is_empty() {
                    return Err(message("小米登录验证尚未完成，请重新扫码。"));
                }
                return Ok(());
            }
            return Err(message("二维码已过期或登录未确认，请重新生成。"));
        }
        Err(message("二维码已过期，请点击“重新生成二维码”。"))
    }
    pub fn api(&self, path: &str, payload: Value) -> Result<Value> {
        self.check_cancel()?;
        if self.security.is_empty() {
            return Err(message("请先完成米家扫码。"));
        }
        let mut nonce = Vec::from(rand::random::<[u8; 8]>());
        nonce.extend_from_slice(&((now() / 60) as u32).to_be_bytes());
        let mut hash = Sha256::new();
        hash.update(decode(&self.security)?);
        hash.update(&nonce);
        let key = Zeroizing::new(hash.finalize().to_vec());
        let signed = BASE64.encode(&*key);
        let signature = |fields: &[(String, String)]| {
            let mut terms = vec!["POST".to_string(), path.into()];
            terms.extend(fields.iter().map(|(k, v)| format!("{k}={v}")));
            terms.push(signed.clone());
            BASE64.encode(Sha1::digest(terms.join("&").as_bytes()))
        };
        let mut fields = vec![("data".to_string(), serde_json::to_string(&payload)?)];
        fields.push(("rc4_hash__".into(), signature(&fields)));
        for (_, value) in &mut fields {
            *value = BASE64.encode(rc4(value.as_bytes(), &key, 1024)?);
        }
        fields.extend([
            ("signature".into(), signature(&fields)),
            ("ssecurity".into(), self.security.to_string()),
            ("_nonce".into(), BASE64.encode(nonce)),
        ]);
        let host = if self.region == "cn" {
            "api.io.mi.com".to_string()
        } else {
            format!("{}.api.io.mi.com", self.region)
        };
        // Credentials are carried in the encrypted form and Cookie header, never in URLs/logs.
        let cookie=format!("userId={}; serviceToken={}; yetAnotherServiceToken={}; locale=zh_CN; timezone=GMT+08:00; is_daylight=0; dst_offset=0; channel=MI_APP_STORE",self.user_id,*self.service_token,*self.service_token);
        let response = self
            .client
            .post(format!("https://{host}/app{path}"))
            .form(&fields)
            .header("Cookie", cookie)
            .header("Accept-Encoding", "identity")
            .header("x-xiaomi-protocal-flag-cli", "PROTOCAL-HTTP2")
            .header("MIOT-ENCRYPT-ALGORITHM", "ENCRYPT-RC4")
            .send()?
            .error_for_status()?
            .text()?;
        self.check_cancel()?;
        let plain = Zeroizing::new(rc4(&decode(response.trim())?, &key, 1024)?);
        let result: Value = serde_json::from_slice(&plain)?;
        if result["code"].as_i64() != Some(0) {
            return Err(message(format!(
                "米家服务返回错误代码 {}，请重新扫码。",
                result["code"]
            )));
        }
        Ok(result["result"].clone())
    }
    pub fn find_lamps(&self, targets: &[Device], progress: impl Fn(&str)) -> Result<PairingResult> {
        self.check_cancel()?;
        progress("已登录，正在核对局域网设备身份…");
        let (tx, rx) = crossbeam_channel::unbounded();
        let (jobs_tx, jobs_rx) = crossbeam_channel::unbounded::<Device>();
        for d in targets {
            let _ = jobs_tx.send(d.clone());
        }
        drop(jobs_tx);
        thread::scope(|scope| {
            for _ in 0..targets.len().min(8) {
                let jobs = jobs_rx.clone();
                let tx = tx.clone();
                scope.spawn(move || {
                    while let Ok(device) = jobs.recv() {
                        if self.cancelled.load(Ordering::Relaxed) {
                            break;
                        }
                        let result = probe(&device.ip);
                        let _ = tx.send((device, result));
                    }
                });
            }
        });
        drop(tx);
        self.check_cancel()?;
        let mut originals = HashMap::new();
        let mut errors = HashMap::new();
        for (device, result) in rx {
            match result {
                Ok(did) => {
                    originals.insert(did, device);
                }
                Err(e) => {
                    errors.insert(device.ip.clone(), e.to_string());
                }
            }
        }
        if originals.is_empty() {
            return Ok(PairingResult {
                updates: Vec::new(),
                errors,
            });
        }
        progress("正在读取米家设备列表与型号能力…");
        let mut homes = Vec::new();
        let result = self.api(
            "/v2/homeroom/gethome",
            json!({"fg":true,"fetch_share":true,"fetch_share_dev":true,"limit":300,"app_ver":7}),
        )?;
        // gethome.has_more concerns room/device membership, not the home list.
        // We enumerate every home's devices through its independently paginated endpoint.
        for key in ["homelist", "share_home_list"] {
            if let Some(list) = result[key].as_array() {
                homes.extend(list.iter().cloned());
            }
        }
        let mut selected = HashMap::new();
        let mut visited = HashSet::new();
        for home in homes {
            if !visited.insert(home["id"].to_string()) {
                continue;
            }
            let mut cursor = Value::Null;
            let mut seen = HashSet::new();
            loop {
                let mut payload = json!({"home_owner":home.get("uid").cloned().unwrap_or(json!(self.user_id)),"home_id":home["id"],"limit":200,"get_split_device":true,"support_smart_home":true});
                if !cursor.is_null() {
                    payload["start_did"] = cursor.clone();
                }
                let result = self.api("/v2/home/home_device_list", payload)?;
                let list = result["device_info"]
                    .as_array()
                    .ok_or_else(|| message("米家设备列表格式异常。"))?;
                for cloud in list {
                    if let Ok(did) = string_id(&cloud["did"]).parse::<u32>() {
                        if let Some(original) = originals.get(&did) {
                            let mut device = original.clone();
                            device.did = Some(did);
                            device.token = cloud["token"].as_str().unwrap_or("").into();
                            device.model = cloud["model"].as_str().unwrap_or("").into();
                            device.capabilities = None;
                            selected.insert(did, device);
                        }
                    }
                }
                if selected.len() == originals.len() || result["has_more"] != true {
                    break;
                }
                cursor = result
                    .get("max_did")
                    .cloned()
                    .or_else(|| list.last().and_then(|d| d.get("did").cloned()))
                    .unwrap_or(Value::Null);
                if cursor.is_null() || !seen.insert(cursor.to_string()) || seen.len() > 100 {
                    return Err(message("米家设备列表分页异常，未保存本次配对。"));
                }
            }
            if selected.len() == originals.len() {
                break;
            }
        }
        let mut caps_cache: HashMap<String, Capabilities> = HashMap::new();
        let mut candidates = Vec::new();
        for (did, original) in originals {
            self.check_cancel()?;
            let Some(mut device) = selected.remove(&did) else {
                errors.insert(
                    original.ip.clone(),
                    "账号中未找到此灯，请核对米家账号、地区与家庭权限。".into(),
                );
                continue;
            };
            let caps = if let Some(caps) = caps_cache.get(&device.model) {
                Ok(caps.clone())
            } else {
                fetch_capabilities(&device.model)
            };
            match caps {
                Ok(caps) => {
                    caps_cache.insert(device.model.clone(), caps.clone());
                    device.capabilities = Some(caps);
                }
                Err(e) => {
                    errors.insert(original.ip.clone(), format!("型号规格读取失败：{e}"));
                    continue;
                }
            }
            match validate(vec![device]) {
                Ok(mut devices) if devices[0].paired() => candidates.push(devices.remove(0)),
                _ => {
                    errors.insert(
                        original.ip.clone(),
                        "密钥不可用，请使用绑定此灯的米家账号。".into(),
                    );
                }
            }
        }
        progress("正在验证设备密钥与灯光状态…");
        let (tx, rx) = crossbeam_channel::unbounded();
        let (jobs_tx, jobs_rx) = crossbeam_channel::unbounded();
        let count = candidates.len();
        for d in candidates {
            let _ = jobs_tx.send(d);
        }
        drop(jobs_tx);
        thread::scope(|scope| {
            for _ in 0..count.min(8) {
                let tx = tx.clone();
                let jobs = jobs_rx.clone();
                scope.spawn(move || {
                    while let Ok(device) = jobs.recv() {
                        if self.cancelled.load(Ordering::Relaxed) {
                            break;
                        }
                        let result = Session::new(&device).and_then(|session| {
                            Lamp::new(session, device.did.unwrap(), device.caps()).status()
                        });
                        let _ = tx.send((device, result));
                    }
                });
            }
        });
        drop(tx);
        self.check_cancel()?;
        let mut updates = Vec::new();
        for (device, result) in rx {
            match result {
                Ok(_) => updates.push(device),
                Err(e) => {
                    errors.insert(device.ip.clone(), e.to_string());
                }
            }
        }
        Ok(PairingResult { updates, errors })
    }
}
pub struct PairingResult {
    pub updates: Vec<Device>,
    pub errors: HashMap<String, String>,
}
fn string_id(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_else(|| {
        if v.is_number() {
            v.to_string()
        } else {
            String::new()
        }
    })
}

pub fn fetch_capabilities(model: &str) -> Result<Capabilities> {
    if model == "yeelink.light.lamp22" {
        return Ok(Capabilities::for_model(model));
    }
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(6))
        .timeout(Duration::from_secs(20))
        .build()?;
    let list: Value = client
        .get("https://miot-spec.org/miot-spec-v2/instances")
        .query(&[("status", "released")])
        .send()?
        .error_for_status()?
        .json()?;
    let urn = list["instances"]
        .as_array()
        .and_then(|list| {
            list.iter()
                .filter(|v| v["model"].as_str() == Some(model))
                .max_by_key(|v| v["version"].as_u64().unwrap_or(0))
        })
        .and_then(|v| v["type"].as_str())
        .ok_or_else(|| message("未找到该型号的米家公开规格。"))?;
    let spec: Value = client
        .get("https://miot-spec.org/miot-spec-v2/instance")
        .query(&[("type", urn)])
        .send()?
        .error_for_status()?
        .json()?;
    Capabilities::from_spec(&spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_login_urls_and_regions() {
        for url in [
            "https://account.xiaomi.com/qr",
            "https://sts.api.io.mi.com/sts",
        ] {
            assert!(trusted_xiaomi_url(url).is_ok());
        }
        for url in [
            "http://account.xiaomi.com/",
            "https://xiaomi.com.attacker.test/",
            "https://evilxiaomi.com/",
            "https://mi.com@evil.test/",
            "https://mi.com:444/",
            "https://user@mi.com/",
        ] {
            assert!(trusted_xiaomi_url(url).is_err());
        }
        assert!(CloudSetup::new("invalid", Arc::new(AtomicBool::new(false))).is_err());
    }
    #[test]
    fn cancelled_attempt_cannot_start_network_request() {
        let mut setup = CloudSetup::new("cn", Arc::new(AtomicBool::new(true))).unwrap();
        assert!(setup.create_qr().is_err());
    }
}

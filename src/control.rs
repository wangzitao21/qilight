use crate::{
    config::Device,
    message,
    protocol::{Rpc, Session},
    spec::{Capabilities, Property},
    Error, Result,
};
use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LightState {
    pub power: bool,
    pub brightness: Option<u32>,
    pub temperature: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Adjustment {
    pub brightness: Option<u32>,
    pub temperature: Option<u32>,
    pub transition_ms: u32,
}
impl Adjustment {
    pub fn validate(&self, caps: &Capabilities) -> Result<()> {
        if self.brightness.is_none() && self.temperature.is_none() {
            return Err(message("请至少选择亮度或色温。"));
        }
        if self.transition_ms != 0 && !(30..=2000).contains(&self.transition_ms) {
            return Err(message("平滑过渡时间应为 30–2000 毫秒。"));
        }
        if let Some(v) = self.brightness {
            caps.brightness
                .as_ref()
                .ok_or_else(|| message("此型号不支持亮度调节，或尚未读取规格。"))?
                .validate(v)?;
        }
        if let Some(v) = self.temperature {
            caps.temperature
                .as_ref()
                .ok_or_else(|| message("此型号不支持色温调节，或尚未读取规格。"))?
                .validate(v)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub enum Action {
    Refresh,
    Power(bool),
    Adjust(Adjustment),
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Unknown,
    Legacy,
    Miot,
}

pub struct Lamp<R: Rpc> {
    rpc: R,
    did: u32,
    caps: Capabilities,
    mode: Mode,
}
fn unsupported(error: &Error) -> bool {
    matches!(error, Error::Device(-32601 | -4001))
}
fn number(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|x| x.try_into().ok())
        .or_else(|| value.as_str()?.parse().ok())
}
fn property_request(did: u32, p: &Property) -> Value {
    json!({"did":did.to_string(),"siid":p.siid,"piid":p.piid})
}
fn property_value<'a>(rows: &'a Value, p: &Property) -> Result<&'a Value> {
    let row = rows
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|r| r["siid"] == p.siid && r["piid"] == p.piid)
        })
        .ok_or_else(|| message("设备未返回所请求的属性。"))?;
    if row["code"].as_i64() != Some(0) {
        return Err(Error::Device(row["code"].as_i64().unwrap_or(-1)));
    }
    row.get("value")
        .ok_or_else(|| message("设备属性缺少数值。"))
}
impl<R: Rpc> Lamp<R> {
    pub fn new(rpc: R, did: u32, caps: Capabilities) -> Self {
        Self {
            rpc,
            did,
            caps,
            mode: Mode::Unknown,
        }
    }
    fn legacy_status(&mut self) -> Result<LightState> {
        let result = self
            .rpc
            .call("get_prop", json!(["power", "bright", "ct"]))?;
        // Only an authenticated reply with an incompatible status shape warrants
        // trying MIoT. Transport, identity and checksum failures propagate intact.
        self.parse_legacy_status(&result)
            .map_err(|error| Error::InvalidLegacyStatus(error.to_string()))
    }
    fn parse_legacy_status(&self, result: &Value) -> Result<LightState> {
        let rows = result
            .as_array()
            .ok_or_else(|| message("灯具状态格式无效。"))?;
        let power = match rows.first().and_then(Value::as_str) {
            Some("on") => true,
            Some("off") => false,
            _ => return Err(message("未取得灯具开关状态。")),
        };
        let brightness = rows.get(1).and_then(number);
        let temperature = rows.get(2).and_then(number);
        if let Some(p) = &self.caps.brightness {
            p.validate(brightness.ok_or_else(|| message("未取得亮度状态。"))?)?;
        }
        if let Some(p) = &self.caps.temperature {
            p.validate(temperature.ok_or_else(|| message("未取得色温状态。"))?)?;
        }
        Ok(LightState {
            power,
            brightness,
            temperature,
        })
    }
    fn miot_status(&mut self) -> Result<LightState> {
        let power = self
            .caps
            .power
            .as_ref()
            .ok_or_else(|| message("此型号尚无 MIoT 属性映射，请重新扫码读取规格。"))?;
        let mut props = vec![property_request(self.did, power)];
        if let Some(p) = &self.caps.brightness {
            props.push(property_request(self.did, p));
        }
        if let Some(p) = &self.caps.temperature {
            props.push(property_request(self.did, p));
        }
        let result = self.rpc.call("get_properties", Value::Array(props))?;
        let power = property_value(&result, power)?
            .as_bool()
            .ok_or_else(|| message("开关状态无效。"))?;
        let read_number = |p: &Option<Property>| -> Result<Option<u32>> {
            match p {
                None => Ok(None),
                Some(p) => {
                    let n = number(property_value(&result, p)?)
                        .ok_or_else(|| message("灯具属性数值无效。"))?;
                    p.validate(n)?;
                    Ok(Some(n))
                }
            }
        };
        Ok(LightState {
            power,
            brightness: read_number(&self.caps.brightness)?,
            temperature: read_number(&self.caps.temperature)?,
        })
    }
    pub fn status(&mut self) -> Result<LightState> {
        if self.mode == Mode::Miot {
            return self.miot_status();
        }
        match self.legacy_status() {
            Ok(state) => {
                self.mode = Mode::Legacy;
                Ok(state)
            }
            Err(ref error)
                if unsupported(error)
                    || (self.mode == Mode::Unknown
                        && matches!(error, Error::InvalidLegacyStatus(_))) =>
            {
                let state = self.miot_status()?;
                self.mode = Mode::Miot;
                Ok(state)
            }
            Err(error) => Err(error),
        }
    }
    fn miot_write(&mut self, values: &[(Property, Value)]) -> Result<()> {
        let props = values
            .iter()
            .map(|(p, value)| {
                let mut r = property_request(self.did, p);
                r["value"] = value.clone();
                r
            })
            .collect::<Vec<_>>();
        let result = self.rpc.call("set_properties", Value::Array(props))?;
        let rows = result
            .as_array()
            .ok_or_else(|| message("设备未确认调节指令。"))?;
        for (p, _) in values {
            let row = rows
                .iter()
                .find(|r| r["siid"] == p.siid && r["piid"] == p.piid)
                .ok_or_else(|| message("设备仅确认了部分调节指令，请刷新状态。"))?;
            if row["code"].as_i64() != Some(0) {
                return Err(Error::Device(row["code"].as_i64().unwrap_or(-1)));
            }
        }
        Ok(())
    }
    fn legacy_write(&mut self, method: &str, params: Value) -> Result<()> {
        if self.rpc.call(method, params)? != json!(["ok"]) {
            return Err(message("设备未确认调节指令，请刷新后重试。"));
        }
        Ok(())
    }
    pub fn perform(&mut self, action: &Action) -> Result<LightState> {
        match action {
            Action::Refresh => self.status(),
            Action::Power(enabled) => {
                if self.mode != Mode::Miot {
                    match self.legacy_write(
                        "set_power",
                        json!([if *enabled { "on" } else { "off" }, "sudden", 0, 0]),
                    ) {
                        Ok(()) => {}
                        Err(ref e) if unsupported(e) => self.mode = Mode::Miot,
                        Err(e) => return Err(e),
                    }
                }
                if self.mode == Mode::Miot {
                    self.miot_write(&[(
                        self.caps
                            .power
                            .clone()
                            .ok_or_else(|| message("此灯没有开关属性映射。"))?,
                        json!(enabled),
                    )])?;
                }
                self.readback(|s| s.power == *enabled)
            }
            Action::Adjust(settings) => {
                settings.validate(&self.caps)?;
                let before = self.status()?;
                if !before.power {
                    return Err(message("灯当前已关闭，请先开灯再调节亮度或色温。"));
                }
                let brightness = settings
                    .brightness
                    .filter(|v| before.brightness != Some(*v));
                let temperature = settings
                    .temperature
                    .filter(|v| before.temperature != Some(*v));
                // This is fresh readback, not a UI snapshot: avoid restarting a
                // transition or sending another write when the target is reached.
                if brightness.is_none() && temperature.is_none() {
                    return Ok(before);
                }
                let mut values = Vec::new();
                if let Some(v) = brightness {
                    values.push((self.caps.brightness.clone().unwrap(), json!(v)));
                }
                if let Some(v) = temperature {
                    values.push((self.caps.temperature.clone().unwrap(), json!(v)));
                }
                let mut wait = 0;
                if self.mode != Mode::Miot {
                    let effect = if settings.transition_ms == 0 {
                        "sudden"
                    } else {
                        "smooth"
                    };
                    // Keep both changes on the same connection. The UI coalesces live edits.
                    for (method, value) in [("set_bright", brightness), ("set_ct_abx", temperature)]
                    {
                        if let Some(value) = value {
                            match self.legacy_write(
                                method,
                                json!([value, effect, settings.transition_ms]),
                            ) {
                                Ok(()) => wait = settings.transition_ms,
                                Err(ref e) if unsupported(e) => {
                                    self.mode = Mode::Miot;
                                    break;
                                }
                                Err(e) => return Err(e),
                            }
                        }
                    }
                }
                if self.mode == Mode::Miot {
                    self.miot_write(&values)?;
                }
                if wait > 0 {
                    thread::sleep(Duration::from_millis(wait as u64));
                }
                self.readback(|s| {
                    settings.brightness.is_none_or(|v| s.brightness == Some(v))
                        && settings
                            .temperature
                            .is_none_or(|v| s.temperature == Some(v))
                })
            }
        }
    }
    fn readback(&mut self, expected: impl Fn(&LightState) -> bool) -> Result<LightState> {
        for attempt in 0..3 {
            if attempt > 0 {
                thread::sleep(Duration::from_millis(180));
            }
            let state = self.status()?;
            if expected(&state) {
                return Ok(state);
            }
        }
        Err(message("指令已发送，但灯具回读值尚未一致；请刷新或重试。"))
    }
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub ip: String,
    pub state: Option<LightState>,
    pub error: Option<String>,
}
struct CachedLamp {
    device: Device,
    lamp: Mutex<Lamp<Session>>,
}
type Cache = Arc<Mutex<HashMap<String, Arc<CachedLamp>>>>;
pub struct Job {
    pub group: u64,
    pub device: Device,
    pub action: Action,
}
pub struct Completion {
    pub group: u64,
    pub outcome: Outcome,
}
pub struct Engine {
    jobs: Option<Sender<Job>>,
    pub results: Receiver<Completion>,
    cache: Cache,
}

impl Engine {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<Job>();
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        let cache: Cache = Arc::new(Mutex::new(HashMap::new()));
        for _ in 0..8 {
            let rx = rx.clone();
            let results = results_tx.clone();
            let cache = cache.clone();
            let wake = wake.clone();
            thread::spawn(move || {
                while let Ok(job) = rx.recv() {
                    let outcome = operate(&cache, &job.device, &job.action);
                    if results
                        .send(Completion {
                            group: job.group,
                            outcome,
                        })
                        .is_err()
                    {
                        break;
                    }
                    wake();
                }
            });
        }
        Self {
            jobs: Some(tx),
            results: results_rx,
            cache,
        }
    }
    pub fn retain_connections(&self, devices: &[Device]) {
        let devices: HashMap<_, _> = devices.iter().map(|d| (d.ip.as_str(), d)).collect();
        if let Ok(mut cache) = self.cache.lock() {
            // Connection identity is immutable and separate from the active RPC
            // lock, so saving configuration never waits on a slow device.
            cache.retain(|ip, entry| {
                devices
                    .get(ip.as_str())
                    .is_some_and(|device| entry.device.same_connection(device))
            });
        }
    }
    pub fn submit(&self, group: u64, device: Device, action: Action) {
        if let Some(tx) = &self.jobs {
            let _ = tx.send(Job {
                group,
                device,
                action,
            });
        }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.jobs.take();
    }
}

fn operate(cache: &Cache, device: &Device, action: &Action) -> Outcome {
    let mut outcome = Outcome {
        ip: device.ip.clone(),
        state: None,
        error: None,
    };
    let result = (|| -> Result<LightState> {
        if !device.paired() {
            return Err(message("待配对，请点击“米家配对”。"));
        }
        let client = {
            let mut map = cache
                .lock()
                .map_err(|_| message("连接缓存异常，请重启软件。"))?;
            // Bound retained sockets and credentials even after repeated edits/removals.
            if map.len() >= 128 && !map.contains_key(&device.ip) {
                map.clear();
            }
            if let Some(client) = map
                .get(&device.ip)
                .filter(|entry| entry.device.same_connection(device))
            {
                client.clone()
            } else {
                let lamp = Lamp::new(Session::new(device)?, device.did.unwrap(), device.caps());
                let client = Arc::new(CachedLamp {
                    device: device.clone(),
                    lamp: Mutex::new(lamp),
                });
                map.insert(device.ip.clone(), client.clone());
                client
            }
        };
        let mut lamp = client
            .lamp
            .lock()
            .map_err(|_| message("灯具连接异常，请重启软件。"))?;
        match lamp.perform(action) {
            Ok(state) => Ok(state),
            Err(error) => {
                // A partially applied multi-property command must show actual state.
                if matches!(action, Action::Adjust(_) | Action::Power(_))
                    && !matches!(&error, Error::Connection(_) | Error::Io(_))
                {
                    outcome.state = lamp.status().ok();
                }
                Err(error)
            }
        }
    })();
    match result {
        Ok(state) => outcome.state = Some(state),
        Err(error) => outcome.error = Some(error.to_string()),
    };
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    type Calls = Arc<Mutex<Vec<(String, Value)>>>;
    struct Mock {
        responses: VecDeque<Result<Value>>,
        calls: Calls,
    }
    impl Rpc for Mock {
        fn call(&mut self, method: &str, params: Value) -> Result<Value> {
            self.calls.lock().unwrap().push((method.into(), params));
            self.responses.pop_front().expect("unexpected RPC")
        }
    }
    fn setup(responses: Vec<Result<Value>>) -> (Lamp<Mock>, Calls) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Lamp::new(
                Mock {
                    responses: responses.into(),
                    calls: calls.clone(),
                },
                1234,
                Capabilities::for_model("yeelink.light.lamp22"),
            ),
            calls,
        )
    }
    fn state(power: bool, b: u32, t: u32) -> Value {
        json!([{"siid":2,"piid":3,"code":0,"value":t},{"siid":2,"piid":1,"code":0,"value":power},{"siid":2,"piid":2,"code":0,"value":b}])
    }
    #[test]
    fn connection_and_integrity_failures_do_not_probe_another_protocol() {
        for error in [
            Error::Connection(std::io::ErrorKind::TimedOut.into()),
            Error::Io(std::io::ErrorKind::ConnectionReset.into()),
            message("设备密钥校验失败，请点击“米家配对”。"),
            message("此 IP 的设备已变化，请核对地址并重新扫码。"),
        ] {
            let (mut lamp, calls) = setup(vec![Err(error)]);
            assert!(lamp.status().is_err());
            assert_eq!(calls.lock().unwrap().len(), 1);
        }
    }
    #[test]
    fn incomplete_legacy_reply_still_discovers_and_remembers_miot() {
        let (mut lamp, calls) = setup(vec![
            Ok(json!(["on", "", ""])),
            Ok(state(true, 40, 4000)),
            Ok(state(true, 50, 4500)),
        ]);
        assert_eq!(lamp.status().unwrap().brightness, Some(40));
        assert_eq!(lamp.status().unwrap().brightness, Some(50));
        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].0, "get_prop");
        assert_eq!(calls[1].0, "get_properties");
        assert_eq!(calls[2].0, "get_properties");
    }
    #[test]
    fn cache_keeps_renamed_lamps_and_evicts_only_changed_or_removed_connections() {
        let (_, results) = crossbeam_channel::unbounded();
        let engine = Engine {
            jobs: None,
            results,
            cache: Arc::new(Mutex::new(HashMap::new())),
        };
        let mut devices = Vec::new();
        for i in 1..=5 {
            let device = Device {
                ip: format!("192.168.1.{i}"),
                name: format!("灯 {i}"),
                did: Some(i),
                token: "12".repeat(16),
                model: "yeelink.light.test".into(),
                capabilities: Some(Capabilities::for_model("yeelink.light.lamp22")),
            };
            let session = Session::at(
                &device,
                "127.0.0.1:9".parse().unwrap(),
                Duration::from_millis(50),
            )
            .unwrap();
            engine.cache.lock().unwrap().insert(
                device.ip.clone(),
                Arc::new(CachedLamp {
                    device: device.clone(),
                    lamp: Mutex::new(Lamp::new(session, i, device.caps())),
                }),
            );
            devices.push(device);
        }
        let retained = engine.cache.lock().unwrap()[&devices[0].ip].clone();
        devices[0].name = "只改名称".into();
        devices[1].token = "34".repeat(16);
        devices[2].did = Some(30);
        devices[3]
            .capabilities
            .as_mut()
            .unwrap()
            .temperature
            .as_mut()
            .unwrap()
            .range = Some([3000, 5000, 100]);
        devices.pop();
        engine.retain_connections(&devices);
        let cache = engine.cache.lock().unwrap();
        assert_eq!(cache.len(), 1);
        assert!(Arc::ptr_eq(&cache[&devices[0].ip], &retained));
        drop(cache);
        devices[0].ip = "192.168.1.100".into();
        engine.retain_connections(&devices);
        assert!(engine.cache.lock().unwrap().is_empty());
    }
    #[test]
    fn adjusts_independent_properties_and_reads_actual_values() {
        let (mut lamp, calls) = setup(vec![
            Ok(json!(["on", "40", "4000"])),
            Ok(json!(["ok"])),
            Ok(json!(["on", "65", "4000"])),
        ]);
        let state = lamp
            .perform(&Action::Adjust(Adjustment {
                brightness: Some(65),
                temperature: None,
                transition_ms: 0,
            }))
            .unwrap();
        assert_eq!(state.brightness, Some(65));
        assert_eq!(state.temperature, Some(4000));
        assert_eq!(
            calls.lock().unwrap()[1],
            ("set_bright".into(), json!([65, "sudden", 0]))
        );
    }
    #[test]
    fn miot_fallback_batches_properties_and_matches_ids_not_order() {
        let (mut lamp, calls) = setup(vec![
            Err(Error::Device(-32601)),
            Ok(state(true, 40, 4000)),
            Ok(json!([{"siid":2,"piid":2,"code":0},{"siid":2,"piid":3,"code":0}])),
            Ok(state(true, 60, 4500)),
        ]);
        lamp.perform(&Action::Adjust(Adjustment {
            brightness: Some(60),
            temperature: Some(4500),
            transition_ms: 0,
        }))
        .unwrap();
        assert_eq!(calls.lock().unwrap()[2].0, "set_properties");
        assert_eq!(calls.lock().unwrap()[2].1.as_array().unwrap().len(), 2);
    }
    #[test]
    fn already_reached_targets_require_only_fresh_status() {
        for miot in [false, true] {
            let response = if miot {
                state(true, 65, 4000)
            } else {
                json!(["on", "65", "4000"])
            };
            let (mut lamp, calls) = setup(vec![Ok(response)]);
            if miot {
                lamp.mode = Mode::Miot;
            }
            let actual = lamp
                .perform(&Action::Adjust(Adjustment {
                    brightness: Some(65),
                    temperature: Some(4000),
                    transition_ms: 80,
                }))
                .unwrap();
            assert_eq!(actual.brightness, Some(65));
            assert_eq!(actual.temperature, Some(4000));
            assert_eq!(calls.lock().unwrap().len(), 1);
        }
    }
    #[test]
    fn mixed_adjustment_writes_only_changed_property() {
        for miot in [false, true] {
            for change_brightness in [false, true] {
                let (brightness, temperature) = if change_brightness {
                    (65, 4000)
                } else {
                    (40, 4500)
                };
                let piid = if change_brightness { 2 } else { 3 };
                let responses = if miot {
                    vec![
                        Ok(state(true, 40, 4000)),
                        Ok(json!([{"siid":2,"piid":piid,"code":0}])),
                        Ok(state(true, brightness, temperature)),
                    ]
                } else {
                    vec![
                        Ok(json!(["on", "40", "4000"])),
                        Ok(json!(["ok"])),
                        Ok(json!(["on", brightness, temperature])),
                    ]
                };
                let (mut lamp, calls) = setup(responses);
                if miot {
                    lamp.mode = Mode::Miot;
                }
                let actual = lamp
                    .perform(&Action::Adjust(Adjustment {
                        brightness: Some(brightness),
                        temperature: Some(temperature),
                        transition_ms: 0,
                    }))
                    .unwrap();
                assert_eq!(actual.brightness, Some(brightness));
                assert_eq!(actual.temperature, Some(temperature));
                let calls = calls.lock().unwrap();
                assert_eq!(calls.len(), 3);
                if miot {
                    assert_eq!(calls[1].0, "set_properties");
                    assert_eq!(calls[1].1.as_array().unwrap().len(), 1);
                    assert_eq!(calls[1].1[0]["piid"], piid);
                } else {
                    assert_eq!(
                        calls[1].0,
                        if change_brightness {
                            "set_bright"
                        } else {
                            "set_ct_abx"
                        }
                    );
                }
            }
        }
    }
    #[test]
    fn rejects_invalid_range_before_network_and_never_turns_on_closed_light() {
        let (mut lamp, calls) = setup(vec![Ok(json!(["off", "40", "4000"]))]);
        assert!(lamp
            .perform(&Action::Adjust(Adjustment {
                brightness: Some(101),
                ..Default::default()
            }))
            .is_err());
        assert!(calls.lock().unwrap().is_empty());
        assert!(lamp
            .perform(&Action::Adjust(Adjustment {
                brightness: Some(50),
                ..Default::default()
            }))
            .is_err());
        assert_eq!(calls.lock().unwrap().len(), 1);
    }
    #[test]
    fn write_ack_is_not_readback_success() {
        let (mut lamp, _) = setup(vec![
            Ok(json!(["ok"])),
            Ok(json!(["off", "40", "4000"])),
            Ok(json!(["off", "40", "4000"])),
            Ok(json!(["off", "40", "4000"])),
        ]);
        assert!(lamp.perform(&Action::Power(true)).is_err());
    }
    #[test]
    fn partial_miot_failure_is_not_success() {
        let (mut lamp, _) = setup(vec![
            Err(Error::Device(-32601)),
            Ok(state(true, 40, 4000)),
            Ok(json!([{"siid":2,"piid":2,"code":0},{"siid":2,"piid":3,"code":-4001}])),
        ]);
        assert!(lamp
            .perform(&Action::Adjust(Adjustment {
                brightness: Some(60),
                temperature: Some(4500),
                transition_ms: 0
            }))
            .is_err());
    }
}

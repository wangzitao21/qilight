use qilight::{
    config::Device,
    control::{Action, Adjustment, Lamp},
    crypto::Codec,
    protocol::{decode_envelope, encode_packet, Session, HELLO},
    spec::Capabilities,
};
use serde_json::{json, Value};
use std::{
    net::UdpSocket,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

struct MockLight {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    hellos: Arc<AtomicUsize>,
    thread: Option<thread::JoinHandle<()>>,
}
impl MockLight {
    fn start(drop_first: bool, stale: bool, miot_only: bool) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let halt = stop.clone();
        let hellos = Arc::new(AtomicUsize::new(0));
        let hello_count = hellos.clone();
        let thread = thread::spawn(move || {
            let codec = Codec::new([0x12; 16]);
            let mut buffer = [0; 8192];
            let mut power = true;
            let mut brightness = 40u32;
            let mut temperature = 4000u32;
            let mut dropped = false;
            let mut previous: Option<Vec<u8>> = None;
            while !halt.load(Ordering::Relaxed) {
                let Ok((n, peer)) = socket.recv_from(&mut buffer) else {
                    continue;
                };
                if buffer[..n] == HELLO {
                    hello_count.fetch_add(1, Ordering::Relaxed);
                    let mut reply = [0u8; 32];
                    reply[..4].copy_from_slice(&[0x21, 0x31, 0, 32]);
                    reply[8..12].copy_from_slice(&1234u32.to_be_bytes());
                    reply[12..16].copy_from_slice(&10000u32.to_be_bytes());
                    socket.send_to(&reply, peer).unwrap();
                    continue;
                }
                if drop_first && !dropped {
                    dropped = true;
                    continue;
                }
                let request = decode_envelope(&codec, &buffer[..n], 1234).unwrap();
                let id = request["id"].as_u64().unwrap();
                let params = &request["params"];
                let method = request["method"].as_str().unwrap();
                let result = if miot_only
                    && ["get_prop", "set_power", "set_bright", "set_ct_abx"].contains(&method)
                {
                    json!({"id":id,"error":{"code":-32601}})
                } else {
                    let value = match method {
                        "get_prop" => json!([
                            if power { "on" } else { "off" },
                            brightness.to_string(),
                            temperature.to_string()
                        ]),
                        "set_power" => {
                            power = params[0] == "on";
                            json!(["ok"])
                        }
                        "set_bright" => {
                            brightness = params[0].as_u64().unwrap() as u32;
                            json!(["ok"])
                        }
                        "set_ct_abx" => {
                            temperature = params[0].as_u64().unwrap() as u32;
                            json!(["ok"])
                        }
                        "get_properties" => {
                            json!([{"siid":2,"piid":3,"code":0,"value":temperature},{"siid":2,"piid":2,"code":0,"value":brightness},{"siid":2,"piid":1,"code":0,"value":power}])
                        }
                        "set_properties" => Value::Array(
                            params
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|p| {
                                    match p["piid"].as_u64().unwrap() {
                                        1 => power = p["value"].as_bool().unwrap(),
                                        2 => brightness = p["value"].as_u64().unwrap() as u32,
                                        3 => temperature = p["value"].as_u64().unwrap() as u32,
                                        _ => panic!("unknown property"),
                                    };
                                    json!({"siid":p["siid"],"piid":p["piid"],"code":0})
                                })
                                .collect(),
                        ),
                        _ => panic!("unexpected method"),
                    };
                    json!({"id":id,"result":value})
                };
                if stale {
                    if let Some(packet) = &previous {
                        socket.send_to(packet, peer).unwrap();
                    }
                }
                let packet = encode_packet(&codec, 1234, 10000, &result).unwrap();
                socket.send_to(&packet, peer).unwrap();
                previous = Some(packet);
            }
        });
        Self {
            address,
            stop,
            hellos,
            thread: Some(thread),
        }
    }
    fn lamp(&self) -> Lamp<Session> {
        let device = Device {
            ip: "192.168.1.10".into(),
            name: "mock".into(),
            did: Some(1234),
            token: "12".repeat(16),
            model: "yeelink.light.lamp22".into(),
            capabilities: None,
        };
        Lamp::new(
            Session::at(&device, self.address, Duration::from_millis(150)).unwrap(),
            1234,
            Capabilities::for_model(&device.model),
        )
    }
}
impl Drop for MockLight {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn udp_roundtrip_reuses_handshake_and_ignores_stale_responses() {
    let server = MockLight::start(false, true, false);
    let mut lamp = server.lamp();
    assert_eq!(lamp.status().unwrap().brightness, Some(40));
    let state = lamp
        .perform(&Action::Adjust(Adjustment {
            brightness: Some(73),
            temperature: Some(4321),
            transition_ms: 0,
        }))
        .unwrap();
    assert_eq!(
        (state.brightness, state.temperature),
        (Some(73), Some(4321))
    );
    assert!(!lamp.perform(&Action::Power(false)).unwrap().power);
    assert_eq!(server.hellos.load(Ordering::Relaxed), 1);
}
#[test]
fn udp_retries_lost_packet_with_fresh_handshake() {
    let server = MockLight::start(true, false, false);
    assert!(server.lamp().status().is_ok());
    assert_eq!(server.hellos.load(Ordering::Relaxed), 2);
}
#[test]
fn unresponsive_device_has_one_retry_budget_not_two_protocol_attempts() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let halt = stop.clone();
    let worker = thread::spawn(move || {
        let mut hellos = 0;
        let mut buffer = [0; 8192];
        while !halt.load(Ordering::Relaxed) {
            if let Ok((n, _)) = socket.recv_from(&mut buffer) {
                assert_eq!(&buffer[..n], &HELLO);
                hellos += 1;
            }
        }
        hellos
    });
    let device = Device {
        ip: "192.168.1.10".into(),
        name: "silent simulator".into(),
        did: Some(1234),
        token: "12".repeat(16),
        model: "yeelink.light.lamp22".into(),
        capabilities: None,
    };
    let session = Session::at(&device, address, Duration::from_millis(100)).unwrap();
    let result = Lamp::new(session, 1234, device.caps()).status();
    stop.store(true, Ordering::Relaxed);
    let hellos = worker.join().unwrap();
    assert!(matches!(result, Err(qilight::Error::Connection(_))));
    assert_eq!(hellos, 2, "A silent device must not trigger MIoT discovery");
}
#[test]
fn miot_firmware_supports_individual_and_combined_adjustments() {
    let server = MockLight::start(false, false, true);
    let mut lamp = server.lamp();
    let state = lamp
        .perform(&Action::Adjust(Adjustment {
            brightness: Some(1),
            temperature: Some(6500),
            transition_ms: 0,
        }))
        .unwrap();
    assert_eq!((state.brightness, state.temperature), (Some(1), Some(6500)));
    let state = lamp
        .perform(&Action::Adjust(Adjustment {
            brightness: None,
            temperature: Some(2700),
            transition_ms: 0,
        }))
        .unwrap();
    assert_eq!(state.brightness, Some(1));
    assert_eq!(state.temperature, Some(2700));
}

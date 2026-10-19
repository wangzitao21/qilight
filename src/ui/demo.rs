use super::*;

pub(super) fn demo_devices() -> Vec<Device> {
    ["书桌 · 主屏", "书桌 · 副屏", "阅读角"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| Device {
            ip: format!("192.168.6.{}", 100 + i),
            name: name.into(),
            did: Some(100 + i as u32),
            token: "12".repeat(16),
            model: "yeelink.light.lamp22".into(),
            capabilities: None,
        })
        .collect()
}

use super::*;

impl App {
    pub(super) fn persist(&mut self, devices: Vec<Device>) -> bool {
        if self.load_error {
            self.status = "请先备份并重建设备配置。".into();
            return false;
        }
        let result = config::validate(devices).and_then(|devices| {
            if !self.demo {
                config::save(&self.path, &devices)?;
            }
            Ok(devices)
        });
        match result {
            Ok(devices) => {
                self.engine.retain_connections(&devices);
                self.states.retain(|ip, _| {
                    self.devices
                        .iter()
                        .find(|d| &d.ip == ip)
                        .is_some_and(|old| devices.iter().any(|new| old.same_connection(new)))
                });
                self.errors
                    .retain(|ip, _| devices.iter().any(|d| &d.ip == ip));
                self.devices = devices;
                self.selected
                    .retain(|ip| self.devices.iter().any(|d| &d.ip == ip));
                self.queued
                    .retain(|ip, _| self.devices.iter().any(|d| &d.ip == ip));
                self.next_live_send
                    .retain(|ip, _| self.devices.iter().any(|d| &d.ip == ip));
                if self.selected.is_empty() {
                    if let Some(device) = self.devices.first().cloned() {
                        self.select_one(&device);
                    }
                }
                true
            }
            Err(e) => {
                self.status = e.to_string();
                false
            }
        }
    }
    pub(super) fn targets(&self) -> Vec<Device> {
        self.devices
            .iter()
            .filter(|d| self.all_scope || self.selected.contains(&d.ip))
            .cloned()
            .collect()
    }
    pub(super) fn select_one(&mut self, device: &Device) {
        self.all_scope = false;
        self.selected.clear();
        self.selected.insert(device.ip.clone());
        if let Some(state) = self.states.get(&device.ip) {
            if let Some(v) = state.brightness {
                self.brightness = v;
            }
            if let Some(v) = state.temperature {
                self.temperature = v;
            }
        }
        for action in [
            self.pending.get(&device.ip).map(|p| &p.action),
            self.queued.get(&device.ip),
        ] {
            if let Some(Action::Adjust(a)) = action {
                self.brightness = a.brightness.unwrap_or(self.brightness);
                self.temperature = a.temperature.unwrap_or(self.temperature);
            }
        }
    }
    pub(super) fn add(&mut self) -> bool {
        match config::parse_ips(&self.ip_input) {
            Ok(ips) => {
                let mut devices = self.devices.clone();
                for (i, ip) in ips.iter().enumerate() {
                    let name = if ips.len() > 1 && !self.name_input.trim().is_empty() {
                        format!("{} {}", self.name_input.trim(), i + 1)
                    } else {
                        self.name_input.clone()
                    };
                    devices.push(Device::pending(ip, &name));
                }
                if self.persist(devices) {
                    self.selected = ips.into_iter().collect();
                    self.ip_input.clear();
                    self.name_input.clear();
                    self.status.clear();
                    true
                } else {
                    false
                }
            }
            Err(e) => {
                self.status = e.to_string();
                false
            }
        }
    }
}

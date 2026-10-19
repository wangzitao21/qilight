use super::*;

const LIVE_INTERVAL: Duration = Duration::from_millis(80);

impl App {
    pub(super) fn busy(&self) -> bool {
        !self.pending.is_empty() || !self.queued.is_empty()
    }

    pub(super) fn power_value(&self, device: &Device) -> bool {
        for action in [
            self.queued.get(&device.ip),
            self.pending.get(&device.ip).map(|p| &p.action),
        ] {
            if let Some(Action::Power(on)) = action {
                return *on;
            }
        }
        self.states.get(&device.ip).is_some_and(|s| s.power)
    }

    pub(super) fn can_adjust(&self, device: &Device) -> bool {
        self.states.get(&device.ip).is_some_and(|s| s.power)
            && self.power_value(device)
            && !matches!(
                self.pending.get(&device.ip).map(|p| &p.action),
                Some(Action::Power(_))
            )
            && !matches!(self.queued.get(&device.ip), Some(Action::Power(_)))
    }

    pub(super) fn run(&mut self, targets: Vec<Device>, action: Action) {
        if let Action::Adjust(settings) = &action {
            // Validate the whole selection before changing any lamp.
            for device in &targets {
                if let Err(error) = settings.validate(&device.caps()) {
                    self.errors.insert(device.ip.clone(), error.to_string());
                    return;
                }
            }
        }
        for device in targets {
            if matches!(action, Action::Refresh)
                && (self.pending.contains_key(&device.ip) || self.queued.contains_key(&device.ip))
            {
                continue;
            }
            if let Action::Adjust(new) = &action {
                if !self.can_adjust(&device) {
                    continue;
                }
                match self.queued.get_mut(&device.ip) {
                    Some(Action::Adjust(previous)) => {
                        previous.brightness = new.brightness.or(previous.brightness);
                        previous.temperature = new.temperature.or(previous.temperature);
                        previous.transition_ms = new.transition_ms;
                        continue;
                    }
                    Some(Action::Power(_)) => continue,
                    _ => {}
                }
            }
            // Power commands supersede unsent slider edits. Each lamp keeps just
            // one latest request, so a slow connection cannot accumulate a backlog.
            self.queued.insert(device.ip.clone(), action.clone());
        }
        self.flush_controls(Instant::now());
    }

    pub(super) fn apply_live(&mut self, brightness: bool, temperature: bool) {
        if brightness || temperature {
            self.run(
                self.targets(),
                Action::Adjust(Adjustment {
                    brightness: brightness.then_some(self.brightness),
                    temperature: temperature.then_some(self.temperature),
                    transition_ms: if self.smooth { 80 } else { 0 },
                }),
            );
        }
    }

    pub(super) fn flush_controls(&mut self, now: Instant) {
        let ready: Vec<_> = self
            .queued
            .keys()
            .filter(|ip| !self.pending.contains_key(*ip))
            .cloned()
            .collect();
        for ip in ready {
            if matches!(self.queued.get(&ip), Some(Action::Adjust(_))) {
                if let Some(until) = self.next_live_send.get(&ip).filter(|until| **until > now) {
                    self.ctx.request_repaint_after(*until - now);
                    continue;
                }
            }
            let action = self.queued.remove(&ip).unwrap();
            let Some(device) = self.devices.iter().find(|d| d.ip == ip).cloned() else {
                continue;
            };
            if let Some(state) = self.states.get(&ip) {
                let unchanged = match &action {
                    Action::Power(_) => false,
                    Action::Adjust(a) => {
                        state.power
                            && a.brightness.is_none_or(|v| state.brightness == Some(v))
                            && a.temperature.is_none_or(|v| state.temperature == Some(v))
                    }
                    Action::Refresh => false,
                };
                if unchanged && !self.errors.contains_key(&ip) {
                    continue;
                }
            }
            self.errors.remove(&ip);
            if self.demo {
                if let Some(state) = self.states.get_mut(&ip) {
                    match action {
                        Action::Refresh => {}
                        Action::Power(on) => state.power = on,
                        Action::Adjust(a) if state.power => {
                            state.brightness = a.brightness.or(state.brightness);
                            state.temperature = a.temperature.or(state.temperature);
                        }
                        Action::Adjust(_) => {}
                    }
                }
            } else {
                self.group += 1;
                if matches!(action, Action::Adjust(_)) {
                    self.next_live_send.insert(ip.clone(), now + LIVE_INTERVAL);
                }
                self.pending.insert(
                    ip,
                    Pending {
                        group: self.group,
                        action: action.clone(),
                    },
                );
                self.engine.submit(self.group, device, action);
            }
        }
    }

    pub(super) fn complete(&mut self, completion: Completion) {
        let result = completion.outcome;
        if self.pending.get(&result.ip).map(|p| p.group) != Some(completion.group) {
            return;
        }
        self.pending.remove(&result.ip);
        if let Some(state) = result.state {
            self.states.insert(result.ip.clone(), state);
        } else {
            self.states.remove(&result.ip);
        }
        if let Some(error) = result.error {
            self.errors.insert(result.ip.clone(), error);
            // Do not turn a failed drag into an automatic retry loop.
            self.queued.remove(&result.ip);
        } else {
            self.errors.remove(&result.ip);
        }
        if !self.queued.contains_key(&result.ip)
            && self.selected.len() == 1
            && self.selected.contains(&result.ip)
            && !self.all_scope
            && !self.ctx.input(|i| i.pointer.any_down())
            && !self.ctx.wants_keyboard_input()
        {
            if let Some(state) = self.states.get(&result.ip) {
                self.brightness = state.brightness.unwrap_or(self.brightness);
                self.temperature = state.temperature.unwrap_or(self.temperature);
            }
        }
    }
}

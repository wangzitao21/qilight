use super::*;
use design::toggle_at;

impl App {
    pub(super) fn adjustment_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("adjustment")
            .exact_width(340.)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin {
                left: 20,
                right: 28,
                top: 24,
                bottom: 24,
            }))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(egui::Stroke::new(1.0_f32, LINE))
                    .corner_radius(16)
                    .inner_margin(22)
                    .show(ui, |ui| {
                        ui.set_min_height(ui.available_height() - 44.);
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("灯光调节").size(18.).strong());
                        ui.add_space(16.);
                        ui.columns(2, |columns| {
                            let width = columns[0].available_width();
                            if columns[0]
                                .add_sized(
                                    [width, 32.],
                                    egui::Button::new("选中灯具")
                                        .selected(!self.all_scope)
                                        .corner_radius(8)
                                        .stroke(egui::Stroke::NONE),
                                )
                                .clicked()
                            {
                                self.all_scope = false;
                            }
                            let width = columns[1].available_width();
                            if columns[1]
                                .add_sized(
                                    [width, 32.],
                                    egui::Button::new("全部灯具")
                                        .selected(self.all_scope)
                                        .corner_radius(8)
                                        .stroke(egui::Stroke::NONE),
                                )
                                .clicked()
                            {
                                self.all_scope = true;
                            }
                        });
                        ui.add_space(12.);
                        let selected = &self.selected;
                        let all = self.all_scope;
                        let targets = self
                            .devices
                            .iter()
                            .filter(|d| all || selected.contains(&d.ip));
                        let ranges = self.ranges.get(targets.clone());
                        let mut names = targets.clone();
                        let first = names.next();
                        let name = if self.all_scope {
                            "全部灯具"
                        } else {
                            match (first, names.next()) {
                                (None, _) => "选择灯具",
                                (Some(device), None) => &device.name,
                                _ => "多盏灯具",
                            }
                        };
                        ui.add(
                            egui::Label::new(RichText::new(name).size(12.).color(MUTED)).truncate(),
                        );
                        let active = targets.clone().any(|d| self.can_adjust(d));
                        ui.add_space(28.);
                        let bright_changed = ui
                            .add_enabled_ui(active && !ranges.brightness.is_empty(), |ui| {
                                value_slider(
                                    ui,
                                    "brightness",
                                    &mut self.brightness,
                                    &ranges.brightness,
                                    "%",
                                )
                            })
                            .inner;
                        ui.add_space(30.);
                        let temp_changed = ui
                            .add_enabled_ui(active && !ranges.temperature.is_empty(), |ui| {
                                value_slider(
                                    ui,
                                    "temperature",
                                    &mut self.temperature,
                                    &ranges.temperature,
                                    " K",
                                )
                            })
                            .inner;
                        self.apply_live(bright_changed, temp_changed);
                        ui.add_space(30.);
                        ui.separator();
                        ui.add_space(10.);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("平滑过渡").size(12.).color(MUTED));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        Vec2::new(36., 20.),
                                        egui::Sense::hover(),
                                    );
                                    toggle_at(ui, rect, "smooth", &mut self.smooth, "平滑过渡");
                                },
                            );
                        });
                    });
            });
    }
}

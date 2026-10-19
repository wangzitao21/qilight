use super::*;
use design::{button, button_at, icon, lamp_art, toggle_at, Icon, Tone};
use egui::{pos2, vec2, Align2, FontId, Rect, Sense, Stroke, StrokeKind};

impl App {
    pub(super) fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(28, 16)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(34., 34.), Sense::hover());
                    ui.painter().rect_filled(rect, 10, ACCENT);
                    icon(ui.painter(), rect.center(), 22., Icon::Lamp, Color32::WHITE);
                    ui.add_space(5.);
                    ui.label(RichText::new("QiLight").size(20.).strong());
                    if self.demo {
                        ui.label(RichText::new("演示").size(11.).color(MUTED));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let editable = !self.busy() && !self.pairing.running && !self.load_error;
                        if ui
                            .add_enabled_ui(editable, |ui| {
                                button(ui, "添加灯具", Some(Icon::Plus), Tone::Primary, 110., 36.)
                            })
                            .inner
                            .clicked()
                        {
                            self.show_add = true;
                            self.add_error = false;
                        }
                        if ui
                            .add_enabled_ui(editable, |ui| {
                                button(ui, "米家配对", Some(Icon::Scan), Tone::Quiet, 110., 36.)
                            })
                            .inner
                            .clicked()
                        {
                            self.open_pairing();
                        }
                    });
                });
            });
    }

    pub(super) fn device_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin {
                left: 28,
                right: 4,
                top: 24,
                bottom: 24,
            }))
            .show(ctx, |ui| {
                let editable = !self.busy() && !self.pairing.running && !self.load_error;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("灯具").size(18.).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled_ui(!self.load_error, |ui| {
                                button(ui, "", Some(Icon::Refresh), Tone::Quiet, 32., 32.)
                            })
                            .inner
                            .on_hover_text("刷新状态")
                            .clicked()
                        {
                            self.run(self.devices.clone(), Action::Refresh);
                        }
                        if self.devices.len() > 1 {
                            let mut all_on = self.devices.iter().all(|d| self.power_value(d));
                            let (rect, _) = ui.allocate_exact_size(vec2(40., 22.), Sense::hover());
                            if toggle_at(ui, rect, "all-power", &mut all_on, "全部灯具开关")
                                .changed()
                            {
                                self.run(self.devices.clone(), Action::Power(all_on));
                            }
                            ui.label(RichText::new("全部").size(12.).color(MUTED));
                            ui.add_space(8.);
                        }
                    });
                });
                ui.add_space(14.);
                if self.load_error {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(255, 237, 237))
                        .corner_radius(12)
                        .inner_margin(16)
                        .show(ui, |ui| {
                            ui.label(RichText::new("设备配置无法读取").strong().color(ERROR));
                            ui.label(&self.status);
                            if ui.button("备份并重建配置").clicked() {
                                match config::backup_and_reset(&self.path) {
                                    Ok(_) => {
                                        self.load_error = false;
                                        self.devices.clear();
                                    }
                                    Err(e) => self.status = e.to_string(),
                                }
                            }
                        });
                } else if self.devices.is_empty() {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 220.), Sense::hover());
                    icon(
                        ui.painter(),
                        rect.center() - vec2(0., 20.),
                        36.,
                        Icon::Lamp,
                        MUTED,
                    );
                    ui.painter().text(
                        rect.center() + vec2(0., 24.),
                        Align2::CENTER_CENTER,
                        "添加灯具开始使用",
                        FontId::proportional(14.),
                        MUTED,
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("devices-scroll")
                        .auto_shrink([false, false])
                        .show_rows(ui, 96., self.devices.len(), |ui, range| {
                            for i in range {
                                let device = self.devices[i].clone();
                                self.card(ui, &device, editable);
                            }
                        });
                }
            });
    }

    fn card(&mut self, ui: &mut egui::Ui, device: &Device, editable: bool) {
        ui.push_id(&device.ip, |ui| {
            let selected = self.all_scope || self.selected.contains(&device.ip);
            let state = self.states.get(&device.ip).cloned();
            let error = self.errors.get(&device.ip).cloned();
            let on = self.power_value(device);
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), 96.), Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    selected,
                    &device.name,
                )
            });
            let p = ui.painter().clone();
            p.rect(
                rect,
                14,
                if selected {
                    Color32::from_rgb(249, 248, 255)
                } else {
                    Color32::WHITE
                },
                Stroke::new(
                    1.0_f32,
                    if selected {
                        Color32::from_rgb(181, 170, 246)
                    } else {
                        LINE
                    },
                ),
                StrokeKind::Inside,
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let tile = Rect::from_min_size(rect.min + vec2(14., 18.), vec2(66., 60.));
            p.rect_filled(
                tile,
                10,
                if on {
                    Color32::from_rgb(249, 244, 234)
                } else {
                    BG
                },
            );
            lamp_art(
                &p,
                tile.shrink(3.),
                on,
                state.as_ref().and_then(|s| s.temperature).unwrap_or(4000),
                state.as_ref().and_then(|s| s.brightness).unwrap_or(50),
                false,
            );
            let text_left = rect.left() + 94.;
            let text_right = rect.right() - 110.;
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(
                        pos2(text_left, rect.top() + 16.),
                        pos2(text_right, rect.bottom() - 12.),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 4.;
                    ui.add(
                        egui::Label::new(RichText::new(&device.name).size(16.).strong()).truncate(),
                    );
                    ui.label(RichText::new(&device.ip).size(11.).color(MUTED));
                    let detail = if let Some(error) = &error {
                        error.clone()
                    } else if !device.paired() {
                        "待配对".into()
                    } else if let Some(state) = &state {
                        if state.power {
                            match (state.brightness, state.temperature) {
                                (Some(b), Some(t)) => format!("{b}%  ·  {t} K"),
                                (Some(b), None) => format!("{b}%"),
                                _ => String::new(),
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        "未连接".into()
                    };
                    if !detail.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&detail).size(11.).color(if error.is_some() {
                                    ERROR
                                } else {
                                    MUTED
                                }),
                            )
                            .truncate(),
                        )
                        .on_hover_text(detail);
                    }
                },
            );
            let mut power = on;
            let power_response = ui
                .add_enabled_ui(device.paired(), |ui| {
                    toggle_at(
                        ui,
                        Rect::from_center_size(
                            pos2(rect.right() - 36., rect.center().y),
                            vec2(40., 22.),
                        ),
                        "power",
                        &mut power,
                        if on { "关灯" } else { "开灯" },
                    )
                })
                .inner;
            if power_response.changed() {
                self.run(vec![device.clone()], Action::Power(power));
            }
            let edit_response = ui
                .add_enabled_ui(editable, |ui| {
                    button_at(
                        ui,
                        Rect::from_center_size(
                            pos2(rect.right() - 84., rect.center().y),
                            vec2(28., 30.),
                        ),
                        "edit",
                        "",
                        Some(Icon::Edit),
                        Tone::Quiet,
                    )
                    .on_hover_text("编辑灯具")
                })
                .inner;
            if edit_response.clicked() {
                self.edit = Some(Edit {
                    original_ip: device.ip.clone(),
                    ip: device.ip.clone(),
                    name: device.name.clone(),
                    error: None,
                });
            }
            if self.pending.contains_key(&device.ip) {
                ui.put(
                    Rect::from_center_size(
                        pos2(rect.right() - 36., rect.bottom() - 16.),
                        vec2(10., 10.),
                    ),
                    egui::Spinner::new().size(10.).color(MUTED),
                );
            }
            if response.clicked() && !power_response.changed() && !edit_response.clicked() {
                if ui.input(|i| i.modifiers.command || i.modifiers.ctrl) {
                    self.all_scope = false;
                    if !self.selected.remove(&device.ip) {
                        self.selected.insert(device.ip.clone());
                    }
                } else {
                    self.select_one(device);
                }
            }
        });
    }
}

use super::*;
use design::{button, icon, Icon, Tone};

fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(23.).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = button(ui, "", Some(Icon::Close), Tone::Quiet, 30., 30.)
                .on_hover_text("关闭")
                .clicked();
        });
    });
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).size(13.).color(MUTED));
    }
    ui.add_space(16.);
    close
}

fn input(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(RichText::new(label).size(13.));
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY)
            .margin(12.),
    );
}

impl App {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        if self.show_add {
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("add-device")).show(ctx, |ui| {
                ui.set_width(380.);
                close = heading(ui, "添加灯具", "连接你局域网中的屏幕灯");
                input(ui, "局域网 IP", &mut self.ip_input, "例如 192.168.6.100");
                ui.label(
                    RichText::new("添加多盏灯时，用逗号或空格分隔 IP。")
                        .size(12.)
                        .color(MUTED),
                );
                ui.add_space(8.);
                input(ui, "灯具名称", &mut self.name_input, "可选，例如：书桌主屏");
                ui.add_space(12.);
                ui.label(
                    RichText::new("添加后，使用「米家配对」连接。")
                        .size(12.)
                        .color(MUTED),
                );
                if self.add_error {
                    ui.label(RichText::new(&self.status).size(12.).color(ERROR));
                }
                ui.add_space(12.);
                if ui
                    .add_enabled_ui(
                        !self.busy() && !self.pairing.running && !self.load_error,
                        |ui| {
                            button(
                                ui,
                                "添加",
                                Some(Icon::Plus),
                                Tone::Primary,
                                ui.available_width(),
                                40.,
                            )
                        },
                    )
                    .inner
                    .clicked()
                {
                    close = self.add();
                    self.add_error = !close;
                }
            });
            if close || modal.should_close() {
                self.show_add = false;
            }
        }
        if self.pairing.open {
            let mut close = false;
            let mut regenerate = false;
            let modal=egui::Modal::new(egui::Id::new("mijia-pairing")).show(ctx,|ui| {
                ui.set_width(400.);
                close=heading(ui,"米家扫码连接",&format!("为 {} 盏灯更新配对信息",self.pairing.targets.len()));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("米家地区").size(13.));
                    let previous=self.pairing.region;
                    egui::ComboBox::from_id_salt("mijia-region").selected_text(REGIONS[self.pairing.region].0).width(150.).show_ui(ui,|ui| {
                        for (i,(name,_)) in REGIONS.iter().enumerate() {ui.selectable_value(&mut self.pairing.region,i,*name);}
                    });
                    if previous!=self.pairing.region {regenerate=true;}
                });
                ui.add_space(12.);
                egui::Frame::new().fill(BG).corner_radius(14).inner_margin(18).show(ui,|ui| {
                    ui.set_width(364.);
                    ui.vertical_centered(|ui| {
                        if let Some(texture)=&self.pairing.texture {
                            egui::Frame::new().fill(Color32::WHITE).corner_radius(12).inner_margin(10).show(ui,|ui| {
                                ui.add(egui::Image::new(texture).fit_to_exact_size([200.,200.].into()));
                            });
                        } else {
                            ui.add_space(36.);
                            if self.pairing.running {ui.add(egui::Spinner::new().size(32.).color(ACCENT));}
                            else {let (rect,_)=ui.allocate_exact_size(Vec2::splat(44.),egui::Sense::hover());icon(ui.painter(),rect.center(),36.,Icon::Scan,ACCENT);}
                            ui.add_space(36.);
                        }
                        if let Some(expires)=self.pairing.expires {
                            let seconds=expires.saturating_duration_since(Instant::now()).as_secs();
                            ui.add_space(6.);
                            ui.label(RichText::new(format!("二维码剩余 {seconds} 秒")).size(12.).color(MUTED));
                            ctx.request_repaint_after(Duration::from_secs(1));
                        }
                    });
                });
                ui.add_space(8.);
                ui.label(RichText::new(&self.pairing.status).size(13.));
                ui.label(RichText::new("米家 → 右上角 ＋ → 扫一扫，在手机上确认登录。\n所选地区需与米家一致，关闭窗口会保留原配对。").size(12.).color(MUTED));
                ui.add_space(12.);
                if ui.add_enabled_ui(!self.pairing.targets.is_empty() && (!self.pairing.running || self.pairing.texture.is_some()),|ui|button(ui,"重新生成二维码",Some(Icon::Refresh),Tone::Primary,ui.available_width(),40.)).inner.clicked() {regenerate=true;}
            });
            if close || modal.should_close() {
                self.pairing.open = false;
                self.pairing.invalidate();
            } else if regenerate {
                self.start_pairing();
            }
        }
        if let Some(mut edit) = self.edit.take() {
            let mut close = false;
            let mut save = false;
            let mut remove = false;
            let modal = egui::Modal::new(egui::Id::new("edit-device")).show(ctx, |ui| {
                ui.set_width(380.);
                close = heading(ui, "编辑灯具", "");
                input(ui, "灯具名称", &mut edit.name, "例如：书桌主屏");
                ui.add_space(8.);
                input(ui, "局域网 IP", &mut edit.ip, "例如 192.168.6.100");
                if let Some(error) = &edit.error {
                    ui.label(RichText::new(error).size(12.).color(ERROR));
                }
                ui.add_space(16.);
                save = ui
                    .add_enabled_ui(!self.busy() && !self.pairing.running, |ui| {
                        button(
                            ui,
                            "保存修改",
                            Some(Icon::Check),
                            Tone::Primary,
                            ui.available_width(),
                            40.,
                        )
                    })
                    .inner
                    .clicked();
                remove = ui
                    .add_enabled_ui(!self.busy() && !self.pairing.running, |ui| {
                        button(ui, "移除灯具", None, Tone::Quiet, ui.available_width(), 32.)
                    })
                    .inner
                    .clicked();
            });
            if save {
                let mut devices = self.devices.clone();
                let Some(index) = devices.iter().position(|d| d.ip == edit.original_ip) else {
                    self.status = "此灯已从列表移除，请重新选择灯具。".into();
                    return;
                };
                let old = devices[index].ip.clone();
                devices[index].ip = edit.ip.clone();
                devices[index].name = edit.name.clone();
                if self.persist(devices) {
                    if old != edit.ip {
                        self.states.remove(&old);
                        self.errors.remove(&old);
                    }
                } else {
                    edit.error = Some(self.status.clone());
                    self.edit = Some(edit);
                }
            } else if remove {
                self.confirm_remove = Some(vec![edit.original_ip]);
            } else if !close && !modal.should_close() {
                self.edit = Some(edit);
            }
        }
        if let Some(targets) = self.confirm_remove.clone() {
            let mut close = false;
            let mut remove = false;
            let modal = egui::Modal::new(egui::Id::new("remove-devices")).show(ctx, |ui| {
                ui.set_width(380.);
                close = heading(
                    ui,
                    "移除灯具",
                    &format!("将从本机列表移除 {} 盏灯具", targets.len()),
                );
                ui.label(
                    RichText::new("此操作不会移除米家中的设备。\n之后可以重新添加 IP 并扫码连接。")
                        .color(MUTED),
                );
                ui.add_space(18.);
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 8.) / 2.;
                    if button(ui, "取消", None, Tone::Secondary, width, 38.).clicked() {
                        close = true;
                    }
                    remove = button(ui, "确认移除", None, Tone::Primary, width, 38.).clicked();
                });
            });
            if remove {
                let devices = self
                    .devices
                    .iter()
                    .filter(|d| !targets.contains(&d.ip))
                    .cloned()
                    .collect();
                if self.persist(devices) {
                    self.confirm_remove = None;
                }
            }
            if close || modal.should_close() {
                self.confirm_remove = None;
            }
        }
    }
}

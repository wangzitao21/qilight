use super::*;

impl App {
    pub(super) fn open_pairing(&mut self) {
        if self.demo {
            self.pairing.open = true;
            self.pairing.status = "演示模式不连接米家。".into();
            return;
        }
        self.pairing.invalidate();
        self.pairing.open = true;
        let selected: Vec<_> = self
            .devices
            .iter()
            .filter(|d| self.selected.contains(&d.ip))
            .cloned()
            .collect();
        self.pairing.targets = if selected.is_empty() {
            self.devices.clone()
        } else {
            selected
        };
        if self.pairing.targets.is_empty() {
            self.pairing.status = "请先添加灯具 IP；配置损坏时可在主界面备份并重建。".into();
        } else {
            self.start_pairing();
        }
    }
    pub(super) fn start_pairing(&mut self) {
        self.pairing.invalidate();
        if self.pairing.targets.is_empty() {
            return;
        }
        let generation = self.pairing.generation;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.pairing.cancel = Some(cancelled.clone());
        self.pairing.running = true;
        self.pairing.status = "正在生成小米登录二维码…".into();
        let targets = self.pairing.targets.clone();
        let region = REGIONS[self.pairing.region].1;
        let tx = self.pair_tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let send = |event| {
                let _ = tx.send((generation, event));
                ctx.request_repaint();
            };
            let result = (|| {
                let mut setup = CloudSetup::new(region, cancelled.clone())?;
                let qr = setup.create_qr()?;
                send(PairEvent::Qr(qr.bytes, qr.expires_in));
                setup.await_login()?;
                setup.find_lamps(&targets, |text| send(PairEvent::Status(text.into())))
            })();
            if !cancelled.load(Ordering::Relaxed) {
                match result {
                    Ok(result) => send(PairEvent::Done(result)),
                    Err(error) => send(PairEvent::Error(error.to_string())),
                }
            }
        });
    }
    pub(super) fn poll_pairing(&mut self) {
        while let Ok((generation, event)) = self.pair_rx.try_recv() {
            if generation != self.pairing.generation || !self.pairing.open {
                continue;
            }
            match event {
                PairEvent::Qr(bytes, seconds) => match image::load_from_memory(&bytes) {
                    Ok(image) => {
                        let image = image.to_rgba8();
                        let color = egui::ColorImage::from_rgba_unmultiplied(
                            [image.width() as usize, image.height() as usize],
                            image.as_raw(),
                        );
                        self.pairing.texture = Some(self.ctx.load_texture(
                            "mijia-qr",
                            color,
                            egui::TextureOptions::NEAREST,
                        ));
                        self.pairing.expires = Some(Instant::now() + Duration::from_secs(seconds));
                        self.pairing.status = "请用米家扫一扫，并在手机上确认登录。".into();
                    }
                    Err(_) => {
                        self.pairing.invalidate();
                        self.pairing.status = "二维码图片无法显示，请重新生成。".into();
                    }
                },
                PairEvent::Status(text) => {
                    self.pairing.status = text;
                    self.pairing.texture = None;
                    self.pairing.expires = None;
                }
                PairEvent::Error(text) => {
                    self.pairing.running = false;
                    self.pairing.texture = None;
                    self.pairing.status = text;
                }
                PairEvent::Done(result) => {
                    let count = result.updates.len();
                    let failed = result.errors.len();
                    let by_ip: HashMap<_, _> = result
                        .updates
                        .iter()
                        .map(|d| (d.ip.clone(), d.clone()))
                        .collect();
                    let devices = self
                        .devices
                        .iter()
                        .map(|d| by_ip.get(&d.ip).cloned().unwrap_or_else(|| d.clone()))
                        .collect();
                    self.pairing.running = false;
                    self.pairing.texture = None;
                    if count > 0 && !self.persist(devices) {
                        self.pairing.status = format!(
                            "配对成功但保存失败：{}。请检查目录权限后重新扫码。",
                            self.status
                        );
                        continue;
                    }
                    for d in &result.updates {
                        self.errors.remove(&d.ip);
                        self.states.remove(&d.ip);
                    }
                    self.errors.extend(result.errors);
                    self.pairing.status = format!(
                        "{} 盏配对成功，{} 盏未完成。{}",
                        count,
                        failed,
                        if failed > 0 {
                            "可关闭窗口查看每盏灯的提示，或重新扫码。"
                        } else {
                            "已保存，可以关闭窗口。"
                        }
                    );
                    if count > 0 {
                        self.run(result.updates, Action::Refresh);
                    }
                }
            }
        }
    }
}

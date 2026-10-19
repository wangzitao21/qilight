use eframe::egui::{self, Color32, RichText, Vec2};
use qilight::{
    cloud::{CloudSetup, PairingResult, REGIONS},
    config::{self, Device},
    control::{Action, Adjustment, Completion, Engine, LightState},
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

mod adjustment;
mod controller;
mod demo;
mod design;
mod devices;
mod dialogs;
mod pairing;
mod panels;
mod ranges;
#[cfg(test)]
mod tests;
mod widgets;

use design::{setup_style, ACCENT, BG, ERROR, LINE, MUTED, SOFT};
use ranges::RangeCache;
use widgets::value_slider;
pub const TITLE: &str = concat!("QiLight ", env!("CARGO_PKG_VERSION"));

enum PairEvent {
    Qr(Vec<u8>, u64),
    Status(String),
    Done(PairingResult),
    Error(String),
}
#[derive(Default)]
struct Pairing {
    open: bool,
    region: usize,
    generation: u64,
    targets: Vec<Device>,
    cancel: Option<Arc<AtomicBool>>,
    texture: Option<egui::TextureHandle>,
    expires: Option<Instant>,
    running: bool,
    status: String,
}
impl Pairing {
    fn invalidate(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.generation += 1;
        self.running = false;
        self.texture = None;
        self.expires = None;
    }
}
struct Edit {
    original_ip: String,
    ip: String,
    name: String,
    error: Option<String>,
}
struct Pending {
    group: u64,
    action: Action,
}

pub struct App {
    ctx: egui::Context,
    path: PathBuf,
    devices: Vec<Device>,
    selected: HashSet<String>,
    states: HashMap<String, LightState>,
    errors: HashMap<String, String>,
    engine: Engine,
    group: u64,
    pending: HashMap<String, Pending>,
    queued: HashMap<String, Action>,
    next_live_send: HashMap<String, Instant>,
    status: String,
    load_error: bool,
    ip_input: String,
    name_input: String,
    show_add: bool,
    add_error: bool,
    all_scope: bool,
    ranges: RangeCache,
    brightness: u32,
    temperature: u32,
    smooth: bool,
    edit: Option<Edit>,
    confirm_remove: Option<Vec<String>>,
    pairing: Pairing,
    pair_tx: crossbeam_channel::Sender<(u64, PairEvent)>,
    pair_rx: crossbeam_channel::Receiver<(u64, PairEvent)>,
    demo: bool,
    started: Instant,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    smoke: bool,
    closing: Option<Instant>,
}
impl App {
    pub fn new(
        context: &egui::Context,
        path: PathBuf,
        demo: bool,
        smoke: bool,
        screenshot: Option<PathBuf>,
        initial: Action,
    ) -> Self {
        setup_style(context);
        let ctx = context.clone();
        let wake_ctx = ctx.clone();
        let engine = Engine::new(Arc::new(move || wake_ctx.request_repaint()));
        let (pair_tx, pair_rx) = crossbeam_channel::unbounded();
        let loaded = if demo {
            Ok(demo::demo_devices())
        } else {
            config::load(&path)
        };
        let (devices, load_error, status) = match loaded {
            Ok(d) => (d, false, String::new()),
            Err(e) => (Vec::new(), true, e.to_string()),
        };
        let selected = devices.first().map(|d| d.ip.clone()).into_iter().collect();
        let mut app = Self {
            ctx,
            path,
            devices,
            selected,
            states: HashMap::new(),
            errors: HashMap::new(),
            engine,
            group: 0,
            pending: HashMap::new(),
            queued: HashMap::new(),
            next_live_send: HashMap::new(),
            status,
            load_error,
            ip_input: String::new(),
            name_input: String::new(),
            show_add: false,
            add_error: false,
            all_scope: false,
            ranges: RangeCache::default(),
            brightness: 70,
            temperature: 4000,
            smooth: true,
            edit: None,
            confirm_remove: None,
            pairing: Pairing::default(),
            pair_tx,
            pair_rx,
            demo,
            started: Instant::now(),
            screenshot,
            screenshot_requested: false,
            smoke,
            closing: None,
        };
        if demo {
            for (i, d) in app.devices.iter().enumerate() {
                app.states.insert(
                    d.ip.clone(),
                    LightState {
                        power: i != 2,
                        brightness: Some([70, 45, 80][i]),
                        temperature: Some([4000, 3200, 5000][i]),
                    },
                );
            }
        } else if !smoke && !app.load_error {
            app.run(app.devices.clone(), initial);
        }
        app
    }

    fn poll(&mut self) {
        while let Ok(completion) = self.engine.results.try_recv() {
            self.complete(completion);
        }
        self.flush_controls(Instant::now());
        self.poll_pairing();
    }

    fn draw(&mut self, ctx: &egui::Context) {
        self.poll();
        self.header(ctx);
        self.adjustment_panel(ctx);
        self.device_panel(ctx);
        self.dialogs(ctx);
        self.finish_before_close(ctx);
        self.capture_preview(ctx);
    }

    fn finish_before_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            self.closing.get_or_insert_with(Instant::now);
        }
        if let Some(started) = self.closing {
            let writes_pending = self
                .pending
                .values()
                .any(|p| !matches!(p.action, Action::Refresh))
                || self.queued.values().any(|a| !matches!(a, Action::Refresh));
            if writes_pending && started.elapsed() < Duration::from_secs(5) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.request_repaint_after(Duration::from_millis(40));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn capture_preview(&mut self, ctx: &egui::Context) {
        if self.screenshot.is_some()
            && !self.screenshot_requested
            && self.started.elapsed() > Duration::from_millis(500)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.screenshot_requested = true;
        }
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                if let Some(path) = &self.screenshot {
                    let pixels: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    if image::save_buffer(
                        path,
                        &pixels,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    )
                    .is_ok()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
        if self.smoke || self.screenshot.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
            if self.started.elapsed() > Duration::from_secs(8) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
    fn persist_egui_memory(&self) -> bool {
        false
    }
    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(5)
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.pairing.invalidate();
    }
}

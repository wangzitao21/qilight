use eframe::egui::{
    self, pos2, vec2, Color32, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui,
};
use std::sync::{Arc, OnceLock};

pub const INK: Color32 = Color32::from_rgb(36, 39, 52);
pub const MUTED: Color32 = Color32::from_rgb(126, 132, 150);
pub const BG: Color32 = Color32::from_rgb(246, 247, 251);
pub const LINE: Color32 = Color32::from_rgb(231, 234, 242);
pub const ACCENT: Color32 = Color32::from_rgb(107, 91, 226);
pub const SOFT: Color32 = Color32::from_rgb(240, 237, 255);
pub const ERROR: Color32 = Color32::from_rgb(193, 76, 81);

#[derive(Clone, Copy)]
pub enum Icon {
    Plus,
    Scan,
    Refresh,
    Sun,
    Temperature,
    Edit,
    Close,
    Check,
    Lamp,
}
#[derive(Clone, Copy)]
pub enum Tone {
    Primary,
    Secondary,
    Quiet,
}

pub fn icon(p: &egui::Painter, center: egui::Pos2, size: f32, kind: Icon, color: Color32) {
    let s = size / 24.0;
    let pt = |x: f32, y: f32| center + vec2(x - 12.0, y - 12.0) * s;
    let stroke = Stroke::new(1.7 * s, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([pt(a.0, a.1), pt(b.0, b.1)], stroke);
    };
    match kind {
        Icon::Plus => {
            line((5., 12.), (19., 12.));
            line((12., 5.), (12., 19.));
        }
        Icon::Close => {
            line((6., 6.), (18., 18.));
            line((6., 18.), (18., 6.));
        }
        Icon::Check => {
            line((5., 12.), (10., 17.));
            line((10., 17.), (19., 7.));
        }
        Icon::Scan => {
            for (x, y, dx, dy) in [
                (4., 4., 1., 1.),
                (20., 4., -1., 1.),
                (4., 20., 1., -1.),
                (20., 20., -1., -1.),
            ] {
                line((x, y), (x + 5. * dx, y));
                line((x, y), (x, y + 5. * dy));
            }
            line((8., 12.), (16., 12.));
        }
        Icon::Sun => {
            p.circle_stroke(center, 4.0 * s, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([center + d * 7. * s, center + d * 10. * s], stroke);
            }
        }
        Icon::Temperature => {
            p.rect_stroke(
                Rect::from_min_max(pt(9., 3.), pt(15., 17.)),
                3.0 * s,
                stroke,
                StrokeKind::Inside,
            );
            p.circle_filled(pt(12., 18.), 4. * s, color);
            line((12., 8.), (12., 18.));
        }
        Icon::Refresh => {
            let (start, end) = (-0.7, 4.65);
            let points = (0..25)
                .map(|i| {
                    let a = start + (end - start) * i as f32 / 24.;
                    center + vec2(a.cos(), a.sin()) * 8. * s
                })
                .collect();
            p.add(egui::Shape::line(points, stroke));
            line((20., 3.), (20., 8.));
            line((15., 8.), (20., 8.));
        }
        Icon::Edit => {
            p.add(egui::Shape::closed_line(
                vec![
                    pt(5., 15.),
                    pt(16., 4.),
                    pt(20., 8.),
                    pt(9., 19.),
                    pt(4., 20.),
                ],
                stroke,
            ));
            line((13., 7.), (17., 11.));
        }
        Icon::Lamp => {
            p.rect_filled(Rect::from_min_max(pt(3., 4.), pt(21., 7.)), 1.5 * s, color);
            line((7., 10.), (5., 17.));
            line((12., 10.), (12., 18.));
            line((17., 10.), (19., 17.));
            line((4., 21.), (20., 21.));
        }
    }
}

pub fn button(
    ui: &mut Ui,
    label: &str,
    symbol: Option<Icon>,
    tone: Tone,
    width: f32,
    height: f32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    paint_button(ui, rect, &response, label, symbol, tone);
    response
}

pub fn button_at(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash,
    label: &str,
    symbol: Option<Icon>,
    tone: Tone,
) -> Response {
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    paint_button(ui, rect, &response, label, symbol, tone);
    response
}

fn paint_button(
    ui: &Ui,
    rect: Rect,
    response: &Response,
    label: &str,
    symbol: Option<Icon>,
    tone: Tone,
) {
    let accessible_label = if label.is_empty() {
        match symbol {
            Some(Icon::Edit) => "编辑灯具",
            Some(Icon::Close) => "关闭",
            _ => "操作",
        }
    } else {
        label
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), accessible_label)
    });
    let hot = response.hovered() || response.has_focus();
    let (fill, text, border) = match tone {
        Tone::Primary => (
            if hot {
                Color32::from_rgb(90, 74, 207)
            } else {
                ACCENT
            },
            Color32::WHITE,
            Color32::TRANSPARENT,
        ),
        Tone::Secondary => (if hot { BG } else { Color32::WHITE }, INK, LINE),
        Tone::Quiet => (
            if hot { SOFT } else { Color32::TRANSPARENT },
            if hot { ACCENT } else { MUTED },
            Color32::TRANSPARENT,
        ),
    };
    ui.painter().rect(
        rect,
        9,
        fill,
        Stroke::new(1.0_f32, border),
        StrokeKind::Inside,
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.),
            11,
            Stroke::new(1.0_f32, ACCENT),
            StrokeKind::Outside,
        );
    }
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), FontId::proportional(13.), text);
    let icon_width = if symbol.is_some() {
        if label.is_empty() {
            18.
        } else {
            26.
        }
    } else {
        0.
    };
    let x = rect.center().x - (galley.size().x + icon_width) / 2.;
    if let Some(symbol) = symbol {
        icon(
            ui.painter(),
            pos2(x + 9., rect.center().y),
            18.,
            symbol,
            text,
        );
    }
    if !label.is_empty() {
        ui.painter().galley(
            pos2(x + icon_width, rect.center().y - galley.size().y / 2.),
            galley,
            text,
        );
    }
    if response.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

pub fn toggle_at(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash,
    value: &mut bool,
    label: &str,
) -> Response {
    let mut response = ui.interact(rect, ui.id().with(id), Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label)
    });
    let t = ui.ctx().animate_bool_with_time(response.id, *value, 0.16);
    let fill = mix(Color32::from_rgb(217, 221, 232), ACCENT, t);
    ui.painter().rect_filled(rect, rect.height() / 2., fill);
    let r = rect.height() / 2. - 3.;
    let center = pos2(
        egui::lerp((rect.left() + r + 3.)..=(rect.right() - r - 3.), t),
        rect.center().y,
    );
    ui.painter()
        .circle_filled(center + vec2(0., 1.), r, Color32::from_black_alpha(12));
    ui.painter().circle_filled(center, r, Color32::WHITE);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(3.),
            14,
            Stroke::new(1.0_f32, ACCENT),
            StrokeKind::Outside,
        );
    }
    response.on_hover_text(label)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0., 1.);
    Color32::from_rgba_premultiplied(
        egui::lerp(a.r() as f32..=b.r() as f32, t) as u8,
        egui::lerp(a.g() as f32..=b.g() as f32, t) as u8,
        egui::lerp(a.b() as f32..=b.b() as f32, t) as u8,
        egui::lerp(a.a() as f32..=b.a() as f32, t) as u8,
    )
}

pub fn light_color(temperature: u32) -> Color32 {
    let t = (temperature as f32 - 2700.) / 3800.;
    if t < 0.5 {
        mix(
            Color32::from_rgb(255, 184, 88),
            Color32::from_rgb(255, 237, 196),
            t * 2.,
        )
    } else {
        mix(
            Color32::from_rgb(255, 237, 196),
            Color32::from_rgb(172, 207, 255),
            (t - 0.5) * 2.,
        )
    }
}

// Small vector illustrations stay sharp at every Windows display scale.
pub fn lamp_art(
    p: &egui::Painter,
    rect: Rect,
    on: bool,
    temperature: u32,
    brightness: u32,
    dark: bool,
) {
    let scale = (rect.width() / 200.).min(rect.height() / 120.);
    let origin = rect.center() - vec2(100., 60.) * scale;
    let pt = |x, y| origin + vec2(x, y) * scale;
    let r = |x, y, w, h| Rect::from_min_size(pt(x, y), vec2(w, h) * scale);
    let screen = if dark {
        Color32::from_rgb(49, 53, 72)
    } else {
        Color32::from_rgb(83, 88, 110)
    };
    let body = if dark {
        Color32::from_rgb(104, 109, 132)
    } else {
        Color32::from_rgb(104, 111, 135)
    };
    p.rect_filled(r(93., 85., 14., 17.), 2. * scale, body);
    p.rect_filled(r(74., 101., 52., 4.), 2. * scale, body);
    p.rect(
        r(28., 26., 144., 64.),
        6. * scale,
        screen,
        Stroke::new(scale, body),
        StrokeKind::Inside,
    );
    gradient(
        p,
        r(32., 30., 136., 54.),
        4. * scale,
        if dark {
            Color32::from_rgb(48, 50, 70)
        } else {
            Color32::from_rgb(204, 212, 234)
        },
        if dark {
            Color32::from_rgb(71, 62, 91)
        } else {
            Color32::from_rgb(235, 223, 234)
        },
    );
    if on {
        let light = light_color(temperature);
        let alpha = (16. + brightness as f32 * 0.48) as u8;
        let mut mesh = egui::Mesh::default();
        for (pos, a) in [
            (pt(45., 23.), alpha),
            (pt(155., 23.), alpha),
            (pt(191., 110.), 0),
            (pt(9., 110.), 0),
        ] {
            mesh.colored_vertex(
                pos,
                Color32::from_rgba_unmultiplied(light.r(), light.g(), light.b(), a),
            );
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        p.add(mesh);
        for i in (1..=5).rev() {
            p.rect_filled(
                r(
                    42. - i as f32,
                    20. - i as f32,
                    116. + 2. * i as f32,
                    4. + 2. * i as f32,
                ),
                8. * scale,
                Color32::from_rgba_unmultiplied(light.r(), light.g(), light.b(), 6),
            );
        }
        p.rect_filled(r(43., 23., 114., 2.5), scale, light);
    }
    p.rect_filled(
        r(39., 16., 122., 7.),
        3. * scale,
        if dark {
            Color32::from_rgb(173, 175, 186)
        } else {
            Color32::from_rgb(58, 64, 84)
        },
    );
    p.rect_filled(
        r(88., 17., 24., 5.),
        scale,
        if dark {
            Color32::from_rgb(114, 117, 136)
        } else {
            Color32::from_rgb(99, 105, 123)
        },
    );
    p.circle_filled(
        pt(153., 19.5),
        scale,
        if on { light_color(temperature) } else { MUTED },
    );
}

pub fn gradient(p: &egui::Painter, rect: Rect, radius: f32, left: Color32, right: Color32) {
    let r = radius.min(rect.height() / 2.).min(rect.width() / 2.);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), mix(left, right, 0.5));
    for (center, start) in [
        (
            pos2(rect.right() - r, rect.top() + r),
            -std::f32::consts::FRAC_PI_2,
        ),
        (pos2(rect.right() - r, rect.bottom() - r), 0.),
        (
            pos2(rect.left() + r, rect.bottom() - r),
            std::f32::consts::FRAC_PI_2,
        ),
        (pos2(rect.left() + r, rect.top() + r), std::f32::consts::PI),
    ] {
        for i in 0..=8 {
            let a = start + i as f32 * std::f32::consts::FRAC_PI_2 / 8.;
            let pos = center + vec2(a.cos(), a.sin()) * r;
            mesh.colored_vertex(pos, mix(left, right, (pos.x - rect.left()) / rect.width()));
        }
    }
    let n = mesh.vertices.len() as u32 - 1;
    for i in 1..=n {
        mesh.add_triangle(0, i, if i == n { 1 } else { i + 1 });
    }
    p.add(mesh);
}

fn system_fonts() -> &'static [(&'static str, Vec<u8>)] {
    // Keep at most these two system fonts for the process lifetime. Borrowing
    // their bytes avoids epaint's second full copy of every owned font while
    // preserving the exact typefaces, metrics and complete character coverage.
    static FONTS: OnceLock<Vec<(&'static str, Vec<u8>)>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Vec::new();
        for (name, paths) in [
            ("interface", &["C:/Windows/Fonts/segoeui.ttf"][..]),
            (
                "chinese",
                &["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/simhei.ttf"][..],
            ),
        ] {
            for path in paths {
                if let Ok(data) = std::fs::read(path) {
                    fonts.push((name, data));
                    break;
                }
            }
        }
        fonts
    })
}

pub fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Latin digits use Segoe UI; Chinese glyphs use Microsoft's CJK fallback.
    let mut names = Vec::new();
    for (name, data) in system_fonts() {
        fonts
            .font_data
            .insert((*name).into(), Arc::new(egui::FontData::from_static(data)));
        names.push((*name).to_owned());
    }
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .splice(0..0, names);
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(INK);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.window_stroke = Stroke::new(1.0_f32, LINE);
    style.visuals.window_corner_radius = 18.into();
    style.visuals.window_shadow = egui::Shadow {
        offset: [0, 12],
        blur: 40,
        spread: 0,
        color: Color32::from_black_alpha(28),
    };
    style.visuals.selection.bg_fill = SOFT;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.extreme_bg_color = BG;
    style.visuals.faint_bg_color = BG;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    for v in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        v.corner_radius = 8.into();
        v.bg_stroke = Stroke::new(1.0_f32, LINE);
        v.fg_stroke = Stroke::new(1.5_f32, INK);
        v.expansion = 0.;
    }
    style.visuals.widgets.inactive.bg_fill = Color32::WHITE;
    style.visuals.widgets.inactive.weak_bg_fill = BG;
    style.visuals.widgets.hovered.bg_fill = SOFT;
    style.visuals.widgets.hovered.weak_bg_fill = SOFT;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.active.bg_fill = SOFT;
    style.visuals.widgets.active.weak_bg_fill = SOFT;
    style.spacing.item_spacing = vec2(8., 8.);
    style.spacing.button_padding = vec2(12., 8.);
    style.spacing.interact_size = vec2(36., 30.);
    style.spacing.window_margin = 24.into();
    style.spacing.scroll = egui::style::ScrollStyle::thin();
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(13.));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(22.));
    ctx.set_style(style);
}

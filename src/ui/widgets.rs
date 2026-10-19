use super::*;

pub(super) fn value_slider(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut u32,
    values: &[u32],
    suffix: &str,
) -> bool {
    use design::{gradient, icon, Icon};
    use egui::{pos2, vec2, Rect, Stroke};
    let temperature = id == "temperature";
    let width = ui.available_width();
    let mut changed = false;
    ui.push_id(id, |ui| {
        ui.spacing_mut().item_spacing.y = 4.;
        let nearest = |value: u32| match values.binary_search(&value) {
            Ok(index) => index,
            Err(0) => 0,
            Err(index) if index == values.len() => index.saturating_sub(1),
            Err(index) => {
                if value.abs_diff(values[index - 1]) <= value.abs_diff(values[index]) {
                    index - 1
                } else {
                    index
                }
            }
        };
        let mut index = nearest(*value);
        if !values.is_empty() {
            *value = values[index];
        }
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(30., 36.), egui::Sense::hover());
            ui.painter().rect_filled(
                Rect::from_center_size(rect.center(), vec2(30., 30.)),
                9,
                if temperature {
                    Color32::from_rgb(255, 245, 229)
                } else {
                    SOFT
                },
            );
            icon(
                ui.painter(),
                rect.center(),
                18.,
                if temperature {
                    Icon::Temperature
                } else {
                    Icon::Sun
                },
                if temperature {
                    Color32::from_rgb(207, 150, 64)
                } else {
                    ACCENT
                },
            );
            ui.label(RichText::new(if temperature { "色温" } else { "亮度" }).size(14.));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if values.is_empty() {
                    ui.label(RichText::new("—").size(32.).color(MUTED));
                } else {
                    ui.scope(|ui| {
                        ui.style_mut().override_font_id = Some(egui::FontId::proportional(32.));
                        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
                        ui.spacing_mut().button_padding = vec2(2., 0.);
                        if ui
                            .add(
                                egui::DragValue::new(value)
                                    .range(values[0]..=values[values.len() - 1])
                                    .speed(1)
                                    .suffix(suffix)
                                    .update_while_editing(false),
                            )
                            .on_hover_text("点击数值可直接输入")
                            .changed()
                        {
                            index = nearest(*value);
                            *value = values[index];
                            changed = true;
                        }
                    });
                }
            });
        });
        ui.add_space(if ui.ctx().screen_rect().height() < 740. {
            2.
        } else {
            6.
        });
        // Retain egui's pointer, keyboard and focus handling; paint a consistent rail and thumb.
        let response = ui
            .scope(|ui| {
                ui.set_opacity(0.);
                ui.spacing_mut().slider_width = width;
                ui.spacing_mut().interact_size.y = 26.;
                ui.add(
                    egui::Slider::new(&mut index, 0..=values.len().saturating_sub(1))
                        .show_value(false),
                )
            })
            .inner;
        if response.changed() && !values.is_empty() {
            *value = values[index];
            changed = true;
        }
        let rail = Rect::from_center_size(response.rect.center(), vec2(response.rect.width(), 10.));
        let radius = response.rect.height() / 2.5;
        let fraction = if values.len() > 1 {
            index as f32 / (values.len() - 1) as f32
        } else {
            0.
        };
        let x = egui::lerp(
            (response.rect.left() + radius)..=(response.rect.right() - radius),
            fraction,
        );
        if temperature {
            gradient(
                ui.painter(),
                rail,
                5.,
                design::light_color(2700),
                design::light_color(6500),
            );
        } else {
            ui.painter().rect_filled(rail, 5, SOFT);
            gradient(
                ui.painter(),
                Rect::from_min_max(rail.min, pos2(x + radius / 2., rail.bottom())),
                5.,
                Color32::from_rgb(175, 161, 249),
                ACCENT,
            );
        }
        let center = pos2(x, response.rect.center().y);
        if response.dragged() || response.has_focus() {
            ui.painter().circle_filled(
                center,
                radius + 4.,
                Color32::from_rgba_unmultiplied(107, 91, 226, 24),
            );
        }
        ui.painter().circle_filled(
            center + vec2(0., 1.5),
            radius + 1.,
            Color32::from_black_alpha(15),
        );
        ui.painter()
            .circle(center, radius, Color32::WHITE, Stroke::new(1.0_f32, LINE));
        ui.painter().circle_filled(
            center,
            3.,
            if temperature {
                design::light_color(*value)
            } else {
                ACCENT
            },
        );
        let low = values
            .first()
            .map(|v| format!("{v}{suffix}"))
            .unwrap_or_else(|| "—".into());
        let high = values
            .last()
            .map(|v| format!("{v}{suffix}"))
            .unwrap_or_else(|| "—".into());
        let (labels, _) = ui.allocate_exact_size(vec2(width, 18.), egui::Sense::hover());
        ui.painter().text(
            labels.left_top(),
            egui::Align2::LEFT_TOP,
            if temperature {
                format!("暖光  {low}")
            } else {
                low
            },
            egui::FontId::proportional(11.),
            MUTED,
        );
        ui.painter().text(
            labels.right_top(),
            egui::Align2::RIGHT_TOP,
            if temperature {
                format!("{high}  冷白")
            } else {
                high
            },
            egui::FontId::proportional(11.),
            MUTED,
        );
    });
    changed
}

use super::*;
use demo::demo_devices;
use qilight::spec::Property;

const SIZE: Vec2 = Vec2::new(1080., 700.);

fn app(ctx: &egui::Context) -> App {
    App::new(
        ctx,
        PathBuf::from("unused-demo-config"),
        true,
        false,
        None,
        Action::Refresh,
    )
}

fn frame(
    app: &mut App,
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let modifiers = events
        .iter()
        .find_map(|event| match event {
            egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            modifiers,
            focused: true,
            ..Default::default()
        },
        |ctx| app.draw(ctx),
    )
}

fn shapes(output: &egui::FullOutput) -> Vec<(&egui::Shape, egui::Rect)> {
    fn visit<'a>(
        shape: &'a egui::Shape,
        clip: egui::Rect,
        out: &mut Vec<(&'a egui::Shape, egui::Rect)>,
    ) {
        if let egui::Shape::Vec(children) = shape {
            for child in children {
                visit(child, clip, out);
            }
        } else {
            out.push((shape, clip));
        }
    }
    let mut out = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, &mut out);
    }
    out
}

fn text_rects(output: &egui::FullOutput, label: &str) -> Vec<(egui::Rect, egui::Rect)> {
    shapes(output)
        .into_iter()
        .filter_map(|(shape, clip)| match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => Some((
                egui::Rect::from_min_size(text.pos, text.galley.size()),
                clip,
            )),
            _ => None,
        })
        .collect()
}

fn pointer(
    app: &mut App,
    ctx: &egui::Context,
    pos: egui::Pos2,
    pressed: bool,
    modifiers: egui::Modifiers,
) {
    frame(
        app,
        ctx,
        SIZE,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
            },
        ],
    );
}

fn click_at(app: &mut App, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        pointer(app, ctx, pos, pressed, egui::Modifiers::NONE);
    }
}

fn click(app: &mut App, ctx: &egui::Context, label: &str, occurrence: usize) {
    let output = frame(app, ctx, SIZE, Vec::new());
    let pos = text_rects(&output, label)[occurrence].0.center();
    click_at(app, ctx, pos);
}

fn row(app: &mut App, ctx: &egui::Context, index: usize) -> egui::Rect {
    frame(app, ctx, SIZE, Vec::new());
    let output = frame(app, ctx, SIZE, Vec::new());
    let name = text_rects(&output, &app.devices[index].name)
        .into_iter()
        .min_by(|a, b| a.0.left().total_cmp(&b.0.left()))
        .unwrap()
        .0;
    shapes(&output)
        .into_iter()
        .find_map(|(shape, _)| match shape {
            egui::Shape::Rect(r)
                if (r.rect.height() - 96.).abs() < 0.1 && r.rect.contains(name.center()) =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .unwrap()
}

fn slider(app: &mut App, ctx: &egui::Context, temperature: bool) -> egui::Rect {
    let output = frame(app, ctx, SIZE, Vec::new());
    let (low, high) = if temperature {
        ("暖光  2700 K", "6500 K  冷白")
    } else {
        ("1%", "100%")
    };
    let left = text_rects(&output, low)[0].0;
    let right = text_rects(&output, high)[0].0;
    let thumb = shapes(&output)
        .into_iter()
        .find_map(|(shape, _)| match shape {
            egui::Shape::Circle(c)
                if c.radius > 9.
                    && c.radius < 12.
                    && c.fill == Color32::WHITE
                    && c.center.x > SIZE.x - 340.
                    && c.center.y < left.top()
                    && left.top() - c.center.y < 30. =>
            {
                Some(c)
            }
            _ => None,
        })
        .unwrap();
    egui::Rect::from_min_max(
        egui::pos2(left.left() + thumb.radius, thumb.center.y - 10.),
        egui::pos2(right.right() - thumb.radius, thumb.center.y + 10.),
    )
}

fn mark_pending(app: &mut App, group: u64, indexes: &[usize]) {
    app.group = app.group.max(group);
    for index in indexes {
        app.pending.insert(
            app.devices[*index].ip.clone(),
            Pending {
                group,
                action: Action::Refresh,
            },
        );
    }
}

fn completion(app: &App, group: u64, index: usize, brightness: u32) -> Completion {
    Completion {
        group,
        outcome: qilight::control::Outcome {
            ip: app.devices[index].ip.clone(),
            state: Some(LightState {
                power: true,
                brightness: Some(brightness),
                temperature: Some(4000),
            }),
            error: None,
        },
    }
}

#[test]
fn whole_row_selects_without_a_checkbox_and_power_is_independent() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let second = row(&mut app, &ctx, 1);
    // Click whitespace, away from the name, artwork and action controls.
    click_at(&mut app, &ctx, second.left_bottom() + Vec2::new(240., -12.));
    assert_eq!(app.selected, HashSet::from([app.devices[1].ip.clone()]));
    assert_eq!((app.brightness, app.temperature), (45, 3200));
    let first = row(&mut app, &ctx, 0);
    click_at(
        &mut app,
        &ctx,
        egui::pos2(first.right() - 36., first.center().y),
    );
    assert!(!app.states[&app.devices[0].ip].power);
    assert_eq!(app.selected, HashSet::from([app.devices[1].ip.clone()]));
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    for pressed in [true, false] {
        pointer(&mut app, &ctx, first.center(), pressed, modifiers);
    }
    assert_eq!(app.selected.len(), 2);
}

#[test]
fn dragging_applies_before_release_and_only_changes_the_dragged_property() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    frame(&mut app, &ctx, SIZE, Vec::new());
    let bright = slider(&mut app, &ctx, false);
    let start = egui::pos2(egui::lerp(bright.x_range(), 0.3), bright.center().y);
    pointer(&mut app, &ctx, start, true, egui::Modifiers::NONE);
    let first_value = app.states[&app.devices[0].ip].brightness.unwrap();
    assert!(
        (28..=33).contains(&first_value),
        "first value: {first_value}"
    );
    assert_eq!(app.states[&app.devices[0].ip].temperature, Some(4000));
    let end = egui::pos2(egui::lerp(bright.x_range(), 0.85), bright.center().y);
    frame(&mut app, &ctx, SIZE, vec![egui::Event::PointerMoved(end)]);
    assert!(app.states[&app.devices[0].ip].brightness.unwrap() >= 83);
    pointer(&mut app, &ctx, end, false, egui::Modifiers::NONE);
    let final_brightness = app.states[&app.devices[0].ip].brightness;
    let temp = slider(&mut app, &ctx, true);
    let pos = egui::pos2(egui::lerp(temp.x_range(), 0.8), temp.center().y);
    pointer(&mut app, &ctx, pos, true, egui::Modifiers::NONE);
    assert!(app.states[&app.devices[0].ip].temperature.unwrap() > 5600);
    assert_eq!(app.states[&app.devices[0].ip].brightness, final_brightness);
    pointer(&mut app, &ctx, pos, false, egui::Modifiers::NONE);
    assert_eq!(app.states[&app.devices[1].ip].brightness, Some(45));
}

#[test]
fn slow_lamp_coalesces_edits_without_delaying_other_lamps() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    mark_pending(&mut app, 1, &[0]);
    for value in 1..=99 {
        app.brightness = value;
        app.apply_live(true, false);
    }
    app.temperature = 5100;
    app.apply_live(false, true);
    assert_eq!(app.queued.len(), 1);
    assert_eq!(app.states[&app.devices[0].ip].brightness, Some(70));
    let second = app.devices[1].clone();
    app.select_one(&second);
    app.brightness = 62;
    app.apply_live(true, false);
    assert_eq!(app.states[&second.ip].brightness, Some(62));
    app.complete(completion(&app, 1, 0, 71));
    app.flush_controls(Instant::now());
    assert_eq!(app.states[&app.devices[0].ip].brightness, Some(99));
    assert_eq!(app.states[&app.devices[0].ip].temperature, Some(5100));
    assert!(app.queued.is_empty() && !app.busy());
    assert_eq!(
        app.brightness, 62,
        "other lamp's reply must not move this slider"
    );
}

#[test]
fn final_drag_value_flushes_after_throttle_without_more_input() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let now = Instant::now();
    app.next_live_send
        .insert(app.devices[0].ip.clone(), now + Duration::from_secs(1));
    for value in [12, 15, 39, 84] {
        app.brightness = value;
        app.apply_live(true, false);
    }
    assert_eq!(app.states[&app.devices[0].ip].brightness, Some(70));
    app.flush_controls(now + Duration::from_millis(999));
    assert_eq!(app.queued.len(), 1);
    app.flush_controls(now + Duration::from_secs(1));
    assert_eq!(app.states[&app.devices[0].ip].brightness, Some(84));
    assert!(app.queued.is_empty());
}

#[test]
fn power_supersedes_queued_edits_and_off_lamps_are_not_adjusted() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    mark_pending(&mut app, 1, &[0]);
    app.brightness = 31;
    app.apply_live(true, false);
    app.run(vec![app.devices[0].clone()], Action::Power(false));
    assert!(!app.power_value(&app.devices[0]));
    app.brightness = 42;
    app.apply_live(true, false);
    assert!(matches!(
        app.queued[&app.devices[0].ip],
        Action::Power(false)
    ));
    app.complete(completion(&app, 1, 0, 70));
    app.flush_controls(Instant::now());
    assert!(!app.states[&app.devices[0].ip].power);
    assert_eq!(app.states[&app.devices[0].ip].brightness, Some(70));
}

#[test]
fn sliders_wait_for_power_changes_to_finish() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    mark_pending(&mut app, 1, &[0]);
    app.pending.get_mut(&app.devices[0].ip).unwrap().action = Action::Power(false);
    app.run(vec![app.devices[0].clone()], Action::Power(true));
    assert!(!app.can_adjust(&app.devices[0]));
    assert!(app.can_adjust(&app.devices[1]));
    app.complete(completion(&app, 1, 0, 70));
    app.flush_controls(Instant::now());
    assert!(app.can_adjust(&app.devices[0]));
}

#[test]
fn failed_drag_discards_old_queue_and_other_lamps_finish() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    mark_pending(&mut app, 1, &[0, 1]);
    app.all_scope = true;
    app.brightness = 35;
    app.apply_live(true, false);
    let mut failed = completion(&app, 1, 0, 70);
    failed.outcome.state = None;
    failed.outcome.error = Some("连接超时".into());
    app.complete(failed);
    app.complete(completion(&app, 1, 1, 45));
    app.flush_controls(Instant::now());
    assert!(app.errors.contains_key(&app.devices[0].ip));
    assert_eq!(app.states[&app.devices[1].ip].brightness, Some(35));
    assert_eq!(app.states[&app.devices[2].ip].brightness, Some(80));
    assert!(!app.states[&app.devices[2].ip].power && !app.busy());
}

#[test]
fn stale_replies_cannot_release_a_newer_request_or_override_readback() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    mark_pending(&mut app, 1, &[0]);
    mark_pending(&mut app, 2, &[1]);
    app.complete(completion(&app, 2, 1, 60));
    mark_pending(&mut app, 3, &[1]);
    app.complete(completion(&app, 2, 1, 5));
    assert_eq!(app.states[&app.devices[1].ip].brightness, Some(60));
    assert_eq!(app.pending[&app.devices[1].ip].group, 3);
    app.complete(completion(&app, 1, 0, 80));
    app.complete(completion(&app, 3, 1, 90));
    app.complete(completion(&app, 3, 1, 5));
    assert_eq!(app.states[&app.devices[1].ip].brightness, Some(90));
    assert!(!app.busy());
}

#[test]
fn minimum_window_fits_controls_and_has_no_removed_ui() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = Vec2::new(900., 620.);
    frame(&mut app, &ctx, size, Vec::new());
    let output = frame(&mut app, &ctx, size, Vec::new());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    for label in [
        "米家配对",
        "亮度",
        "色温",
        "平滑过渡",
        "暖光  2700 K",
        "6500 K  冷白",
    ] {
        let rects = text_rects(&output, label);
        assert_eq!(rects.len(), 1, "{label}");
        assert!(
            screen.contains_rect(rects[0].0) && rects[0].1.contains_rect(rects[0].0),
            "{label}: {:?}",
            rects[0]
        );
    }
    for (shape, _) in shapes(&output) {
        if let egui::Shape::Text(text) = shape {
            for removed in [
                "点击应用",
                "仅应用",
                "已完成",
                "选中开灯",
                "选中关灯",
                "你的桌面",
                "CONTROL",
                "拖动预览",
            ] {
                assert!(!text.galley.job.text.contains(removed));
            }
        }
    }
}

#[test]
fn closing_waits_for_the_last_slider_edit() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    mark_pending(&mut app, 1, &[0]);
    app.brightness = 27;
    app.apply_live(true, false);
    app.closing = Some(Instant::now());
    let output = frame(&mut app, &ctx, SIZE, Vec::new());
    assert!(output.viewport_output[&egui::ViewportId::ROOT]
        .commands
        .iter()
        .any(|command| matches!(command, egui::ViewportCommand::CancelClose)));
    app.complete(completion(&app, 1, 0, 70));
    let output = frame(&mut app, &ctx, SIZE, Vec::new());
    assert_eq!(app.states[&app.devices[0].ip].brightness, Some(27));
    assert!(output.viewport_output[&egui::ViewportId::ROOT]
        .commands
        .iter()
        .any(|command| matches!(command, egui::ViewportCommand::Close)));
}

#[test]
fn add_dialog_validates_input_and_blocks_background_switches() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let first = row(&mut app, &ctx, 0);
    click(&mut app, &ctx, "添加灯具", 0);
    click(&mut app, &ctx, "添加", 0);
    assert!(app.show_add && app.add_error);
    assert_eq!(app.devices.len(), 3);
    click_at(
        &mut app,
        &ctx,
        egui::pos2(first.right() - 36., first.center().y),
    );
    assert!(app.states[&app.devices[0].ip].power);
    if !app.show_add {
        click(&mut app, &ctx, "添加灯具", 0);
    }
    app.ip_input = "192.168.6.110, 192.168.6.111".into();
    app.name_input = "工作区".into();
    click(&mut app, &ctx, "添加", 0);
    assert!(!app.show_add && !app.add_error);
    assert_eq!(app.devices.len(), 5);
}

#[test]
fn editing_and_removal_target_the_clicked_lamp() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let second = row(&mut app, &ctx, 1);
    click_at(
        &mut app,
        &ctx,
        egui::pos2(second.right() - 84., second.center().y),
    );
    assert_eq!(app.edit.as_ref().unwrap().original_ip, app.devices[1].ip);
    app.edit.as_mut().unwrap().name = "副屏新名称".into();
    click(&mut app, &ctx, "保存修改", 0);
    assert_eq!(app.devices[1].name, "副屏新名称");
    let second = row(&mut app, &ctx, 1);
    click_at(
        &mut app,
        &ctx,
        egui::pos2(second.right() - 84., second.center().y),
    );
    click(&mut app, &ctx, "移除灯具", 0);
    click(&mut app, &ctx, "确认移除", 0);
    assert_eq!(app.devices.len(), 2);
    assert_eq!(app.devices[0].ip, "192.168.6.100");
    assert_eq!(app.devices[1].ip, "192.168.6.102");
}

#[test]
fn renaming_keeps_readback_while_changing_address_invalidates_it() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let original = app.states.clone();
    let mut devices = app.devices.clone();
    devices[0].name = "新的灯具名称".into();
    assert!(app.persist(devices));
    assert_eq!(app.states, original);
    let old_ip = app.devices[0].ip.clone();
    let mut devices = app.devices.clone();
    devices[0].ip = "192.168.6.199".into();
    assert!(app.persist(devices));
    assert!(!app.states.contains_key(&old_ip));
    assert!(!app.states.contains_key("192.168.6.199"));
    assert_eq!(app.states.len(), original.len() - 1);
}
#[test]
fn pairing_dialog_fits_minimum_window_and_escape_cancels_request() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.pairing.open = true;
    app.pairing.targets = app.devices.clone();
    app.pairing.status = "请用米家扫一扫，并在手机上确认登录。".into();
    app.pairing.running = true;
    let cancel = Arc::new(AtomicBool::new(false));
    app.pairing.cancel = Some(cancel.clone());
    app.pairing.texture = Some(ctx.load_texture(
        "test-qr",
        egui::ColorImage::new([240, 240], Color32::WHITE),
        egui::TextureOptions::NEAREST,
    ));
    app.pairing.expires = Some(Instant::now() + Duration::from_secs(60));
    let size = Vec2::new(900., 620.);
    frame(&mut app, &ctx, size, Vec::new());
    let output = frame(&mut app, &ctx, size, Vec::new());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    for label in ["米家扫码连接", "重新生成二维码"] {
        let rects = text_rects(&output, label);
        assert_eq!(rects.len(), 1);
        assert!(
            screen.contains_rect(rects[0].0) && rects[0].1.contains_rect(rects[0].0),
            "{label} is clipped"
        );
    }
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!app.pairing.open);
    assert!(cancel.load(Ordering::Relaxed));
    assert_eq!(app.devices[0].token, "12".repeat(16));
}
#[test]
fn stale_pairing_completion_cannot_replace_current_credentials() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.pairing.open = true;
    app.pairing.generation = 9;
    let mut old = app.devices[0].clone();
    old.token = "34".repeat(16);
    app.pair_tx
        .send((
            8,
            PairEvent::Done(PairingResult {
                updates: vec![old],
                errors: HashMap::new(),
            }),
        ))
        .unwrap();
    app.poll();
    assert_eq!(app.devices[0].token, "12".repeat(16));
}
#[test]
fn bulk_range_is_intersection_not_union() {
    let mut devices = demo_devices();
    devices[1].model = "yeelink.light.test".into();
    devices[1].capabilities = Some(qilight::spec::Capabilities {
        temperature: Some(Property {
            siid: 2,
            piid: 3,
            range: Some([3000, 5000, 100]),
        }),
        ..Default::default()
    });
    let values = RangeCache::default().get(devices.iter());
    assert_eq!(values.temperature.first(), Some(&3000));
    assert_eq!(values.temperature.last(), Some(&5000));
    assert_eq!(values.temperature.len(), 21);
    assert!(values.brightness.is_empty());
}

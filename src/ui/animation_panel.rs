use super::*;

const MIN_TIMELINE_TRACK_HEIGHT: f32 = 28.0;
const MAX_TIMELINE_TRACK_HEIGHT: f32 = 220.0;
const MIN_TIMELINE_TIME_SCALE: f32 = 0.0001;
const MAX_TIMELINE_TIME_SCALE: f32 = 10_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TimelineScrollZoom {
    None,
    Time,
    Vertical,
    Both,
}

pub(super) fn timeline_scroll_zoom(modifiers: egui::Modifiers) -> TimelineScrollZoom {
    let time = modifiers.command || modifiers.ctrl;
    match (time, modifiers.shift) {
        (true, true) => TimelineScrollZoom::Both,
        (true, false) => TimelineScrollZoom::Time,
        (false, true) => TimelineScrollZoom::Vertical,
        (false, false) => TimelineScrollZoom::None,
    }
}

pub(super) fn dominant_timeline_scroll_axis(delta: egui::Vec2) -> Option<TimelineScrollAxis> {
    if delta == egui::Vec2::ZERO {
        None
    } else if delta.x.abs() > delta.y.abs() {
        Some(TimelineScrollAxis::Horizontal)
    } else {
        Some(TimelineScrollAxis::Vertical)
    }
}

pub(super) fn timeline_time_bounds(
    runtime: &AnimationRuntime,
    start_frame: u32,
    end_frame: u32,
) -> (f32, f32) {
    let full_start = start_frame as f32;
    let full_end = end_frame.max(start_frame.saturating_add(1)) as f32;
    let full_span = full_end - full_start;
    let visible_span = full_span
        / runtime
            .timeline_time_scale
            .clamp(MIN_TIMELINE_TIME_SCALE, MAX_TIMELINE_TIME_SCALE);
    let center = runtime
        .timeline_time_center
        .unwrap_or((full_start + full_end) * 0.5);
    let view_start = center - visible_span * 0.5;
    (view_start, view_start + visible_span)
}

pub(super) fn apply_timeline_zoom(
    runtime: &mut AnimationRuntime,
    start_frame: u32,
    end_frame: u32,
    pointer_fraction: f32,
    scroll_amount: f32,
    axes: TimelineScrollZoom,
) {
    let factor = (scroll_amount * 0.01).exp();
    if matches!(axes, TimelineScrollZoom::Time | TimelineScrollZoom::Both) {
        let (old_start, old_end) = timeline_time_bounds(runtime, start_frame, end_frame);
        let fraction = pointer_fraction.clamp(0.0, 1.0);
        let anchor = old_start + (old_end - old_start) * fraction;
        runtime.timeline_time_scale = (runtime.timeline_time_scale * factor)
            .clamp(MIN_TIMELINE_TIME_SCALE, MAX_TIMELINE_TIME_SCALE);
        let full_start = start_frame as f32;
        let full_end = end_frame.max(start_frame.saturating_add(1)) as f32;
        let visible_span =
            (full_end - full_start) / runtime.timeline_time_scale.max(MIN_TIMELINE_TIME_SCALE);
        let view_start = anchor - visible_span * fraction;
        runtime.timeline_time_center = Some(view_start + visible_span * 0.5);
    }
    if matches!(
        axes,
        TimelineScrollZoom::Vertical | TimelineScrollZoom::Both
    ) {
        runtime.timeline_track_height = (runtime.timeline_track_height * factor)
            .clamp(MIN_TIMELINE_TRACK_HEIGHT, MAX_TIMELINE_TRACK_HEIGHT);
    }
}

pub(super) fn pan_timeline_time(
    runtime: &mut AnimationRuntime,
    start_frame: u32,
    end_frame: u32,
    horizontal_scroll: f32,
    chart_width: f32,
) {
    let (view_start, view_end) = timeline_time_bounds(runtime, start_frame, end_frame);
    let visible_span = view_end - view_start;
    let center = (view_start + view_end) * 0.5;
    runtime.timeline_time_center =
        Some(center - horizontal_scroll / chart_width.max(1.0) * visible_span);
}

pub(super) fn displayed_keyframe_frame(
    keyframe: SelectedKeyframe,
    drag: Option<&KeyframeDrag>,
) -> u32 {
    drag.map_or(keyframe.frame, |drag| {
        if drag.keyframes.contains(&keyframe) {
            (keyframe.frame as i64 + drag.preview_delta as i64).max(0) as u32
        } else {
            keyframe.frame
        }
    })
}

pub(super) fn animation_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            animation::migrate_legacy_lattice_tracks(tree);
            let mut data = animation::animation_data(tree);
            let mut data_changed = false;
            let mut requested_frame = None;
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(if runtime.playing { "Pause" } else { "Play" })
                    .clicked()
                {
                    runtime.toggle_playback();
                }
                if ui.button("Stop").clicked() {
                    runtime.stop(tree);
                }
                ui.separator();
                ui.label("Frame");
                let mut frame = runtime.current_frame;
                if ui
                    .add(
                        egui::DragValue::new(&mut frame)
                            .speed(1.0)
                            .range(data.start_frame as f32..=data.end_frame as f32),
                    )
                    .changed()
                {
                    requested_frame = Some(frame);
                }
                ui.label("Start");
                data_changed |= ui
                    .add(egui::DragValue::new(&mut data.start_frame).range(0..=100_000))
                    .changed();
                ui.label("End");
                data_changed |= ui
                    .add(egui::DragValue::new(&mut data.end_frame).range(1..=100_000))
                    .changed();
                ui.label("FPS");
                data_changed |= ui
                    .add(
                        egui::DragValue::new(&mut data.fps)
                            .speed(1.0)
                            .range(1.0..=240.0),
                    )
                    .changed();
                ui.checkbox(&mut runtime.looping, "Loop");
            });
            if data.end_frame <= data.start_frame {
                data.end_frame = data.start_frame.saturating_add(1);
                data_changed = true;
            }
            if data_changed {
                animation::set_animation_data(tree, data.clone());
                runtime.current_frame = runtime
                    .current_frame
                    .clamp(data.start_frame as f32, data.end_frame as f32);
                tree.make_undo_redo_snapshot();
            }
            if let Some(frame) = requested_frame {
                runtime.set_frame(tree, frame);
            }

            ui.separator();
            let selection = selected(tree);
            let objects = objects(tree);
            let tracks: Vec<_> = data
                .tracks
                .iter()
                .filter(|track| selection.is_empty() || selection.contains(&track.binding.object))
                .cloned()
                .collect();
            if tracks.is_empty() {
                ui.label("No keyframes for the current selection.");
                ui.weak("Hover an editable property and press I to insert one.");
                return;
            }

            let timeline_hovered = ui.rect_contains_pointer(ui.max_rect());
            let delete_pressed = timeline_hovered
                && ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
                        || input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                });
            if delete_pressed && !runtime.selected_keyframes.is_empty() {
                tree.make_undo_redo_snapshot();
                animation::delete_keyframes(tree, &runtime.selected_keyframes);
                runtime.selected_keyframes.clear();
                runtime.keyframe_drag = None;
                runtime.set_frame(tree, runtime.current_frame);
                tree.make_undo_redo_snapshot();
                return;
            }
            let start_keyboard_grab = timeline_hovered
                && !runtime.selected_keyframes.is_empty()
                && runtime.keyframe_drag.is_none()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::G));

            let label_width = 155.0_f32.min(ui.available_width() * 0.42);
            let timeline_area = ui.available_rect_before_wrap();
            let chart_left = timeline_area.left() + label_width + ui.spacing().item_spacing.x;
            let chart_width = (timeline_area.right() - chart_left).max(1.0);
            let (zoom_request, horizontal_pan) = ui.input_mut(|input| {
                if !input.is_scrolling() {
                    runtime.timeline_scroll_axis = None;
                }
                let modifier_axes = timeline_scroll_zoom(input.modifiers);
                let modifier_wheel = input.raw.events.iter().rev().find_map(|event| {
                    let egui::Event::MouseWheel {
                        delta, modifiers, ..
                    } = event
                    else {
                        return None;
                    };
                    let axes = timeline_scroll_zoom(*modifiers);
                    (axes != TimelineScrollZoom::None).then_some((axes, *delta))
                });
                let gesture_zoom = input.zoom_delta_2d();
                let gesture_changed =
                    (gesture_zoom.x - 1.0).abs() > 0.0001 || (gesture_zoom.y - 1.0).abs() > 0.0001;
                let scroll_delta = input.smooth_scroll_delta;
                let request = if let Some((axes, delta)) = modifier_wheel {
                    input.smooth_scroll_delta = egui::Vec2::ZERO;
                    let amount = if delta.y.abs() >= delta.x.abs() {
                        delta.y
                    } else {
                        delta.x
                    };
                    // Wheel deltas describe content motion, so invert them for
                    // the expected view-scale direction.
                    Some((axes, -amount))
                } else if gesture_changed {
                    // egui turns Ctrl/Cmd-wheel into a zoom gesture before widgets see
                    // the scroll delta. Native trackpad pinches arrive through this path too.
                    let axes = if modifier_axes == TimelineScrollZoom::None {
                        TimelineScrollZoom::Time
                    } else {
                        modifier_axes
                    };
                    let factor = match axes {
                        TimelineScrollZoom::Time => gesture_zoom.x,
                        TimelineScrollZoom::Vertical => gesture_zoom.y,
                        TimelineScrollZoom::Both => (gesture_zoom.x * gesture_zoom.y).sqrt(),
                        TimelineScrollZoom::None => 1.0,
                    }
                    .max(0.001);
                    Some((axes, factor.ln() / 0.01))
                } else if modifier_axes != TimelineScrollZoom::None
                    && scroll_delta != egui::Vec2::ZERO
                {
                    input.smooth_scroll_delta = egui::Vec2::ZERO;
                    let amount = if scroll_delta.y.abs() >= scroll_delta.x.abs() {
                        scroll_delta.y
                    } else {
                        scroll_delta.x
                    };
                    Some((modifier_axes, -amount))
                } else {
                    None
                };
                if timeline_hovered {
                    let zoom = request.map(|(axes, amount)| {
                        let fraction = input
                            .pointer
                            .hover_pos()
                            .map_or(0.5, |pointer| (pointer.x - chart_left) / chart_width);
                        (axes, amount, fraction)
                    });
                    let pan = if zoom.is_none() && modifier_axes == TimelineScrollZoom::None {
                        if runtime.timeline_scroll_axis.is_none() {
                            runtime.timeline_scroll_axis =
                                dominant_timeline_scroll_axis(input.smooth_scroll_delta);
                        }
                        match runtime.timeline_scroll_axis {
                            Some(TimelineScrollAxis::Horizontal) => {
                                let horizontal = input.smooth_scroll_delta.x;
                                // Consume both components so diagonal noise cannot also
                                // move the vertical trace list.
                                input.smooth_scroll_delta = egui::Vec2::ZERO;
                                Some(horizontal)
                            }
                            Some(TimelineScrollAxis::Vertical) => {
                                input.smooth_scroll_delta.x = 0.0;
                                None
                            }
                            None => None,
                        }
                    } else {
                        None
                    };
                    (zoom, pan)
                } else {
                    (None, None)
                }
            });
            if let Some((axes, amount, fraction)) = zoom_request {
                apply_timeline_zoom(
                    runtime,
                    data.start_frame,
                    data.end_frame,
                    fraction,
                    amount,
                    axes,
                );
            }
            if let Some(horizontal) = horizontal_pan {
                pan_timeline_time(
                    runtime,
                    data.start_frame,
                    data.end_frame,
                    horizontal,
                    chart_width,
                );
            }
            let (view_start_frame, view_end_frame) =
                timeline_time_bounds(runtime, data.start_frame, data.end_frame);
            let mut visible_keyframes = Vec::new();
            let mut box_start = None;
            egui::ScrollArea::vertical()
                .id_salt("animation-tracks")
                .show(ui, |ui| {
                    for track in &tracks {
                        ui.horizontal(|ui| {
                            let object_name = objects
                                .iter()
                                .find(|object| object.uuid == track.binding.object)
                                .map(SdfObject::display_name)
                                .unwrap_or_else(|| "Missing object".into());
                            let label =
                                format!("{} · {}", object_name, track.binding.property.label());
                            ui.add_sized(
                                egui::vec2(label_width, runtime.timeline_track_height),
                                egui::Label::new(&label).truncate(),
                            )
                            .on_hover_text(label);
                            let width = ui.available_width().max(80.0);
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(width, runtime.timeline_track_height),
                                egui::Sense::click_and_drag(),
                            );
                            response.clone().on_hover_text(
                                "Two-finger horizontal: pan time · Ctrl/Cmd + scroll: time zoom · Shift + scroll: vertical zoom",
                            );
                            let painter = ui.painter_at(rect);
                            painter.rect_filled(rect, 3.0, Color32::from_rgb(34, 35, 39));
                            painter.line_segment(
                                [rect.left_center(), rect.right_center()],
                                Stroke::new(1.0, Color32::from_gray(75)),
                            );
                            let frame_span = view_end_frame - view_start_frame;
                            if start_keyboard_grab && runtime.keyframe_drag.is_none() {
                                if let Some(pointer) =
                                    ui.input(|input| input.pointer.interact_pos())
                                {
                                    runtime.keyframe_drag = Some(KeyframeDrag {
                                        anchor: runtime.selected_keyframes[0],
                                        keyframes: runtime.selected_keyframes.clone(),
                                        preview_delta: 0,
                                        start_pointer_x: pointer.x,
                                        pixels_per_frame: rect.width() / frame_span,
                                        keyboard_initiated: true,
                                    });
                                }
                            }
                            let frame_x = |frame: f32| {
                                rect.left() + (frame - view_start_frame) / frame_span * rect.width()
                            };
                            let shape_count = (track.binding.property == AnimatableProperty::LatticeShape)
                                .then(|| objects.iter().find(|object| object.uuid == track.binding.object)
                                    .and_then(|object| object.lattice.as_ref())
                                    .map_or(0, |lattice| lattice.shape_keys.len()));
                            let (minimum, maximum) = if let Some(count) = shape_count {
                                (-0.1, count.max(1) as f32 + 0.1)
                            } else {
                                timeline_value_bounds(track)
                            };
                            let value_span = maximum - minimum;
                            let value_y = |value: f32| {
                                rect.bottom()
                                    - 5.0
                                    - (value - minimum) / value_span * (rect.height() - 10.0)
                            };
                            let value_at_y = |y: f32| {
                                minimum
                                    + (rect.bottom() - 5.0 - y) / (rect.height() - 10.0)
                                        * value_span
                            };
                            if let Some(count) = shape_count {
                                for index in 0..=count {
                                    let y = value_y(index as f32);
                                    painter.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                                        Stroke::new(0.5, Color32::from_gray(58)));
                                    painter.text(egui::pos2(rect.left() + 3.0, y - 2.0),
                                        egui::Align2::LEFT_BOTTOM, index.to_string(),
                                        egui::FontId::proportional(10.0), Color32::from_gray(145));
                                }
                            }
                            let frame_at_x = |x: f32| {
                                view_start_frame + (x - rect.left()) / rect.width() * frame_span
                            };
                            let mut bezier_handle_active = false;
                            for pair in track.keyframes.windows(2) {
                                let left_source = &pair[0];
                                let right_source = &pair[1];
                                let mut left = left_source.clone();
                                let mut right = right_source.clone();
                                left.frame = displayed_keyframe_frame(
                                    SelectedKeyframe {
                                        binding: track.binding,
                                        frame: left_source.frame,
                                    },
                                    runtime.keyframe_drag.as_ref(),
                                );
                                right.frame = displayed_keyframe_frame(
                                    SelectedKeyframe {
                                        binding: track.binding,
                                        frame: right_source.frame,
                                    },
                                    runtime.keyframe_drag.as_ref(),
                                );
                                let left_point =
                                    egui::pos2(frame_x(left.frame as f32), value_y(left.value));
                                let right_point =
                                    egui::pos2(frame_x(right.frame as f32), value_y(right.value));
                                match left.interpolation {
                                    KeyframeInterpolation::Linear => painter.line_segment(
                                        [left_point, right_point],
                                        Stroke::new(1.5, Color32::from_rgb(112, 161, 255)),
                                    ),
                                    KeyframeInterpolation::Constant => {
                                        let corner = egui::pos2(right_point.x, left_point.y);
                                        painter.line_segment(
                                            [left_point, corner],
                                            Stroke::new(1.5, Color32::from_rgb(112, 161, 255)),
                                        );
                                        painter.line_segment(
                                            [corner, right_point],
                                            Stroke::new(1.5, Color32::from_rgb(112, 161, 255)),
                                        )
                                    }
                                    KeyframeInterpolation::Bezier => {
                                        let controls =
                                            animation::bezier_control_points(&left, &right);
                                        let mut previous = left_point;
                                        for step in 1..=24 {
                                            let point = animation::bezier_point(
                                                controls,
                                                step as f32 / 24.0,
                                            );
                                            let current =
                                                egui::pos2(frame_x(point.0), value_y(point.1));
                                            painter.line_segment(
                                                [previous, current],
                                                Stroke::new(1.5, Color32::from_rgb(112, 161, 255)),
                                            );
                                            previous = current;
                                        }
                                        for (incoming, source, displayed, control) in [
                                            (false, left_source, &left, controls[1]),
                                            (true, right_source, &right, controls[2]),
                                        ] {
                                            let anchor = egui::pos2(
                                                frame_x(displayed.frame as f32),
                                                value_y(displayed.value),
                                            );
                                            let center =
                                                egui::pos2(frame_x(control.0), value_y(control.1));
                                            painter.line_segment(
                                                [anchor, center],
                                                Stroke::new(1.0, Color32::from_gray(135)),
                                            );
                                            let handle_id = egui::Id::new((
                                                "bezier-handle",
                                                track.binding,
                                                source.frame,
                                                incoming,
                                            ));
                                            let captured = ui.ctx().is_being_dragged(handle_id)
                                                || ui.ctx().drag_stopped_id() == Some(handle_id);
                                            if !captured && !rect.expand(6.0).contains(center) {
                                                continue;
                                            }
                                            let handle_response = ui.interact(
                                                egui::Rect::from_center_size(
                                                    center,
                                                    egui::vec2(12.0, 12.0),
                                                ),
                                                handle_id,
                                                egui::Sense::drag(),
                                            );
                                            handle_response
                                                .clone()
                                                .on_hover_text("Drag Bézier control point");
                                            bezier_handle_active |= handle_response.hovered()
                                                || handle_response.dragged();
                                            painter.circle_filled(
                                                center,
                                                if handle_response.hovered() { 4.5 } else { 3.5 },
                                                Color32::from_rgb(239, 184, 255),
                                            );
                                            if handle_response.drag_started() {
                                                tree.make_undo_redo_snapshot();
                                            }
                                            if handle_response.dragged() {
                                                if let Some(pointer) =
                                                    handle_response.interact_pointer_pos()
                                                {
                                                    let control_frame = frame_at_x(pointer.x);
                                                    let control_value =
                                                        value_at_y(pointer.y.clamp(
                                                            rect.top() + 5.0,
                                                            rect.bottom() - 5.0,
                                                        ));
                                                    animation::set_bezier_handle(
                                                        tree,
                                                        SelectedKeyframe {
                                                            binding: track.binding,
                                                            frame: source.frame,
                                                        },
                                                        incoming,
                                                        crate::model::BezierHandle {
                                                            frame_offset: control_frame
                                                                - displayed.frame as f32,
                                                            value_offset: control_value
                                                                - displayed.value,
                                                        },
                                                    );
                                                    runtime.selected_keyframes =
                                                        vec![SelectedKeyframe {
                                                            binding: track.binding,
                                                            frame: source.frame,
                                                        }];
                                                    runtime.set_frame(tree, runtime.current_frame);
                                                }
                                            }
                                            if handle_response.drag_stopped() {
                                                tree.make_undo_redo_snapshot();
                                            }
                                        }
                                        painter.line_segment([left_point, left_point], Stroke::NONE)
                                    }
                                };
                            }
                            let mut keyframe_active = false;
                            for keyframe in &track.keyframes {
                                let keyframe_id = SelectedKeyframe {
                                    binding: track.binding,
                                    frame: keyframe.frame,
                                };
                                let displayed_frame = displayed_keyframe_frame(
                                    keyframe_id,
                                    runtime.keyframe_drag.as_ref(),
                                );
                                let center = egui::pos2(
                                    frame_x(displayed_frame as f32),
                                    value_y(keyframe.value),
                                );
                                let key_id = egui::Id::new((
                                    "timeline-keyframe",
                                    track.binding,
                                    keyframe.frame,
                                ));
                                let captured = ui.ctx().is_being_dragged(key_id)
                                    || ui.ctx().drag_stopped_id() == Some(key_id);
                                if !captured && !rect.expand(7.0).contains(center) {
                                    continue;
                                }
                                visible_keyframes.push((keyframe_id, center));
                                let key_response = ui.interact(
                                    egui::Rect::from_center_size(center, egui::vec2(14.0, 14.0)),
                                    key_id,
                                    egui::Sense::click_and_drag(),
                                );
                                key_response.clone().on_hover_text("Drag to move keyframe");
                                keyframe_active |= key_response.hovered() || key_response.dragged();
                                let selected = runtime.selected_keyframes.contains(&keyframe_id);
                                let radius = if selected { 6.0 } else { 4.5 };
                                painter.add(egui::Shape::convex_polygon(
                                    vec![
                                        center + egui::vec2(0.0, -radius),
                                        center + egui::vec2(radius, 0.0),
                                        center + egui::vec2(0.0, radius),
                                        center + egui::vec2(-radius, 0.0),
                                    ],
                                    if selected {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(112, 161, 255)
                                    },
                                    Stroke::NONE,
                                ));
                                if key_response.clicked() {
                                    key_response.request_focus();
                                    if ui.input(|input| input.modifiers.shift) {
                                        if selected {
                                            runtime
                                                .selected_keyframes
                                                .retain(|selected| *selected != keyframe_id);
                                        } else {
                                            runtime.selected_keyframes.push(keyframe_id);
                                        }
                                    } else {
                                        runtime.selected_keyframes = vec![keyframe_id];
                                    }
                                    runtime.set_frame(tree, keyframe.frame as f32);
                                }
                                if key_response.drag_started() {
                                    if !runtime.selected_keyframes.contains(&keyframe_id) {
                                        runtime.selected_keyframes = vec![keyframe_id];
                                    }
                                    runtime.keyframe_drag = Some(KeyframeDrag {
                                        anchor: keyframe_id,
                                        keyframes: runtime.selected_keyframes.clone(),
                                        preview_delta: 0,
                                        start_pointer_x: ui.input(|input| {
                                            input
                                                .pointer
                                                .press_origin()
                                                .map_or(center.x, |pointer| pointer.x)
                                        }),
                                        pixels_per_frame: rect.width() / frame_span,
                                        keyboard_initiated: false,
                                    });
                                }
                                if key_response.dragged() {
                                    if let Some(pointer) = key_response.interact_pointer_pos() {
                                        if let Some(drag) = &mut runtime.keyframe_drag {
                                            let requested = ((pointer.x - drag.start_pointer_x)
                                                / drag.pixels_per_frame.max(0.001))
                                            .round()
                                                as i32;
                                            drag.preview_delta = clamp_keyframe_delta(
                                                &drag.keyframes,
                                                requested,
                                                data.start_frame,
                                                data.end_frame,
                                            );
                                        }
                                    }
                                }
                                if key_response.drag_stopped() {
                                    if let Some(drag) = runtime.keyframe_drag.take() {
                                        if drag.anchor == keyframe_id && drag.preview_delta != 0 {
                                            tree.make_undo_redo_snapshot();
                                            runtime.selected_keyframes = animation::move_keyframes(
                                                tree,
                                                &drag.keyframes,
                                                drag.preview_delta,
                                            );
                                            runtime.set_frame(tree, runtime.current_frame);
                                            tree.make_undo_redo_snapshot();
                                        }
                                    }
                                }
                            }
                            let playhead_x = frame_x(runtime.current_frame);
                            painter.line_segment(
                                [
                                    egui::pos2(playhead_x, rect.top()),
                                    egui::pos2(playhead_x, rect.bottom()),
                                ],
                                Stroke::new(1.5, Color32::from_rgb(255, 118, 107)),
                            );
                            if !bezier_handle_active && !keyframe_active && response.clicked() {
                                let Some(pointer) = response.interact_pointer_pos() else {
                                    return;
                                };
                                runtime.set_frame(tree, frame_at_x(pointer.x).round());
                            }
                            if !bezier_handle_active && !keyframe_active && response.drag_started()
                            {
                                response.request_focus();
                                if let Some(pointer) = ui.input(|input| {
                                    input
                                        .pointer
                                        .press_origin()
                                        .or_else(|| response.interact_pointer_pos())
                                }) {
                                    box_start =
                                        Some((pointer, ui.input(|input| input.modifiers.shift)));
                                }
                            }
                        });
                    }
                });

            if let Some((start, additive)) = box_start {
                runtime.timeline_box_selection = Some(TimelineBoxSelection {
                    start,
                    end: start,
                    initial: runtime.selected_keyframes.clone(),
                    additive,
                });
            }
            let pointer = ui.input(|input| input.pointer.interact_pos());
            let released = ui.input(|input| input.pointer.primary_released());
            if let Some(selection_box) = &mut runtime.timeline_box_selection {
                if let Some(pointer) = pointer {
                    selection_box.end = pointer;
                }
                let rect = egui::Rect::from_two_pos(selection_box.start, selection_box.end);
                let mut selected = if selection_box.additive {
                    selection_box.initial.clone()
                } else {
                    Vec::new()
                };
                for (keyframe, position) in &visible_keyframes {
                    if rect.contains(*position) && !selected.contains(keyframe) {
                        selected.push(*keyframe);
                    }
                }
                runtime.selected_keyframes = selected;
                let color = ui.visuals().selection.bg_fill;
                ui.painter()
                    .rect_filled(rect, 0.0, color.gamma_multiply(0.15));
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
                if released {
                    runtime.timeline_box_selection = None;
                }
            }

            let cancel_keyboard_drag = ui.input(|input| input.key_pressed(egui::Key::Escape));
            let commit_keyboard_drag = ui.input(|input| input.pointer.primary_clicked());
            if let Some(drag) = &mut runtime.keyframe_drag {
                if drag.keyboard_initiated {
                    if let Some(pointer) = pointer {
                        let requested = ((pointer.x - drag.start_pointer_x)
                            / drag.pixels_per_frame.max(0.001))
                        .round() as i32;
                        drag.preview_delta = clamp_keyframe_delta(
                            &drag.keyframes,
                            requested,
                            data.start_frame,
                            data.end_frame,
                        );
                    }
                }
            }
            if cancel_keyboard_drag
                && runtime
                    .keyframe_drag
                    .as_ref()
                    .is_some_and(|drag| drag.keyboard_initiated)
            {
                runtime.keyframe_drag = None;
            } else if commit_keyboard_drag
                && runtime
                    .keyframe_drag
                    .as_ref()
                    .is_some_and(|drag| drag.keyboard_initiated)
            {
                let drag = runtime.keyframe_drag.take().expect("checked keyboard drag");
                if drag.preview_delta != 0 {
                    tree.make_undo_redo_snapshot();
                    runtime.selected_keyframes =
                        animation::move_keyframes(tree, &drag.keyframes, drag.preview_delta);
                    runtime.set_frame(tree, runtime.current_frame);
                    tree.make_undo_redo_snapshot();
                }
            }

            // Timeline interactions may have authored animation data above.
            let data = animation::animation_data(tree);
            if runtime.selected_keyframes.len() > 1 {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.strong(format!(
                        "{} keyframes selected",
                        runtime.selected_keyframes.len()
                    ));
                    if ui.button("Delete selected").clicked() {
                        tree.make_undo_redo_snapshot();
                        animation::delete_keyframes(tree, &runtime.selected_keyframes);
                        runtime.selected_keyframes.clear();
                        tree.make_undo_redo_snapshot();
                    }
                });
                return;
            }
            let Some(selected_keyframe) = runtime.selected_keyframes.first().copied() else {
                return;
            };
            let Some(keyframe) = data
                .tracks
                .iter()
                .find(|track| track.binding == selected_keyframe.binding)
                .and_then(|track| {
                    track
                        .keyframes
                        .iter()
                        .find(|keyframe| keyframe.frame == selected_keyframe.frame)
                })
                .cloned()
            else {
                runtime.selected_keyframes.clear();
                return;
            };
            ui.separator();
            let mut frame = keyframe.frame;
            let mut value = keyframe.value;
            let mut update = false;
            let mut delete = false;
            let selected_track = data
                .tracks
                .iter()
                .find(|track| track.binding == selected_keyframe.binding);
            let easing = selected_track
                .and_then(|track| animation::easing_preset(track, selected_keyframe.frame));
            let shape_max = (selected_keyframe.binding.property == AnimatableProperty::LatticeShape)
                .then(|| objects.iter().find(|object| object.uuid == selected_keyframe.binding.object)
                    .and_then(|object| object.lattice.as_ref())
                    .map_or(0, |lattice| lattice.shape_keys.len()));
            let mut requested_easing = None;
            ui.horizontal_wrapped(|ui| {
                ui.strong(selected_keyframe.binding.property.label());
                ui.label("Frame");
                update |= ui
                    .add(egui::DragValue::new(&mut frame).range(data.start_frame..=data.end_frame))
                    .changed();
                ui.label("Value");
                let mut value_editor = egui::DragValue::new(&mut value).speed(0.01);
                if let Some(maximum) = shape_max {
                    value_editor = value_editor.range(0.0..=maximum as f32);
                }
                update |= ui.add(value_editor).changed();
                if shape_max.is_some() {
                    ui.weak("0 = reset grid; integers = saved shapes");
                }
                if let Some(easing) = easing {
                    egui::ComboBox::from_id_salt("keyframe-easing")
                        .selected_text(easing.label())
                        .show_ui(ui, |ui| {
                            for candidate in animation::EasingPreset::EDITABLE {
                                if ui
                                    .selectable_label(easing == candidate, candidate.label())
                                    .clicked()
                                {
                                    requested_easing = Some(candidate);
                                    ui.close();
                                }
                            }
                        });
                } else {
                    ui.weak("End key");
                }
                delete = ui.button("Delete key").clicked();
            });
            if update || requested_easing.is_some() {
                tree.make_undo_redo_snapshot();
                let selected_keyframe = if update {
                    animation::update_keyframe(
                        tree,
                        selected_keyframe,
                        frame,
                        value,
                        keyframe.interpolation,
                    )
                } else {
                    selected_keyframe
                };
                if let Some(easing) = requested_easing {
                    animation::apply_easing_preset(tree, selected_keyframe, easing);
                }
                runtime.selected_keyframes = vec![selected_keyframe];
                runtime.set_frame(tree, runtime.current_frame);
                tree.make_undo_redo_snapshot();
            } else if delete {
                tree.make_undo_redo_snapshot();
                animation::delete_keyframe(tree, selected_keyframe);
                runtime.selected_keyframes.clear();
                tree.make_undo_redo_snapshot();
            }
        });
}

pub(super) fn timeline_value_bounds(track: &AnimationTrack) -> (f32, f32) {
    let minimum = track
        .keyframes
        .iter()
        .map(|keyframe| keyframe.value)
        .fold(f32::INFINITY, f32::min);
    let maximum = track
        .keyframes
        .iter()
        .map(|keyframe| keyframe.value)
        .fold(f32::NEG_INFINITY, f32::max);
    let span = maximum - minimum;
    if span.abs() < 0.0001 {
        (minimum - 0.5, maximum + 0.5)
    } else {
        let margin = span * 0.12;
        (minimum - margin, maximum + margin)
    }
}

fn clamp_keyframe_delta(
    keyframes: &[SelectedKeyframe],
    requested: i32,
    start_frame: u32,
    end_frame: u32,
) -> i32 {
    let Some(minimum) = keyframes.iter().map(|keyframe| keyframe.frame).min() else {
        return 0;
    };
    let maximum = keyframes
        .iter()
        .map(|keyframe| keyframe.frame)
        .max()
        .unwrap_or(minimum);
    let minimum_delta =
        (start_frame as i64 - minimum as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let maximum_delta =
        (end_frame as i64 - maximum as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    requested.clamp(minimum_delta, maximum_delta)
}

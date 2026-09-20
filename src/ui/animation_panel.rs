use super::*;

pub(super) fn animation_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
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

            let label_width = 155.0_f32.min(ui.available_width() * 0.42);
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
                                egui::vec2(label_width, 64.0),
                                egui::Label::new(&label).truncate(),
                            )
                            .on_hover_text(label);
                            let width = ui.available_width().max(80.0);
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(width, 64.0),
                                egui::Sense::click_and_drag(),
                            );
                            let painter = ui.painter_at(rect);
                            painter.rect_filled(rect, 3.0, Color32::from_rgb(34, 35, 39));
                            painter.line_segment(
                                [rect.left_center(), rect.right_center()],
                                Stroke::new(1.0, Color32::from_gray(75)),
                            );
                            let frame_span =
                                data.end_frame.saturating_sub(data.start_frame).max(1) as f32;
                            let frame_x = |frame: f32| {
                                rect.left()
                                    + (frame - data.start_frame as f32) / frame_span * rect.width()
                            };
                            let (minimum, maximum) = timeline_value_bounds(track);
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
                            let frame_at_x = |x: f32| {
                                data.start_frame as f32
                                    + (x - rect.left()) / rect.width() * frame_span
                            };
                            let mut bezier_handle_active = false;
                            for pair in track.keyframes.windows(2) {
                                let left = &pair[0];
                                let right = &pair[1];
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
                                            animation::bezier_control_points(left, right);
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
                                        for (incoming, keyframe, control) in
                                            [(false, left, controls[1]), (true, right, controls[2])]
                                        {
                                            let anchor = egui::pos2(
                                                frame_x(keyframe.frame as f32),
                                                value_y(keyframe.value),
                                            );
                                            let center =
                                                egui::pos2(frame_x(control.0), value_y(control.1));
                                            painter.line_segment(
                                                [anchor, center],
                                                Stroke::new(1.0, Color32::from_gray(135)),
                                            );
                                            let handle_response = ui.interact(
                                                egui::Rect::from_center_size(
                                                    center,
                                                    egui::vec2(12.0, 12.0),
                                                ),
                                                egui::Id::new((
                                                    "bezier-handle",
                                                    track.binding,
                                                    keyframe.frame,
                                                    incoming,
                                                )),
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
                                                            frame: keyframe.frame,
                                                        },
                                                        incoming,
                                                        crate::model::BezierHandle {
                                                            frame_offset: control_frame
                                                                - keyframe.frame as f32,
                                                            value_offset: control_value
                                                                - keyframe.value,
                                                        },
                                                    );
                                                    runtime.selected_keyframe =
                                                        Some(SelectedKeyframe {
                                                            binding: track.binding,
                                                            frame: keyframe.frame,
                                                        });
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
                                let displayed_frame = runtime
                                    .keyframe_drag
                                    .filter(|drag| drag.keyframe == keyframe_id)
                                    .map_or(keyframe.frame, |drag| drag.preview_frame);
                                let center = egui::pos2(
                                    frame_x(displayed_frame as f32),
                                    value_y(keyframe.value),
                                );
                                let key_response = ui.interact(
                                    egui::Rect::from_center_size(center, egui::vec2(14.0, 14.0)),
                                    egui::Id::new((
                                        "timeline-keyframe",
                                        track.binding,
                                        keyframe.frame,
                                    )),
                                    egui::Sense::click_and_drag(),
                                );
                                key_response.clone().on_hover_text("Drag to move keyframe");
                                keyframe_active |= key_response.hovered() || key_response.dragged();
                                let selected = runtime.selected_keyframe == Some(keyframe_id);
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
                                    runtime.selected_keyframe = Some(keyframe_id);
                                    runtime.set_frame(tree, keyframe.frame as f32);
                                }
                                if key_response.drag_started() {
                                    runtime.selected_keyframe = Some(keyframe_id);
                                    runtime.keyframe_drag = Some(KeyframeDrag {
                                        keyframe: keyframe_id,
                                        preview_frame: keyframe.frame,
                                    });
                                }
                                if key_response.dragged() {
                                    if let Some(pointer) = key_response.interact_pointer_pos() {
                                        let preview_frame = frame_at_x(pointer.x)
                                            .round()
                                            .clamp(data.start_frame as f32, data.end_frame as f32)
                                            as u32;
                                        runtime.keyframe_drag = Some(KeyframeDrag {
                                            keyframe: keyframe_id,
                                            preview_frame,
                                        });
                                    }
                                }
                                if key_response.drag_stopped() {
                                    if let Some(drag) = runtime.keyframe_drag.take() {
                                        if drag.keyframe == keyframe_id
                                            && drag.preview_frame != keyframe.frame
                                        {
                                            tree.make_undo_redo_snapshot();
                                            runtime.selected_keyframe =
                                                Some(animation::update_keyframe(
                                                    tree,
                                                    drag.keyframe,
                                                    drag.preview_frame,
                                                    keyframe.value,
                                                    keyframe.interpolation,
                                                ));
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
                            if !bezier_handle_active
                                && !keyframe_active
                                && (response.clicked() || response.dragged())
                            {
                                let Some(pointer) = response.interact_pointer_pos() else {
                                    return;
                                };
                                runtime.set_frame(tree, frame_at_x(pointer.x).round());
                            }
                        });
                    }
                });

            // Timeline interactions may have authored animation data above.
            let data = animation::animation_data(tree);
            let Some(selected_keyframe) = runtime.selected_keyframe else {
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
                runtime.selected_keyframe = None;
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
            let mut requested_easing = None;
            ui.horizontal_wrapped(|ui| {
                ui.strong(selected_keyframe.binding.property.label());
                ui.label("Frame");
                update |= ui
                    .add(egui::DragValue::new(&mut frame).range(data.start_frame..=data.end_frame))
                    .changed();
                ui.label("Value");
                update |= ui
                    .add(egui::DragValue::new(&mut value).speed(0.01))
                    .changed();
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
                runtime.selected_keyframe = Some(selected_keyframe);
                runtime.set_frame(tree, runtime.current_frame);
                tree.make_undo_redo_snapshot();
            } else if delete {
                tree.make_undo_redo_snapshot();
                animation::delete_keyframe(tree, selected_keyframe);
                runtime.selected_keyframe = None;
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

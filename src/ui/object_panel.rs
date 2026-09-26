use super::*;

pub(super) fn object_panel(ui: &mut egui::Ui, tree: &mut DataTree, runtime: &mut AnimationRuntime) {
    let selection = selected(tree);
    if selection.len() == 1
        && crate::model::scene_cameras(tree)
            .iter()
            .any(|camera| camera.uuid == selection[0])
    {
        camera_object_panel(ui, tree, runtime, selection[0]);
        return;
    }
    if let Some(group) = commands::selected_group_id(tree) {
        group_transform_panel(ui, tree, runtime, group);
        ui.separator();
        modifiers_panel(ui, tree, runtime);
        return;
    }
    if selection.len() != 1 {
        ui.label("Select one object to edit it.");
        return;
    }
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == selection[0]) else {
        return;
    };
    let object_id = object.uuid;
    let mut keyframes = Vec::new();
    let mut changed = false;
    ui.label(RichText::new("Object settings").strong());
    changed |= ui.text_edit_singleline(&mut object.name).changed();
    ui.separator();
    let reset_position = ui
        .horizontal(|ui| {
            ui.label("Position");
            ui.add_enabled(
                object.transform.translation != Vec3::ZERO,
                egui::Button::new("Reset"),
            )
            .on_hover_text("Reset position to the origin")
            .clicked()
        })
        .inner;
    if reset_position {
        tree.make_undo_redo_snapshot();
        object.transform.translation = Vec3::ZERO;
        changed = true;
    }
    changed |= animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut object.transform.translation,
        0.01,
        object_id,
        [
            AnimatableProperty::Position(VectorAxis::X),
            AnimatableProperty::Position(VectorAxis::Y),
            AnimatableProperty::Position(VectorAxis::Z),
        ],
        &mut keyframes,
    );
    let reset_rotation = ui
        .horizontal(|ui| {
            ui.label("Rotation");
            ui.add_enabled(
                object.transform.rotation != glam::Quat::IDENTITY,
                egui::Button::new("Reset"),
            )
            .on_hover_text("Reset rotation to zero on all axes")
            .clicked()
        })
        .inner;
    if reset_rotation {
        tree.make_undo_redo_snapshot();
        object.transform.rotation = glam::Quat::IDENTITY;
        changed = true;
    }
    let (x, y, z) = object.transform.rotation.to_euler(EulerRot::XYZ);
    let mut degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
    if animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut degrees,
        1.0,
        object_id,
        [
            AnimatableProperty::Rotation(VectorAxis::X),
            AnimatableProperty::Rotation(VectorAxis::Y),
            AnimatableProperty::Rotation(VectorAxis::Z),
        ],
        &mut keyframes,
    ) {
        object.transform.rotation = glam::Quat::from_euler(
            EulerRot::XYZ,
            degrees.x.to_radians(),
            degrees.y.to_radians(),
            degrees.z.to_radians(),
        );
        changed = true;
    }
    ui.label("Scale");
    changed |= animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut object.transform.scale,
        0.01,
        object_id,
        [
            AnimatableProperty::Scale(VectorAxis::X),
            AnimatableProperty::Scale(VectorAxis::Y),
            AnimatableProperty::Scale(VectorAxis::Z),
        ],
        &mut keyframes,
    );
    ui.separator();
    changed |= params_editor(
        ui,
        tree,
        runtime,
        &mut object.params,
        object_id,
        &mut keyframes,
    );
    if changed {
        set_objects(tree, scene);
        if reset_position || reset_rotation {
            tree.make_undo_redo_snapshot();
        }
    }
    apply_keyframe_requests(tree, runtime, keyframes);
    ui.separator();
    modifiers_panel(ui, tree, runtime);
}

fn camera_object_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
    camera_id: uuid::Uuid,
) {
    let mut cameras = crate::model::scene_cameras(tree);
    let Some(camera) = cameras.iter_mut().find(|camera| camera.uuid == camera_id) else {
        return;
    };
    let mut changed = false;
    let mut keyframes = Vec::new();
    ui.label(RichText::new("Camera settings").strong());
    changed |= ui.text_edit_singleline(&mut camera.name).changed();
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Position");
        if ui
            .add_enabled(
                camera.transform.translation != Vec3::ZERO,
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            camera.transform.translation = Vec3::ZERO;
            changed = true;
        }
    });
    changed |= animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut camera.transform.translation,
        0.01,
        camera_id,
        [
            AnimatableProperty::Position(VectorAxis::X),
            AnimatableProperty::Position(VectorAxis::Y),
            AnimatableProperty::Position(VectorAxis::Z),
        ],
        &mut keyframes,
    );

    ui.horizontal(|ui| {
        ui.label("Rotation");
        if ui
            .add_enabled(
                camera.transform.rotation != glam::Quat::IDENTITY,
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            camera.transform.rotation = glam::Quat::IDENTITY;
            changed = true;
        }
    });
    let (x, y, z) = camera.transform.rotation.to_euler(EulerRot::XYZ);
    let mut degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
    if animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut degrees,
        1.0,
        camera_id,
        [
            AnimatableProperty::Rotation(VectorAxis::X),
            AnimatableProperty::Rotation(VectorAxis::Y),
            AnimatableProperty::Rotation(VectorAxis::Z),
        ],
        &mut keyframes,
    ) {
        camera.transform.rotation = glam::Quat::from_euler(
            EulerRot::XYZ,
            degrees.x.to_radians(),
            degrees.y.to_radians(),
            degrees.z.to_radians(),
        );
        changed = true;
    }

    ui.horizontal(|ui| {
        ui.label("Scale");
        if ui
            .add_enabled(
                camera.transform.scale != Vec3::ONE,
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            camera.transform.scale = Vec3::ONE;
            changed = true;
        }
    });
    changed |= animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut camera.transform.scale,
        0.01,
        camera_id,
        [
            AnimatableProperty::Scale(VectorAxis::X),
            AnimatableProperty::Scale(VectorAxis::Y),
            AnimatableProperty::Scale(VectorAxis::Z),
        ],
        &mut keyframes,
    );

    ui.separator();
    let focal_binding = AnimationBinding {
        object: camera_id,
        property: AnimatableProperty::CameraFocalDistance,
    };
    let focal_response = animatable_widget(ui, tree, runtime, focal_binding, |ui| {
        ui.add(
            egui::DragValue::new(&mut camera.focal_distance)
                .speed(0.02)
                .range(0.01..=100.0)
                .prefix("Focal distance  "),
        )
    });
    changed |= focal_response.changed();
    animatable_response(
        ui,
        &focal_response,
        focal_binding,
        camera.focal_distance,
        &mut keyframes,
    );

    let mut projection = camera.projection_mode;
    let projection_binding = AnimationBinding {
        object: camera_id,
        property: AnimatableProperty::CameraProjection,
    };
    let projection_response = animatable_widget(ui, tree, runtime, projection_binding, |ui| {
        egui::ComboBox::from_label("Projection")
            .selected_text(projection.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut projection,
                    crate::camera::ProjectionMode::Perspective,
                    "Perspective",
                );
                ui.selectable_value(
                    &mut projection,
                    crate::camera::ProjectionMode::Orthographic,
                    "Orthographic",
                );
            })
            .response
    });
    changed |= projection_response.changed();
    camera.projection_mode = projection;
    animatable_response(
        ui,
        &projection_response,
        projection_binding,
        if projection == crate::camera::ProjectionMode::Orthographic {
            1.0
        } else {
            0.0
        },
        &mut keyframes,
    );

    let mut camera_view = matches!(
        tree.get_path("editor.camera_view"),
        crate::model::ClaydashValue::Bool(true)
    );
    if ui
        .checkbox(&mut camera_view, "View through camera")
        .changed()
    {
        tree.set_path(
            "scene.active_camera",
            crate::model::ClaydashValue::Uuid(camera_id),
        );
        tree.set_transient_path(
            "editor.camera_view",
            crate::model::ClaydashValue::Bool(camera_view),
        );
    }

    if changed {
        crate::model::set_scene_cameras(tree, cameras);
        tree.make_undo_redo_snapshot();
    }
    apply_keyframe_requests(tree, runtime, keyframes);
}

fn group_transform_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
    group: uuid::Uuid,
) {
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == group) else {
        return;
    };
    ui.label(RichText::new("Group transform").strong());
    ui.label("Transforms the complete Boolean group.");
    let mut changed = false;
    let mut keyframes = Vec::new();
    ui.horizontal(|ui| {
        ui.label("Position");
        if ui
            .add_enabled(
                object.group_transform.translation != Vec3::ZERO,
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            object.group_transform.translation = Vec3::ZERO;
            changed = true;
        }
    });
    changed |= animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut object.group_transform.translation,
        0.01,
        group,
        [
            AnimatableProperty::GroupPosition(VectorAxis::X),
            AnimatableProperty::GroupPosition(VectorAxis::Y),
            AnimatableProperty::GroupPosition(VectorAxis::Z),
        ],
        &mut keyframes,
    );
    ui.horizontal(|ui| {
        ui.label("Rotation");
        if ui
            .add_enabled(
                object.group_transform.rotation != glam::Quat::IDENTITY,
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            object.group_transform.rotation = glam::Quat::IDENTITY;
            changed = true;
        }
    });
    let (x, y, z) = object.group_transform.rotation.to_euler(EulerRot::XYZ);
    let mut degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
    if animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut degrees,
        1.0,
        group,
        [
            AnimatableProperty::GroupRotation(VectorAxis::X),
            AnimatableProperty::GroupRotation(VectorAxis::Y),
            AnimatableProperty::GroupRotation(VectorAxis::Z),
        ],
        &mut keyframes,
    ) {
        object.group_transform.rotation = glam::Quat::from_euler(
            EulerRot::XYZ,
            degrees.x.to_radians(),
            degrees.y.to_radians(),
            degrees.z.to_radians(),
        );
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.label("Scale");
        if ui
            .add_enabled(
                object.group_transform.scale != Vec3::ONE,
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            object.group_transform.scale = Vec3::ONE;
            changed = true;
        }
    });
    changed |= animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut object.group_transform.scale,
        0.01,
        group,
        [
            AnimatableProperty::GroupScale(VectorAxis::X),
            AnimatableProperty::GroupScale(VectorAxis::Y),
            AnimatableProperty::GroupScale(VectorAxis::Z),
        ],
        &mut keyframes,
    );
    if changed {
        set_objects(tree, scene);
    }
    apply_keyframe_requests(tree, runtime, keyframes);
}

pub(super) fn params_editor(
    ui: &mut egui::Ui,
    tree: &DataTree,
    runtime: &AnimationRuntime,
    params: &mut SdfParams,
    object: uuid::Uuid,
    keyframes: &mut Vec<KeyframeRequest>,
) -> bool {
    match params {
        SdfParams::SphereParams(value) => {
            let binding = AnimationBinding {
                object,
                property: AnimatableProperty::SphereRadius,
            };
            let response = animatable_widget(ui, tree, runtime, binding, |ui| {
                ui.add(egui::Slider::new(&mut value.radius, 0.01..=4.0).text("Radius"))
            });
            animatable_response(ui, &response, binding, value.radius, keyframes);
            response.changed()
        }
        SdfParams::BoxParams(value) => {
            ui.label("Half extents");
            let extent_changed = animatable_vec3_editor(
                ui,
                tree,
                runtime,
                &mut value.box_q,
                0.01,
                object,
                [
                    AnimatableProperty::BoxHalfExtent(VectorAxis::X),
                    AnimatableProperty::BoxHalfExtent(VectorAxis::Y),
                    AnimatableProperty::BoxHalfExtent(VectorAxis::Z),
                ],
                keyframes,
            );
            let radius_changed = ui
                .add(
                    egui::Slider::new(
                        &mut value.corner_radius,
                        0.0..=value.box_q.min_element().max(0.0),
                    )
                    .text("Corner radius"),
                )
                .changed();
            extent_changed | radius_changed
        }
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => {
            let radius_binding = AnimationBinding {
                object,
                property: AnimatableProperty::CylinderRadius,
            };
            let radius_response = animatable_widget(ui, tree, runtime, radius_binding, |ui| {
                ui.add(egui::Slider::new(radius, 0.01..=4.0).text("Radius"))
            });
            animatable_response(ui, &radius_response, radius_binding, *radius, keyframes);
            let height_binding = AnimationBinding {
                object,
                property: AnimatableProperty::CylinderHalfHeight,
            };
            let height_response = animatable_widget(ui, tree, runtime, height_binding, |ui| {
                ui.add(egui::Slider::new(half_height, 0.01..=4.0).text("Half height"))
            });
            animatable_response(
                ui,
                &height_response,
                height_binding,
                *half_height,
                keyframes,
            );
            radius_response.changed() | height_response.changed()
        }
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => {
            let major_binding = AnimationBinding {
                object,
                property: AnimatableProperty::TorusMajorRadius,
            };
            let major_response = animatable_widget(ui, tree, runtime, major_binding, |ui| {
                ui.add(egui::Slider::new(major_radius, 0.02..=4.0).text("Major radius"))
            });
            animatable_response(ui, &major_response, major_binding, *major_radius, keyframes);
            let minor_binding = AnimationBinding {
                object,
                property: AnimatableProperty::TorusMinorRadius,
            };
            let minor_response = animatable_widget(ui, tree, runtime, minor_binding, |ui| {
                ui.add(egui::Slider::new(minor_radius, 0.01..=2.0).text("Tube radius"))
            });
            animatable_response(ui, &minor_response, minor_binding, *minor_radius, keyframes);
            major_response.changed() | minor_response.changed()
        }
        SdfParams::PolygonPrismParams(value) => ui
            .add(egui::Slider::new(&mut value.half_depth, 0.005..=4.0).text("Half depth"))
            .changed(),
        SdfParams::LoftParams(value) => {
            ui.label("Closed sections along local X");
            ui.weak("Profile Y/Z values scale with the section's half height and half width. Matching point numbers connect along the loft.");
            let mut changed = false;
            let mut remove = None;
            let mut add_profile_point = None;
            let mut remove_profile_point = None;
            let profile_count = value.profile_count();
            for index in 0..value.sections.len() {
                let minimum = if index == 0 {
                    -10.0
                } else {
                    value.sections[index - 1].x + 0.01
                };
                let maximum = if index + 1 == value.sections.len() {
                    10.0
                } else {
                    value.sections[index + 1].x - 0.01
                };
                let can_remove = value.sections.len() > 2;
                let section = &mut value.sections[index];
                ui.collapsing(format!("Section {}", index + 1), |ui| {
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut section.x)
                                .speed(0.01)
                                .range(minimum..=maximum)
                                .prefix("X "),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut section.center_y)
                                .speed(0.01)
                                .prefix("Center Y "),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut section.center_z)
                                .speed(0.01)
                                .prefix("Center Z "),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut section.half_height)
                                .speed(0.01)
                                .range(0.01..=10.0)
                                .prefix("Half height "),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut section.half_width)
                                .speed(0.01)
                                .range(0.01..=10.0)
                                .prefix("Half width "),
                        )
                        .changed();
                    if section.profile.is_none() {
                        if ui.button("Draw custom profile").clicked() {
                            let count = profile_count.max(12);
                            section.profile = Some((0..count).map(|index| {
                                let angle = std::f32::consts::TAU * index as f32 / count as f32;
                                Vec2::new(angle.cos(), angle.sin())
                            }).collect());
                            changed = true;
                        }
                    } else {
                        if ui.button("Use ellipse").clicked() {
                            section.profile = None;
                            changed = true;
                        } else if let Some(profile) = &mut section.profile {
                            changed |= loft_profile_editor(ui, profile, index);
                            let can_remove_point = profile.len() > 3;
                            for (point_index, point) in profile.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{}", point_index + 1));
                                    changed |= ui.add(egui::DragValue::new(&mut point.x).speed(0.01).prefix("Y ")).changed();
                                    changed |= ui.add(egui::DragValue::new(&mut point.y).speed(0.01).prefix("Z ")).changed();
                                    if ui.small_button("+").on_hover_text("Insert a point after this point in every custom profile").clicked() { add_profile_point = Some(point_index); }
                                    if can_remove_point && ui.small_button("−").on_hover_text("Remove this point from every custom profile").clicked() { remove_profile_point = Some(point_index); }
                                });
                            }
                        }
                    }
                    if can_remove && ui.button("Remove section").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                value.sections.remove(index);
                changed = true;
            }
            if let Some(index) = add_profile_point {
                if profile_count < crate::model::LoftParams::MAX_PROFILE_POINTS {
                    for section in &mut value.sections {
                        if let Some(profile) = &mut section.profile {
                            let next = (index + 1) % profile.len();
                            let point = (profile[index] + profile[next]) * 0.5;
                            profile.insert(index + 1, point);
                        }
                    }
                    changed = true;
                }
            } else if let Some(index) = remove_profile_point {
                for section in &mut value.sections {
                    if let Some(profile) = &mut section.profile {
                        profile.remove(index);
                    }
                }
                changed = true;
            }
            if value.sections.len() < crate::model::LoftParams::MAX_SECTIONS
                && ui.button("Add section").clicked()
            {
                let index = value.sections.len() - 2;
                let a = &value.sections[index];
                let b = &value.sections[index + 1];
                let profile = (profile_count > 0).then(|| {
                    (0..profile_count)
                        .map(|point| {
                            (crate::model::LoftParams::profile_point(a, point, profile_count)
                                + crate::model::LoftParams::profile_point(b, point, profile_count))
                                * 0.5
                        })
                        .collect()
                });
                value.sections.insert(
                    index + 1,
                    crate::model::LoftSection {
                        x: (a.x + b.x) * 0.5,
                        center_y: (a.center_y + b.center_y) * 0.5,
                        center_z: (a.center_z + b.center_z) * 0.5,
                        half_height: (a.half_height + b.half_height) * 0.5,
                        half_width: (a.half_width + b.half_width) * 0.5,
                        profile,
                    },
                );
                changed = true;
            }
            changed
        }
        SdfParams::BezierCurveParams(value) => {
            ui.label(format!(
                "{} Bézier segment{}",
                value.segment_count(),
                if value.segment_count() == 1 { "" } else { "s" }
            ));
            ui.weak("G moves a point · Backspace removes an anchor · E extends · Enter closes the loop.");
            false
        }
    }
}

fn loft_profile_editor(ui: &mut egui::Ui, profile: &mut [Vec2], section_index: usize) -> bool {
    let size = ui.available_width().min(200.0).max(120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let scale = size / 4.5;
    let frame = ui.visuals().widgets.noninteractive;
    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    painter.line_segment(
        [
            egui::pos2(rect.left(), center.y),
            egui::pos2(rect.right(), center.y),
        ],
        frame.bg_stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x, rect.top()),
            egui::pos2(center.x, rect.bottom()),
        ],
        frame.bg_stroke,
    );
    let position = |point: Vec2| center + egui::vec2(point.x * scale, -point.y * scale);
    for index in 0..profile.len() {
        painter.line_segment(
            [
                position(profile[index]),
                position(profile[(index + 1) % profile.len()]),
            ],
            egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
        );
    }
    let mut changed = false;
    for (index, point) in profile.iter_mut().enumerate() {
        let handle = position(*point);
        let hit = egui::Rect::from_center_size(handle, egui::vec2(14.0, 14.0));
        let response = ui.interact(
            hit,
            ui.id().with(("loft point", section_index, index)),
            egui::Sense::drag(),
        );
        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                point.x = (pointer.x - center.x) / scale;
                point.y = (center.y - pointer.y) / scale;
                changed = true;
            }
        }
        painter.circle_filled(position(*point), 4.0, ui.visuals().selection.stroke.color);
    }
    changed
}

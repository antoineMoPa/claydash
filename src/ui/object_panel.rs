use super::*;

pub(super) fn object_panel(ui: &mut egui::Ui, tree: &mut DataTree, runtime: &mut AnimationRuntime) {
    if let Some(group) = commands::selected_group_id(tree) {
        group_transform_panel(ui, tree, runtime, group);
        return;
    }
    let selection = selected(tree);
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
            animatable_vec3_editor(
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
            )
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
    }
}

use super::*;

pub(super) fn operand_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
    if commands::selected_group_id(tree).is_some() {
        ui.label("Drill into the group to edit an operand.");
        return;
    }
    let selection = selected(tree);
    let mut scene = objects(tree);
    let Some(first) = scene.iter().find(|object| selection.contains(&object.uuid)) else {
        ui.label("Select an object to edit its operand properties.");
        return;
    };
    ui.label(RichText::new("Operand properties").strong());
    if let Some(parent) = first
        .boolean_parent
        .and_then(|id| scene.iter().find(|o| o.uuid == id))
    {
        ui.label(format!("Target: {}", parent.display_name()));
    } else {
        ui.label("These settings apply when this object is used as an operand.");
    }
    let mut operation = first.operation;
    let mut softness = first.softness;
    let first_id = first.uuid;
    let mut changed = false;
    egui::ComboBox::from_label("Operation")
        .selected_text(operation.label())
        .show_ui(ui, |ui| {
            for value in [
                BooleanOperation::Union,
                BooleanOperation::Subtract,
                BooleanOperation::Intersect,
            ] {
                changed |= ui
                    .selectable_value(&mut operation, value, value.label())
                    .changed();
            }
        });
    let softness_binding = AnimationBinding {
        object: first_id,
        property: AnimatableProperty::OperandSoftness,
    };
    let response = animatable_widget(ui, tree, runtime, softness_binding, |ui| {
        ui.add(egui::Slider::new(&mut softness, 0.0..=0.5).text("Softness"))
    });
    let mut keyframes = Vec::new();
    if response.hovered() && ui.input(plain_i_pressed) {
        for object in scene
            .iter()
            .filter(|object| selection.contains(&object.uuid))
        {
            keyframes.push(KeyframeRequest {
                binding: AnimationBinding {
                    object: object.uuid,
                    property: AnimatableProperty::OperandSoftness,
                },
                value: softness,
            });
        }
    }
    response
        .clone()
        .on_hover_text("Press I to insert a keyframe at the current frame");
    let operation_changed = changed;
    changed |= response.changed();
    ui.label("0 = sharp edges. Softness is measured in world units.");
    if response.drag_started() || operation_changed || (changed && !response.dragged()) {
        tree.make_undo_redo_snapshot();
    }
    if changed {
        for object in &mut scene {
            if selection.contains(&object.uuid) {
                if operation_changed {
                    object.operation = operation;
                }
                if response.changed() {
                    object.softness = softness;
                }
            }
        }
        set_objects(tree, scene);
    }
    if response.drag_stopped() || operation_changed || (changed && !response.dragged()) {
        tree.make_undo_redo_snapshot();
    }
    apply_keyframe_requests(tree, runtime, keyframes);
}

pub(super) fn repetition_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
    if commands::selected_group_id(tree).is_some() {
        ui.label("Drill into the group to repeat a primitive.");
        return;
    }
    let selection = selected(tree);
    if selection.is_empty() {
        ui.label("Select objects to repeat.");
        return;
    }
    let mut scene = objects(tree);
    let Some(first) = scene.iter().find(|object| selection.contains(&object.uuid)) else {
        return;
    };
    let first_id = first.uuid;
    let mut repetition = first.repetition;
    let mut keyframes = Vec::new();
    let enabled_response = animatable_widget(
        ui,
        tree,
        runtime,
        AnimationBinding {
            object: first_id,
            property: AnimatableProperty::RepetitionEnabled,
        },
        |ui| ui.checkbox(&mut repetition.enabled, "Enable domain repetition"),
    );
    let mut changed = enabled_response.changed();
    append_selected_keyframes_on_hover(
        ui,
        &enabled_response,
        &scene,
        &selection,
        AnimatableProperty::RepetitionEnabled,
        if repetition.enabled { 1.0 } else { 0.0 },
        &mut keyframes,
    );
    ui.label("Axes");
    ui.horizontal(|ui| {
        for axis in VectorAxis::ALL {
            let index = axis.index();
            let response = animatable_widget(
                ui,
                tree,
                runtime,
                AnimationBinding {
                    object: first_id,
                    property: AnimatableProperty::RepetitionAxis(axis),
                },
                |ui| ui.checkbox(&mut repetition.axes[index], axis.label()),
            );
            changed |= response.changed();
            append_selected_keyframes_on_hover(
                ui,
                &response,
                &scene,
                &selection,
                AnimatableProperty::RepetitionAxis(axis),
                if repetition.axes[index] { 1.0 } else { 0.0 },
                &mut keyframes,
            );
        }
    });
    for axis in VectorAxis::ALL {
        let index = axis.index();
        ui.horizontal_wrapped(|ui| {
            ui.label(axis.label());
            let count_response = animatable_widget(
                ui,
                tree,
                runtime,
                AnimationBinding {
                    object: first_id,
                    property: AnimatableProperty::RepetitionCount(axis),
                },
                |ui| {
                    ui.add(
                        egui::DragValue::new(&mut repetition.count[index])
                            .range(1..=32)
                            .prefix("count "),
                    )
                },
            );
            changed |= count_response.changed();
            append_selected_keyframes_on_hover(
                ui,
                &count_response,
                &scene,
                &selection,
                AnimatableProperty::RepetitionCount(axis),
                repetition.count[index] as f32,
                &mut keyframes,
            );
            let spacing_response = animatable_widget(
                ui,
                tree,
                runtime,
                AnimationBinding {
                    object: first_id,
                    property: AnimatableProperty::RepetitionSpacing(axis),
                },
                |ui| {
                    ui.add(
                        egui::DragValue::new(&mut repetition.spacing[index])
                            .speed(0.02)
                            .range(0.01..=20.0)
                            .prefix("spacing "),
                    )
                },
            );
            changed |= spacing_response.changed();
            append_selected_keyframes_on_hover(
                ui,
                &spacing_response,
                &scene,
                &selection,
                AnimatableProperty::RepetitionSpacing(axis),
                repetition.spacing[index],
                &mut keyframes,
            );
        });
    }
    if changed {
        for object in &mut scene {
            if selection.contains(&object.uuid) {
                object.repetition = repetition;
            }
        }
        set_objects(tree, scene);
    }
    apply_keyframe_requests(tree, runtime, keyframes);
}

pub(super) fn append_selected_keyframes_on_hover(
    ui: &egui::Ui,
    response: &egui::Response,
    scene: &[SdfObject],
    selection: &[uuid::Uuid],
    property: AnimatableProperty,
    value: f32,
    keyframes: &mut Vec<KeyframeRequest>,
) {
    response
        .clone()
        .on_hover_text("Press I to insert a keyframe at the current frame");
    if !response.hovered() || !ui.input(plain_i_pressed) {
        return;
    }
    for object in scene
        .iter()
        .filter(|object| selection.contains(&object.uuid))
    {
        keyframes.push(KeyframeRequest {
            binding: AnimationBinding {
                object: object.uuid,
                property,
            },
            value,
        });
    }
}

pub(super) fn animatable_vec3_editor(
    ui: &mut egui::Ui,
    tree: &DataTree,
    runtime: &AnimationRuntime,
    value: &mut Vec3,
    speed: f64,
    object: uuid::Uuid,
    properties: [AnimatableProperty; 3],
    keyframes: &mut Vec<KeyframeRequest>,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for axis in 0..3 {
            let binding = AnimationBinding {
                object,
                property: properties[axis],
            };
            let response = animatable_widget(ui, tree, runtime, binding, |ui| {
                ui.add(
                    egui::DragValue::new(&mut value[axis])
                        .speed(speed)
                        .prefix(format!("{} ", axis_label(axis))),
                )
            });
            animatable_response(ui, &response, binding, value[axis], keyframes);
            changed |= response.changed();
        }
    });
    changed
}

pub(super) struct ResizeHandle {
    pub(super) world: Vec3,
    pub(super) guide_origin: Vec3,
    pub(super) direction: Vec3,
    pub(super) patch: Vec<Vec3>,
    pub(super) label: String,
    pub(super) color: Color32,
    pub(super) parameter: usize,
    pub(super) value: f32,
}

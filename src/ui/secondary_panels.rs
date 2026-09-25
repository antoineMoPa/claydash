use super::*;

pub(super) fn operand_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
    let selection = selected(tree);
    let mut scene = objects(tree);
    let Some(first) = scene.iter().find(|object| selection.contains(&object.uuid)) else {
        ui.label("Select an object to edit its operand properties.");
        return;
    };
    let first_id = first.uuid;
    let parent_id = first.boolean_parent;
    let direct_children: Vec<_> = scene
        .iter()
        .filter(|object| object.boolean_parent == Some(first_id))
        .map(|object| object.uuid)
        .collect();
    let softness_owner = if direct_children.is_empty() {
        parent_id
    } else {
        Some(first_id)
    };
    if parent_id.is_none() && direct_children.is_empty() {
        ui.label("This object is not part of a boolean group.");
        return;
    }
    ui.label(RichText::new("Operand properties").strong());
    if let Some(parent) = parent_id.and_then(|id| scene.iter().find(|o| o.uuid == id)) {
        ui.label(format!("Target: {}", parent.display_name()));
    }
    let mut parent_operation = first.operation;
    let parent_operation_changed = parent_id.is_some()
        && boolean_operation_combo(ui, "Parent operation", &mut parent_operation, false);
    let first_child_operation = direct_children.first().and_then(|id| {
        scene
            .iter()
            .find(|object| object.uuid == *id)
            .map(|object| object.operation)
    });
    let mixed_child_operations = first_child_operation.is_some_and(|operation| {
        direct_children.iter().any(|id| {
            scene
                .iter()
                .find(|object| object.uuid == *id)
                .is_some_and(|object| object.operation != operation)
        })
    });
    let mut group_operation = first_child_operation.unwrap_or(BooleanOperation::Union);
    let group_operation_changed = !direct_children.is_empty()
        && boolean_operation_combo(
            ui,
            "Group operation",
            &mut group_operation,
            mixed_child_operations,
        );
    let Some(softness_owner) = softness_owner else {
        return;
    };
    let mut softness = scene
        .iter()
        .find(|object| object.uuid == softness_owner)
        .map(|object| object.softness)
        .unwrap_or_default();
    let softness_binding = AnimationBinding {
        object: softness_owner,
        property: AnimatableProperty::OperandSoftness,
    };
    let response = animatable_widget(ui, tree, runtime, softness_binding, |ui| {
        ui.add(egui::Slider::new(&mut softness, 0.0..=0.5).text("Group softness"))
    });
    let mut keyframes = Vec::new();
    if response.hovered() && ui.input(plain_i_pressed) {
        keyframes.push(KeyframeRequest {
            binding: softness_binding,
            value: softness,
        });
    }
    response
        .clone()
        .on_hover_text("Press I to insert a keyframe at the current frame");
    let softness_changed = response.changed();
    let changed = parent_operation_changed || group_operation_changed || softness_changed;
    ui.label("0 = sharp edges. Softness is measured in world units.");
    if response.drag_started() || parent_operation_changed || group_operation_changed {
        tree.make_undo_redo_snapshot();
    }
    if changed {
        for object in &mut scene {
            if parent_operation_changed && selection.contains(&object.uuid) {
                object.operation = parent_operation;
            }
            if group_operation_changed && direct_children.contains(&object.uuid) {
                object.operation = group_operation;
            }
            if softness_changed && object.uuid == softness_owner {
                object.softness = softness;
            }
        }
        set_objects(tree, scene);
    }
    if response.drag_stopped() || parent_operation_changed || group_operation_changed {
        tree.make_undo_redo_snapshot();
    }
    apply_keyframe_requests(tree, runtime, keyframes);
}

fn boolean_operation_combo(
    ui: &mut egui::Ui,
    label: &str,
    operation: &mut BooleanOperation,
    mixed: bool,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(if mixed { "Mixed" } else { operation.label() })
        .show_ui(ui, |ui| {
            for value in [
                BooleanOperation::Union,
                BooleanOperation::Subtract,
                BooleanOperation::Intersect,
            ] {
                changed |= ui
                    .selectable_value(operation, value, value.label())
                    .changed();
            }
        });
    changed
}

pub(super) fn repetition_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
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
    let mut changed = false;
    ui.label("Copies · 1 means off for that axis");
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

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) enum ResizeHandleId {
    BoxFace { axis: usize, positive: bool },
    SphereAxis(usize),
    CylinderRadius,
    CylinderHeight,
    TorusMajorRadius,
    TorusMinorRadius,
}

pub(super) struct ResizeHandle {
    pub(super) id: ResizeHandleId,
    pub(super) world: Vec3,
    pub(super) guide_origin: Vec3,
    pub(super) direction: Vec3,
    pub(super) local_direction: Vec3,
    pub(super) patch: Vec<Vec3>,
    pub(super) full_patch: Vec<Vec3>,
    pub(super) label: String,
    pub(super) color: Color32,
    pub(super) parameter: usize,
    pub(super) value: f32,
    pub(super) camera_facing: bool,
    pub(super) edge_on: bool,
}

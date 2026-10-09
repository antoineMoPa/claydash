use super::*;
mod rigid;
use crate::model::{
    objects_ref, scene_variables, set_scene_variables, vector_binding_frame, SceneVariables,
    VariableSpace, VectorBinding, VectorBindingTarget,
};

pub(super) fn variable_bindings_editor(ui: &mut egui::Ui, tree: &mut DataTree) {
    let before = scene_variables(tree);
    let mut variables = before.clone();
    let mut begin_edit = false;
    egui::CollapsingHeader::new("Variable bindings").show(ui, |ui| {
        if let [id] = selected(tree).as_slice() {
            let scene = objects_ref(tree);
            if let Some(object) = scene.iter().find(|object| object.uuid == *id) {
                ui.separator();
                ui.label("Links for selected object");
                begin_edit |= ui
                    .add_enabled_ui(
                        crate::model::rigid_binding(
                            &variables,
                            *id,
                            crate::model::RigidBindingTarget::Object,
                        )
                        .is_none(),
                        |ui| {
                            binding_editor(
                                ui,
                                &mut variables,
                                scene,
                                *id,
                                VectorBindingTarget::Position,
                                "Object position",
                                object.transform.translation,
                            )
                        },
                    )
                    .inner;
                if crate::model::has_boolean_children(scene, *id) {
                    begin_edit |= ui
                        .add_enabled_ui(
                            crate::model::rigid_binding(
                                &variables,
                                *id,
                                crate::model::RigidBindingTarget::Group,
                            )
                            .is_none(),
                            |ui| {
                                binding_editor(
                                    ui,
                                    &mut variables,
                                    scene,
                                    *id,
                                    VectorBindingTarget::GroupPosition,
                                    "Group position",
                                    object.group_transform.translation,
                                )
                            },
                        )
                        .inner;
                }
                let rigid_bound = crate::model::is_object_rigid_bound(&variables, *id);
                if let SdfParams::BezierCurveParams(params) = &object.params {
                    begin_edit |= ui
                        .add_enabled_ui(!rigid_bound, |ui| {
                            let mut begin_edit = false;
                            for (index, point) in params.points.iter().enumerate() {
                                begin_edit |= binding_editor(
                                    ui,
                                    &mut variables,
                                    scene,
                                    *id,
                                    VectorBindingTarget::BezierPoint(index),
                                    &format!("Curve point {}", index + 1),
                                    *point,
                                );
                            }
                            begin_edit
                        })
                        .inner;
                }
                if rigid_bound {
                    ui.label("Rigid transform driven by shared points");
                }
            }
        }
    });
    if begin_edit {
        tree.make_undo_redo_snapshot();
    }
    if variables != before {
        set_scene_variables(tree, variables);
    }
    rigid::editor(ui, tree);
}

fn binding_editor(
    ui: &mut egui::Ui,
    variables: &mut SceneVariables,
    scene: &[SdfObject],
    object: uuid::Uuid,
    target: VectorBindingTarget,
    label: &str,
    current: Vec3,
) -> bool {
    let previous = variables
        .bindings
        .iter()
        .find(|binding| binding.object == object && binding.target == target)
        .map(|binding| binding.variable);
    let mut choice = previous;
    let mut begin_edit = false;
    ui.push_id((object, label), |ui| {
        ui.horizontal(|ui| {
            ui.label(label);
            egui::ComboBox::from_id_salt("variable")
                .selected_text(
                    choice
                        .and_then(|id| variables.vectors.iter().find(|variable| variable.id == id))
                        .map(|variable| variable.name.as_str())
                        .unwrap_or("Unlinked"),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, "Unlinked");
                    for variable in &variables.vectors {
                        ui.selectable_value(&mut choice, Some(variable.id), &variable.name);
                    }
                });
        });
        if previous != choice {
            begin_edit = true;
            variables
                .bindings
                .retain(|binding| binding.object != object || binding.target != target);
            if let Some(variable) = choice {
                let value = variables
                    .vectors
                    .iter()
                    .find(|value| value.id == variable)
                    .unwrap();
                let current = match value.space {
                    VariableSpace::Local => current,
                    VariableSpace::World => {
                        vector_binding_frame(scene, object, target).transform_point3(current)
                    }
                };
                variables.bindings.push(VectorBinding {
                    object,
                    target,
                    variable,
                    offset: current - value.value,
                });
            }
        }
        if let Some(binding) = variables
            .bindings
            .iter_mut()
            .find(|binding| binding.object == object && binding.target == target)
        {
            ui.horizontal(|ui| {
                ui.label("Offset");
                for component in [
                    &mut binding.offset.x,
                    &mut binding.offset.y,
                    &mut binding.offset.z,
                ] {
                    let response = ui.add(egui::DragValue::new(component).speed(0.01));
                    begin_edit |= response.drag_started() || response.gained_focus();
                }
                if ui
                    .small_button("Use vector point")
                    .on_hover_text(
                        "Clear the offset so this point sits exactly at the shared vector",
                    )
                    .clicked()
                {
                    begin_edit = true;
                    binding.offset = Vec3::ZERO;
                }
            });
        }
    });
    begin_edit
}

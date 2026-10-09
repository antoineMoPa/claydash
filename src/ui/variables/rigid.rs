use super::*;
use crate::model::{
    evaluate_scene_variables, group_world_matrix, object_world_matrix, rigid_binding,
    rigid_binding_pose, RigidBinding, RigidBindingTarget, RigidBindingUp,
};

#[derive(Clone)]
struct Draft {
    applied: Option<RigidBinding>,
    binding: RigidBinding,
    captured: bool,
    error: Option<String>,
}

impl Draft {
    fn new(object: uuid::Uuid, target: RigidBindingTarget, applied: Option<RigidBinding>) -> Self {
        Self {
            binding: applied.clone().unwrap_or(RigidBinding {
                object,
                target,
                origin: uuid::Uuid::nil(),
                aim: uuid::Uuid::nil(),
                up: RigidBindingUp::WorldDirection(Vec3::Z),
                local_origin: Vec3::ZERO,
                local_aim: Vec3::X,
                local_up: Vec3::Z,
            }),
            captured: applied.is_some(),
            applied,
            error: None,
        }
    }
}

pub(super) fn editor(ui: &mut egui::Ui, tree: &mut DataTree) {
    let selection = selected(tree);
    let [object] = selection.as_slice() else {
        return;
    };
    let object = *object;
    let scene = objects_ref(tree).to_vec();
    let variables = scene_variables(tree);
    let target_id = ui.id().with(("rigid-target", object));
    let mut target = ui
        .ctx()
        .data(|d| d.get_temp::<RigidBindingTarget>(target_id))
        .unwrap_or(if commands::selected_group_id(tree) == Some(object) {
            RigidBindingTarget::Group
        } else {
            RigidBindingTarget::Object
        });
    egui::CollapsingHeader::new("Rigid transform")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Frame");
                ui.selectable_value(&mut target, RigidBindingTarget::Object, "Object");
                if crate::model::has_boolean_children(&scene, object) {
                    ui.selectable_value(&mut target, RigidBindingTarget::Group, "Group");
                }
            });
            let target_key = match target {
                RigidBindingTarget::Object => "object",
                RigidBindingTarget::Group => "group",
            };
            let draft_id = ui.id().with(("rigid-draft", object, target_key));
            let applied = rigid_binding(&variables, object, target).cloned();
            let mut draft = ui
                .ctx()
                .data(|d| d.get_temp::<Draft>(draft_id))
                .filter(|draft| draft.applied == applied)
                .unwrap_or_else(|| Draft::new(object, target, applied));
            let mut select = None;
            point_selector(
                ui,
                &variables,
                "Origin",
                &mut draft.binding.origin,
                &mut select,
            );
            point_selector(ui, &variables, "Aim", &mut draft.binding.aim, &mut select);
            ui.horizontal(|ui| {
                ui.label("Up");
                let point_mode = matches!(draft.binding.up, RigidBindingUp::Point(_));
                if ui.selectable_label(point_mode, "Point").clicked() && !point_mode {
                    draft.binding.up = RigidBindingUp::Point(uuid::Uuid::nil());
                }
                if ui
                    .selectable_label(!point_mode, "World direction")
                    .clicked()
                    && point_mode
                {
                    draft.binding.up = RigidBindingUp::WorldDirection(Vec3::Z);
                }
            });
            match &mut draft.binding.up {
                RigidBindingUp::Point(id) => {
                    point_selector(ui, &variables, "Up point", id, &mut select)
                }
                RigidBindingUp::WorldDirection(direction) => xyz(ui, "Direction", direction),
            }
            if let Some(id) = select {
                crate::model::set_selected_variable(tree, Some(id));
            }
            let complete = !draft.binding.origin.is_nil()
                && !draft.binding.aim.is_nil()
                && !matches!(draft.binding.up, RigidBindingUp::Point(id) if id.is_nil());
            if !draft.captured && complete {
                match capture(&variables, &scene, &mut draft.binding) {
                    Ok(()) => {
                        draft.captured = true;
                        draft.error = None;
                    }
                    Err(error) => draft.error = Some(error),
                }
            }
            egui::CollapsingHeader::new("Local anchors").show(ui, |ui| {
                xyz(ui, "Local origin", &mut draft.binding.local_origin);
                xyz(ui, "Local aim", &mut draft.binding.local_aim);
                xyz(ui, "Local up", &mut draft.binding.local_up);
                if ui
                    .add_enabled(complete, egui::Button::new("Capture current pose"))
                    .clicked()
                {
                    match capture(&variables, &scene, &mut draft.binding) {
                        Ok(()) => {
                            draft.captured = true;
                            draft.error = None;
                        }
                        Err(error) => draft.error = Some(error),
                    }
                }
            });
            let conflict = position_conflict(&variables, &draft.binding);
            if conflict {
                ui.label("Unlink this frame's position and curve points above before applying.");
            }
            ui.horizontal_wrapped(|ui| {
                let label = if draft.applied.is_some() {
                    "Apply rigid transform"
                } else {
                    "Add rigid transform"
                };
                let dirty = draft.applied.as_ref() != Some(&draft.binding);
                if ui
                    .add_enabled(complete && !conflict && dirty, egui::Button::new(label))
                    .clicked()
                {
                    match apply(tree, &draft.binding) {
                        Ok(()) => {
                            draft.applied = Some(draft.binding.clone());
                            draft.error = None;
                        }
                        Err(error) => draft.error = Some(error),
                    }
                }
                if draft.applied.is_some() && ui.button("Remove").clicked() {
                    remove(tree, object, target);
                    draft = Draft::new(object, target, None);
                }
                if dirty && draft.applied.is_some() && ui.button("Reset").clicked() {
                    draft = Draft::new(object, target, draft.applied.clone());
                }
            });
            if let Some(error) = &draft.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.ctx().data_mut(|d| d.insert_temp(draft_id, draft));
        });
    ui.ctx().data_mut(|d| d.insert_temp(target_id, target));
}

fn point_selector(
    ui: &mut egui::Ui,
    variables: &SceneVariables,
    label: &str,
    id: &mut uuid::Uuid,
    select: &mut Option<uuid::Uuid>,
) {
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.label(label);
            egui::ComboBox::from_id_salt("point")
                .width((ui.available_width() - 65.0).max(60.0))
                .selected_text(
                    variables
                        .vectors
                        .iter()
                        .find(|v| v.id == *id)
                        .map(|v| v.name.as_str())
                        .unwrap_or(
                            if variables
                                .vectors
                                .iter()
                                .any(|v| v.space == VariableSpace::World)
                            {
                                "Choose World point"
                            } else {
                                "No World points"
                            },
                        ),
                )
                .show_ui(ui, |ui| {
                    for point in variables
                        .vectors
                        .iter()
                        .filter(|v| v.space == VariableSpace::World)
                    {
                        ui.selectable_value(id, point.id, &point.name);
                    }
                });
            let response = ui.add_enabled(!id.is_nil(), egui::Button::new("Select"));
            if response.clicked() {
                *select = Some(*id);
                response.surrender_focus();
            }
        });
    });
}

fn xyz(ui: &mut egui::Ui, label: &str, value: &mut Vec3) {
    ui.push_id(label, |ui| {
        ui.label(label);
        ui.horizontal(|ui| {
            let width =
                ((ui.available_width() - ui.spacing().item_spacing.x * 2.0) / 3.0).max(30.0);
            for (axis, component) in [
                ("X ", &mut value.x),
                ("Y ", &mut value.y),
                ("Z ", &mut value.z),
            ] {
                ui.add_sized(
                    [width, 20.0],
                    egui::DragValue::new(component).prefix(axis).speed(0.01),
                );
            }
        });
    });
}

fn position_conflict(variables: &SceneVariables, binding: &RigidBinding) -> bool {
    let target = match binding.target {
        RigidBindingTarget::Object => VectorBindingTarget::Position,
        RigidBindingTarget::Group => VectorBindingTarget::GroupPosition,
    };
    variables.bindings.iter().any(|b| {
        b.object == binding.object
            && (b.target == target
                || (binding.target == RigidBindingTarget::Object
                    && matches!(b.target, VectorBindingTarget::BezierPoint(_))))
    })
}

fn capture(
    variables: &SceneVariables,
    scene: &[SdfObject],
    binding: &mut RigidBinding,
) -> Result<(), String> {
    let mut evaluated = variables.clone();
    evaluate_scene_variables(&mut evaluated)?;
    let point = |id| {
        evaluated
            .vectors
            .iter()
            .find(|v| v.id == id && v.space == VariableSpace::World)
            .map(|v| v.value)
            .ok_or_else(|| "Choose existing World points for the frame".to_string())
    };
    let origin = point(binding.origin)?;
    let aim = point(binding.aim)?;
    let up = match binding.up {
        RigidBindingUp::Point(id) => point(id)?,
        RigidBindingUp::WorldDirection(direction) => origin + direction,
    };
    let world = match binding.target {
        RigidBindingTarget::Object => object_world_matrix(scene, binding.object),
        RigidBindingTarget::Group => group_world_matrix(scene, binding.object),
    };
    let inverse = world.inverse();
    if !inverse.is_finite() {
        return Err("Cannot capture a singular or nonfinite frame".into());
    }
    let mut captured = binding.clone();
    captured.local_origin = inverse.transform_point3(origin);
    captured.local_aim = inverse.transform_point3(aim);
    captured.local_up = inverse.transform_point3(up);
    rigid_binding_pose(&evaluated, scene, &captured)?;
    *binding = captured;
    Ok(())
}

fn apply(tree: &mut DataTree, binding: &RigidBinding) -> Result<(), String> {
    let mut variables = scene_variables(tree);
    if position_conflict(&variables, binding) {
        return Err("Unlink the frame's position and curve points first".into());
    }
    variables
        .rigid_bindings
        .retain(|b| b.object != binding.object || b.target != binding.target);
    variables.rigid_bindings.push(binding.clone());
    evaluate_scene_variables(&mut variables)?;
    let mut scene = objects_ref(tree).to_vec();
    crate::model::apply_vector_bindings(&variables, &mut scene);
    for binding in &variables.rigid_bindings {
        rigid_binding_pose(&variables, &scene, binding)?;
    }
    tree.make_undo_redo_snapshot();
    set_scene_variables(tree, variables);
    tree.make_undo_redo_snapshot();
    Ok(())
}

fn remove(tree: &mut DataTree, object: uuid::Uuid, target: RigidBindingTarget) {
    let mut variables = scene_variables(tree);
    variables
        .rigid_bindings
        .retain(|b| b.object != object || b.target != target);
    tree.make_undo_redo_snapshot();
    set_scene_variables(tree, variables);
    tree.make_undo_redo_snapshot();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{set_objects, VectorVariable};

    fn setup() -> (DataTree, RigidBinding) {
        let mut tree = DataTree::default();
        let mut object = SdfObject::create(sdf_consts::TYPE_BOX);
        object.transform.translation = Vec3::new(3.0, 2.0, 1.0);
        object.transform.rotation = glam::Quat::from_rotation_z(0.4);
        object.transform.scale = Vec3::new(2.0, 3.0, 4.0);
        let id = object.uuid;
        set_objects(&mut tree, vec![object]);
        set_selected(&mut tree, vec![id]);
        let points: Vec<_> = [
            ("Origin point", Vec3::new(1.0, 2.0, 3.0)),
            ("Aim point", Vec3::new(4.0, 2.0, 3.0)),
            ("Up point", Vec3::new(1.0, 2.0, 5.0)),
        ]
        .into_iter()
        .map(|(name, value)| VectorVariable {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            value,
            space: VariableSpace::World,
        })
        .collect();
        let mut binding = Draft::new(id, RigidBindingTarget::Object, None).binding;
        binding.origin = points[0].id;
        binding.aim = points[1].id;
        binding.up = RigidBindingUp::Point(points[2].id);
        set_scene_variables(
            &mut tree,
            SceneVariables {
                vectors: points,
                ..Default::default()
            },
        );
        (tree, binding)
    }

    fn frame(
        ctx: &egui::Context,
        tree: &mut DataTree,
        events: Vec<egui::Event>,
    ) -> Vec<egui::epaint::ClippedShape> {
        ctx.style_mut_of(egui::Theme::Dark, |style| style.animation_time = 0.0);
        ctx.style_mut_of(egui::Theme::Light, |style| style.animation_time = 0.0);
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                ui.set_width(350.0);
                editor(ui, tree);
            },
        );
        output.textures_delta.clear();
        output.shapes
    }

    fn click(ctx: &egui::Context, tree: &mut DataTree, label: &str) {
        let position = frame(ctx, tree, vec![])
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing rigid control: {label}"));
        for pressed in [true, false] {
            frame(
                ctx,
                tree,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn capture_apply_invalid_draft_remove_and_undo_preserve_pose() {
        let (mut tree, mut binding) = setup();
        let original = objects_ref(&tree)[0].transform;
        capture(&scene_variables(&tree), objects_ref(&tree), &mut binding).unwrap();
        apply(&mut tree, &binding).unwrap();
        let pose = objects_ref(&tree)[0].transform;
        assert!(pose.translation.distance(original.translation) < 1e-5);
        assert!(pose.rotation.dot(original.rotation).abs() > 0.99999);
        assert_eq!(pose.scale, original.scale);
        let valid = scene_variables(&tree);
        binding.local_aim = binding.local_origin;
        assert!(apply(&mut tree, &binding).is_err());
        assert_eq!(scene_variables(&tree), valid);
        remove(&mut tree, binding.object, binding.target);
        assert_eq!(objects_ref(&tree)[0].transform, pose);
        assert!(scene_variables(&tree).rigid_bindings.is_empty());
        tree.undo();
        assert_eq!(scene_variables(&tree), valid);
    }

    #[test]
    fn inspector_select_remove_and_undo_resync_live_binding() {
        let (mut tree, mut binding) = setup();
        capture(&scene_variables(&tree), objects_ref(&tree), &mut binding).unwrap();
        apply(&mut tree, &binding).unwrap();
        let ctx = egui::Context::default();
        click(&ctx, &mut tree, "Select");
        assert_eq!(selected(&tree), vec![binding.object]);
        assert_eq!(crate::model::selected_variable(&tree), Some(binding.origin));
        assert!(ctx.memory(|m| m.focused()).is_none());
        click(&ctx, &mut tree, "Remove");
        assert!(scene_variables(&tree).rigid_bindings.is_empty());
        tree.undo();
        let shapes = frame(&ctx, &mut tree, vec![]);
        assert!(shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Apply rigid transform")));
    }

    #[test]
    fn position_conflict_does_not_delete_unrelated_links() {
        let (mut tree, mut binding) = setup();
        capture(&scene_variables(&tree), objects_ref(&tree), &mut binding).unwrap();
        let mut variables = scene_variables(&tree);
        variables.bindings.push(VectorBinding {
            object: binding.object,
            target: VectorBindingTarget::Position,
            variable: binding.origin,
            offset: Vec3::ZERO,
        });
        set_scene_variables(&mut tree, variables.clone());
        assert!(apply(&mut tree, &binding).is_err());
        assert_eq!(scene_variables(&tree), variables);
    }
    #[test]
    fn inspector_edits_up_and_local_anchors_and_rejects_collinear_references() {
        let (mut tree, mut binding) = setup();
        capture(&scene_variables(&tree), objects_ref(&tree), &mut binding).unwrap();
        apply(&mut tree, &binding).unwrap();
        let ctx = egui::Context::default();
        click(&ctx, &mut tree, "World direction");
        click(&ctx, &mut tree, "Apply rigid transform");
        assert!(matches!(
            scene_variables(&tree).rigid_bindings[0].up,
            RigidBindingUp::WorldDirection(_)
        ));
        click(&ctx, &mut tree, "Local anchors");
        click(&ctx, &mut tree, "Capture current pose");
        let shapes = frame(&ctx, &mut tree, vec![]);
        let position = shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text().starts_with("X ") => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                _ => None,
            })
            .nth(1)
            .expect("local origin X control");
        for events in [
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerMoved(position + egui::vec2(30.0, 0.0))],
            vec![egui::Event::PointerMoved(position + egui::vec2(60.0, 0.0))],
            vec![egui::Event::PointerButton {
                pos: position + egui::vec2(60.0, 0.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            frame(&ctx, &mut tree, events);
        }
        click(&ctx, &mut tree, "Apply rigid transform");
        assert_ne!(
            scene_variables(&tree).rigid_bindings[0].local_origin,
            binding.local_origin
        );
        let valid = scene_variables(&tree);
        click(&ctx, &mut tree, "Aim point");
        let shapes = frame(&ctx, &mut tree, vec![]);
        let position = shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Origin point" => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                _ => None,
            })
            .last()
            .expect("Origin point menu item");
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut tree,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        click(&ctx, &mut tree, "Apply rigid transform");
        assert_eq!(scene_variables(&tree), valid);
        let shapes = frame(&ctx, &mut tree, vec![]);
        assert!(shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("collinear") || t.galley.text().contains("heading"))));
    }
    #[test]
    fn inspector_creates_binding_from_named_world_points_and_undo_resets_draft() {
        let (mut tree, binding) = setup();
        let original = objects_ref(&tree)[0].transform;
        let ctx = egui::Context::default();
        click(&ctx, &mut tree, "Choose World point");
        click(&ctx, &mut tree, "Origin point");
        click(&ctx, &mut tree, "Choose World point");
        click(&ctx, &mut tree, "Aim point");
        click(&ctx, &mut tree, "Add rigid transform");
        let actual = scene_variables(&tree).rigid_bindings[0].clone();
        assert_eq!(actual.origin, binding.origin);
        assert_eq!(actual.aim, binding.aim);
        assert!(
            objects_ref(&tree)[0]
                .transform
                .translation
                .distance(original.translation)
                < 1e-5
        );
        tree.undo();
        assert!(scene_variables(&tree).rigid_bindings.is_empty());
        let shapes = frame(&ctx, &mut tree, vec![]);
        assert_eq!(shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Choose World point")).count(), 2);
    }
    #[test]
    fn inspector_without_points_keeps_rigid_card_visible() {
        let (mut tree, _) = setup();
        set_scene_variables(&mut tree, SceneVariables::default());
        let ctx = egui::Context::default();
        let shapes = frame(&ctx, &mut tree, vec![]);
        assert!(shapes.iter().any(
            |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Rigid transform")
        ));
        assert_eq!(shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "No World points")).count(), 2);
        click(&ctx, &mut tree, "Select");
        assert_eq!(crate::model::selected_variable(&tree), None);
        click(&ctx, &mut tree, "Add rigid transform");
        assert!(scene_variables(&tree).rigid_bindings.is_empty());
    }
}

use super::*;

#[test]
fn selected_group_exposes_move_rotate_and_scale_gizmos() {
    let ctx = egui::Context::default();
    let camera = camera();
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    let root = SdfObject::create(sdf_consts::TYPE_BOX);
    let mut child = SdfObject::create(sdf_consts::TYPE_SPHERE);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation = Vec3::X;
    set_objects(&mut tree, vec![root.clone(), child]);
    set_selected(&mut tree, vec![root.uuid]);

    draw(&mut state, &ctx, &mut tree, &camera, vec![]);
    assert!(state.regions.len() > 3);
    assert_eq!(commands::transform_targets(&tree).len(), 1);
    assert_eq!(
        commands::transform_targets(&tree)[0].kind,
        commands::TransformTargetKind::Group
    );
}

#[test]
fn transform_gizmo_matches_the_mockup_handle_layout() {
    let mut camera = camera();
    camera.position = Vec3::new(3.0, 2.0, 8.0);
    let geometry = selection_gizmo_geometry(&camera, Vec3::ZERO, 1.0).unwrap();

    assert_eq!(geometry.axes.len(), 3);
    assert_eq!(geometry.rotation_arcs.len(), 3);
    assert!(geometry
        .rotation_arcs
        .iter()
        .all(|arc| arc.points.len() == 33));
    for axis in &geometry.axes {
        assert!(axis.scale_handle.distance(geometry.center) < ROTATION_RING_RADIUS);
        assert_eq!(
            gizmo_action_at(axis.move_tip, &geometry),
            Some(GizmoAction::MoveAxis(axis.axis))
        );
        assert_eq!(
            gizmo_action_at(axis.scale_handle, &geometry),
            Some(GizmoAction::ScaleAxis(axis.axis))
        );
    }
    assert_eq!(
        gizmo_action_at(geometry.uniform_scale, &geometry),
        Some(GizmoAction::ScaleUniform)
    );
    assert_eq!(
        gizmo_action_at(geometry.free_move, &geometry),
        Some(GizmoAction::MoveFree)
    );
    assert_eq!(
        gizmo_action_at(geometry.view_rotate, &geometry),
        Some(GizmoAction::RotateView)
    );

    let former_free_move_offset = egui::vec2(34.0, -64.0).length();
    let new_free_move_offset = geometry.free_move.distance(geometry.center);
    assert!((new_free_move_offset - former_free_move_offset * 1.25).abs() < 0.001);
    let free_move_offset = geometry.free_move - geometry.center;
    assert!((free_move_offset.x + free_move_offset.y).abs() < 0.001);
    assert!((geometry.free_move.y - geometry.uniform_scale.y).abs() < 0.001);
    assert!(geometry.view_ring[16].distance(geometry.view_rotate) < 0.001);
    let view_arc_start = geometry.view_ring[0] - geometry.center;
    let view_arc_end = geometry.view_ring[32] - geometry.center;
    let view_arc_span =
        (view_arc_end.y.atan2(view_arc_end.x) - view_arc_start.y.atan2(view_arc_start.x)).abs();
    assert!((view_arc_span - 45.0_f32.to_radians()).abs() < 0.001);

    let idle_red = gizmo_color(axis_color(0), false);
    let hovered_red = gizmo_color(axis_color(0), true);
    assert_eq!(idle_red.a(), 180);
    assert_eq!(hovered_red.a(), 255);
    let base_angle = VIEW_ROTATE_ANGLE_DEGREES.to_radians();
    assert!((view_rotation_gizmo_angle(0.2, 0.6, false) - (base_angle + 0.4)).abs() < 0.001);
    assert!(
        (view_rotation_gizmo_angle(0.0, 7.0_f32.to_radians(), true)
            - (base_angle + 5.0_f32.to_radians()))
        .abs()
            < 0.001
    );

    let z_rotation_arc = &geometry.rotation_arcs[2];
    let y_axis = geometry.axes.iter().find(|axis| axis.axis == 1).unwrap();
    let z_arc_middle_direction = z_rotation_arc.points[16] - geometry.center;
    assert!(z_arc_middle_direction.dot(y_axis.direction) > 0.0);
    assert!(
        distance_to_segment(
            z_rotation_arc.points[16],
            geometry.center,
            geometry.center + y_axis.direction * MOVE_HANDLE_DISTANCE,
        ) < 0.001
    );
}

#[test]
fn axis_move_handle_only_moves_along_its_colored_axis() {
    let ctx = egui::Context::default();
    let camera = camera();
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    let object = SdfObject::create(sdf_consts::TYPE_BOX);
    set_objects(&mut tree, vec![object.clone()]);
    set_selected(&mut tree, vec![object.uuid]);
    tree.make_undo_redo_snapshot();
    let geometry = selection_gizmo_geometry(&camera, Vec3::ZERO, 1.0).unwrap();
    let x_axis = geometry.axes.iter().find(|axis| axis.axis == 0).unwrap();
    let start = x_axis.move_tip;
    let end = start + x_axis.direction * 50.0;
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };

    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(end), button(end, false)],
    );

    let moved = objects(&tree)[0].transform.translation;
    assert!(moved.x > 0.0);
    assert!(moved.y.abs() < 0.0001);
    assert!(moved.z.abs() < 0.0001);
}

#[test]
fn axis_scale_handle_only_scales_its_colored_axis() {
    let ctx = egui::Context::default();
    let camera = camera();
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    let object = SdfObject::create(sdf_consts::TYPE_BOX);
    set_objects(&mut tree, vec![object.clone()]);
    set_selected(&mut tree, vec![object.uuid]);
    let geometry = selection_gizmo_geometry(&camera, Vec3::ZERO, 1.0).unwrap();
    let y_axis = geometry.axes.iter().find(|axis| axis.axis == 1).unwrap();
    let start = y_axis.scale_handle;
    let end = start + y_axis.direction * SCALE_HANDLE_DISTANCE;
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };

    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(end), button(end, false)],
    );

    let scaled = objects(&tree)[0].transform.scale;
    assert!((scaled.x - 1.0).abs() < 0.0001);
    assert!((scaled.y - 2.0).abs() < 0.0001);
    assert!((scaled.z - 1.0).abs() < 0.0001);
}

#[test]
fn colored_rotation_ring_rotates_around_its_axis() {
    let ctx = egui::Context::default();
    let mut camera = camera();
    camera.position = Vec3::new(3.0, 2.0, 8.0);
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    let object = SdfObject::create(sdf_consts::TYPE_BOX);
    set_objects(&mut tree, vec![object.clone()]);
    set_selected(&mut tree, vec![object.uuid]);
    let geometry = selection_gizmo_geometry(&camera, Vec3::ZERO, 1.0).unwrap();
    let start = geometry.rotation_arcs[0]
        .points
        .iter()
        .copied()
        .find(|point| gizmo_action_at(*point, &geometry) == Some(GizmoAction::RotateAxis(0)))
        .expect("an exposed section of the X rotation ring");
    let delta = start - geometry.center;
    let angle = 0.45_f32;
    let end = geometry.center
        + egui::vec2(
            delta.x * angle.cos() - delta.y * angle.sin(),
            delta.x * angle.sin() + delta.y * angle.cos(),
        );
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };

    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(end), button(end, false)],
    );

    let rotation = objects(&tree)[0].transform.rotation;
    assert_ne!(rotation, glam::Quat::IDENTITY);
    assert!((rotation * Vec3::X).distance(Vec3::X) < 0.0001);
}

#[test]
fn reverse_x_view_uses_world_space_axis_rotation_direction() {
    let ctx = egui::Context::default();
    let mut camera = camera();
    camera.position = Vec3::NEG_X * 8.0;
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    let object = SdfObject::create(sdf_consts::TYPE_BOX);
    set_objects(&mut tree, vec![object.clone()]);
    set_selected(&mut tree, vec![object.uuid]);
    let geometry = selection_gizmo_geometry(&camera, Vec3::ZERO, 1.0).unwrap();
    let x_arc = &geometry.rotation_arcs[0];
    let start = x_arc.points[16];
    assert_eq!(
        gizmo_action_at(start, &geometry),
        Some(GizmoAction::RotateAxis(0))
    );
    let world_angle = 20.0_f32.to_radians();
    let end = camera
        .project(
            x_arc.u * (x_arc.middle_angle + world_angle).cos() * geometry.world_ring_radius
                + x_arc.v * (x_arc.middle_angle + world_angle).sin() * geometry.world_ring_radius,
            1.0,
        )
        .unwrap();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };

    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(end), button(end, false)],
    );

    let rotation = objects(&tree)[0].transform.rotation;
    let expected_y = glam::Quat::from_axis_angle(Vec3::X, world_angle) * Vec3::Y;
    assert!((rotation * Vec3::Y).distance(expected_y) < 0.001);
}

#[test]
fn viewport_rotation_follows_the_mouse_in_screen_space() {
    let ctx = egui::Context::default();
    let camera = camera();
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    let object = SdfObject::create(sdf_consts::TYPE_BOX);
    set_objects(&mut tree, vec![object.clone()]);
    set_selected(&mut tree, vec![object.uuid]);
    let geometry = selection_gizmo_geometry(&camera, Vec3::ZERO, 1.0).unwrap();
    let start = geometry.view_rotate;
    let delta = start - geometry.center;
    let pointer_angle = 0.45_f32;
    let end = geometry.center
        + egui::vec2(
            delta.x * pointer_angle.cos() - delta.y * pointer_angle.sin(),
            delta.x * pointer_angle.sin() + delta.y * pointer_angle.cos(),
        );
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };

    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(end), button(end, false)],
    );

    let rotation = objects(&tree)[0].transform.rotation;
    let center_screen = camera.project(Vec3::ZERO, 1.0).unwrap();
    let rotated_x = camera.project(rotation * Vec3::X, 1.0).unwrap() - center_screen;
    let screen_angle = rotated_x.y.atan2(rotated_x.x);
    assert!((screen_angle - pointer_angle).abs() < 0.001);
}

#[test]
fn viewport_rotation_uses_the_3d_cursor_pivot() {
    let ctx = egui::Context::default();
    let camera = camera();
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    let mut object = SdfObject::create(sdf_consts::TYPE_BOX);
    object.transform.translation = Vec3::X;
    set_objects(&mut tree, vec![object.clone()]);
    set_selected(&mut tree, vec![object.uuid]);
    tree.set_path(
        "editor.rotation_pivot",
        ClaydashValue::RotationPivot(crate::model::RotationPivot::Cursor),
    );
    tree.set_path("scene.cursor_position", ClaydashValue::Vec3(Vec3::Z));
    let geometry = selection_gizmo_geometry(&camera, Vec3::X, 1.0).unwrap();
    let start = geometry.view_rotate;
    let delta = start - geometry.center;
    let end = geometry.center + egui::vec2(-delta.y, delta.x);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };

    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(end), button(end, false)],
    );

    let object = &objects(&tree)[0];
    assert!(object.transform.translation.distance(Vec3::NEG_Y) < 0.001);
    assert!((object.transform.rotation * Vec3::X).distance(Vec3::NEG_Y) < 0.001);
}

#[test]
fn selected_variable_uses_regular_move_arrows_and_free_control() {
    use crate::model::{
        scene_variables, set_scene_variables, set_selected_variable, SceneVariables, VariableSpace,
        VectorVariable,
    };
    let ctx = egui::Context::default();
    let mut camera = camera();
    camera.position = Vec3::new(3.0, 2.0, 8.0);
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    let variable = VectorVariable {
        id: uuid::Uuid::new_v4(),
        name: "Point".into(),
        value: Vec3::ZERO,
        space: VariableSpace::World,
    };
    set_scene_variables(
        &mut tree,
        SceneVariables {
            vectors: vec![variable.clone()],
            bindings: vec![],
            ..SceneVariables::default()
        },
    );
    set_selected_variable(&mut tree, Some(variable.id));
    let geometry = selection_gizmo_geometry(&camera, Vec3::ZERO, 1.0).unwrap();
    let start = geometry
        .axes
        .iter()
        .find(|axis| axis.axis == 0)
        .unwrap()
        .move_tip;
    let end = start
        + geometry
            .axes
            .iter()
            .find(|axis| axis.axis == 0)
            .unwrap()
            .direction
            * 30.0;
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let mut full_frame = |events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                state.regions.clear();
                state.draw_variable_gizmos(ui, &mut tree, &camera, 0);
                state.draw_selection_tools(ui, &mut tree, &camera);
            },
        );
        output.textures_delta.clear();
    };
    full_frame(vec![]);
    full_frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
    full_frame(vec![egui::Event::PointerMoved(end)]);
    full_frame(vec![button(end, false)]);
    drop(full_frame);
    assert_eq!(crate::model::selected_variable(&tree), Some(variable.id));
    let value = scene_variables(&tree).vectors[0].value;
    assert!(value.x.abs() > 0.1);
    assert_eq!(value.y, 0.0);
    assert_eq!(value.z, 0.0);
    let updated_geometry = selection_gizmo_geometry(&camera, value, 1.0).unwrap();
    assert!(state
        .regions
        .iter()
        .any(|region| region.contains(updated_geometry.free_move)));
    // Scale/rotation handles are absent from the point's transform control.
    assert!(!state
        .regions
        .iter()
        .any(|region| region.contains(updated_geometry.uniform_scale)));
    tree.undo();
    assert_eq!(scene_variables(&tree).vectors[0].value, Vec3::ZERO);
}

fn linkage_fixture() -> (DataTree, uuid::Uuid, uuid::Uuid, uuid::Uuid) {
    use crate::model::*;
    let mut tree = DataTree::default();
    let point = |name: &str, value| VectorVariable {
        id: uuid::Uuid::new_v4(),
        name: name.into(),
        value,
        space: VariableSpace::World,
    };
    let driver = point("Wheel travel", Vec3::X);
    let lower_pivot = point("Lower pivot", Vec3::ZERO);
    let upper_pivot = point("Upper pivot", Vec3::Y);
    let lower_joint = point("Lower joint", Vec3::X);
    let upper_joint = point("Upper joint", Vec3::X + Vec3::Y);
    let object = SdfObject::create(sdf_consts::TYPE_BOX);
    set_objects(&mut tree, vec![object.clone()]);
    set_scene_variables(
        &mut tree,
        SceneVariables {
            vectors: vec![
                driver.clone(),
                lower_pivot.clone(),
                upper_pivot.clone(),
                lower_joint.clone(),
                upper_joint.clone(),
            ],
            rigid_bindings: vec![RigidBinding {
                object: object.uuid,
                target: RigidBindingTarget::Object,
                origin: lower_pivot.id,
                aim: lower_joint.id,
                up: RigidBindingUp::WorldDirection(Vec3::Z),
                local_origin: Vec3::ZERO,
                local_aim: Vec3::X,
                local_up: Vec3::Z,
            }],
            four_bar_constraints: vec![PlanarFourBarConstraint {
                driver: driver.id,
                lower_pivot: lower_pivot.id,
                upper_pivot: upper_pivot.id,
                lower_joint: lower_joint.id,
                upper_joint: upper_joint.id,
                lower_length: 1.0,
                upper_length: 1.0,
                upright_length: 1.0,
                driver_y_offset: 0.0,
                travel_min: -0.4,
                travel_max: 0.4,
                branch: CircleIntersectionBranch::Positive,
            }],
            ..SceneVariables::default()
        },
    );
    (tree, driver.id, lower_joint.id, object.uuid)
}

#[test]
fn derived_point_grab_cannot_fall_through_to_selected_object() {
    let (mut tree, _, derived, _) = linkage_fixture();
    let free_object = SdfObject::create(sdf_consts::TYPE_SPHERE);
    let object = free_object.uuid;
    let mut scene = objects(&tree);
    scene.push(free_object);
    set_objects(&mut tree, scene);
    crate::model::set_selected_exact(&mut tree, vec![object]);
    crate::model::set_selected_variable(&mut tree, Some(derived));
    // Keep an object selected as well, matching possible stale selection state.
    tree.set_transient_path("scene.selected_uuids", ClaydashValue::VecUuid(vec![object]));
    assert_eq!(selected(&tree), vec![object]);
    let before = objects(&tree);
    assert!(commands::transform_targets(&tree).is_empty());
    let mut registered = Commands::new();
    commands::register_all(&mut registered);
    let mut interaction = crate::interactions::InteractionState::default();
    let mut test_camera = camera();
    interaction.mouse_position = test_camera.viewport / 2.0;
    interaction.key_pressed(
        winit::keyboard::KeyCode::KeyG,
        false,
        &registered,
        &mut tree,
    );
    interaction.update(&mut test_camera, &mut tree);
    interaction.mouse_position += Vec2::new(40.0, 30.0);
    interaction.update(&mut test_camera, &mut tree);
    assert!(!matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(crate::model::EditorState::Grabbing)
    ));
    let ctx = egui::Context::default();
    let mut state = UiState::default();
    draw(&mut state, &ctx, &mut tree, &camera(), vec![]);
    assert!(
        state.regions.is_empty(),
        "derived points must have no transform gizmo"
    );
    assert_eq!(
        serde_json::to_value(objects(&tree)).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(crate::model::selected_variable(&tree), Some(derived));
}

#[test]
fn rigid_bound_object_has_no_transform_gizmo_or_keyboard_edits() {
    let (mut tree, _, _, object) = linkage_fixture();
    crate::model::set_selected_exact(&mut tree, vec![object]);
    assert!(commands::transform_targets(&tree).is_empty());
    for start in [
        commands::start_grab,
        commands::start_rotate,
        commands::start_scale,
    ] {
        start(&mut tree);
        assert!(!matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(
                crate::model::EditorState::Grabbing
                    | crate::model::EditorState::Rotating
                    | crate::model::EditorState::Scaling
            )
        ));
    }
    let ctx = egui::Context::default();
    let mut state = UiState::default();
    draw(&mut state, &ctx, &mut tree, &camera(), vec![]);
    assert!(state.regions.is_empty());
}

#[test]
fn driver_grab_updates_linkage_points_and_rigid_pose() {
    let (mut tree, driver, derived, object) = linkage_fixture();
    crate::model::set_selected_variable(&mut tree, Some(driver));
    let before = objects(&tree)[0].transform;
    let mut camera = camera();
    let mut interaction = crate::interactions::InteractionState::default();
    interaction.mouse_position = camera.viewport / 2.0;
    let mut registered = Commands::new();
    commands::register_all(&mut registered);
    interaction.key_pressed(
        winit::keyboard::KeyCode::KeyG,
        false,
        &registered,
        &mut tree,
    );
    tree.set_path("editor.constrain_y", ClaydashValue::Bool(true));
    interaction.update(&mut camera, &mut tree);
    interaction.mouse_position += Vec2::new(0.0, -30.0);
    interaction.update(&mut camera, &mut tree);
    let variables = crate::model::scene_variables(&tree);
    let driver_value = variables
        .vectors
        .iter()
        .find(|v| v.id == driver)
        .unwrap()
        .value;
    let derived_value = variables
        .vectors
        .iter()
        .find(|v| v.id == derived)
        .unwrap()
        .value;
    assert!(driver_value.y.abs() > 0.01);
    assert!((derived_value.y - driver_value.y).abs() < 1e-5);
    assert!((derived_value.length() - 1.0).abs() < 1e-5);
    let pose = objects(&tree)
        .into_iter()
        .find(|v| v.uuid == object)
        .unwrap()
        .transform;
    assert_ne!(pose.rotation, before.rotation);
    assert!((pose.rotation * Vec3::X).distance(derived_value) < 1e-5);
}

#[test]
fn invalid_pivot_drag_keeps_last_valid_linkage_values() {
    let (tree, _, _, _) = linkage_fixture();
    let mut variables = crate::model::scene_variables(&tree);
    let pivot = variables.four_bar_constraints[0].lower_pivot;
    let before = variables.clone();
    let mut scene = objects(&tree);
    let mut cameras = crate::model::scene_cameras(&tree);
    let changed = commands::set_transform_target(
        &mut scene,
        &mut cameras,
        &mut variables,
        commands::TransformTargetKind::Variable,
        pivot,
        crate::model::Transform {
            translation: Vec3::new(0.0, 20.0, 0.0),
            ..Default::default()
        },
    );
    assert!(!changed);
    assert_eq!(variables, before);
}

#[test]
fn rigid_group_lock_matches_group_selection_scope() {
    let (mut tree, _, _, object) = linkage_fixture();
    let mut scene = objects(&tree);
    let mut child = SdfObject::create(sdf_consts::TYPE_SPHERE);
    child.boolean_parent = Some(object);
    scene.push(child);
    set_objects(&mut tree, scene);
    let mut variables = crate::model::scene_variables(&tree);
    variables.rigid_bindings[0].target = crate::model::RigidBindingTarget::Group;
    crate::model::set_scene_variables(&mut tree, variables);
    set_selected(&mut tree, vec![object]);
    assert!(commands::transform_targets(&tree).is_empty());
    commands::start_grab(&mut tree);
    assert!(!matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(crate::model::EditorState::Grabbing)
    ));
    // Exact scope still edits the root primitive, whose own transform is unbound.
    crate::model::set_selected_exact(&mut tree, vec![object]);
    assert_eq!(commands::transform_targets(&tree).len(), 1);
    assert_eq!(
        commands::transform_targets(&tree)[0].kind,
        commands::TransformTargetKind::Object
    );
}

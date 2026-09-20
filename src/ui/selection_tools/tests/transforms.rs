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

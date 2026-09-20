use super::*;
use crate::camera::ProjectionMode;

fn camera() -> Camera {
    let mut camera = Camera::new();
    camera.position = Vec3::new(0.0, 0.0, 8.0);
    camera.viewport = Vec2::new(800.0, 600.0);
    camera
}

#[test]
fn box_selection_shortcut_enters_mode_only_while_idle() {
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );

    assert!(state.enter_box_selection_mode(&tree));
    assert!(state.selection_tools.box_mode());

    state.selection_tools.tool = SelectionTool::Select;
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Grabbing),
    );
    assert!(!state.enter_box_selection_mode(&tree));
    assert!(!state.selection_tools.box_mode());
}

#[test]
fn box_select_handles_scale_direction_groups_and_hidden_origins() {
    let mut camera = camera();
    let root = SdfObject::create(sdf_consts::TYPE_BOX);
    let mut child = SdfObject::create(sdf_consts::TYPE_SPHERE);
    child.boolean_parent = Some(root.uuid);
    let other = SdfObject::create(sdf_consts::TYPE_BOX);
    let mut behind = SdfObject::create(sdf_consts::TYPE_BOX);
    behind.transform.translation = Vec3::new(0.0, 0.0, 10.0);
    let scene = vec![root.clone(), child, other.clone(), behind];
    for mode in [ProjectionMode::Perspective, ProjectionMode::Orthographic] {
        camera.projection_mode = mode;
        for scale in [1.0, 2.0] {
            for (a, b) in [
                (egui::pos2(350.0, 250.0), egui::pos2(450.0, 350.0)),
                (egui::pos2(450.0, 250.0), egui::pos2(350.0, 350.0)),
            ] {
                for (start, end) in [(a, b), (b, a)] {
                    let rect = egui::Rect::from_two_pos(start / scale, end / scale);
                    assert_eq!(
                        box_selection(&scene, &camera, rect, scale, vec![]),
                        vec![root.uuid, other.uuid]
                    );
                    assert_eq!(
                        box_selection(&scene, &camera, rect, scale, vec![other.uuid]),
                        vec![other.uuid, root.uuid]
                    );
                }
            }
        }
    }
    let empty = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(10.0, 10.0));
    assert!(box_selection(&scene, &camera, empty, 1.0, vec![]).is_empty());
    assert_eq!(
        box_selection(&scene, &camera, empty, 1.0, vec![root.uuid]),
        vec![root.uuid]
    );
}

#[test]
fn movement_stays_in_camera_plane_in_both_projections() {
    let mut camera = camera();
    for mode in [ProjectionMode::Perspective, ProjectionMode::Orthographic] {
        camera.projection_mode = mode;
        let start = camera.cursor_on_plane(Vec2::new(400.0, 300.0), Vec3::ZERO);
        let end = camera.cursor_on_plane(Vec2::new(530.0, 240.0), Vec3::ZERO);
        assert!((end - start).dot(camera.target - camera.position).abs() < 0.0001);
        assert!(end.x > start.x && end.y > start.y);
    }
}

fn draw(
    state: &mut UiState,
    ctx: &egui::Context,
    tree: &mut DataTree,
    camera: &Camera,
    events: Vec<egui::Event>,
) {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            focused: true,
            ..Default::default()
        },
        |ui| {
            state.regions.clear();
            state.draw_selection_tools(ui, tree, camera);
        },
    );
    output.textures_delta.clear();
}

#[test]
fn box_gesture_commits_on_release_and_escape_keeps_selection() {
    let ctx = egui::Context::default();
    let camera = camera();
    let mut state = UiState::default();
    state.selection_tools.tool = SelectionTool::Box;
    let mut tree = DataTree::default();
    let object = SdfObject::create(sdf_consts::TYPE_BOX);
    set_objects(&mut tree, vec![object.clone()]);
    let start = egui::pos2(300.0, 200.0);
    let end = egui::pos2(500.0, 400.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::default(),
    };
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    assert!(state.selection_gesture_active());
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![egui::Event::PointerMoved(end)],
    );
    assert!(selected(&tree).is_empty());
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![button(end, false)],
    );
    assert_eq!(selected(&tree), vec![object.uuid]);
    assert!(state.selection_tools.box_mode());
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
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }],
    );
    draw(
        &mut state,
        &ctx,
        &mut tree,
        &camera,
        vec![button(start, false)],
    );
    assert_eq!(selected(&tree), vec![object.uuid]);
    assert!(!state.selection_gesture_active());
}

#[test]
fn tiny_box_returns_to_select_and_background_click_clears_selection() {
    for (delta, tiny) in [
        (egui::vec2(0.0, 0.0), true),
        (egui::vec2(2.9, 0.0), true),
        (egui::vec2(-2.0, -2.0), true),
        (egui::vec2(3.0, 0.0), false),
        (egui::vec2(0.0, 3.1), false),
    ] {
        let ctx = egui::Context::default();
        let camera = camera();
        let mut state = UiState::default();
        state.selection_tools.tool = SelectionTool::Box;
        let mut tree = DataTree::default();
        let object = SdfObject::create(sdf_consts::TYPE_BOX);
        set_objects(&mut tree, vec![object.clone()]);
        set_selected(&mut tree, vec![object.uuid]);
        let start = egui::pos2(100.0, 100.0);
        let end = start + delta;
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
        assert!(state.selection_tools.box_mode());
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![egui::Event::PointerMoved(end), button(end, false)],
        );
        assert_eq!(state.selection_tools.box_mode(), !tiny);
        assert!(!state.selection_gesture_active());
        assert!(selected(&tree).is_empty());
    }
}

#[test]
fn tiny_box_picks_object_like_regular_click_with_shift() {
    for shift in [false, true] {
        let ctx = egui::Context::default();
        let camera = camera();
        let mut state = UiState::default();
        state.selection_tools.tool = SelectionTool::Box;
        let mut tree = DataTree::default();
        let target = SdfObject::create(sdf_consts::TYPE_BOX);
        let mut other = SdfObject::create(sdf_consts::TYPE_BOX);
        other.transform.translation = Vec3::X * 3.0;
        set_objects(&mut tree, vec![target.clone(), other.clone()]);
        set_selected(&mut tree, vec![other.uuid]);
        let start = egui::pos2(400.0, 300.0);
        state.selection_tools.gesture = Some(Gesture::Box {
            start,
            end: start,
            additive: shift,
        });
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![
                egui::Event::PointerMoved(start + egui::vec2(1.0, 0.0)),
                egui::Event::PointerButton {
                    pos: start + egui::vec2(1.0, 0.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(!state.selection_tools.box_mode());
        assert_eq!(
            selected(&tree),
            if shift {
                vec![other.uuid, target.uuid]
            } else {
                vec![target.uuid]
            }
        );
    }
}

#[test]
fn move_gesture_cancel_and_commit_preserve_children_and_undo() {
    let ctx = egui::Context::default();
    let camera = camera();
    let mut state = UiState::default();
    let mut tree = DataTree::default();
    let root = SdfObject::create(sdf_consts::TYPE_BOX);
    let mut child = SdfObject::create(sdf_consts::TYPE_SPHERE);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation = Vec3::X;
    let originals = vec![root.clone(), child.clone()];
    set_objects(&mut tree, originals.clone());
    set_selected(&mut tree, vec![root.uuid, child.uuid]);
    tree.make_undo_redo_snapshot();
    for cancel in [true, false] {
        draw(&mut state, &ctx, &mut tree, &camera, vec![]);
        let handle = state.regions.last().expect("move handle").center();
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![
                egui::Event::PointerMoved(handle),
                egui::Event::PointerButton {
                    pos: handle,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
        assert!(state.selection_gesture_active());
        let destination = handle + egui::vec2(80.0, 0.0);
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![egui::Event::PointerMoved(destination)],
        );
        let moved = objects(&tree);
        let root_world =
            crate::model::object_world_matrix(&moved, root.uuid).transform_point3(Vec3::ZERO);
        let child_world =
            crate::model::object_world_matrix(&moved, child.uuid).transform_point3(Vec3::ZERO);
        assert!(root_world.x > 0.0);
        assert!((child_world - root_world - Vec3::X).length() < 0.0001);
        assert_eq!(moved[0].transform, root.transform);
        assert_eq!(moved[1].transform, child.transform);
        let event = if cancel {
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }
        } else {
            egui::Event::PointerButton {
                pos: destination,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }
        };
        draw(&mut state, &ctx, &mut tree, &camera, vec![event]);
        assert!(!state.selection_gesture_active());
        if !cancel {
            tree.undo();
        } else {
            draw(
                &mut state,
                &ctx,
                &mut tree,
                &camera,
                vec![
                    egui::Event::PointerButton {
                        pos: destination,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    },
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
        }
        assert_eq!(
            objects(&tree)[0].transform.translation,
            root.transform.translation
        );
        assert_eq!(
            objects(&tree)[1].transform.translation,
            child.transform.translation
        );
        assert_eq!(objects(&tree)[0].group_transform, root.group_transform);
        if !cancel {
            tree.redo();
            assert_eq!(
                objects(&tree)[0].transform.translation,
                moved[0].transform.translation
            );
        }
    }
}

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

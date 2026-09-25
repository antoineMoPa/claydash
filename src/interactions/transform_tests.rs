use super::*;
use crate::{
    commands,
    model::{BooleanOperation, SdfObject},
};
use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

fn selected_object() -> (DataTree, uuid::Uuid) {
    let mut tree = DataTree::default();
    let object = SdfObject::create(TYPE_BOX);
    let uuid = object.uuid;
    set_objects(&mut tree, vec![object]);
    set_selected(&mut tree, vec![uuid]);
    (tree, uuid)
}

#[test]
fn boolean_keys_combine_multiple_selections_immediately_and_undo() {
    for (key, operation) in [
        (KeyCode::Equal, BooleanOperation::Union),
        (KeyCode::Minus, BooleanOperation::Subtract),
        (KeyCode::NumpadMultiply, BooleanOperation::Intersect),
    ] {
        let (mut tree, target) = selected_object();
        let group = SdfObject::create(TYPE_SPHERE);
        let mut child = SdfObject::create(TYPE_BOX);
        child.boolean_parent = Some(group.uuid);
        child.operation = BooleanOperation::Subtract;
        let third = SdfObject::create(TYPE_BOX);
        let mut scene = objects(&tree);
        scene.extend([group.clone(), child.clone(), third.clone()]);
        set_objects(&mut tree, scene);
        set_selected(&mut tree, vec![target, group.uuid, third.uuid]);
        let mut interactions = InteractionState::default();
        interactions.key_pressed(key, false, &Commands::new(), &mut tree);
        assert!(crate::ui::scene_actions::pending_boolean(&tree).is_none());
        let result = objects(&tree);
        for id in [group.uuid, third.uuid] {
            let operand = result.iter().find(|o| o.uuid == id).unwrap();
            assert_eq!(operand.boolean_parent, Some(target));
            assert_eq!(operand.operation, operation);
        }
        assert_eq!(result[2].boolean_parent, Some(group.uuid));
        assert_eq!(result[2].operation, BooleanOperation::Subtract);
        crate::undo_redo::undo(&mut tree);
        assert!(objects(&tree)[1].boolean_parent.is_none());
        assert!(objects(&tree)[3].boolean_parent.is_none());
        assert_eq!(selected(&tree), vec![target]);
    }
}

#[test]
fn boolean_pick_can_reach_an_operand_behind_the_target() {
    for key in [KeyCode::Equal, KeyCode::Minus, KeyCode::NumpadMultiply] {
        for projection in [
            crate::camera::ProjectionMode::Perspective,
            crate::camera::ProjectionMode::Orthographic,
        ] {
            let (mut tree, target) = selected_object();
            let mut scene = objects(&tree);
            let operand = SdfObject::create(TYPE_SPHERE);
            // Sphere is entirely hidden by the selected box.
            scene.push(operand);
            set_objects(&mut tree, scene);
            let mut camera = Camera::new();
            camera.viewport = Vec2::new(800.0, 600.0);
            camera.projection_mode = projection;
            let mut interactions = InteractionState {
                mouse_position: camera.viewport / 2.0,
                ..Default::default()
            };
            let (origin, direction) = camera.ray(interactions.mouse_position);
            assert_eq!(raymarch(origin, direction, &objects(&tree)), Some(target));
            interactions.key_pressed(key, false, &Commands::new(), &mut tree);
            interactions.pointer_down(&camera, &mut tree, None);
            assert!(crate::ui::scene_actions::pending_boolean(&tree).is_none());
            assert_eq!(objects(&tree)[1].boolean_parent, Some(target));
            assert_eq!(selected(&tree), vec![target]);
            crate::undo_redo::undo(&mut tree);
            assert!(objects(&tree)[1].boolean_parent.is_none());
            assert!(crate::ui::scene_actions::pending_boolean(&tree).is_none());
        }
    }
}

#[test]
fn boolean_operator_ends_duplicate_placement_before_picking() {
    let (mut tree, target) = selected_object();
    let mut command_map = Commands::new();
    commands::register_all(&mut command_map);
    commands::duplicate_object(&mut tree, target);
    let duplicate = selected(&tree)[0];
    let mut interactions = InteractionState::default();
    interactions.key_pressed(KeyCode::Equal, false, &command_map, &mut tree);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Start)
    ));
    assert_eq!(
        crate::ui::scene_actions::pending_boolean(&tree)
            .unwrap()
            .target,
        duplicate
    );
    assert!(crate::ui::scene_actions::apply_boolean_pick(
        &mut tree, target
    ));
    assert_eq!(objects(&tree)[0].boolean_parent, Some(duplicate));
}

#[test]
fn boolean_shortcut_then_viewport_click_attaches_the_hit_group() {
    let (mut tree, target) = selected_object();
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
    let mut scene = objects(&tree);
    scene[0].transform.translation = Vec3::new(-2.0, 0.0, 0.0);
    let group = SdfObject::create(TYPE_SPHERE);
    let mut child = SdfObject::create(TYPE_BOX);
    child.boolean_parent = Some(group.uuid);
    scene.extend([group.clone(), child.clone()]);
    set_objects(&mut tree, scene);
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };
    interactions.key_pressed(KeyCode::Minus, false, &Commands::new(), &mut tree);
    interactions.pointer_down(&camera, &mut tree, None);
    let result = objects(&tree);
    assert_eq!(result[1].boolean_parent, Some(target));
    assert_eq!(result[1].operation, BooleanOperation::Subtract);
    assert_eq!(result[2].boolean_parent, Some(group.uuid));
    assert!(crate::ui::scene_actions::pending_boolean(&tree).is_none());
}

#[test]
fn boolean_shortcuts_respect_text_input_and_cancel_without_editing() {
    let (mut tree, target) = selected_object();
    let mut interactions = InteractionState::default();
    let commands = Commands::new();
    interactions.key_pressed(KeyCode::Minus, true, &commands, &mut tree);
    assert!(crate::ui::scene_actions::pending_boolean(&tree).is_none());
    interactions.key_pressed(KeyCode::ShiftLeft, false, &commands, &mut tree);
    interactions.key_pressed(KeyCode::Digit8, false, &commands, &mut tree);
    assert_eq!(
        crate::ui::scene_actions::pending_boolean(&tree)
            .unwrap()
            .operation,
        BooleanOperation::Intersect
    );
    assert!(crate::ui::scene_actions::apply_boolean_pick(
        &mut tree, target
    ));
    assert!(
        crate::ui::scene_actions::pending_boolean(&tree).is_some(),
        "self-pick must not mutate the scene"
    );
    interactions.key_pressed(KeyCode::Escape, false, &commands, &mut tree);
    assert!(crate::ui::scene_actions::pending_boolean(&tree).is_none());
    assert!(objects(&tree)[0].boolean_parent.is_none());
    assert_eq!(selected(&tree), vec![target]);
}

#[test]
fn primary_modifier_i_inverts_selection() {
    for modifier in [KeyCode::ControlLeft, KeyCode::SuperLeft] {
        let (mut tree, selected_id) = selected_object();
        let other = SdfObject::create(TYPE_SPHERE);
        let other_id = other.uuid;
        let mut scene = objects(&tree);
        scene.push(other);
        set_objects(&mut tree, scene);
        let mut interactions = InteractionState::default();
        let mut command_map = Commands::new();
        commands::register_all(&mut command_map);

        interactions.key_pressed(modifier, false, &command_map, &mut tree);
        interactions.key_pressed(KeyCode::KeyI, false, &command_map, &mut tree);

        assert_eq!(selected(&tree), vec![other_id]);
        assert!(!selected(&tree).contains(&selected_id));
    }
}

#[test]
fn new_primitives_spawn_under_cursor_and_follow_it_without_an_offset() {
    for mode in [
        crate::camera::ProjectionMode::Perspective,
        crate::camera::ProjectionMode::Orthographic,
    ] {
        for kind in crate::model::PrimitiveKind::SPAWNABLE {
            let mut tree = DataTree::default();
            let mut camera = Camera::new();
            camera.viewport_origin = Vec2::new(220.0, 60.0);
            camera.viewport = Vec2::new(800.0, 600.0);
            camera.projection_mode = mode;
            let mut interactions = InteractionState {
                mouse_position: camera.viewport_origin + Vec2::new(570.0, 210.0),
                ..Default::default()
            };
            commands::spawn(&mut tree, kind.object_type());
            interactions.place_pending_spawn(&camera, &mut tree);
            for delta in [Vec2::ZERO, Vec2::new(55.0, 30.0)] {
                interactions.mouse_position += delta;
                interactions.update(&mut camera, &mut tree);
                let object = &objects(&tree)[0];
                let projected = camera.project(object.transform.translation, 1.0).unwrap();
                let projected = Vec2::new(projected.x, projected.y);
                assert!(
                    projected.distance(interactions.mouse_position) < 0.01,
                    "{kind:?} in {mode:?} should stay under cursor"
                );
            }
        }
    }
}

#[test]
fn mouse_release_commits_grab_and_stops_following_the_cursor() {
    let (mut tree, _) = selected_object();
    tree.make_undo_redo_snapshot();
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };
    commands::start_grab(&mut tree);
    interactions.update(&mut camera, &mut tree);
    interactions.pointer_down(&camera, &mut tree, None);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Grabbing)
    ));
    interactions.mouse_position.x += 100.0;
    interactions.pointer_up(&camera, &mut tree);
    let committed = objects(&tree)[0].transform.translation;
    assert!(committed.length() > 0.01);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Start)
    ));
    interactions.mouse_position.x += 100.0;
    interactions.update(&mut camera, &mut tree);
    assert_eq!(objects(&tree)[0].transform.translation, committed);
    crate::undo_redo::undo(&mut tree);
    assert_eq!(objects(&tree)[0].transform.translation, Vec3::ZERO);
}

#[test]
fn rotate_turns_the_selection_with_the_cursor() {
    let (mut tree, _) = selected_object();
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0 + Vec2::X * 100.0,
        ..Default::default()
    };
    commands::start_rotate(&mut tree);
    interactions.update(&mut camera, &mut tree);

    interactions.mouse_position = camera.viewport / 2.0 + Vec2::Y * 100.0;
    interactions.update(&mut camera, &mut tree);

    assert_ne!(objects(&tree)[0].transform.rotation, Quat::IDENTITY);
}

#[test]
fn top_view_rotation_follows_pointer_direction() {
    let (mut tree, _) = selected_object();
    let mut scene = objects(&tree);
    scene[0].transform.translation = Vec3::X;
    set_objects(&mut tree, scene);
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    camera.snap(crate::camera::ViewAngle::Top);
    let center = camera.project(Vec3::X, 1.0).unwrap();
    let mut interactions = InteractionState {
        mouse_position: Vec2::new(center.x + 100.0, center.y),
        ..Default::default()
    };
    commands::start_rotate(&mut tree);
    interactions.update(&mut camera, &mut tree);
    interactions.mouse_position = Vec2::new(center.x, center.y + 100.0);
    interactions.update(&mut camera, &mut tree);
    let rotated_x = objects(&tree)[0].transform.rotation * Vec3::X;
    assert!(rotated_x.distance(Vec3::Z) < 0.001, "{rotated_x:?}");
}

#[test]
fn cursor_pivot_rotates_translation_and_orientation() {
    let (mut tree, _) = selected_object();
    let mut scene = objects(&tree);
    scene[0].transform.translation = Vec3::X;
    set_objects(&mut tree, scene);
    tree.set_path(
        "editor.rotation_pivot",
        ClaydashValue::RotationPivot(crate::model::RotationPivot::Cursor),
    );
    tree.set_path("scene.cursor_position", ClaydashValue::Vec3(Vec3::Z));
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    camera.snap(crate::camera::ViewAngle::Top);
    let pivot = camera.project(Vec3::Z, 1.0).unwrap();
    let mut interactions = InteractionState {
        mouse_position: Vec2::new(pivot.x + 100.0, pivot.y),
        ..Default::default()
    };
    commands::start_rotate(&mut tree);
    interactions.update(&mut camera, &mut tree);
    interactions.mouse_position = Vec2::new(pivot.x, pivot.y + 100.0);
    interactions.update(&mut camera, &mut tree);
    let object = &objects(&tree)[0];
    assert!(
        object
            .transform
            .translation
            .distance(Vec3::new(1.0, 0.0, 2.0))
            < 0.001,
        "{:?}",
        object.transform.translation
    );
    assert!((object.transform.rotation * Vec3::X).distance(Vec3::Z) < 0.001);
}

#[test]
fn releasing_ctrl_keeps_rotation_snapped_until_the_pointer_moves() {
    assert!(rotation_snap_active(true, false, false));
    assert!(!rotation_snap_active(true, false, true));

    let (mut tree, _) = selected_object();
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let center = camera.viewport / 2.0;
    let mut interactions = InteractionState {
        mouse_position: center + Vec2::X * 100.0,
        ..Default::default()
    };
    let mut command_map = Commands::new();
    commands::register_all(&mut command_map);
    commands::start_rotate(&mut tree);
    interactions.update(&mut camera, &mut tree);

    interactions.key_pressed(KeyCode::ControlLeft, false, &command_map, &mut tree);
    let seven_degrees = 7.0_f32.to_radians();
    interactions.mouse_position =
        center + Vec2::new(seven_degrees.cos(), seven_degrees.sin()) * 100.0;
    interactions.update(&mut camera, &mut tree);
    let snapped = objects(&tree)[0].transform.rotation;

    interactions.key_released(KeyCode::ControlLeft);
    interactions.update(&mut camera, &mut tree);
    assert_eq!(objects(&tree)[0].transform.rotation, snapped);

    let eight_degrees = 8.0_f32.to_radians();
    interactions.mouse_position =
        center + Vec2::new(eight_degrees.cos(), eight_degrees.sin()) * 100.0;
    interactions.update(&mut camera, &mut tree);
    assert_ne!(objects(&tree)[0].transform.rotation, snapped);
}

#[test]
fn numeric_rotation_uses_degrees_and_escape_cancels_the_sequence() {
    let (mut tree, _) = selected_object();
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0 + Vec2::X * 100.0,
        ..Default::default()
    };
    let mut command_map = Commands::new();
    commands::register_all(&mut command_map);

    for key in [
        KeyCode::KeyR,
        KeyCode::KeyY,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ] {
        interactions.key_pressed(key, false, &command_map, &mut tree);
        interactions.key_released(key);
        interactions.update(&mut camera, &mut tree);
    }

    let rotated_x = objects(&tree)[0].transform.rotation * Vec3::X;
    assert!(rotated_x.distance(Vec3::NEG_Z) < 0.0001);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Rotating)
    ));

    interactions.key_pressed(KeyCode::Escape, false, &command_map, &mut tree);

    assert_eq!(objects(&tree)[0].transform.rotation, Quat::IDENTITY);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Start)
    ));
}

#[test]
fn group_grab_moves_operands_and_escape_restores_them() {
    let (mut tree, target) = selected_object();
    let mut scene = objects(&tree);
    let mut operand = SdfObject::create(TYPE_SPHERE);
    operand.boolean_parent = Some(target);
    operand.transform.translation = Vec3::X * 0.2;
    scene.push(operand);
    set_objects(&mut tree, scene.clone());
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };
    commands::start_grab(&mut tree);
    interactions.update(&mut camera, &mut tree);
    interactions.mouse_position.x += 100.0;
    interactions.update(&mut camera, &mut tree);
    let moved = objects(&tree);
    let delta = crate::model::object_world_matrix(&moved, target).transform_point3(Vec3::ZERO)
        - crate::model::object_world_matrix(&scene, target).transform_point3(Vec3::ZERO);
    assert!(delta.length() > 0.01);
    let operand_delta = crate::model::object_world_matrix(&moved, moved[1].uuid)
        .transform_point3(Vec3::ZERO)
        - crate::model::object_world_matrix(&scene, scene[1].uuid).transform_point3(Vec3::ZERO);
    assert!((operand_delta - delta).length() < 0.00001);
    assert_eq!(moved[0].transform, scene[0].transform);
    assert_eq!(moved[1].transform, scene[1].transform);
    assert_ne!(moved[0].group_transform, scene[0].group_transform);
    let mut commands = Commands::new();
    crate::commands::register_all(&mut commands);
    interactions.key_pressed(KeyCode::Escape, false, &commands, &mut tree);
    for (restored, initial) in objects(&tree).iter().zip(&scene) {
        assert_eq!(restored.transform, initial.transform);
        assert_eq!(restored.group_transform, initial.group_transform);
    }
}

#[test]
fn grab_moves_an_exact_object_inside_a_union() {
    let mut tree = DataTree::default();
    let root = SdfObject::create(TYPE_BOX);
    let mut child = SdfObject::create(TYPE_SPHERE);
    child.boolean_parent = Some(root.uuid);
    set_objects(&mut tree, vec![root.clone(), child.clone()]);
    set_selected_exact(&mut tree, vec![child.uuid]);
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };
    let mut command_map = Commands::new();
    commands::register_all(&mut command_map);

    interactions.key_pressed(KeyCode::KeyG, false, &command_map, &mut tree);
    interactions.update(&mut camera, &mut tree);
    interactions.mouse_position.x += 100.0;
    interactions.update(&mut camera, &mut tree);

    let moved = objects(&tree);
    assert_eq!(moved[0].transform.translation, root.transform.translation);
    assert_eq!(moved[0].transform.rotation, root.transform.rotation);
    assert_eq!(moved[0].transform.scale, root.transform.scale);
    assert_ne!(moved[1].transform.translation, child.transform.translation);
}

#[test]
fn g_grabs_a_multi_selection_without_creating_a_union() {
    let mut tree = DataTree::default();
    let first = SdfObject::create(TYPE_BOX);
    let second = SdfObject::create(TYPE_SPHERE);
    set_objects(&mut tree, vec![first.clone(), second.clone()]);
    set_selected(&mut tree, vec![first.uuid, second.uuid]);
    let mut interactions = InteractionState::default();
    let mut command_map = Commands::new();
    commands::register_all(&mut command_map);

    interactions.key_pressed(KeyCode::KeyG, false, &command_map, &mut tree);

    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Grabbing)
    ));
    assert!(objects(&tree)
        .iter()
        .all(|object| object.boolean_parent.is_none()));
}

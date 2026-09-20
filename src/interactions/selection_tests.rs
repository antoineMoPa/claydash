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
fn raymarch_selects_an_object_in_front_of_the_camera() {
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let object = SdfObject::create(TYPE_SPHERE);
    let expected = object.uuid;
    let (origin, direction) = camera.ray(camera.viewport / 2.0);

    assert_eq!(raymarch(origin, direction, &[object]), Some(expected));
}

#[test]
fn raymarch_hit_includes_visible_surface_position() {
    let object = SdfObject::create(TYPE_SPHERE);
    let origin = Vec3::new(0.0, 0.0, 3.0);
    let hit = raymarch_hit(origin, Vec3::NEG_Z, &[object]).unwrap();

    assert!(hit.position.z > 0.0);
    assert!(hit.position.distance(origin) < origin.length());
}

#[test]
fn background_click_clears_selection() {
    let (mut tree, _) = selected_object();
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);

    InteractionState::select_at(&camera, &mut tree, Vec2::ZERO, None, false);

    assert!(selected(&tree).is_empty());
}

#[test]
fn escape_clears_selection_when_no_operation_is_active() {
    let (mut tree, _) = selected_object();
    let mut interactions = InteractionState::default();

    interactions.key_pressed(KeyCode::Escape, false, &Commands::new(), &mut tree);

    assert!(selected(&tree).is_empty());
}

#[test]
fn subtraction_changes_the_cpu_selection_distance_field() {
    let mut outer = SdfObject::create(TYPE_SPHERE);
    let mut cutter = SdfObject::create(TYPE_SPHERE);
    if let crate::model::SdfParams::SphereParams(params) = &mut outer.params {
        params.radius = 1.0;
    }
    if let crate::model::SdfParams::SphereParams(params) = &mut cutter.params {
        params.radius = 0.5;
    }
    cutter.operation = BooleanOperation::Subtract;
    cutter.boolean_parent = Some(outer.uuid);

    let (distance, hit) = scene_distance(Vec3::ZERO, &[outer.clone(), cutter]).unwrap();
    assert!(distance > 0.0, "the cutter should leave a cavity");
    assert_eq!(hit, outer.uuid, "the cut surface belongs to the target");
}

#[test]
fn repeated_ghost_click_dives_into_group_and_shift_toggles_the_group() {
    let (mut tree, target) = selected_object();
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
    let mut scene = objects(&tree);
    let mut cutter = SdfObject::create(TYPE_SPHERE);
    cutter.boolean_parent = Some(target);
    cutter.operation = BooleanOperation::Subtract;
    let id = cutter.uuid;
    scene.push(cutter);
    set_objects(&mut tree, scene);
    let camera = Camera::new();
    let mut interactions = InteractionState::default();
    interactions.pointer_down(&camera, &mut tree, Some(id));
    assert_eq!(selected(&tree), vec![id]);
    set_selected(&mut tree, vec![target]);
    interactions.keys.insert(KeyCode::ShiftLeft);
    interactions.pointer_down(&camera, &mut tree, Some(id));
    interactions.pointer_up(&camera, &mut tree);
    assert!(selected(&tree).is_empty());
    interactions.pointer_down(&camera, &mut tree, Some(id));
    interactions.pointer_up(&camera, &mut tree);
    assert_eq!(selected(&tree), vec![target]);
}

#[test]
fn shift_drag_pans_camera_without_changing_selection() {
    let (mut tree, selected_id) = selected_object();
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let initial_target = camera.target;
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };
    interactions.keys.insert(KeyCode::ShiftLeft);

    interactions.pointer_down(&camera, &mut tree, Some(selected_id));
    interactions.cursor_moved(interactions.mouse_position + Vec2::new(80.0, 30.0), false);
    interactions.update(&mut camera, &mut tree);
    interactions.pointer_up(&camera, &mut tree);

    assert_ne!(camera.target, initial_target);
    assert_eq!(selected(&tree), vec![selected_id]);
}

#[test]
fn suspended_navigation_releases_buttons_and_modifiers() {
    let mut interactions = InteractionState::default();
    interactions.keys.insert(KeyCode::ControlLeft);
    interactions.right_down = true;
    interactions.mouse_delta = Vec2::new(20.0, 10.0);
    interactions.right_pan_reference = Some(Vec3::ZERO);

    interactions.suspend_navigation();

    assert!(interactions.keys.is_empty());
    assert!(!interactions.right_down);
    assert_eq!(interactions.mouse_delta, Vec2::ZERO);
    assert!(interactions.right_pan_reference.is_none());
}

#[test]
fn repeated_click_on_group_root_switches_from_group_to_exact_primitive() {
    let mut tree = DataTree::default();
    let root = SdfObject::create(TYPE_BOX);
    let mut child = SdfObject::create(TYPE_SPHERE);
    child.boolean_parent = Some(root.uuid);
    set_objects(&mut tree, vec![root.clone(), child.clone()]);
    let camera = Camera::new();

    InteractionState::select_at(&camera, &mut tree, Vec2::ZERO, Some(root.uuid), false);
    assert_eq!(selected(&tree), vec![root.uuid]);
    assert_eq!(
        commands::effective_selected_ids(&tree),
        vec![root.uuid, child.uuid]
    );

    InteractionState::select_at(&camera, &mut tree, Vec2::ZERO, Some(root.uuid), false);
    assert_eq!(selected(&tree), vec![root.uuid]);
    assert_eq!(commands::effective_selected_ids(&tree), vec![root.uuid]);
    assert_eq!(
        crate::model::selection_scope(&tree),
        crate::model::SelectionScope::Exact
    );
}

#[test]
fn duplicate_then_y_with_modifier_held_moves_only_the_copy_on_y() {
    for modifier in [KeyCode::ShiftLeft, KeyCode::SuperLeft] {
        for already_grabbing in [false, true] {
            let (mut tree, original_id) = selected_object();
            let mut camera = Camera::new();
            camera.viewport = Vec2::new(800.0, 600.0);
            let mut commands = Commands::new();
            commands::register_all(&mut commands);
            let mut interaction = InteractionState {
                mouse_position: camera.viewport / 2.0,
                ..Default::default()
            };
            if already_grabbing {
                commands::start_grab(&mut tree);
                interaction.update(&mut camera, &mut tree);
            }
            interaction.key_pressed(modifier, false, &commands, &mut tree);
            interaction.key_pressed(KeyCode::KeyD, false, &commands, &mut tree);
            interaction.key_released(KeyCode::KeyD);
            interaction.update(&mut camera, &mut tree);
            let scene = objects(&tree);
            let original = scene
                .iter()
                .find(|o| o.uuid == original_id)
                .unwrap()
                .transform
                .translation;
            let copy_id = selected(&tree)[0];
            assert_ne!(copy_id, original_id);
            let initial_copy = scene
                .iter()
                .find(|o| o.uuid == copy_id)
                .unwrap()
                .transform
                .translation;
            interaction.key_pressed(KeyCode::KeyY, false, &commands, &mut tree);
            interaction.mouse_position += Vec2::new(110.0, 80.0);
            interaction.update(&mut camera, &mut tree);
            let result = objects(&tree);
            assert_eq!(
                result
                    .iter()
                    .find(|o| o.uuid == original_id)
                    .unwrap()
                    .transform
                    .translation,
                original
            );
            let delta = result
                .iter()
                .find(|o| o.uuid == copy_id)
                .unwrap()
                .transform
                .translation
                - initial_copy;
            assert_eq!(delta.x, 0.0);
            assert_eq!(delta.z, 0.0);
            assert!(
                delta.y.abs() > 0.001,
                "{modifier:?}, already grabbing: {already_grabbing}"
            );
        }
    }
}

#[test]
fn axis_switches_and_key_repeats_keep_grabs_on_one_axis() {
    for projection in [
        crate::camera::ProjectionMode::Perspective,
        crate::camera::ProjectionMode::Orthographic,
    ] {
        let (mut tree, _) = selected_object();
        let mut camera = Camera::new();
        camera.projection_mode = projection;
        camera.viewport = Vec2::new(800.0, 600.0);
        let mut commands = Commands::new();
        commands::register_all(&mut commands);
        let mut interaction = InteractionState {
            mouse_position: camera.viewport / 2.0,
            ..Default::default()
        };
        let initial = objects(&tree)[0].transform.translation;
        commands::start_grab(&mut tree);
        interaction.update(&mut camera, &mut tree);
        interaction.mouse_position += Vec2::new(110.0, 80.0);
        interaction.update(&mut camera, &mut tree);
        for (key, axis) in [(KeyCode::KeyX, 0), (KeyCode::KeyY, 1), (KeyCode::KeyZ, 2)] {
            interaction.key_pressed(key, false, &commands, &mut tree);
            // OS auto-repeat must not toggle the selected axis back off.
            interaction.key_pressed(key, false, &commands, &mut tree);
            interaction.update(&mut camera, &mut tree);
            let delta = objects(&tree)[0].transform.translation - initial;
            assert!(delta[axis].abs() > 0.001, "{projection:?}: {key:?}");
            for other in 0..3 {
                if other != axis {
                    assert_eq!(delta[other], 0.0);
                }
            }
            interaction.key_released(key);
        }
        // A separate press of the same axis intentionally clears the lock.
        interaction.key_pressed(KeyCode::KeyZ, false, &commands, &mut tree);
        interaction.update(&mut camera, &mut tree);
        let delta = objects(&tree)[0].transform.translation - initial;
        assert!(delta.x.abs() > 0.001 && delta.y.abs() > 0.001);
    }
}

#[test]
fn shift_y_during_grab_constrains_instead_of_redo_and_text_input_is_respected() {
    let (mut tree, _) = selected_object();
    let mut commands = Commands::new();
    commands::register_all(&mut commands);
    let mut interaction = InteractionState::default();
    commands::start_grab(&mut tree);
    interaction.key_pressed(KeyCode::KeyY, true, &commands, &mut tree);
    assert!(matches!(
        tree.get_path("editor.constrain_y"),
        ClaydashValue::Bool(false)
    ));
    interaction.key_released(KeyCode::KeyY);
    interaction.key_pressed(KeyCode::ShiftLeft, false, &commands, &mut tree);
    interaction.key_pressed(KeyCode::KeyY, false, &commands, &mut tree);
    assert!(matches!(
        tree.get_path("editor.constrain_y"),
        ClaydashValue::Bool(true)
    ));
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Grabbing)
    ));
}

#[test]
fn grab_moves_the_selection_with_the_cursor() {
    let (mut tree, _) = selected_object();
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };
    let mut command_map = Commands::new();
    commands::register_all(&mut command_map);
    commands::start_grab(&mut tree);
    interactions.update(&mut camera, &mut tree);

    interactions.mouse_position.x += 100.0;
    interactions.update(&mut camera, &mut tree);

    let movement = objects(&tree)[0].transform.translation;
    assert!(movement.length() > 0.01);
    assert!(movement.dot(camera.target - camera.position).abs() < 0.0001);
}

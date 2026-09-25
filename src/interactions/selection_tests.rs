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
fn empty_scene_click_places_cursor_on_its_current_view_plane() {
    let (mut tree, _) = selected_object();
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    tree.set_path("scene.cursor_position", ClaydashValue::Vec3(Vec3::Z));
    let click = Vec2::new(750.0, 550.0);
    let expected = camera.cursor_on_plane(click, Vec3::Z);
    let mut interactions = InteractionState {
        mouse_position: click,
        ..Default::default()
    };

    interactions.pointer_down(&camera, &mut tree, None);

    assert!(crate::model::cursor_position(&tree).distance(expected) < 0.001);
    assert!(selected(&tree).is_empty());
}

#[test]
fn placement_mode_can_click_an_object_without_changing_selection() {
    let (mut tree, id) = selected_object();
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    tree.set_path("scene.cursor_position", ClaydashValue::Vec3(Vec3::Z));
    tree.set_transient_path("editor.place_cursor", ClaydashValue::Bool(true));
    let click = camera.viewport / 2.0;
    let expected = camera.cursor_on_plane(click, Vec3::Z);
    let mut interactions = InteractionState {
        mouse_position: click,
        ..Default::default()
    };

    interactions.pointer_down(&camera, &mut tree, None);

    assert!(crate::model::cursor_position(&tree).distance(expected) < 0.001);
    assert_eq!(selected(&tree), vec![id]);
    assert!(matches!(
        tree.get_path("editor.place_cursor"),
        ClaydashValue::Bool(false)
    ));
}

#[test]
fn escape_cancels_cursor_placement_without_clearing_selection() {
    let (mut tree, id) = selected_object();
    tree.set_transient_path("editor.place_cursor", ClaydashValue::Bool(true));
    let mut interactions = InteractionState::default();

    interactions.key_pressed(KeyCode::Escape, false, &Commands::new(), &mut tree);

    assert!(matches!(
        tree.get_path("editor.place_cursor"),
        ClaydashValue::Bool(false)
    ));
    assert_eq!(selected(&tree), vec![id]);
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
fn selecting_an_object_then_its_face_takes_three_clicks() {
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let object = SdfObject::create(TYPE_BOX);
    let id = object.uuid;
    let mut tree = DataTree::default();
    set_objects(&mut tree, vec![object]);

    InteractionState::select_at(&camera, &mut tree, camera.viewport / 2.0, None, false);

    assert_eq!(selected(&tree), vec![id]);
    assert_eq!(crate::model::selected_box_face(&tree), None);
    commands::start_grab(&mut tree);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Grabbing)
    ));
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
    InteractionState::select_at(&camera, &mut tree, camera.viewport / 2.0, None, false);
    assert_eq!(crate::model::selected_box_face(&tree), None);
    InteractionState::select_at(&camera, &mut tree, camera.viewport / 2.0, None, false);

    let face = crate::model::selected_box_face(&tree).expect("box face selection");
    assert_eq!(face.object, id);
    commands::start_grab(&mut tree);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::DraggingFace)
    ));
}

#[test]
fn clicking_an_extruded_polygon_selects_its_face_even_inside_a_group() {
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut root = SdfObject::create(TYPE_SPHERE);
    root.transform.translation = Vec3::X * 10.0;
    let mut extrusion = SdfObject::create_kind(crate::model::PrimitiveKind::PolygonPrism);
    extrusion.boolean_parent = Some(root.uuid);
    let extrusion_id = extrusion.uuid;
    let root_id = root.uuid;
    let mut tree = DataTree::default();
    set_objects(&mut tree, vec![root, extrusion]);

    InteractionState::select_at(&camera, &mut tree, camera.viewport / 2.0, None, false);

    assert_eq!(selected(&tree), vec![root_id]);
    assert_eq!(crate::model::selected_modeling_face(&tree), None);
    for _ in 0..3 {
        InteractionState::select_at(&camera, &mut tree, camera.viewport / 2.0, None, false);
    }
    assert!(matches!(
        crate::model::selected_modeling_face(&tree),
        Some(crate::model::ModelingFaceSelection::PolygonPrism(face))
            if face.object == extrusion_id
    ));
}

#[test]
fn cylinder_caps_are_selected_but_the_curved_side_is_not() {
    let cylinder = SdfObject::create_kind(crate::model::PrimitiveKind::Cylinder);
    let scene = [cylinder.clone()];
    for (y, positive) in [(0.35, true), (-0.35, false)] {
        assert_eq!(
            crate::model::modeling_face_at_world_position(&scene, cylinder.uuid, Vec3::Y * y),
            Some(crate::model::ModelingFaceSelection::CylinderCap(
                crate::model::CylinderCapSelection {
                    object: cylinder.uuid,
                    positive
                },
            )),
        );
    }
    assert_eq!(
        crate::model::modeling_face_at_world_position(&scene, cylinder.uuid, Vec3::X * 0.25),
        None,
    );
}

#[test]
fn clicking_cylinder_top_and_bottom_selects_the_visible_cap() {
    for (view, positive) in [
        (crate::camera::ViewAngle::Top, true),
        (crate::camera::ViewAngle::Bottom, false),
    ] {
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800.0, 600.0);
        camera.snap(view);
        let cylinder = SdfObject::create_kind(crate::model::PrimitiveKind::Cylinder);
        let mut tree = DataTree::default();
        set_objects(&mut tree, vec![cylinder.clone()]);
        InteractionState::select_at(&camera, &mut tree, camera.viewport / 2.0, None, false);
        assert_eq!(crate::model::selected_modeling_face(&tree), None);
        InteractionState::select_at(&camera, &mut tree, camera.viewport / 2.0, None, false);
        InteractionState::select_at(&camera, &mut tree, camera.viewport / 2.0, None, false);
        assert_eq!(
            crate::model::selected_modeling_face(&tree),
            Some(crate::model::ModelingFaceSelection::CylinderCap(
                crate::model::CylinderCapSelection {
                    object: cylinder.uuid,
                    positive
                },
            )),
        );
    }
}

#[test]
fn cylinder_cap_extrusion_drags_and_escape_restores_the_source() {
    let mut tree = DataTree::default();
    let cylinder = SdfObject::create_kind(crate::model::PrimitiveKind::Cylinder);
    let source = cylinder.uuid;
    set_objects(&mut tree, vec![cylinder]);
    set_selected(&mut tree, vec![source]);
    let cap =
        crate::model::ModelingFaceSelection::CylinderCap(crate::model::CylinderCapSelection {
            object: source,
            positive: true,
        });
    crate::model::set_selected_modeling_face(&mut tree, Some(cap));
    let mut commands = Commands::new();
    commands::register_all(&mut commands);
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };
    interactions.key_pressed(KeyCode::KeyE, false, &commands, &mut tree);
    assert_eq!(objects(&tree).len(), 2);
    interactions.update(&mut camera, &mut tree);
    let session = interactions.extrusion_session.as_ref().unwrap().clone();
    interactions.cursor_moved(
        interactions.mouse_position
            + session.projected_axis / session.projected_axis.length() * 80.0,
        false,
    );
    interactions.update(&mut camera, &mut tree);
    let extrusion = objects(&tree)
        .into_iter()
        .find(|object| object.uuid == session.object)
        .unwrap();
    let crate::model::SdfParams::CylinderParams { half_height, .. } = extrusion.params else {
        unreachable!()
    };
    assert!(half_height > commands::EXTRUSION_INITIAL_HALF_EXTENT);
    interactions.key_released(KeyCode::KeyE);
    interactions.key_pressed(KeyCode::Escape, false, &commands, &mut tree);
    assert_eq!(objects(&tree).len(), 1);
    assert_eq!(selected(&tree), vec![source]);
    assert_eq!(crate::model::selected_modeling_face(&tree), Some(cap));
}

#[test]
fn e_extrudes_the_selected_box_face() {
    let (mut tree, object) = selected_object();
    crate::model::set_selected_box_face(
        &mut tree,
        Some(crate::model::BoxFaceSelection {
            object,
            axis: crate::model::VectorAxis::Z,
            positive: true,
        }),
    );
    let mut commands = Commands::new();
    commands::register_all(&mut commands);
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };

    interactions.key_pressed(KeyCode::KeyE, false, &commands, &mut tree);

    assert_eq!(objects(&tree).len(), 2);
    assert_ne!(selected(&tree), vec![object]);
    interactions.update(&mut camera, &mut tree);
    let session = interactions
        .extrusion_session
        .as_ref()
        .expect("active extrusion drag")
        .clone();
    interactions.cursor_moved(
        interactions.mouse_position + session.projected_axis.normalize() * 80.0,
        false,
    );
    interactions.update(&mut camera, &mut tree);
    let extrusion = objects(&tree)
        .into_iter()
        .find(|candidate| candidate.uuid == session.object)
        .unwrap();
    let crate::model::SdfParams::BoxParams(params) = extrusion.params else {
        unreachable!()
    };
    assert!(params.box_q.z > commands::EXTRUSION_INITIAL_HALF_EXTENT);

    interactions.pointer_down(&camera, &mut tree, None);

    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Start)
    ));
}

#[test]
fn g_drags_the_selected_box_face_and_escape_restores_it() {
    let (mut tree, object) = selected_object();
    let face = crate::model::BoxFaceSelection {
        object,
        axis: crate::model::VectorAxis::Z,
        positive: true,
    };
    crate::model::set_selected_box_face(&mut tree, Some(face));
    let original = objects(&tree)[0].clone();
    let mut commands = Commands::new();
    commands::register_all(&mut commands);
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };

    interactions.key_pressed(KeyCode::KeyG, false, &commands, &mut tree);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::DraggingFace)
    ));
    interactions.update(&mut camera, &mut tree);
    let axis = interactions
        .extrusion_session
        .as_ref()
        .unwrap()
        .projected_axis;
    interactions.cursor_moved(interactions.mouse_position + axis.normalize() * 80.0, false);
    interactions.update(&mut camera, &mut tree);
    let dragged = objects(&tree)[0].clone();
    let (
        crate::model::SdfParams::BoxParams(original_params),
        crate::model::SdfParams::BoxParams(dragged_params),
    ) = (&original.params, &dragged.params)
    else {
        panic!("expected boxes");
    };
    assert!(dragged_params.box_q.z > original_params.box_q.z);
    assert!(dragged.transform.translation.z > original.transform.translation.z);
    assert!(
        (dragged.transform.translation.z
            - dragged_params.box_q.z
            - (original.transform.translation.z - original_params.box_q.z))
            .abs()
            < 0.0001
    );

    interactions.key_released(KeyCode::KeyG);
    interactions.key_pressed(KeyCode::Escape, false, &commands, &mut tree);
    let restored = objects(&tree)[0].clone();
    assert_eq!(restored.transform, original.transform);
    let crate::model::SdfParams::BoxParams(restored_params) = restored.params else {
        panic!("expected a box");
    };
    assert_eq!(restored_params.box_q, original_params.box_q);
}

#[test]
fn g_after_extrusion_moves_the_outer_face_without_moving_the_source() {
    let (mut tree, source_id) = selected_object();
    crate::model::set_selected_box_face(
        &mut tree,
        Some(crate::model::BoxFaceSelection {
            object: source_id,
            axis: crate::model::VectorAxis::Z,
            positive: true,
        }),
    );
    let source = objects(&tree)[0].clone();
    let mut commands = Commands::new();
    commands::register_all(&mut commands);
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let mut interactions = InteractionState {
        mouse_position: camera.viewport / 2.0,
        ..Default::default()
    };
    interactions.key_pressed(KeyCode::KeyE, false, &commands, &mut tree);
    interactions.update(&mut camera, &mut tree);
    interactions.pointer_down(&camera, &mut tree, None);
    interactions.key_released(KeyCode::KeyE);
    let extrusion_id = selected(&tree)[0];
    let before = objects(&tree)
        .into_iter()
        .find(|object| object.uuid == extrusion_id)
        .unwrap();

    interactions.key_pressed(KeyCode::KeyG, false, &commands, &mut tree);
    interactions.update(&mut camera, &mut tree);
    let axis = interactions
        .extrusion_session
        .as_ref()
        .unwrap()
        .projected_axis;
    interactions.cursor_moved(interactions.mouse_position + axis.normalize() * 80.0, false);
    interactions.update(&mut camera, &mut tree);
    let scene = objects(&tree);
    let after = scene
        .iter()
        .find(|object| object.uuid == extrusion_id)
        .unwrap();
    assert_eq!(
        scene
            .iter()
            .find(|object| object.uuid == source_id)
            .unwrap()
            .transform,
        source.transform
    );
    let (
        crate::model::SdfParams::BoxParams(before_params),
        crate::model::SdfParams::BoxParams(after_params),
    ) = (&before.params, &after.params)
    else {
        panic!("expected boxes");
    };
    assert!(after_params.box_q.z > before_params.box_q.z);
}

#[test]
fn escape_cancels_an_in_progress_extrusion() {
    let (mut tree, object) = selected_object();
    let source_face = crate::model::BoxFaceSelection {
        object,
        axis: crate::model::VectorAxis::X,
        positive: false,
    };
    crate::model::set_selected_box_face(&mut tree, Some(source_face));
    let mut commands = Commands::new();
    commands::register_all(&mut commands);
    let mut interactions = InteractionState::default();

    interactions.key_pressed(KeyCode::KeyE, false, &commands, &mut tree);
    interactions.key_released(KeyCode::KeyE);
    interactions.key_pressed(KeyCode::Escape, false, &commands, &mut tree);

    assert_eq!(objects(&tree).len(), 1);
    assert_eq!(selected(&tree), vec![object]);
    assert_eq!(crate::model::selected_box_face(&tree), Some(source_face));
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Start)
    ));
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
    assert!(interactions.update(&mut camera, &mut tree));
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

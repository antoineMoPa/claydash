use std::collections::HashSet;

use glam::{Quat, Vec2, Vec3};
use winit::keyboard::KeyCode;

use crate::{
    camera::Camera,
    commands::{self, Commands},
    model::{
        objects, selected, set_objects, set_selected, ClaydashValue, DataTree, EditorState,
        SdfObject, Transform,
    },
};

#[derive(Clone)]
struct TransformSession {
    mode: EditorState,
    selection: Vec<uuid::Uuid>,
    center: Vec3,
    initial_cursor: Vec3,
    initial_angle: f32,
    initial_radius: f32,
    objects: Vec<(uuid::Uuid, Transform)>,
}

pub struct InteractionState {
    keys: HashSet<KeyCode>,
    pub mouse_position: Vec2,
    mouse_delta: Vec2,
    right_down: bool,
    transform_session: Option<TransformSession>,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            keys: HashSet::new(),
            mouse_position: Vec2::ZERO,
            mouse_delta: Vec2::ZERO,
            right_down: false,
            transform_session: None,
        }
    }
}

impl InteractionState {
    pub fn key_pressed(
        &mut self,
        key: KeyCode,
        egui_wants_keyboard: bool,
        command_map: &Commands,
        tree: &mut DataTree,
    ) {
        let first_press = self.keys.insert(key);
        if egui_wants_keyboard || !first_press {
            return;
        }
        if key == KeyCode::Escape && crate::ui::scene_actions::pending_boolean(tree).is_some() {
            crate::ui::scene_actions::cancel_boolean_pick(tree);
            return;
        }
        let has_command_modifier = [
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ]
        .iter()
        .any(|key| self.keys.contains(key));
        if !has_command_modifier {
            let operation = match key {
                KeyCode::Equal | KeyCode::NumpadAdd => Some(crate::model::BooleanOperation::Union),
                KeyCode::Minus | KeyCode::NumpadSubtract => {
                    Some(crate::model::BooleanOperation::Subtract)
                }
                KeyCode::NumpadMultiply => Some(crate::model::BooleanOperation::Intersect),
                KeyCode::Digit8
                    if self.keys.contains(&KeyCode::ShiftLeft)
                        || self.keys.contains(&KeyCode::ShiftRight) =>
                {
                    Some(crate::model::BooleanOperation::Intersect)
                }
                _ => None,
            };
            if let Some(operation) = operation {
                crate::ui::scene_actions::begin_boolean_pick(tree, operation);
                return;
            }
        }
        let shift = self.keys.contains(&KeyCode::ShiftLeft)
            || self.keys.contains(&KeyCode::ShiftRight)
            || self.keys.contains(&KeyCode::SuperLeft)
            || self.keys.contains(&KeyCode::SuperRight);
        let transforming = matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(
                EditorState::Grabbing | EditorState::Scaling | EditorState::Rotating
            )
        );
        let name = match key {
            KeyCode::KeyG => "grab",
            KeyCode::KeyS => "scale",
            KeyCode::KeyR => "rotate",
            KeyCode::KeyX => "constrain_x",
            KeyCode::KeyY if transforming => "constrain_y",
            KeyCode::KeyZ if transforming => "constrain_z",
            KeyCode::KeyZ if shift => "undo",
            KeyCode::KeyY if shift => "redo",
            KeyCode::KeyY => "constrain_y",
            KeyCode::KeyZ => "constrain_z",
            KeyCode::KeyA if shift => "select_all_or_none",
            KeyCode::KeyD if shift => "duplicate",
            KeyCode::Escape => "quit",
            KeyCode::Enter => "finish",
            KeyCode::Backspace => "delete",
            _ => return,
        };
        crate::ui::scene_actions::cancel_boolean_pick(tree);
        commands::execute(command_map, name, tree);
    }

    pub fn key_released(&mut self, key: KeyCode) {
        self.keys.remove(&key);
    }

    pub fn cursor_moved(&mut self, position: Vec2, over_ui: bool) {
        let delta = position - self.mouse_position;
        self.mouse_position = position;
        if !over_ui {
            self.mouse_delta += delta;
        }
    }

    pub fn set_right_button(&mut self, pressed: bool, over_ui: bool) {
        if !pressed || !over_ui {
            self.right_down = pressed;
        }
    }

    pub fn update(&mut self, camera: &mut Camera, tree: &mut DataTree) {
        if self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight) {
            camera.orbit(self.mouse_delta);
        }
        if self.right_down {
            camera.pan(self.mouse_delta);
        }
        self.mouse_delta = Vec2::ZERO;
        self.update_transformation(camera, tree);
    }

    pub fn pointer_down(
        &mut self,
        camera: &Camera,
        tree: &mut DataTree,
        ghost: Option<uuid::Uuid>,
    ) {
        if matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Grabbing)
        ) {
            return; // A grab commits on release, not on press.
        }
        if !matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Start) | ClaydashValue::None
        ) {
            tree.set_path(
                "editor.state",
                ClaydashValue::EditorState(EditorState::Start),
            );
            tree.make_undo_redo_snapshot();
            return;
        }
        let (origin, direction) = camera.ray(self.mouse_position);
        let scene = objects(tree);
        let ghost = ghost.filter(|id| scene.iter().any(|object| object.uuid == *id));
        if let Some(pick) = crate::ui::scene_actions::pending_boolean(tree) {
            let ghost = ghost
                .filter(|id| crate::ui::scene_actions::can_attach(&scene, pick.target, &[*id]));
            // Ignore the target's group so an overlapping target cannot swallow
            // every click intended for the second object.
            let target_root = crate::ui::scene_actions::viewport_group_root(&scene, pick.target);
            let candidates: Vec<_> = scene
                .iter()
                .filter(|object| {
                    let root = crate::ui::scene_actions::viewport_group_root(&scene, object.uuid);
                    root != target_root
                })
                .cloned()
                .collect();
            let hit = ghost.or_else(|| raymarch(origin, direction, &candidates));
            if let Some(hit) = hit {
                let operand = ghost
                    .unwrap_or_else(|| crate::ui::scene_actions::viewport_group_root(&scene, hit));
                crate::ui::scene_actions::apply_boolean_pick(tree, operand);
            }
            return;
        }
        let Some(hit) = ghost.or_else(|| raymarch(origin, direction, &scene)) else {
            return;
        };
        let mut selection = selected(tree);
        let shift =
            self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
        if selection.contains(&hit) {
            if shift {
                selection.retain(|uuid| *uuid != hit);
            } else {
                selection = if selection.len() == 1 {
                    vec![]
                } else {
                    vec![hit]
                };
            }
        } else if shift {
            selection.push(hit);
        } else {
            selection = vec![hit];
        }
        set_selected(tree, selection);
    }

    pub fn pointer_up(&mut self, camera: &Camera, tree: &mut DataTree) {
        if matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Grabbing)
        ) {
            self.update_transformation(camera, tree);
            tree.set_path(
                "editor.state",
                ClaydashValue::EditorState(EditorState::Start),
            );
            self.transform_session = None;
            tree.make_undo_redo_snapshot();
        }
    }

    fn begin_transform_session(&mut self, mode: EditorState, camera: &Camera, tree: &mut DataTree) {
        let selection = commands::selected_subtree_ids(&objects(tree), &selected(tree));
        let selected_objects: Vec<_> = objects(tree)
            .into_iter()
            .filter(|object| selection.contains(&object.uuid))
            .map(|object| (object.uuid, object.transform))
            .collect();
        if selected_objects.is_empty() {
            tree.set_path(
                "editor.state",
                ClaydashValue::EditorState(EditorState::Start),
            );
            return;
        }

        let center = selected_objects
            .iter()
            .map(|(_, transform)| transform.translation)
            .sum::<Vec3>()
            / selected_objects.len() as f32;
        for (uuid, transform) in &selected_objects {
            tree.set_path(
                &format!("editor.initial_transform.{uuid}"),
                ClaydashValue::Transform(*transform),
            );
        }
        let initial_cursor = camera.cursor_at_depth(self.mouse_position, center);
        self.transform_session = Some(TransformSession {
            mode,
            selection: selected(tree),
            center,
            initial_cursor,
            initial_angle: camera.cursor_angle(self.mouse_position, center),
            initial_radius: initial_cursor.distance(center).max(0.001),
            objects: selected_objects,
        });
    }

    pub fn place_pending_spawn(&mut self, camera: &Camera, tree: &mut DataTree) {
        let ClaydashValue::Uuid(id) = tree.get_path("editor.spawn_at_cursor") else {
            return;
        };
        tree.set_path("editor.spawn_at_cursor", ClaydashValue::None);
        if !matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Grabbing)
        ) {
            return;
        }
        let mut scene = objects(tree);
        let Some(object) = scene.iter_mut().find(|object| object.uuid == id) else {
            return;
        };
        object.transform.translation = camera.cursor_at_depth(self.mouse_position, camera.target);
        set_objects(tree, scene);
        self.begin_transform_session(EditorState::Grabbing, camera, tree);
    }

    fn update_transformation(&mut self, camera: &Camera, tree: &mut DataTree) {
        self.place_pending_spawn(camera, tree);
        let mode = match tree.get_path("editor.state") {
            ClaydashValue::EditorState(mode) => mode,
            _ => EditorState::Start,
        };
        if mode == EditorState::Start {
            self.transform_session = None;
            return;
        }
        if self.transform_session.as_ref().is_none_or(|session| {
            session.mode != mode || session.selection != crate::model::selected_ref(tree)
        }) {
            self.begin_transform_session(mode, camera, tree);
        }
        let Some(session) = self.transform_session.clone() else {
            return;
        };

        let constrain_x = matches!(
            tree.get_path("editor.constrain_x"),
            ClaydashValue::Bool(true)
        );
        let constrain_y = matches!(
            tree.get_path("editor.constrain_y"),
            ClaydashValue::Bool(true)
        );
        let constrain_z = matches!(
            tree.get_path("editor.constrain_z"),
            ClaydashValue::Bool(true)
        );
        let constrained = constrain_x || constrain_y || constrain_z;
        let mask = if constrained {
            Vec3::new(
                constrain_x as u8 as f32,
                constrain_y as u8 as f32,
                constrain_z as u8 as f32,
            )
        } else {
            Vec3::ONE
        };
        let current_cursor = camera.cursor_at_depth(self.mouse_position, session.center);
        let mut scene = objects(tree);

        for object in &mut scene {
            let Some((_, initial)) = session
                .objects
                .iter()
                .find(|(uuid, _)| *uuid == object.uuid)
            else {
                continue;
            };
            match mode {
                EditorState::Grabbing => {
                    object.transform.translation =
                        initial.translation + (current_cursor - session.initial_cursor) * mask;
                }
                EditorState::Scaling => {
                    let factor = (current_cursor.distance(session.center) / session.initial_radius)
                        .max(0.001);
                    let factors = Vec3::ONE + (Vec3::splat(factor) - Vec3::ONE) * mask;
                    object.transform.scale = initial.scale * factors;
                    object.transform.translation =
                        session.center + (initial.translation - session.center) * factors;
                }
                EditorState::Rotating => {
                    let current_angle = camera.cursor_angle(self.mouse_position, session.center);
                    let raw_angle = current_angle - session.initial_angle;
                    let angle = raw_angle.sin().atan2(raw_angle.cos());
                    let axis = if constrained {
                        mask.normalize_or_zero()
                    } else {
                        (camera.target - camera.position).normalize()
                    };
                    let rotation = Quat::from_axis_angle(axis, -angle);
                    object.transform.rotation = rotation * initial.rotation;
                    object.transform.translation =
                        session.center + rotation * (initial.translation - session.center);
                }
                EditorState::Start => {}
            }
        }
        set_objects(tree, scene);
    }
}

pub fn raymarch(origin: Vec3, direction: Vec3, objects: &[SdfObject]) -> Option<uuid::Uuid> {
    let mut point = origin;
    for _ in 0..64 {
        let (distance, object) = scene_distance(point, objects)?;
        if distance < 0.01 {
            return Some(object);
        }
        point += direction * distance.max(0.003) * 0.8;
        if point.distance(origin) > 100.0 {
            return None;
        }
    }
    None
}

fn scene_distance(point: Vec3, objects: &[SdfObject]) -> Option<(f32, uuid::Uuid)> {
    crate::model::scene_sample(point, objects)
}

#[cfg(test)]
mod tests {
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
    fn ghost_click_selects_exact_operand_and_supports_shift() {
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
        assert_eq!(selected(&tree), vec![target, id]);
        interactions.pointer_down(&camera, &mut tree, Some(id));
        assert_eq!(selected(&tree), vec![target]);
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
        commands::start_grab(&mut tree);
        interactions.update(&mut camera, &mut tree);

        interactions.mouse_position.x += 100.0;
        interactions.update(&mut camera, &mut tree);

        assert!(objects(&tree)[0].transform.translation.length() > 0.01);
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
            assert_eq!(selected(&tree), vec![target, group.uuid, third.uuid]);
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
                let operand_id = operand.uuid;
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
                assert_eq!(selected(&tree), vec![operand_id]);
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
    fn new_primitives_spawn_under_cursor_and_follow_it_without_an_offset() {
        for mode in [
            crate::camera::ProjectionMode::Perspective,
            crate::camera::ProjectionMode::Orthographic,
        ] {
            for kind in crate::model::PrimitiveKind::ALL {
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
        let delta = moved[0].transform.translation - scene[0].transform.translation;
        assert!(delta.length() > 0.01);
        assert!(
            (moved[1].transform.translation - scene[1].transform.translation - delta).length()
                < 0.00001
        );
        let mut commands = Commands::new();
        crate::commands::register_all(&mut commands);
        interactions.key_pressed(KeyCode::Escape, false, &commands, &mut tree);
        for (restored, initial) in objects(&tree).iter().zip(&scene) {
            assert_eq!(
                restored.transform.translation,
                initial.transform.translation
            );
        }
    }
}

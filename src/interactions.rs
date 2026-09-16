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
        self.keys.insert(key);
        if egui_wants_keyboard {
            return;
        }
        let shift = self.keys.contains(&KeyCode::ShiftLeft)
            || self.keys.contains(&KeyCode::ShiftRight)
            || self.keys.contains(&KeyCode::SuperLeft)
            || self.keys.contains(&KeyCode::SuperRight);
        let name = match key {
            KeyCode::KeyG => "grab",
            KeyCode::KeyS => "scale",
            KeyCode::KeyR => "rotate",
            KeyCode::KeyX => "constrain_x",
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

    pub fn pointer_down(&mut self, camera: &Camera, tree: &mut DataTree) {
        if !matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Start)
        ) {
            tree.set_path(
                "editor.state",
                ClaydashValue::EditorState(EditorState::Start),
            );
            tree.make_undo_redo_snapshot();
            return;
        }
        let (origin, direction) = camera.ray(self.mouse_position);
        let Some(hit) = raymarch(origin, direction, &objects(tree)) else {
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

    fn begin_transform_session(&mut self, mode: EditorState, camera: &Camera, tree: &mut DataTree) {
        let selection = selected(tree);
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
            center,
            initial_cursor,
            initial_angle: camera.cursor_angle(self.mouse_position, center),
            initial_radius: initial_cursor.distance(center).max(0.001),
            objects: selected_objects,
        });
    }

    fn update_transformation(&mut self, camera: &Camera, tree: &mut DataTree) {
        let mode = match tree.get_path("editor.state") {
            ClaydashValue::EditorState(mode) => mode,
            _ => EditorState::Start,
        };
        if mode == EditorState::Start {
            self.transform_session = None;
            return;
        }
        if self
            .transform_session
            .as_ref()
            .is_none_or(|session| session.mode != mode)
        {
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
        let (distance, object) = objects
            .iter()
            .map(|object| (object.distance(point), object.uuid))
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{commands, model::SdfObject};
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
}

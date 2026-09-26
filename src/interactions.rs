use std::collections::HashSet;

use glam::{Mat4, Quat, Vec2, Vec3};
use winit::keyboard::KeyCode;

use crate::{
    camera::Camera,
    commands::{self, Commands},
    model::{
        objects, selected, set_objects, set_selected, set_selected_exact, ClaydashValue, DataTree,
        EditorState, SdfObject,
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
    last_mouse_position: Vec2,
    rotation_snap_active: bool,
    targets: Vec<commands::TransformTarget>,
    anchors: Vec<Vec3>,
    guides: Vec<crate::guides::FaceGuide>,
}

#[derive(Clone)]
struct ExtrusionSession {
    mode: EditorState,
    object: uuid::Uuid,
    axis: crate::model::VectorAxis,
    positive: bool,
    start_mouse_position: Vec2,
    projected_axis: Vec2,
    initial_transform: crate::model::Transform,
    initial_half_extent: f32,
    initial_face_center: Vec3,
    world_direction_per_unit: Vec3,
    guides: Vec<crate::guides::FaceGuide>,
}

struct ShiftPanSession {
    start: Vec2,
    last_applied: Vec2,
    ghost: Option<uuid::Uuid>,
    reference: Option<Vec3>,
}

const PAN_DRAG_THRESHOLD: f32 = 3.0;
const ROTATION_SNAP_DEGREES: f32 = 5.0;

pub(crate) fn rotation_drag_angle(raw_angle: f32, snap: bool) -> f32 {
    let wrapped = raw_angle.sin().atan2(raw_angle.cos());
    if snap {
        let increment = ROTATION_SNAP_DEGREES.to_radians();
        (wrapped / increment).round() * increment
    } else {
        wrapped
    }
}

pub(crate) fn rotation_snap_active(was_active: bool, ctrl_down: bool, pointer_moved: bool) -> bool {
    if ctrl_down {
        true
    } else if pointer_moved {
        false
    } else {
        was_active
    }
}

#[derive(Default)]
enum NumericRotationInput {
    #[default]
    Idle,
    Editing(String),
}

fn rotation_input_character(key: KeyCode) -> Option<char> {
    match key {
        KeyCode::Digit0 | KeyCode::Numpad0 => Some('0'),
        KeyCode::Digit1 | KeyCode::Numpad1 => Some('1'),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some('2'),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some('3'),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some('4'),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some('5'),
        KeyCode::Digit6 | KeyCode::Numpad6 => Some('6'),
        KeyCode::Digit7 | KeyCode::Numpad7 => Some('7'),
        KeyCode::Digit8 | KeyCode::Numpad8 => Some('8'),
        KeyCode::Digit9 | KeyCode::Numpad9 => Some('9'),
        KeyCode::Period | KeyCode::NumpadDecimal => Some('.'),
        _ => None,
    }
}

pub struct InteractionState {
    keys: HashSet<KeyCode>,
    pub mouse_position: Vec2,
    mouse_delta: Vec2,
    right_down: bool,
    right_pan_reference: Option<Vec3>,
    shift_pan: Option<ShiftPanSession>,
    transform_session: Option<TransformSession>,
    curve_grab_mouse_start: Option<Vec2>,
    extrusion_session: Option<ExtrusionSession>,
    active_guide: Option<crate::guides::ActiveGuideSet>,
    numeric_rotation: NumericRotationInput,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            keys: HashSet::new(),
            mouse_position: Vec2::ZERO,
            mouse_delta: Vec2::ZERO,
            right_down: false,
            right_pan_reference: None,
            shift_pan: None,
            transform_session: None,
            curve_grab_mouse_start: None,
            extrusion_session: None,
            active_guide: None,
            numeric_rotation: NumericRotationInput::Idle,
        }
    }
}

impl InteractionState {
    pub(crate) fn active_guide(&self) -> Option<crate::guides::ActiveGuideSet> {
        self.active_guide
    }
}

mod keyboard;
mod pointer;
mod transform;

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub object: uuid::Uuid,
    pub position: Vec3,
}

pub fn raymarch_hit(origin: Vec3, direction: Vec3, objects: &[SdfObject]) -> Option<RayHit> {
    let mut point = origin;
    let march_factor = objects.iter().fold(0.8_f32, |factor, object| {
        if let crate::model::SdfParams::LoftParams(loft) = &object.params {
            factor.min(loft.march_factor())
        } else {
            factor
        }
    });
    let max_steps = if march_factor < 0.5 { 128 } else { 64 };
    for _ in 0..max_steps {
        let (distance, object) = scene_distance(point, objects)?;
        if distance < 0.01 {
            return Some(RayHit {
                object,
                position: point,
            });
        }
        point += direction * distance.max(0.003) * march_factor;
        if point.distance(origin) > 100.0 {
            return None;
        }
    }
    None
}

pub fn raymarch(origin: Vec3, direction: Vec3, objects: &[SdfObject]) -> Option<uuid::Uuid> {
    raymarch_hit(origin, direction, objects).map(|hit| hit.object)
}

fn pan_reference(camera: &Camera, tree: &DataTree, cursor: Vec2) -> Option<Vec3> {
    let (origin, direction) = camera.ray(cursor);
    raymarch_hit(origin, direction, &objects(tree)).map(|hit| hit.position)
}

fn scene_distance(point: Vec3, objects: &[SdfObject]) -> Option<(f32, uuid::Uuid)> {
    crate::model::scene_sample(point, objects)
}

#[cfg(test)]
mod selection_tests;

#[cfg(test)]
mod transform_tests;

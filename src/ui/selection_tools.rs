use super::*;
use crate::model::{ClaydashValue, EditorState};

#[derive(Default, PartialEq, Eq)]
enum SelectionTool {
    #[default]
    Select,
    Box,
    FaceCut,
}

struct FaceCutDraft {
    face: crate::model::ModelingFaceSelection,
    vertices: Vec<Vec2>,
    phase: FaceCutPhase,
}

enum FaceCutPhase {
    Outline,
    ChooseRegion {
        closed: bool,
    },
    Depth {
        object: uuid::Uuid,
        hole: Option<uuid::Uuid>,
        region: usize,
        closed: bool,
        start_pointer: egui::Pos2,
        projected_axis: egui::Vec2,
    },
}

struct TransformGesture {
    action: GizmoAction,
    start: Vec3,
    start_pointer: egui::Pos2,
    last_pointer: egui::Pos2,
    center: Vec3,
    initial_angle: f32,
    initial_axis_vector: Option<Vec3>,
    initial_radius: f32,
    axis_screen_direction: Option<egui::Vec2>,
    world_units_per_point: f32,
    rotation_snap_active: bool,
    targets: Vec<commands::TransformTarget>,
    anchors: Vec<Vec3>,
    guides: Vec<crate::guides::FaceGuide>,
    active_guide: Option<crate::guides::ActiveGuideSet>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum GizmoAction {
    MoveFree,
    MoveAxis(usize),
    ScaleUniform,
    ScaleAxis(usize),
    RotateView,
    RotateAxis(usize),
}

impl GizmoAction {
    fn tooltip(self) -> String {
        match self {
            Self::MoveFree => {
                "Drag to move in the view plane · Alt bypasses guides · Esc cancels".into()
            }
            Self::MoveAxis(axis) => {
                format!(
                    "Drag to move on {} · Alt bypasses guides · Esc cancels",
                    axis_label(axis)
                )
            }
            Self::ScaleUniform => "Drag to scale uniformly · Esc cancels".into(),
            Self::ScaleAxis(axis) => {
                format!("Drag to scale on {} · Esc cancels", axis_label(axis))
            }
            Self::RotateView => "Drag to rotate in the view plane · Esc cancels".into(),
            Self::RotateAxis(axis) => {
                format!("Drag to rotate around {} · Esc cancels", axis_label(axis))
            }
        }
    }
}

#[derive(Clone)]
struct AxisGizmo {
    axis: usize,
    direction: egui::Vec2,
    scale_handle: egui::Pos2,
    move_tip: egui::Pos2,
}

struct RotationArc {
    axis: usize,
    u: Vec3,
    v: Vec3,
    middle_angle: f32,
    points: Vec<egui::Pos2>,
}

struct GizmoGeometry {
    center_world: Vec3,
    center: egui::Pos2,
    axes: Vec<AxisGizmo>,
    rotation_arcs: Vec<RotationArc>,
    world_ring_radius: f32,
    pixels_per_point: f32,
    uniform_scale: egui::Pos2,
    free_move: egui::Pos2,
    view_ring: Vec<egui::Pos2>,
    view_rotate: egui::Pos2,
    world_units_per_point: f32,
}

const SCALE_HANDLE_DISTANCE: f32 = 38.0;
const MOVE_HANDLE_DISTANCE: f32 = 82.0;
const ROTATION_RING_RADIUS: f32 = 68.0;
const VIEW_RING_RADIUS: f32 = 92.0;
const ROTATION_ARC_DEGREES: f32 = 45.0;
const VIEW_ROTATE_ANGLE_DEGREES: f32 = 145.0;
const HANDLE_HIT_RADIUS: f32 = 10.0;
const RING_HIT_RADIUS: f32 = 6.0;

fn world_axis(axis: usize) -> Vec3 {
    match axis {
        0 => Vec3::X,
        1 => Vec3::Y,
        _ => Vec3::Z,
    }
}

fn cursor_vector_on_axis_plane(
    camera: &Camera,
    cursor: Vec2,
    center: Vec3,
    axis: Vec3,
) -> Option<Vec3> {
    let (origin, direction) = camera.ray(cursor);
    let denominator = direction.dot(axis);
    if denominator.abs() < 0.0001 {
        return None;
    }
    let point = origin + direction * ((center - origin).dot(axis) / denominator);
    let vector = point - center;
    let length = vector.length();
    (length > 0.0001).then_some(vector / length)
}

fn axis_rotation_drag_angle(
    camera: &Camera,
    cursor: Vec2,
    center: Vec3,
    axis: Vec3,
    initial: Vec3,
    snap: bool,
) -> Option<f32> {
    let current = cursor_vector_on_axis_plane(camera, cursor, center, axis)?;
    let raw_angle = axis.dot(initial.cross(current)).atan2(initial.dot(current));
    Some(crate::interactions::rotation_drag_angle(raw_angle, snap))
}

fn rotation_ring_basis(axis: usize) -> (Vec3, Vec3) {
    match axis {
        0 => (Vec3::Y, Vec3::Z),
        1 => (Vec3::Z, Vec3::X),
        _ => (Vec3::X, Vec3::Y),
    }
}

fn rotation_arc_points(
    camera: &Camera,
    center: Vec3,
    pixels_per_point: f32,
    u: Vec3,
    v: Vec3,
    world_radius: f32,
    middle_angle: f32,
) -> Vec<egui::Pos2> {
    let half_arc = (ROTATION_ARC_DEGREES * 0.5).to_radians();
    (0..=32)
        .filter_map(|step| {
            let angle = middle_angle - half_arc + step as f32 / 32.0 * half_arc * 2.0;
            camera.project(
                center + (u * angle.cos() + v * angle.sin()) * world_radius,
                pixels_per_point,
            )
        })
        .collect()
}

fn selection_gizmo_geometry(
    camera: &Camera,
    center: Vec3,
    pixels_per_point: f32,
) -> Option<GizmoGeometry> {
    let center_screen = camera.project(center, pixels_per_point)?;
    let projected_axes: Vec<_> = (0..3)
        .filter_map(|axis| {
            camera
                .project(center + world_axis(axis), pixels_per_point)
                .map(|point| (axis, point - center_screen))
        })
        .collect();
    let pixels_per_world_unit = projected_axes
        .iter()
        .map(|(_, delta)| delta.length())
        .fold(0.0_f32, f32::max);
    if pixels_per_world_unit < 0.001 {
        return None;
    }
    let world_units_per_point = 1.0 / pixels_per_world_unit;
    let axes = projected_axes
        .iter()
        .filter_map(|(axis, delta)| {
            let length = delta.length();
            (length > 1.0).then(|| {
                let direction = *delta / length;
                AxisGizmo {
                    axis: *axis,
                    direction,
                    scale_handle: center_screen + direction * SCALE_HANDLE_DISTANCE,
                    move_tip: center_screen + direction * MOVE_HANDLE_DISTANCE,
                }
            })
        })
        .collect();

    let world_ring_radius = ROTATION_RING_RADIUS * world_units_per_point;
    let rotation_arcs = (0..3)
        .map(|axis| {
            let (u, v) = rotation_ring_basis(axis);
            // Explicit cyclic anchors keep each arc centered on a different
            // positive axis: X rotation on Z, Y on X, and Z on Y.
            let middle_angle = std::f32::consts::FRAC_PI_2;
            let points = rotation_arc_points(
                camera,
                center,
                pixels_per_point,
                u,
                v,
                world_ring_radius,
                middle_angle,
            );
            RotationArc {
                axis,
                u,
                v,
                middle_angle,
                points,
            }
        })
        .collect();

    let global_handle_distance = egui::vec2(34.0, -64.0).length() * 1.25;
    let global_handle_diagonal = global_handle_distance / std::f32::consts::SQRT_2;
    let uniform_scale =
        center_screen + egui::vec2(-global_handle_diagonal, -global_handle_diagonal);
    // Mirror the global scale handle across the vertical center line. Both
    // global controls remain on 45-degree diagonals and share one baseline.
    let free_move = center_screen + egui::vec2(global_handle_diagonal, -global_handle_diagonal);
    let view_angle = VIEW_ROTATE_ANGLE_DEGREES.to_radians();
    let view_rotate =
        center_screen + egui::vec2(view_angle.cos(), view_angle.sin()) * VIEW_RING_RADIUS;
    let view_ring = screen_arc(
        center_screen,
        VIEW_RING_RADIUS,
        view_angle,
        ROTATION_ARC_DEGREES.to_radians(),
    );
    Some(GizmoGeometry {
        center_world: center,
        center: center_screen,
        axes,
        rotation_arcs,
        world_ring_radius,
        pixels_per_point,
        uniform_scale,
        free_move,
        view_ring,
        view_rotate,
        world_units_per_point,
    })
}

fn screen_arc(center: egui::Pos2, radius: f32, middle_angle: f32, span: f32) -> Vec<egui::Pos2> {
    (0..=32)
        .map(|step| {
            let angle = middle_angle - span * 0.5 + step as f32 / 32.0 * span;
            center + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

fn gizmo_color(color: Color32, emphasized: bool) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        if emphasized { 255 } else { 180 },
    )
}

fn gizmo_fill(emphasized: bool) -> Color32 {
    Color32::from_black_alpha(if emphasized { 200 } else { 110 })
}

fn view_rotation_gizmo_angle(
    initial_pointer_angle: f32,
    current_pointer_angle: f32,
    snap: bool,
) -> f32 {
    let raw_angle = current_pointer_angle - initial_pointer_angle;
    let applied_angle = crate::interactions::rotation_drag_angle(raw_angle, snap);
    VIEW_ROTATE_ANGLE_DEGREES.to_radians() + applied_angle
}

fn distance_to_segment(point: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let segment = b - a;
    let length_sq = segment.length_sq();
    if length_sq <= 0.0001 {
        return point.distance(a);
    }
    let t = ((point - a).dot(segment) / length_sq).clamp(0.0, 1.0);
    point.distance(a + segment * t)
}

fn distance_to_polyline(point: egui::Pos2, points: &[egui::Pos2]) -> f32 {
    points
        .windows(2)
        .map(|segment| distance_to_segment(point, segment[0], segment[1]))
        .fold(f32::INFINITY, f32::min)
}

fn gizmo_action_at(point: egui::Pos2, geometry: &GizmoGeometry) -> Option<GizmoAction> {
    if point.distance(geometry.free_move) <= HANDLE_HIT_RADIUS + 4.0 {
        return Some(GizmoAction::MoveFree);
    }
    if point.distance(geometry.uniform_scale) <= HANDLE_HIT_RADIUS {
        return Some(GizmoAction::ScaleUniform);
    }
    if point.distance(geometry.view_rotate) <= HANDLE_HIT_RADIUS {
        return Some(GizmoAction::RotateView);
    }
    for axis in &geometry.axes {
        if point.distance(axis.move_tip) <= HANDLE_HIT_RADIUS {
            return Some(GizmoAction::MoveAxis(axis.axis));
        }
        if point.distance(axis.scale_handle) <= HANDLE_HIT_RADIUS {
            return Some(GizmoAction::ScaleAxis(axis.axis));
        }
    }
    if distance_to_polyline(point, &geometry.view_ring) <= RING_HIT_RADIUS {
        return Some(GizmoAction::RotateView);
    }
    geometry
        .rotation_arcs
        .iter()
        .filter_map(|arc| {
            let distance = distance_to_polyline(point, &arc.points);
            (distance <= RING_HIT_RADIUS).then_some((distance, arc.axis))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, axis)| GizmoAction::RotateAxis(axis))
}

fn register_polyline_regions(regions: &mut Vec<egui::Rect>, points: &[egui::Pos2]) {
    for segment in points.windows(2) {
        regions.push(egui::Rect::from_two_pos(segment[0], segment[1]).expand(RING_HIT_RADIUS));
    }
}

fn paint_dashed_line(painter: &egui::Painter, start: egui::Pos2, end: egui::Pos2, stroke: Stroke) {
    let delta = end - start;
    for step in 0..4 {
        let a = step as f32 / 4.0;
        let b = (a + 0.14).min(1.0);
        painter.line_segment([start + delta * a, start + delta * b], stroke);
    }
}

fn paint_move_glyph(painter: &egui::Painter, position: egui::Pos2, stroke: Stroke) {
    for axis in [egui::vec2(9.0, 0.0), egui::vec2(0.0, 9.0)] {
        painter.line_segment([position - axis, position + axis], stroke);
        for sign in [-1.0, 1.0] {
            let tip = position + axis * sign;
            let side = egui::vec2(-axis.y, axis.x) / 3.0;
            painter.line_segment([tip, tip - axis * sign / 3.0 + side], stroke);
            painter.line_segment([tip, tip - axis * sign / 3.0 - side], stroke);
        }
    }
}

enum Gesture {
    Transform(TransformGesture),
    Box {
        start: egui::Pos2,
        end: egui::Pos2,
        additive: bool,
    },
}

#[derive(Default)]
pub(super) struct SelectionTools {
    tool: SelectionTool,
    gesture: Option<Gesture>,
    face_cut: Option<FaceCutDraft>,
}

impl SelectionTools {
    pub fn active(&self) -> bool {
        self.gesture.is_some() || self.tool == SelectionTool::FaceCut
    }
    pub fn box_mode(&self) -> bool {
        self.tool == SelectionTool::Box
    }

    pub fn face_cut_mode(&self) -> bool {
        self.tool == SelectionTool::FaceCut
    }
}

fn box_selection(
    scene: &[SdfObject],
    camera: &Camera,
    rect: egui::Rect,
    scale: f32,
    mut ids: Vec<uuid::Uuid>,
) -> Vec<uuid::Uuid> {
    for object in scene {
        let position =
            crate::model::object_world_matrix(scene, object.uuid).transform_point3(Vec3::ZERO);
        if (position - camera.position).dot(camera.target - camera.position) <= 0.0 {
            continue;
        }
        if camera
            .project(position, scale)
            .is_some_and(|point| rect.contains(point))
        {
            let id = scene_actions::viewport_group_root(scene, object.uuid);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

mod state;

mod gizmo;

mod face_cut;

#[cfg(test)]
mod tests;

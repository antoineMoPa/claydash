use super::*;
use crate::model::{ClaydashValue, EditorState};

#[derive(Default, PartialEq, Eq)]
enum SelectionTool {
    #[default]
    Select,
    Box,
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
            Self::MoveFree => "Drag to move in the view plane · Esc cancels".into(),
            Self::MoveAxis(axis) => {
                format!("Drag to move on {} · Esc cancels", axis_label(axis))
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
}

impl SelectionTools {
    pub fn active(&self) -> bool {
        self.gesture.is_some()
    }
    pub fn box_mode(&self) -> bool {
        self.tool == SelectionTool::Box
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

impl UiState {
    pub fn selection_gesture_active(&self) -> bool {
        self.selection_tools.active()
    }

    pub fn box_selection_enabled(&self, tree: &DataTree) -> bool {
        self.selection_tools.box_mode()
            && scene_actions::pending_boolean(tree).is_none()
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
            )
    }

    pub fn enter_box_selection_mode(&mut self, tree: &DataTree) -> bool {
        if self.selection_tools.active()
            || scene_actions::pending_boolean(tree).is_some()
            || !matches!(
                tree.get_path("editor.state"),
                ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
            )
        {
            return false;
        }
        self.selection_tools.tool = SelectionTool::Box;
        true
    }

    pub(super) fn draw_selection_toolbar(&mut self, ctx: &egui::Context, viewport: egui::Rect) {
        let area = egui::Area::new("selection-tools".into())
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(viewport.right_bottom() - egui::vec2(6.0, 6.0))
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport);
                ui.add_enabled_ui(!self.selection_tools.active(), |ui| {
                    ui.horizontal(|ui| {
                        ui.set_height(26.0);
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (tool, source, tooltip) in [
                            (
                                SelectionTool::Select,
                                egui::include_image!(
                                    "../../assets/icons/lucide/mouse-pointer-2.svg"
                                ),
                                "Select objects",
                            ),
                            (
                                SelectionTool::Box,
                                egui::include_image!("../../assets/icons/lucide/scan.svg"),
                                "Box select (B) · drag a rectangle · Shift adds",
                            ),
                        ] {
                            if selectable_view_button(
                                ui,
                                source,
                                tooltip,
                                self.selection_tools.tool == tool,
                            )
                            .clicked()
                            {
                                self.selection_tools.tool = tool;
                            }
                        }
                    });
                });
            });
        self.regions.push(area.response.rect);
    }

    fn draw_transform_gizmo(
        &mut self,
        ui: &mut egui::Ui,
        geometry: &GizmoGeometry,
        pointer: Option<egui::Pos2>,
        camera: &Camera,
        snap_rotation: bool,
    ) {
        let hovered = pointer.and_then(|point| gizmo_action_at(point, geometry));
        let (active, active_start_pointer, active_initial_angle, active_initial_axis_vector) =
            match &self.selection_tools.gesture {
                Some(Gesture::Transform(session)) => (
                    Some(session.action),
                    Some(session.start_pointer),
                    Some(session.initial_angle),
                    session.initial_axis_vector,
                ),
                _ => (None, None, None, None),
            };
        let emphasized = |action| hovered == Some(action) || active == Some(action);
        let stroke_width = |action| {
            if action == GizmoAction::MoveFree {
                1.7
            } else if emphasized(action) {
                2.8
            } else {
                1.6
            }
        };
        let rotation_stroke_width = |action| if emphasized(action) { 3.6 } else { 2.6 };
        let painter = ui.painter();

        for arc in &geometry.rotation_arcs {
            let action = GizmoAction::RotateAxis(arc.axis);
            let points = if active == Some(action) {
                pointer
                    .zip(active_initial_angle)
                    .map(|(point, initial_angle)| {
                        let delta = point - geometry.center;
                        let current_angle = delta.y.atan2(delta.x);
                        let raw_angle = current_angle - initial_angle;
                        let applied_angle = active_initial_axis_vector
                            .and_then(|initial| {
                                axis_rotation_drag_angle(
                                    camera,
                                    Vec2::new(point.x, point.y) * geometry.pixels_per_point,
                                    geometry.center_world,
                                    world_axis(arc.axis),
                                    initial,
                                    snap_rotation,
                                )
                            })
                            .unwrap_or_else(|| {
                                -crate::interactions::rotation_drag_angle(raw_angle, snap_rotation)
                            });
                        rotation_arc_points(
                            camera,
                            geometry.center_world,
                            geometry.pixels_per_point,
                            arc.u,
                            arc.v,
                            geometry.world_ring_radius,
                            arc.middle_angle + applied_angle,
                        )
                    })
                    .unwrap_or_else(|| arc.points.clone())
            } else {
                arc.points.clone()
            };
            painter.add(egui::Shape::line(
                points.clone(),
                Stroke::new(
                    rotation_stroke_width(action),
                    gizmo_color(axis_color(arc.axis), emphasized(action)),
                ),
            ));
            register_polyline_regions(&mut self.regions, &points);
        }

        let view_action = GizmoAction::RotateView;
        let (view_rotate, view_ring) = if active == Some(view_action) {
            if let (Some(point), Some(initial_angle)) = (pointer, active_initial_angle) {
                let delta = point - geometry.center;
                let current_angle = delta.y.atan2(delta.x);
                let angle = view_rotation_gizmo_angle(initial_angle, current_angle, snap_rotation);
                let direction = egui::vec2(angle.cos(), angle.sin());
                (
                    geometry.center + direction * VIEW_RING_RADIUS,
                    screen_arc(
                        geometry.center,
                        VIEW_RING_RADIUS,
                        angle,
                        ROTATION_ARC_DEGREES.to_radians(),
                    ),
                )
            } else {
                (geometry.view_rotate, geometry.view_ring.clone())
            }
        } else {
            (geometry.view_rotate, geometry.view_ring.clone())
        };
        let view_stroke = Stroke::new(
            rotation_stroke_width(view_action),
            gizmo_color(Color32::WHITE, emphasized(view_action)),
        );
        painter.add(egui::Shape::line(view_ring.clone(), view_stroke));
        register_polyline_regions(&mut self.regions, &view_ring);

        for axis in &geometry.axes {
            let scale_action = GizmoAction::ScaleAxis(axis.axis);
            let move_action = GizmoAction::MoveAxis(axis.axis);
            let scale_color = gizmo_color(axis_color(axis.axis), emphasized(scale_action));
            let move_color = gizmo_color(axis_color(axis.axis), emphasized(move_action));
            let scale_stroke = Stroke::new(stroke_width(scale_action), scale_color);
            let move_stroke = Stroke::new(stroke_width(move_action), move_color);
            let scale_handle = if active == Some(scale_action) {
                pointer
                    .zip(active_start_pointer)
                    .map(|(point, start)| {
                        axis.scale_handle + axis.direction * (point - start).dot(axis.direction)
                    })
                    .unwrap_or(axis.scale_handle)
            } else {
                axis.scale_handle
            };
            painter.line_segment([geometry.center, scale_handle], scale_stroke);
            let arrow_base = axis.move_tip - axis.direction * 11.0;
            paint_dashed_line(painter, scale_handle, arrow_base, move_stroke);

            let scale_rect = egui::Rect::from_center_size(scale_handle, egui::vec2(13.0, 13.0));
            painter.rect_stroke(scale_rect, 1.0, scale_stroke, egui::StrokeKind::Inside);

            let perpendicular = egui::vec2(-axis.direction.y, axis.direction.x);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    axis.move_tip,
                    arrow_base + perpendicular * 6.0,
                    arrow_base - perpendicular * 6.0,
                ],
                move_color,
                move_stroke,
            ));
            self.regions.push(
                egui::Rect::from_center_size(
                    scale_handle,
                    egui::Vec2::splat(HANDLE_HIT_RADIUS * 2.0),
                )
                .intersect(ui.clip_rect()),
            );
            self.regions.push(
                egui::Rect::from_center_size(
                    axis.move_tip,
                    egui::Vec2::splat(HANDLE_HIT_RADIUS * 2.0),
                )
                .intersect(ui.clip_rect()),
            );
        }

        let uniform_action = GizmoAction::ScaleUniform;
        let uniform_scale = if active == Some(uniform_action) {
            pointer.unwrap_or(geometry.uniform_scale)
        } else {
            geometry.uniform_scale
        };
        let uniform_stroke = Stroke::new(
            stroke_width(uniform_action),
            gizmo_color(Color32::WHITE, emphasized(uniform_action)),
        );
        paint_dashed_line(painter, geometry.center, uniform_scale, uniform_stroke);
        let uniform_rect = egui::Rect::from_center_size(uniform_scale, egui::vec2(15.0, 15.0));
        painter.rect_stroke(uniform_rect, 1.0, uniform_stroke, egui::StrokeKind::Inside);
        self.regions.push(
            egui::Rect::from_center_size(uniform_scale, egui::Vec2::splat(HANDLE_HIT_RADIUS * 2.0))
                .intersect(ui.clip_rect()),
        );

        painter.circle_filled(view_rotate, 7.0, gizmo_fill(emphasized(view_action)));
        painter.circle_stroke(view_rotate, 7.0, view_stroke);
        self.regions.push(
            egui::Rect::from_center_size(view_rotate, egui::Vec2::splat(HANDLE_HIT_RADIUS * 2.0))
                .intersect(ui.clip_rect()),
        );

        painter.circle_filled(geometry.center, 3.5, Color32::from_black_alpha(115));
        painter.circle_stroke(
            geometry.center,
            3.5,
            Stroke::new(1.3, gizmo_color(Color32::WHITE, hovered.is_some())),
        );

        let free_action = GizmoAction::MoveFree;
        paint_move_glyph(
            painter,
            geometry.free_move,
            Stroke::new(
                stroke_width(free_action),
                gizmo_color(Color32::WHITE, emphasized(free_action)),
            ),
        );
        // Keep this last: focused gesture tests and assistive tooling identify
        // the free-move control as the final discrete gizmo region.
        self.regions.push(
            egui::Rect::from_center_size(geometry.free_move, egui::vec2(28.0, 28.0))
                .intersect(ui.clip_rect()),
        );
    }

    pub(super) fn draw_selection_tools(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
    ) {
        let scale = ui.ctx().pixels_per_point();
        let (pointer, pressed, released, cancel, additive, navigating, ctrl_snap_rotation) = ui
            .input(|i| {
                (
                    i.pointer.interact_pos(),
                    i.pointer.primary_pressed(),
                    i.pointer.primary_released(),
                    i.key_pressed(egui::Key::Escape) || !i.focused,
                    i.modifiers.shift,
                    i.modifiers.ctrl || i.pointer.secondary_down(),
                    i.modifiers.ctrl,
                )
            });
        let snap_rotation = match &mut self.selection_tools.gesture {
            Some(Gesture::Transform(session))
                if matches!(
                    session.action,
                    GizmoAction::RotateView | GizmoAction::RotateAxis(_)
                ) =>
            {
                let pointer_moved =
                    pointer.is_some_and(|point| point.distance(session.last_pointer) > 0.001);
                session.rotation_snap_active = crate::interactions::rotation_snap_active(
                    session.rotation_snap_active,
                    ctrl_snap_rotation,
                    pointer_moved,
                );
                if let Some(point) = pointer {
                    session.last_pointer = point;
                }
                session.rotation_snap_active
            }
            _ => ctrl_snap_rotation,
        };
        if cancel {
            if let Some(Gesture::Transform(session)) = self.selection_tools.gesture.take() {
                let mut scene = objects(tree);
                for target in session.targets {
                    commands::set_transform_target(
                        &mut scene,
                        target.kind,
                        target.id,
                        target.transform,
                    );
                }
                set_objects(tree, scene);
            }
            self.selection_tools.gesture = None;
            return;
        }
        if !matches!(
            tree.get_path("editor.state"),
            ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
        ) {
            return;
        }
        let scene = objects(tree);
        let selection = selected(tree);
        let ids = commands::effective_selected_ids(tree);
        let selected_objects: Vec<_> = scene.iter().filter(|o| ids.contains(&o.uuid)).collect();
        if !self.selection_tools.box_mode() && !selected_objects.is_empty() {
            let center = selected_objects
                .iter()
                .map(|o| {
                    crate::model::object_world_matrix(&scene, o.uuid).transform_point3(Vec3::ZERO)
                })
                .sum::<Vec3>()
                / selected_objects.len() as f32;
            if let Some(geometry) = selection_gizmo_geometry(camera, center, scale) {
                self.draw_transform_gizmo(ui, &geometry, pointer, camera, snap_rotation);
                let hovered_action = pointer.and_then(|point| gizmo_action_at(point, &geometry));
                if let (Some(point), Some(action)) = (pointer, hovered_action) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    ui.interact(
                        egui::Rect::from_center_size(point, egui::vec2(2.0, 2.0)),
                        egui::Id::new(("selection-transform-tooltip", action)),
                        egui::Sense::hover(),
                    )
                    .on_hover_text(action.tooltip());
                    if pressed && !navigating && !self.selection_tools.active() {
                        let physical = Vec2::new(point.x, point.y) * scale;
                        let delta = point - geometry.center;
                        let axis_screen_direction = match action {
                            GizmoAction::MoveAxis(axis) | GizmoAction::ScaleAxis(axis) => geometry
                                .axes
                                .iter()
                                .find(|gizmo| gizmo.axis == axis)
                                .map(|gizmo| gizmo.direction),
                            _ => None,
                        };
                        let initial_axis_vector = match action {
                            GizmoAction::RotateAxis(axis) => cursor_vector_on_axis_plane(
                                camera,
                                physical,
                                center,
                                world_axis(axis),
                            ),
                            _ => None,
                        };
                        self.selection_tools.gesture = Some(Gesture::Transform(TransformGesture {
                            action,
                            start: camera.cursor_on_plane(physical, center),
                            start_pointer: point,
                            last_pointer: point,
                            center,
                            initial_angle: delta.y.atan2(delta.x),
                            initial_axis_vector,
                            initial_radius: delta.length().max(0.001),
                            axis_screen_direction,
                            world_units_per_point: geometry.world_units_per_point,
                            rotation_snap_active: snap_rotation,
                            targets: commands::transform_targets(tree),
                        }));
                    }
                }
            }
        }
        if self.selection_tools.box_mode()
            && !egui::Popup::is_any_open(ui.ctx())
            && pressed
            && !navigating
            && !self.selection_tools.active()
        {
            if let Some(p) = pointer.filter(|p| {
                ui.clip_rect().contains(*p) && !self.regions.iter().any(|r| r.contains(*p))
            }) {
                self.selection_tools.gesture = Some(Gesture::Box {
                    start: p,
                    end: p,
                    additive,
                });
            }
        }
        match &mut self.selection_tools.gesture {
            Some(Gesture::Transform(session)) => {
                if let Some(p) = pointer {
                    let mut scene = scene.clone();
                    let operation = match session.action {
                        GizmoAction::MoveFree => {
                            let physical = Vec2::new(p.x, p.y) * scale;
                            let delta =
                                camera.cursor_on_plane(physical, session.center) - session.start;
                            glam::Mat4::from_translation(delta)
                        }
                        GizmoAction::MoveAxis(axis) => {
                            let screen_direction =
                                session.axis_screen_direction.unwrap_or_default();
                            let amount = (p - session.start_pointer).dot(screen_direction)
                                * session.world_units_per_point;
                            glam::Mat4::from_translation(world_axis(axis) * amount)
                        }
                        GizmoAction::ScaleUniform => {
                            let center_screen = camera.project(session.center, scale).unwrap_or(p);
                            let factor =
                                (p.distance(center_screen) / session.initial_radius).max(0.001);
                            glam::Mat4::from_translation(session.center)
                                * glam::Mat4::from_scale(Vec3::splat(factor))
                                * glam::Mat4::from_translation(-session.center)
                        }
                        GizmoAction::ScaleAxis(axis) => {
                            let screen_direction =
                                session.axis_screen_direction.unwrap_or_default();
                            let factor = (1.0
                                + (p - session.start_pointer).dot(screen_direction)
                                    / SCALE_HANDLE_DISTANCE)
                                .max(0.001);
                            let mut factors = Vec3::ONE;
                            factors[axis] = factor;
                            glam::Mat4::from_translation(session.center)
                                * glam::Mat4::from_scale(factors)
                                * glam::Mat4::from_translation(-session.center)
                        }
                        GizmoAction::RotateView | GizmoAction::RotateAxis(_) => {
                            let center_screen = camera.project(session.center, scale).unwrap_or(p);
                            let delta = p - center_screen;
                            let raw_angle = delta.y.atan2(delta.x) - session.initial_angle;
                            let angle =
                                crate::interactions::rotation_drag_angle(raw_angle, snap_rotation);
                            let (rotation_axis, applied_angle) = match session.action {
                                GizmoAction::RotateAxis(axis) => {
                                    let rotation_axis = world_axis(axis);
                                    let world_angle = session
                                        .initial_axis_vector
                                        .and_then(|initial| {
                                            axis_rotation_drag_angle(
                                                camera,
                                                Vec2::new(p.x, p.y) * scale,
                                                session.center,
                                                rotation_axis,
                                                initial,
                                                snap_rotation,
                                            )
                                        })
                                        .unwrap_or(-angle);
                                    (rotation_axis, world_angle)
                                }
                                GizmoAction::RotateView => {
                                    let view = camera.target - camera.position;
                                    (view / view.length().max(0.0001), angle)
                                }
                                _ => unreachable!(),
                            };
                            glam::Mat4::from_translation(session.center)
                                * glam::Mat4::from_quat(glam::Quat::from_axis_angle(
                                    rotation_axis,
                                    applied_angle,
                                ))
                                * glam::Mat4::from_translation(-session.center)
                        }
                    };
                    for target in &session.targets {
                        let local = target.parent_world.inverse() * operation * target.world;
                        let (scale, rotation, translation) = local.to_scale_rotation_translation();
                        commands::set_transform_target(
                            &mut scene,
                            target.kind,
                            target.id,
                            crate::model::Transform {
                                translation,
                                rotation,
                                scale,
                            },
                        );
                    }
                    set_objects(tree, scene);
                }
                if released {
                    tree.make_undo_redo_snapshot();
                }
            }
            Some(Gesture::Box {
                start,
                end,
                additive,
            }) => {
                if let Some(p) = pointer {
                    *end = ui.clip_rect().clamp(p);
                }
                let rect = egui::Rect::from_two_pos(*start, *end);
                let color = ui.visuals().selection.bg_fill;
                ui.painter()
                    .rect_filled(rect, 0.0, color.gamma_multiply(0.15));
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
                if released {
                    // Match UI coordinates so the click tolerance stays consistent
                    // across display scales. Exactly 3 px remains a box gesture.
                    if start.distance(*end) < 3.0 {
                        self.selection_tools.tool = SelectionTool::Select;
                        crate::interactions::InteractionState::select_at(
                            camera,
                            tree,
                            Vec2::new(end.x, end.y) * scale,
                            self.ghosts.pick(*end),
                            *additive,
                        );
                    } else {
                        set_selected(
                            tree,
                            box_selection(
                                &scene,
                                camera,
                                rect,
                                scale,
                                if *additive { selection } else { vec![] },
                            ),
                        );
                    }
                }
            }
            None => {}
        }
        if released {
            self.selection_tools.gesture = None;
        }
    }
}

#[cfg(test)]
#[path = "selection_tools_tests.rs"]
mod tests;

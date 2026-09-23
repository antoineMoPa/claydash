//! Lightweight editing guides for operands whose surfaces are hidden by CSG.
use std::collections::{HashMap, HashSet};

use egui::{Color32, Stroke};
use glam::{Mat4, Vec3, Vec4};

use crate::{
    camera::Camera,
    model::{
        objects_ref, selected_ref, selection_scope, BooleanOperation, DataTree, SdfObject,
        SdfParams, SelectionScope,
    },
};

const SEGMENT_BUDGET: usize = 8192;
const CURVE_STEPS: usize = 32;

#[derive(Default)]
pub(super) struct Ghosts(Vec<(uuid::Uuid, [egui::Pos2; 2])>);

impl Ghosts {
    pub(super) fn pick(&self, point: egui::Pos2) -> Option<uuid::Uuid> {
        let mut nearest = 5.0_f32.powi(2);
        let mut hit = None;
        for &(id, [a, b]) in &self.0 {
            let edge = b - a;
            let t = if edge.length_sq() > 0.0 {
                ((point - a).dot(edge) / edge.length_sq()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let distance = point.distance_sq(a + edge * t);
            if distance < nearest {
                nearest = distance;
                hit = Some(id);
            }
        }
        hit
    }
}

pub(super) fn draw(ui: &egui::Ui, tree: &DataTree, camera: &Camera) -> Ghosts {
    let mut ghosts = Ghosts::default();
    let selected = selected_ref(tree);
    if selected.is_empty() {
        return ghosts;
    }
    let scene = objects_ref(tree);
    let operands = editing_operands(
        scene,
        selected,
        selection_scope(tree) == SelectionScope::Group,
    );
    if operands.is_empty() {
        return ghosts;
    }
    let projection = camera.projection() * camera.view();
    let scale = ui.ctx().pixels_per_point();
    let mut remaining = SEGMENT_BUDGET;
    let mut simplified = false;
    for (index, active) in operands {
        let object = &scene[index];
        let color = if object.operation == BooleanOperation::Subtract {
            Color32::from_rgba_unmultiplied(255, 192, 115, if active { 195 } else { 85 })
        } else {
            Color32::from_rgba_unmultiplied(159, 219, 255, if active { 195 } else { 85 })
        };
        let stroke = Stroke::new(if active { 1.5 } else { 1.0 }, color);
        let segments = primitive_segments(&object.params);
        let transform = crate::model::object_world_matrix(scene, object.uuid);
        let world_to_clip = projection * transform;
        let local_eye = transform.inverse().transform_point3(camera.position);
        let cells: [Vec<Cell>; 3] = std::array::from_fn(|axis| {
            let count = if object.repetition.enabled && object.repetition.axes[axis] {
                object.repetition.count[axis]
            } else {
                1
            };
            repetition_cells(count, object.repetition.spacing[axis], local_eye[axis])
        });
        'copies: for x in &cells[0] {
            for y in &cells[1] {
                for z in &cells[2] {
                    let offset = Vec3::new(x.offset, y.offset, z.offset);
                    let minimum = Vec3::new(x.minimum, y.minimum, z.minimum);
                    let maximum = Vec3::new(x.maximum, y.maximum, z.maximum);
                    for &[a, b] in &segments {
                        if remaining == 0 {
                            simplified = true;
                            break 'copies;
                        }
                        remaining -= 1;
                        let Some((a, b)) = clip_to_cell(a + offset, b + offset, minimum, maximum)
                        else {
                            continue;
                        };
                        let Some(points) = project_segment(world_to_clip, a, b, camera, scale)
                        else {
                            continue;
                        };
                        ui.painter().line_segment(points, stroke);
                        ghosts.0.push((object.uuid, points));
                    }
                }
            }
        }
        if remaining == 0 {
            simplified = true;
            break;
        }
    }
    if simplified {
        ui.painter().text(
            ui.clip_rect().right_bottom() - egui::vec2(10.0, 10.0),
            egui::Align2::RIGHT_BOTTOM,
            "Additional operand guides hidden",
            egui::FontId::proportional(11.0),
            Color32::LIGHT_GRAY,
        );
    }
    ghosts
}

fn editing_operands(
    scene: &[SdfObject],
    selection: &[uuid::Uuid],
    include_descendants: bool,
) -> Vec<(usize, bool)> {
    let selected: HashSet<_> = selection.iter().copied().collect();
    let mut children: HashMap<_, Vec<_>> = HashMap::new();
    for object in scene {
        if let Some(parent) = object.boolean_parent {
            children.entry(parent).or_default().push(object.uuid);
        }
    }
    let mut related = HashSet::new();
    let mut pending = selection.to_vec();
    while let Some(id) = pending.pop() {
        if related.insert(id) {
            if include_descendants {
                if let Some(children) = children.get(&id) {
                    pending.extend(children);
                }
            }
        }
    }
    let mut operands: Vec<_> = scene
        .iter()
        .enumerate()
        .filter_map(|(index, object)| {
            object.boolean_parent?;
            let active = selected.contains(&object.uuid);
            (active
                || (related.contains(&object.uuid) && object.operation != BooleanOperation::Union))
                .then_some((index, active))
        })
        .collect();
    // Spend the guide budget on the shapes being manipulated before faint context.
    operands.sort_by_key(|(_, active)| !active);
    operands
}

fn primitive_segments(params: &SdfParams) -> Vec<[Vec3; 2]> {
    let mut result = Vec::new();
    let mut ring = |center: Vec3, a: Vec3, b: Vec3| {
        for i in 0..CURVE_STEPS {
            let point = |step: usize| {
                let angle = std::f32::consts::TAU * step as f32 / CURVE_STEPS as f32;
                center + a * angle.cos() + b * angle.sin()
            };
            result.push([point(i), point(i + 1)]);
        }
    };
    match params {
        SdfParams::SphereParams(p) => {
            ring(Vec3::ZERO, Vec3::X * p.radius, Vec3::Y * p.radius);
            ring(Vec3::ZERO, Vec3::X * p.radius, Vec3::Z * p.radius);
            ring(Vec3::ZERO, Vec3::Y * p.radius, Vec3::Z * p.radius);
        }
        SdfParams::BoxParams(p) => {
            for axis in 0..3 {
                for a in [-1.0, 1.0] {
                    for b in [-1.0, 1.0] {
                        let mut start = -p.box_q;
                        start[(axis + 1) % 3] *= a;
                        start[(axis + 2) % 3] *= b;
                        let mut end = start;
                        end[axis] = p.box_q[axis];
                        result.push([start, end]);
                    }
                }
            }
        }
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => {
            for sign in [-1.0, 1.0] {
                ring(
                    Vec3::Y * half_height * sign,
                    Vec3::X * radius,
                    Vec3::Z * radius,
                );
            }
            for side in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                result.push([
                    side * radius - Vec3::Y * half_height,
                    side * radius + Vec3::Y * half_height,
                ]);
            }
        }
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => {
            for radius in [major_radius - minor_radius, major_radius + minor_radius] {
                ring(Vec3::ZERO, Vec3::X * radius, Vec3::Z * radius);
            }
            for side in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                ring(
                    side * major_radius,
                    side * minor_radius,
                    Vec3::Y * minor_radius,
                );
            }
        }
        SdfParams::PolygonPrismParams(polygon) => {
            for index in 0..polygon.vertices.len() {
                let a = polygon.vertices[index];
                let b = polygon.vertices[(index + 1) % polygon.vertices.len()];
                for z in [-polygon.half_depth, polygon.half_depth] {
                    result.push([Vec3::new(a.x, a.y, z), Vec3::new(b.x, b.y, z)]);
                }
                result.push([
                    Vec3::new(a.x, a.y, -polygon.half_depth),
                    Vec3::new(a.x, a.y, polygon.half_depth),
                ]);
            }
        }
    }
    result
}

struct Cell {
    offset: f32,
    minimum: f32,
    maximum: f32,
}
fn repetition_cells(count: u32, spacing: f32, eye: f32) -> Vec<Cell> {
    if count <= 1 {
        return vec![Cell {
            offset: 0.0,
            minimum: f32::NEG_INFINITY,
            maximum: f32::INFINITY,
        }];
    }
    let spacing = spacing.max(0.001);
    let half = (count - 1) as f32 * 0.5;
    let end = half.ceil() as i32;
    // Counts in the editor are <=32. Limit allocation for imported huge counts
    // and visit nearby cells first; the drawing budget bounds the remaining work.
    let nearest = (eye / spacing).round().clamp(-half, half) as i32;
    let first = (-end).max(nearest.saturating_sub(32));
    let last = end.min(nearest.saturating_add(32));
    let mut cells: Vec<_> = (first..=last)
        .map(|cell| Cell {
            offset: (cell as f32).clamp(-half, half) * spacing,
            minimum: if cell == -end {
                f32::NEG_INFINITY
            } else {
                (cell as f32 - 0.5) * spacing
            },
            maximum: if cell == end {
                f32::INFINITY
            } else {
                (cell as f32 + 0.5) * spacing
            },
        })
        .collect();
    cells.sort_by(|a, b| (a.offset - eye).abs().total_cmp(&(b.offset - eye).abs()));
    cells
}

fn clip_to_cell(a: Vec3, b: Vec3, minimum: Vec3, maximum: Vec3) -> Option<(Vec3, Vec3)> {
    let delta = b - a;
    let mut start: f32 = 0.0;
    let mut end: f32 = 1.0;
    for axis in 0..3 {
        if delta[axis].abs() < 1e-8 {
            if a[axis] < minimum[axis] || a[axis] > maximum[axis] {
                return None;
            }
        } else {
            let first = (minimum[axis] - a[axis]) / delta[axis];
            let last = (maximum[axis] - a[axis]) / delta[axis];
            start = start.max(first.min(last));
            end = end.min(first.max(last));
        }
    }
    (start <= end).then_some((a + delta * start, a + delta * end))
}

fn project_segment(
    matrix: Mat4,
    a: Vec3,
    b: Vec3,
    camera: &Camera,
    scale: f32,
) -> Option<[egui::Pos2; 2]> {
    let a = matrix * a.extend(1.0);
    let b = matrix * b.extend(1.0);
    let mut start: f32 = 0.0;
    let mut end: f32 = 1.0;
    // Clip homogeneous endpoints before division, including near-plane crossings.
    let planes = |p: Vec4| [p.w + p.x, p.w - p.x, p.w + p.y, p.w - p.y, p.z, p.w - p.z];
    for (first, last) in planes(a).into_iter().zip(planes(b)) {
        if first < 0.0 && last < 0.0 {
            return None;
        }
        if first < 0.0 {
            start = start.max(first / (first - last));
        }
        if last < 0.0 {
            end = end.min(first / (first - last));
        }
    }
    if start > end {
        return None;
    }
    let mut points = [egui::Pos2::ZERO; 2];
    for (index, t) in [start, end].into_iter().enumerate() {
        let clip = a.lerp(b, t);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        points[index] = egui::pos2(
            (camera.viewport_origin.x + (ndc.x * 0.5 + 0.5) * camera.viewport.x) / scale,
            (camera.viewport_origin.y + (0.5 - ndc.y * 0.5) * camera.viewport.y) / scale,
        );
    }
    Some(points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PrimitiveKind;

    #[test]
    fn selected_cutters_and_nested_target_operands_are_visible_without_unrelated_roots() {
        let root = SdfObject::create_kind(PrimitiveKind::Box);
        let mut cutter = SdfObject::create_kind(PrimitiveKind::Sphere);
        cutter.boolean_parent = Some(root.uuid);
        cutter.operation = BooleanOperation::Subtract;
        let mut nested = cutter.clone();
        nested.uuid = uuid::Uuid::new_v4();
        nested.boolean_parent = Some(cutter.uuid);
        let mut unrelated = cutter.clone();
        unrelated.uuid = uuid::Uuid::new_v4();
        unrelated.boolean_parent = Some(uuid::Uuid::new_v4());
        let scene = [root.clone(), cutter.clone(), nested, unrelated];
        assert_eq!(
            editing_operands(&scene, &[root.uuid], true),
            [(1, false), (2, false)]
        );
        assert_eq!(
            editing_operands(&scene, &[cutter.uuid], true),
            [(1, true), (2, false)]
        );
        assert!(editing_operands(&scene, &[], true).is_empty());
    }

    #[test]
    fn primitive_guide_vertices_follow_rotated_scaled_surfaces() {
        for kind in PrimitiveKind::SPAWNABLE {
            let mut object = SdfObject::create_kind(kind);
            object.transform.translation = Vec3::new(1.0, 2.0, -1.0);
            object.transform.scale = Vec3::new(0.6, 1.8, 1.2);
            object.transform.rotation = glam::Quat::from_rotation_z(0.7);
            for point in primitive_segments(&object.params).into_iter().flatten() {
                let world = object.transform.matrix().transform_point3(point);
                assert!(object.distance(world).abs() < 0.0001, "{kind:?}: {world:?}");
            }
        }
    }

    #[test]
    fn a_fully_hidden_cutter_still_paints_translucent_viewport_guides() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let target = SdfObject::create_kind(PrimitiveKind::Box);
        let mut cutter = SdfObject::create_kind(PrimitiveKind::Sphere);
        cutter.params = SdfParams::SphereParams(crate::model::SphereParams { radius: 0.1 });
        cutter.boolean_parent = Some(target.uuid);
        cutter.operation = BooleanOperation::Subtract;
        let cutter_id = cutter.uuid;
        crate::model::set_selected(&mut tree, vec![target.uuid]);
        crate::model::set_objects(&mut tree, vec![target, cutter]);
        let mut camera = Camera::new();
        camera.viewport = glam::Vec2::new(400.0, 300.0);
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let mut ghosts = Ghosts::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_clip_rect(viewport);
            ghosts = draw(ui, &tree, &camera);
        });
        output.textures_delta.clear(); // Headless test has no GPU texture consumer.
        let mut lines = 0;
        for shape in output.shapes {
            if let egui::Shape::LineSegment { points, stroke } = shape.shape {
                lines += 1;
                assert!(points.iter().all(|point| viewport.contains(*point)));
                assert!(stroke.color.a() > 0 && stroke.color.a() < 255);
                assert_eq!(shape.clip_rect, viewport);
                assert_eq!(ghosts.pick(points[0].lerp(points[1], 0.5)), Some(cutter_id));
            }
        }
        assert_eq!(lines, 3 * CURVE_STEPS);
        assert_eq!(ghosts.pick(viewport.left_top()), None);
    }

    #[test]
    fn even_repetition_includes_the_central_cell_and_clips_edge_copies() {
        let cells = repetition_cells(2, 2.0, 0.0);
        assert_eq!(cells.len(), 3);
        let center = cells.iter().find(|cell| cell.offset == 0.0).unwrap();
        assert_eq!((center.minimum, center.maximum), (-1.0, 1.0));
        let edge = cells.iter().find(|cell| cell.offset == 1.0).unwrap();
        let (a, b) = clip_to_cell(
            Vec3::ZERO,
            Vec3::X * 2.0,
            Vec3::new(edge.minimum, -1.0, -1.0),
            Vec3::splat(f32::INFINITY),
        )
        .unwrap();
        assert_eq!(a.x, 1.0);
        assert_eq!(b.x, 2.0);
    }
}

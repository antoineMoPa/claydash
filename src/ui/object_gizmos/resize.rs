use super::*;

const EDGE_ON_FACE_DOT_LIMIT: f32 = 0.25;

pub(super) fn longest_projected_segment(points: &[egui::Pos2]) -> Option<[egui::Pos2; 2]> {
    if points.len() < 2 {
        return None;
    }
    let mut longest = None;
    let mut longest_length = 0.0;
    for index in 0..points.len() {
        let start = points[index];
        let end = points[(index + 1) % points.len()];
        let length = start.distance_sq(end);
        if length > longest_length {
            longest = Some([start, end]);
            longest_length = length;
        }
    }
    longest
}

pub(super) fn vector_axis(axis: usize) -> crate::model::VectorAxis {
    match axis {
        0 => crate::model::VectorAxis::X,
        1 => crate::model::VectorAxis::Y,
        _ => crate::model::VectorAxis::Z,
    }
}

pub(in crate::ui) fn gizmo_label_rect(
    viewport: egui::Rect,
    tip: egui::Pos2,
    text_size: egui::Vec2,
    occupied: &[egui::Rect],
) -> Option<egui::Rect> {
    let size = text_size + egui::vec2(8.0, 8.0);
    let offsets = [
        egui::vec2(12.0, -size.y * 0.5),
        egui::vec2(-size.x - 12.0, -size.y * 0.5),
        egui::vec2(-size.x * 0.5, -size.y - 12.0),
        egui::vec2(-size.x * 0.5, 12.0),
    ];
    offsets
        .into_iter()
        .map(|offset| egui::Rect::from_min_size(tip + offset, size))
        .find(|rect| {
            viewport.contains_rect(*rect) && !occupied.iter().any(|other| rect.intersects(*other))
        })
}

#[cfg(test)]
pub(in crate::ui) fn resize_handles(object: &SdfObject, camera: &Camera) -> Vec<ResizeHandle> {
    resize_handles_with_matrix(object, object.transform.matrix(), camera)
}

#[cfg(test)]
pub(in crate::ui) fn resize_handles_with_matrix(
    object: &SdfObject,
    matrix: glam::Mat4,
    camera: &Camera,
) -> Vec<ResizeHandle> {
    resize_handles_with_matrix_impl(object, matrix, camera, false)
}

pub(super) fn resize_handles_with_matrix_including_hidden(
    object: &SdfObject,
    matrix: glam::Mat4,
    camera: &Camera,
) -> Vec<ResizeHandle> {
    resize_handles_with_matrix_impl(object, matrix, camera, true)
}

fn resize_handles_with_matrix_impl(
    object: &SdfObject,
    matrix: glam::Mat4,
    camera: &Camera,
    include_hidden_box_faces: bool,
) -> Vec<ResizeHandle> {
    let mut handles = Vec::new();
    let origin = matrix.transform_point3(Vec3::ZERO);
    match &object.params {
        SdfParams::BoxParams(params) => {
            for axis in 0..3 {
                for sign in [-1.0, 1.0] {
                    let mut local = Vec3::ZERO;
                    local[axis] = params.box_q[axis] * sign;
                    let mut local_direction = Vec3::ZERO;
                    local_direction[axis] = sign;
                    let world = matrix.transform_point3(local);
                    let direction = matrix.transform_vector3(local_direction);
                    let camera_facing = direction.dot(camera.position - world) > 0.0;
                    let view_to_camera = (camera.position - camera.target).normalize_or_zero();
                    let edge_on = direction.normalize_or_zero().dot(view_to_camera).abs()
                        <= EDGE_ON_FACE_DOT_LIMIT;
                    if !camera_facing && !edge_on && !include_hidden_box_faces {
                        continue;
                    }
                    let u = (axis + 1) % 3;
                    let v = (axis + 2) % 3;
                    let patch_scale = if edge_on { 1.0 } else { 0.3 };
                    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
                    let patch = corners
                        .into_iter()
                        .map(|(a, b)| {
                            let mut point = local;
                            point[u] = params.box_q[u] * patch_scale * a;
                            point[v] = params.box_q[v] * patch_scale * b;
                            matrix.transform_point3(point)
                        })
                        .collect();
                    let full_patch = corners
                        .into_iter()
                        .map(|(a, b)| {
                            let mut point = local;
                            point[u] = params.box_q[u] * a;
                            point[v] = params.box_q[v] * b;
                            matrix.transform_point3(point)
                        })
                        .collect();
                    handles.push(ResizeHandle {
                        id: ResizeHandleId::BoxFace {
                            axis,
                            positive: sign > 0.0,
                        },
                        world,
                        guide_origin: origin,
                        direction,
                        local_direction,
                        patch,
                        full_patch,
                        label: format!("Resize {}", axis_label(axis)),
                        color: axis_color(axis),
                        parameter: axis,
                        value: params.box_q[axis] * 2.0,
                        camera_facing,
                        edge_on,
                    });
                }
            }
        }
        SdfParams::SphereParams(params) => {
            for (axis, local_axis) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
                let sign = if object.transform.scale[axis] < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let axis_direction = matrix.transform_vector3(local_axis * sign).normalize();
                // Use the camera-facing end of each local axis so handles do
                // not bunch together on the far side of a small sphere.
                let side = if axis_direction.dot(camera.position - origin) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let direction = axis_direction * side;
                handles.push(ResizeHandle {
                    id: ResizeHandleId::SphereAxis(axis),
                    world: matrix.transform_point3(local_axis * params.radius * side),
                    guide_origin: origin,
                    direction,
                    local_direction: local_axis * side,
                    patch: vec![],
                    full_patch: vec![],
                    label: format!("Radius {}", axis_label(axis)),
                    color: axis_color(axis),
                    parameter: axis,
                    value: params.radius * matrix.transform_vector3(local_axis).length(),
                    camera_facing: true,
                    edge_on: false,
                });
            }
        }
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => {
            for (axis, value, label) in [(0, *radius, "Radius"), (1, *half_height, "Height")] {
                let direction = if axis == 0 { Vec3::X } else { Vec3::Y };
                handles.push(ResizeHandle {
                    id: if axis == 0 {
                        ResizeHandleId::CylinderRadius
                    } else {
                        ResizeHandleId::CylinderHeight
                    },
                    world: matrix.transform_point3(direction * value),
                    guide_origin: origin,
                    direction: matrix.transform_vector3(direction),
                    local_direction: direction,
                    patch: vec![],
                    full_patch: vec![],
                    label: label.into(),
                    color: axis_color(axis),
                    parameter: axis,
                    value: if axis == 1 { value * 2.0 } else { value },
                    camera_facing: true,
                    edge_on: false,
                });
            }
        }
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => {
            for (parameter, local, direction, label, value) in [
                (
                    0,
                    Vec3::X * *major_radius,
                    Vec3::X,
                    "Ring radius",
                    *major_radius,
                ),
                (
                    1,
                    Vec3::X * *major_radius + Vec3::Y * *minor_radius,
                    Vec3::Y,
                    "Tube radius",
                    *minor_radius,
                ),
            ] {
                handles.push(ResizeHandle {
                    id: if parameter == 0 {
                        ResizeHandleId::TorusMajorRadius
                    } else {
                        ResizeHandleId::TorusMinorRadius
                    },
                    world: matrix.transform_point3(local),
                    guide_origin: matrix.transform_point3(if parameter == 1 {
                        Vec3::X * *major_radius
                    } else {
                        Vec3::ZERO
                    }),
                    direction: matrix.transform_vector3(direction),
                    local_direction: direction,
                    patch: vec![],
                    full_patch: vec![],
                    label: label.into(),
                    color: axis_color(parameter),
                    parameter,
                    value,
                    camera_facing: true,
                    edge_on: false,
                });
            }
        }
        SdfParams::PolygonPrismParams(_)
        | SdfParams::BezierCurveParams(_)
        | SdfParams::LoftParams(_) => {}
    }
    handles
}

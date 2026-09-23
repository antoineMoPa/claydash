use super::shape::signed_area;
use super::*;

pub(super) struct FaceFrame {
    pub(super) u: Vec3,
    pub(super) v: Vec3,
    pub(super) normal: Vec3,
    pub(super) origin: Vec3,
    pub(super) domain: FaceDomain,
}

pub(super) enum FaceDomain {
    Rectangle { extent_u: f32, extent_v: f32 },
    Circle { radius: f32 },
    Polygon(Vec<Vec2>),
}

fn box_face_frame(face: crate::model::BoxFaceSelection, half_extents: Vec3) -> FaceFrame {
    match (face.axis, face.positive) {
        (VectorAxis::X, true) => FaceFrame {
            u: Vec3::Y,
            v: Vec3::Z,
            normal: Vec3::X,
            origin: Vec3::X * half_extents.x,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.y,
                extent_v: half_extents.z,
            },
        },
        (VectorAxis::X, false) => FaceFrame {
            u: Vec3::Y,
            v: Vec3::NEG_Z,
            normal: Vec3::NEG_X,
            origin: Vec3::NEG_X * half_extents.x,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.y,
                extent_v: half_extents.z,
            },
        },
        (VectorAxis::Y, true) => FaceFrame {
            u: Vec3::Z,
            v: Vec3::X,
            normal: Vec3::Y,
            origin: Vec3::Y * half_extents.y,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.z,
                extent_v: half_extents.x,
            },
        },
        (VectorAxis::Y, false) => FaceFrame {
            u: Vec3::Z,
            v: Vec3::NEG_X,
            normal: Vec3::NEG_Y,
            origin: Vec3::NEG_Y * half_extents.y,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.z,
                extent_v: half_extents.x,
            },
        },
        (VectorAxis::Z, true) => FaceFrame {
            u: Vec3::X,
            v: Vec3::Y,
            normal: Vec3::Z,
            origin: Vec3::Z * half_extents.z,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.x,
                extent_v: half_extents.y,
            },
        },
        (VectorAxis::Z, false) => FaceFrame {
            u: Vec3::X,
            v: Vec3::NEG_Y,
            normal: Vec3::NEG_Z,
            origin: Vec3::NEG_Z * half_extents.z,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.x,
                extent_v: half_extents.y,
            },
        },
    }
}

pub(super) fn source_and_frame(
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
) -> Option<(&SdfObject, FaceFrame)> {
    let source = scene.iter().find(|object| object.uuid == face.object())?;
    let frame = match (face, &source.params) {
        (crate::model::ModelingFaceSelection::Box(face), SdfParams::BoxParams(params)) => {
            box_face_frame(face, params.box_q)
        }
        (
            crate::model::ModelingFaceSelection::CylinderCap(face),
            SdfParams::CylinderParams {
                radius,
                half_height,
            },
        ) => FaceFrame {
            u: Vec3::X,
            v: if face.positive { Vec3::NEG_Z } else { Vec3::Z },
            normal: if face.positive { Vec3::Y } else { Vec3::NEG_Y },
            origin: Vec3::Y * *half_height * if face.positive { 1.0 } else { -1.0 },
            domain: FaceDomain::Circle { radius: *radius },
        },
        (
            crate::model::ModelingFaceSelection::PolygonPrism(selection),
            SdfParams::PolygonPrismParams(params),
        ) => match selection.face {
            crate::model::PolygonPrismFace::Cap { positive } => {
                let (v, normal, z) = if positive {
                    (Vec3::Y, Vec3::Z, params.half_depth)
                } else {
                    (Vec3::NEG_Y, Vec3::NEG_Z, -params.half_depth)
                };
                let vertices = params
                    .vertices
                    .iter()
                    .map(|point| Vec2::new(point.x, if positive { point.y } else { -point.y }))
                    .collect();
                FaceFrame {
                    u: Vec3::X,
                    v,
                    normal,
                    origin: Vec3::Z * z,
                    domain: FaceDomain::Polygon(vertices),
                }
            }
            crate::model::PolygonPrismFace::Side { edge } => {
                let a = params.vertices.get(edge).copied()?;
                let b = params
                    .vertices
                    .get((edge + 1) % params.vertices.len())
                    .copied()?;
                let delta = b - a;
                let length = delta.length();
                if length <= 0.000_01 {
                    return None;
                }
                let winding = signed_area(&params.vertices);
                let mut u = Vec3::new(delta.x / length, delta.y / length, 0.0);
                let mut normal = Vec3::new(u.y, -u.x, 0.0);
                if winding < 0.0 {
                    u = -u;
                    normal = -normal;
                }
                FaceFrame {
                    u,
                    v: normal.cross(u),
                    normal,
                    origin: Vec3::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5, 0.0),
                    domain: FaceDomain::Rectangle {
                        extent_u: length * 0.5,
                        extent_v: params.half_depth,
                    },
                }
            }
        },
        _ => return None,
    };
    Some((source, frame))
}

pub(super) fn point_on_face(
    camera: &Camera,
    physical_pointer: Vec2,
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
) -> Option<Vec2> {
    let (_, frame) = source_and_frame(scene, face)?;
    let inverse = crate::model::object_world_matrix(scene, face.object()).inverse();
    let (world_origin, world_direction) = camera.ray(physical_pointer);
    let origin = inverse.transform_point3(world_origin);
    let direction = inverse.transform_vector3(world_direction);
    let denominator = direction.dot(frame.normal);
    if denominator.abs() < 0.000_01 {
        return None;
    }
    let local = origin + direction * ((frame.origin - origin).dot(frame.normal) / denominator);
    let relative = local - frame.origin;
    let point = Vec2::new(relative.dot(frame.u), relative.dot(frame.v));
    match frame.domain {
        FaceDomain::Rectangle { extent_u, extent_v } => Some(Vec2::new(
            point.x.clamp(-extent_u, extent_u),
            point.y.clamp(-extent_v, extent_v),
        )),
        FaceDomain::Circle { radius } => (point.length() <= radius + 0.001).then_some(point),
        FaceDomain::Polygon(vertices) => {
            (crate::model::polygon_distance(point, &vertices) <= 0.001).then_some(point)
        }
    }
}

pub(super) fn point_world(
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    point: Vec2,
) -> Option<Vec3> {
    let (_, frame) = source_and_frame(scene, face)?;
    Some(
        crate::model::object_world_matrix(scene, face.object())
            .transform_point3(frame.origin + frame.u * point.x + frame.v * point.y),
    )
}

pub(super) fn projected_depth_axis(
    camera: &Camera,
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    scale: f32,
) -> Option<egui::Vec2> {
    let (_, frame) = source_and_frame(scene, face)?;
    let matrix = crate::model::object_world_matrix(scene, face.object());
    let center = matrix.transform_point3(frame.origin);
    let direction = matrix.transform_vector3(frame.normal);
    let at = camera.project(center, scale)?;
    let ahead = camera.project(center + direction * 0.1, scale)?;
    let mut projected = (ahead - at) * 10.0;
    if projected.length_sq() < 4.0 {
        let screen_up = camera.view().inverse().y_axis.truncate();
        if let Some(fallback) = camera.project(center + screen_up * 0.1, scale) {
            projected = (fallback - at) * 10.0;
        }
    }
    (projected.length_sq() > 0.0001).then_some(projected)
}

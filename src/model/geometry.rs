use glam::{Mat4, Vec2, Vec3, Vec4};
use sdf_consts::{
    TYPE_BEZIER_CURVE, TYPE_BOX, TYPE_CYLINDER, TYPE_POLYGON_PRISM, TYPE_SPHERE, TYPE_TORUS,
};
use serde::{Deserialize, Serialize};

mod materials;
mod primitives;
mod spatial;

pub use materials::*;
use primitives::curve_profile_frame;
pub use primitives::*;
pub use spatial::*;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorState {
    Start,
    Grabbing,
    Scaling,
    Rotating,
    Extruding,
    DraggingFace,
    ExtendingCurve,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxFaceSelection {
    pub object: uuid::Uuid,
    pub axis: crate::model::VectorAxis,
    pub positive: bool,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct CylinderCapSelection {
    pub object: uuid::Uuid,
    pub positive: bool,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolygonPrismFace {
    Cap { positive: bool },
    Side { edge: usize },
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolygonPrismFaceSelection {
    pub object: uuid::Uuid,
    pub face: PolygonPrismFace,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelingFaceSelection {
    Box(BoxFaceSelection),
    CylinderCap(CylinderCapSelection),
    PolygonPrism(PolygonPrismFaceSelection),
}

impl ModelingFaceSelection {
    pub fn object(self) -> uuid::Uuid {
        match self {
            Self::Box(face) => face.object,
            Self::CylinderCap(face) => face.object,
            Self::PolygonPrism(face) => face.object,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveKind {
    Sphere,
    Box,
    Cylinder,
    Torus,
    PolygonPrism,
    BezierCurve,
}

impl PrimitiveKind {
    pub const ALL: [Self; 6] = [
        Self::Sphere,
        Self::Box,
        Self::Cylinder,
        Self::Torus,
        Self::PolygonPrism,
        Self::BezierCurve,
    ];
    pub const SPAWNABLE: [Self; 5] = [
        Self::Sphere,
        Self::Box,
        Self::Cylinder,
        Self::Torus,
        Self::BezierCurve,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sphere => "Sphere",
            Self::Box => "Box",
            Self::Cylinder => "Cylinder",
            Self::Torus => "Torus",
            Self::PolygonPrism => "Face shape",
            Self::BezierCurve => "Bézier curve",
        }
    }

    pub fn object_type(self) -> i32 {
        match self {
            Self::Sphere => TYPE_SPHERE,
            Self::Box => TYPE_BOX,
            Self::Cylinder => TYPE_CYLINDER,
            Self::Torus => TYPE_TORUS,
            Self::PolygonPrism => TYPE_POLYGON_PRISM,
            Self::BezierCurve => TYPE_BEZIER_CURVE,
        }
    }

    pub fn from_object_type(value: i32) -> Self {
        match value {
            TYPE_BOX => Self::Box,
            TYPE_CYLINDER => Self::Cylinder,
            TYPE_TORUS => Self::Torus,
            TYPE_POLYGON_PRISM => Self::PolygonPrism,
            TYPE_BEZIER_CURVE => Self::BezierCurve,
            _ => Self::Sphere,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BooleanOperation {
    #[default]
    Union,
    Subtract,
    Intersect,
}

impl BooleanOperation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Union => "Union",
            Self::Subtract => "Subtract",
            Self::Intersect => "Intersect",
        }
    }

    pub fn gpu_code(self) -> i32 {
        match self {
            Self::Union => 0,
            Self::Subtract => 1,
            Self::Intersect => 2,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SdfObject {
    pub uuid: uuid::Uuid,
    pub transform: Transform,
    pub group_transform: Transform,
    pub color: Vec4,
    pub object_type: i32,
    pub params: SdfParams,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub operation: BooleanOperation,
    #[serde(default)]
    pub boolean_parent: Option<uuid::Uuid>,
    /// World-space blend width for combining this object's direct children.
    /// Older documents retain their hard edges.
    #[serde(default)]
    pub softness: f32,
    #[serde(default)]
    pub material: Material,
    #[serde(default)]
    pub material_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub repetition: Repetition,
    #[serde(default)]
    pub mirror: Option<Mirror>,
    #[serde(default)]
    pub lattice: Option<Lattice>,
    #[serde(default)]
    pub path_extrusion: Option<PathExtrusion>,
}

impl SdfObject {
    pub fn create(object_type: i32) -> Self {
        Self::create_kind(PrimitiveKind::from_object_type(object_type))
    }

    pub fn create_kind(kind: PrimitiveKind) -> Self {
        let material = Material::default();
        Self {
            uuid: uuid::Uuid::new_v4(),
            transform: Transform::default(),
            group_transform: Transform::default(),
            color: material.color,
            object_type: kind.object_type(),
            params: match kind {
                PrimitiveKind::Box => SdfParams::BoxParams(BoxParams {
                    box_q: Vec3::splat(0.3),
                }),
                PrimitiveKind::Sphere => SdfParams::SphereParams(SphereParams { radius: 0.25 }),
                PrimitiveKind::Cylinder => SdfParams::CylinderParams {
                    radius: 0.25,
                    half_height: 0.35,
                },
                PrimitiveKind::Torus => SdfParams::TorusParams {
                    major_radius: 0.3,
                    minor_radius: 0.1,
                },
                PrimitiveKind::PolygonPrism => SdfParams::PolygonPrismParams(PolygonPrismParams {
                    vertices: vec![
                        Vec2::new(-0.25, -0.2),
                        Vec2::new(0.25, -0.2),
                        Vec2::new(0.0, 0.25),
                    ],
                    half_depth: 0.1,
                }),
                PrimitiveKind::BezierCurve => SdfParams::BezierCurveParams(BezierCurveParams {
                    points: vec![
                        Vec3::new(-0.65, 0.0, 0.0),
                        Vec3::new(-0.25, 0.7, 0.0),
                        Vec3::new(0.25, 0.7, 0.0),
                        Vec3::new(0.65, 0.0, 0.0),
                    ],
                    closed: false,
                }),
            },
            name: kind.label().to_string(),
            operation: BooleanOperation::Union,
            boolean_parent: None,
            softness: 0.05,
            material,
            material_id: None,
            repetition: Repetition::default(),
            mirror: None,
            lattice: None,
            path_extrusion: None,
        }
    }

    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            PrimitiveKind::from_object_type(self.object_type)
                .label()
                .to_string()
        } else {
            self.name.clone()
        }
    }

    pub fn duplicate(&self) -> Self {
        let mut copy = self.clone();
        copy.uuid = uuid::Uuid::new_v4();
        copy
    }

    #[cfg(test)]
    pub fn distance(&self, point: Vec3) -> f32 {
        self.distance_with_matrix(point, self.transform.matrix())
    }

    pub fn distance_with_matrix(&self, point: Vec3, matrix: Mat4) -> f32 {
        self.distance_with_matrix_at(point, matrix, true, None)
    }

    pub(crate) fn distance_with_matrix_with_profile(
        &self,
        point: Vec3,
        matrix: Mat4,
        repeat: bool,
        profile: Option<&[Vec2]>,
    ) -> f32 {
        self.distance_with_matrix_at(point, matrix, repeat, profile)
    }

    fn distance_with_matrix_at(
        &self,
        point: Vec3,
        matrix: Mat4,
        repeat: bool,
        profile: Option<&[Vec2]>,
    ) -> f32 {
        let local = (matrix.inverse() * point.extend(1.0)).truncate();
        let local = if repeat {
            self.repeated_local_point(local)
        } else {
            local
        };
        let distance = match self.params {
            SdfParams::SphereParams(ref params) => local.length() - params.radius,
            SdfParams::BoxParams(ref params) => {
                let q = local.abs() - params.box_q;
                q.max(Vec3::ZERO).length() + q.max_element().min(0.0)
            }
            SdfParams::CylinderParams {
                radius,
                half_height,
            } => {
                let radial = Vec2::new(local.x, local.z).length() - radius;
                let q = Vec2::new(radial, local.y.abs() - half_height);
                q.max(Vec2::ZERO).length() + q.max_element().min(0.0)
            }
            SdfParams::TorusParams {
                major_radius,
                minor_radius,
            } => {
                Vec2::new(Vec2::new(local.x, local.z).length() - major_radius, local.y).length()
                    - minor_radius
            }
            SdfParams::PolygonPrismParams(ref params) => {
                let polygon = polygon_distance(local.truncate(), &params.vertices);
                let depth = local.z.abs() - params.half_depth;
                let outside = Vec2::new(polygon.max(0.0), depth.max(0.0)).length();
                outside + polygon.max(depth).min(0.0)
            }
            SdfParams::BezierCurveParams(ref params) => {
                let Some(modifier) = self.path_extrusion else {
                    return 100.0;
                };
                let Some((position, tangent, segment, t)) = params.closest(local) else {
                    return 100.0;
                };
                let delta = local - position;
                if modifier.profile_curve.is_none() && modifier.profile == BezierProfile::Round {
                    delta.length() - modifier.radius
                } else {
                    let tangent = tangent.normalize_or_zero();
                    let (side, up) = curve_profile_frame(params, tangent);
                    let cross = Vec2::new(delta.dot(side), delta.dot(up));
                    let cross_distance = if let Some(vertices) = profile {
                        polygon_distance(cross / modifier.radius.max(0.0001), vertices)
                            * modifier.radius
                    } else if modifier.profile_curve.is_some() {
                        100.0
                    } else {
                        let q = cross.abs() - Vec2::splat(modifier.radius);
                        q.max(Vec2::ZERO).length() + q.max_element().min(0.0)
                    };
                    let cap = if params.closed {
                        -100.0
                    } else if segment == 0 && t <= 0.0001 {
                        -(local - params.points[0]).dot(tangent)
                    } else if segment + 1 == params.segment_count() && t >= 0.9999 {
                        (local - *params.points.last().unwrap()).dot(tangent)
                    } else {
                        -100.0
                    };
                    let q = Vec2::new(cross_distance, cap);
                    q.max(Vec2::ZERO).length() + q.max_element().min(0.0)
                }
            }
        };
        let scale = Vec3::new(
            matrix.x_axis.truncate().length(),
            matrix.y_axis.truncate().length(),
            matrix.z_axis.truncate().length(),
        );
        distance * scale.min_element()
    }

    pub(crate) fn repeated_local_point(&self, mut local: Vec3) -> Vec3 {
        if !self.repetition.enabled {
            return local;
        }
        for axis in 0..3 {
            if self.repetition.count[axis] <= 1 {
                continue;
            }
            let spacing = self.repetition.spacing[axis].max(0.001);
            let half = (self.repetition.count[axis] - 1) as f32 * 0.5;
            let cell = (local[axis] / spacing).round().clamp(-half, half);
            local[axis] -= cell * spacing;
        }
        local
    }
}

pub fn box_face_at_world_position(
    scene: &[SdfObject],
    id: uuid::Uuid,
    world_position: Vec3,
) -> Option<BoxFaceSelection> {
    let object = scene.iter().find(|object| object.uuid == id)?;
    let SdfParams::BoxParams(params) = &object.params else {
        return None;
    };
    let local = object_world_matrix(scene, id)
        .inverse()
        .transform_point3(world_position);
    let relative = local.abs() / params.box_q.max(Vec3::splat(0.0001));
    let axis = if relative.x >= relative.y && relative.x >= relative.z {
        crate::model::VectorAxis::X
    } else if relative.y >= relative.z {
        crate::model::VectorAxis::Y
    } else {
        crate::model::VectorAxis::Z
    };
    let axis_index = axis.index();
    let face_distance = (local[axis_index].abs() - params.box_q[axis_index]).abs();
    if face_distance > params.box_q[axis_index].max(0.0001) * 0.08 + 0.015 {
        return None;
    }
    Some(BoxFaceSelection {
        object: id,
        axis,
        positive: local[axis_index] >= 0.0,
    })
}

pub fn modeling_face_at_world_position(
    scene: &[SdfObject],
    id: uuid::Uuid,
    world_position: Vec3,
) -> Option<ModelingFaceSelection> {
    let object = scene.iter().find(|object| object.uuid == id)?;
    match &object.params {
        SdfParams::BoxParams(_) => {
            box_face_at_world_position(scene, id, world_position).map(ModelingFaceSelection::Box)
        }
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => {
            let local = object_world_matrix(scene, id)
                .inverse()
                .transform_point3(world_position);
            let tolerance = radius.max(*half_height).max(0.01) * 0.08 + 0.015;
            let radial = Vec2::new(local.x, local.z).length();
            ((local.y.abs() - *half_height).abs() <= tolerance && radial <= *radius + tolerance)
                .then_some(ModelingFaceSelection::CylinderCap(CylinderCapSelection {
                    object: id,
                    positive: local.y >= 0.0,
                }))
        }
        SdfParams::PolygonPrismParams(params) => {
            if params.vertices.len() < 3 {
                return None;
            }
            let local = object_world_matrix(scene, id)
                .inverse()
                .transform_point3(world_position);
            let planar_extent = params
                .vertices
                .iter()
                .map(|point| point.abs().max_element())
                .fold(0.0_f32, f32::max);
            let tolerance = planar_extent.max(params.half_depth).max(0.01) * 0.08 + 0.015;
            let cap_distance = (local.z.abs() - params.half_depth).abs();
            let on_cap = polygon_distance(local.truncate(), &params.vertices) <= tolerance;
            let mut best = on_cap.then_some((
                cap_distance,
                PolygonPrismFace::Cap {
                    positive: local.z >= 0.0,
                },
            ));
            if local.z.abs() <= params.half_depth + tolerance {
                for edge in 0..params.vertices.len() {
                    let a = params.vertices[edge];
                    let b = params.vertices[(edge + 1) % params.vertices.len()];
                    let segment = b - a;
                    let length_squared = segment.length_squared();
                    if length_squared <= 0.000_000_1 {
                        continue;
                    }
                    let closest = a + segment
                        * ((local.truncate() - a).dot(segment) / length_squared).clamp(0.0, 1.0);
                    let distance = local.truncate().distance(closest);
                    if best.is_none_or(|(current, _)| distance < current) {
                        best = Some((distance, PolygonPrismFace::Side { edge }));
                    }
                }
            }
            best.filter(|(distance, _)| *distance <= tolerance)
                .map(|(_, face)| {
                    ModelingFaceSelection::PolygonPrism(PolygonPrismFaceSelection {
                        object: id,
                        face,
                    })
                })
        }
        _ => None,
    }
}

pub fn map_leaf_group_transforms_to_primitives(scene: &mut [SdfObject]) {
    let group_ids: std::collections::HashSet<_> = scene
        .iter()
        .filter_map(|object| object.boolean_parent)
        .collect();
    for object in scene {
        if group_ids.contains(&object.uuid) || object.group_transform == Transform::default() {
            continue;
        }
        let matrix = object.group_transform.matrix() * object.transform.matrix();
        let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
        object.transform = Transform {
            translation,
            rotation,
            scale,
        };
        object.group_transform = Transform::default();
    }
}

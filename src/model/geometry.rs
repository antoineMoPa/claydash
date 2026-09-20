use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use sdf_consts::{TYPE_BOX, TYPE_CYLINDER, TYPE_SPHERE, TYPE_TORUS};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorState {
    Start,
    Grabbing,
    Scaling,
    Rotating,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveKind {
    Sphere,
    Box,
    Cylinder,
    Torus,
}

impl PrimitiveKind {
    pub const ALL: [Self; 4] = [Self::Sphere, Self::Box, Self::Cylinder, Self::Torus];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sphere => "Sphere",
            Self::Box => "Box",
            Self::Cylinder => "Cylinder",
            Self::Torus => "Torus",
        }
    }

    pub fn object_type(self) -> i32 {
        match self {
            Self::Sphere => TYPE_SPHERE,
            Self::Box => TYPE_BOX,
            Self::Cylinder => TYPE_CYLINDER,
            Self::Torus => TYPE_TORUS,
        }
    }

    pub fn from_object_type(value: i32) -> Self {
        match value {
            TYPE_BOX => Self::Box,
            TYPE_CYLINDER => Self::Cylinder,
            TYPE_TORUS => Self::Torus,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialKind {
    Transparent,
    Metallic,
    #[default]
    Solid,
    Wood,
}

impl MaterialKind {
    pub const ALL: [Self; 4] = [Self::Transparent, Self::Metallic, Self::Solid, Self::Wood];

    pub fn label(self) -> &'static str {
        match self {
            Self::Transparent => "Transparent",
            Self::Metallic => "Metallic",
            Self::Solid => "Solid",
            Self::Wood => "Wood",
        }
    }

    pub fn gpu_code(self) -> u32 {
        match self {
            Self::Solid => 0,
            Self::Wood => 1,
            Self::Transparent => 2,
            Self::Metallic => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub kind: MaterialKind,
    pub color: Vec4,
    pub roughness: f32,
    pub metallic: f32,
    pub reflectivity: f32,
    pub refractive_index: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialAsset {
    pub uuid: uuid::Uuid,
    pub name: String,
    pub material: Material,
}

impl MaterialAsset {
    pub fn new(material: Material) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            name: material.kind.label().to_string(),
            material,
        }
    }
}

impl Default for Material {
    fn default() -> Self {
        Self {
            kind: MaterialKind::Solid,
            color: Vec4::new(0.8, 0.0, 0.3, 1.0),
            roughness: 0.55,
            metallic: 0.0,
            reflectivity: 0.04,
            refractive_index: 1.45,
            opacity: 1.0,
        }
    }
}

impl Material {
    pub fn preset(kind: MaterialKind) -> Self {
        match kind {
            MaterialKind::Transparent => Self {
                kind,
                color: Vec4::new(0.55, 0.82, 1.0, 1.0),
                roughness: 0.08,
                metallic: 0.0,
                reflectivity: 0.12,
                refractive_index: 1.52,
                opacity: 0.22,
                ..Self::default()
            },
            MaterialKind::Metallic => Self {
                kind,
                color: Vec4::new(0.62, 0.66, 0.72, 1.0),
                roughness: 0.24,
                metallic: 1.0,
                reflectivity: 0.82,
                refractive_index: 1.0,
                opacity: 1.0,
                ..Self::default()
            },
            MaterialKind::Solid => Self::default(),
            MaterialKind::Wood => Self {
                kind,
                color: Vec4::new(0.64, 0.32, 0.12, 1.0),
                roughness: 0.65,
                ..Self::default()
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Repetition {
    pub enabled: bool,
    pub axes: [bool; 3],
    pub count: [u32; 3],
    pub spacing: Vec3,
}

impl Default for Repetition {
    fn default() -> Self {
        Self {
            enabled: false,
            axes: [true, false, false],
            count: [3, 1, 1],
            spacing: Vec3::splat(0.8),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BoxParams {
    pub box_q: Vec3,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SphereParams {
    pub radius: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum SdfParams {
    BoxParams(BoxParams),
    SphereParams(SphereParams),
    CylinderParams {
        radius: f32,
        half_height: f32,
    },
    TorusParams {
        major_radius: f32,
        minor_radius: f32,
    },
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
    /// World-space blend width. Older documents retain their hard edges.
    #[serde(default)]
    pub softness: f32,
    #[serde(default)]
    pub material: Material,
    #[serde(default)]
    pub material_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub repetition: Repetition,
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
            },
            name: kind.label().to_string(),
            operation: BooleanOperation::Union,
            boolean_parent: None,
            softness: 0.05,
            material,
            material_id: None,
            repetition: Repetition::default(),
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
        let local = (matrix.inverse() * point.extend(1.0)).truncate();
        let local = self.repeated_local_point(local);
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
        };
        let scale = Vec3::new(
            matrix.x_axis.truncate().length(),
            matrix.y_axis.truncate().length(),
            matrix.z_axis.truncate().length(),
        );
        distance * scale.min_element()
    }

    fn repeated_local_point(&self, mut local: Vec3) -> Vec3 {
        if !self.repetition.enabled {
            return local;
        }
        for axis in 0..3 {
            if !self.repetition.axes[axis] || self.repetition.count[axis] <= 1 {
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

pub fn has_boolean_children(scene: &[SdfObject], id: uuid::Uuid) -> bool {
    scene.iter().any(|object| object.boolean_parent == Some(id))
}

pub fn group_world_matrix(scene: &[SdfObject], id: uuid::Uuid) -> Mat4 {
    fn visit(
        scene: &[SdfObject],
        id: uuid::Uuid,
        visited: &mut std::collections::HashSet<uuid::Uuid>,
    ) -> Mat4 {
        if !visited.insert(id) {
            return Mat4::IDENTITY;
        }
        let Some(object) = scene.iter().find(|object| object.uuid == id) else {
            return Mat4::IDENTITY;
        };
        let parent = object
            .boolean_parent
            .map(|parent| visit(scene, parent, visited))
            .unwrap_or(Mat4::IDENTITY);
        parent * object.group_transform.matrix()
    }
    visit(scene, id, &mut std::collections::HashSet::new())
}

pub fn parent_group_world_matrix(scene: &[SdfObject], id: uuid::Uuid) -> Mat4 {
    scene
        .iter()
        .find(|object| object.uuid == id)
        .and_then(|object| object.boolean_parent)
        .map(|parent| group_world_matrix(scene, parent))
        .unwrap_or(Mat4::IDENTITY)
}

pub fn object_world_matrix(scene: &[SdfObject], id: uuid::Uuid) -> Mat4 {
    let Some(object) = scene.iter().find(|object| object.uuid == id) else {
        return Mat4::IDENTITY;
    };
    group_world_matrix(scene, id) * object.transform.matrix()
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

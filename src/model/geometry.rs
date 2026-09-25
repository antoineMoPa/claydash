use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use sdf_consts::{TYPE_BOX, TYPE_CYLINDER, TYPE_POLYGON_PRISM, TYPE_SPHERE, TYPE_TORUS};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorState {
    Start,
    Grabbing,
    Scaling,
    Rotating,
    Extruding,
    DraggingFace,
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
}

impl PrimitiveKind {
    pub const ALL: [Self; 5] = [
        Self::Sphere,
        Self::Box,
        Self::Cylinder,
        Self::Torus,
        Self::PolygonPrism,
    ];
    pub const SPAWNABLE: [Self; 4] = [Self::Sphere, Self::Box, Self::Cylinder, Self::Torus];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sphere => "Sphere",
            Self::Box => "Box",
            Self::Cylinder => "Cylinder",
            Self::Torus => "Torus",
            Self::PolygonPrism => "Face shape",
        }
    }

    pub fn object_type(self) -> i32 {
        match self {
            Self::Sphere => TYPE_SPHERE,
            Self::Box => TYPE_BOX,
            Self::Cylinder => TYPE_CYLINDER,
            Self::Torus => TYPE_TORUS,
            Self::PolygonPrism => TYPE_POLYGON_PRISM,
        }
    }

    pub fn from_object_type(value: i32) -> Self {
        match value {
            TYPE_BOX => Self::Box,
            TYPE_CYLINDER => Self::Cylinder,
            TYPE_TORUS => Self::Torus,
            TYPE_POLYGON_PRISM => Self::PolygonPrism,
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
    Brick,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrickSettings {
    pub width: f32,
    pub course_height: f32,
    pub mortar_width: f32,
    pub wear: f32,
    pub relief: f32,
    pub bevel: f32,
    pub porosity: f32,
    pub firing: f32,
    pub mortar_color: Vec3,
    pub efflorescence: f32,
}

impl Default for BrickSettings {
    fn default() -> Self {
        Self {
            width: 0.52,
            course_height: 0.25,
            mortar_width: 0.030,
            wear: 0.42,
            relief: 0.038,
            bevel: 0.015,
            porosity: 0.62,
            firing: 0.48,
            mortar_color: Vec3::new(0.70, 0.67, 0.61),
            efflorescence: 0.16,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WoodSpecies {
    #[default]
    Oak,
    Walnut,
    Pine,
    Maple,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WoodStain {
    #[default]
    Honey,
    Cherry,
    Walnut,
    Smoked,
}

impl WoodStain {
    pub const ALL: [Self; 4] = [Self::Honey, Self::Cherry, Self::Walnut, Self::Smoked];

    pub fn label(self) -> &'static str {
        match self {
            Self::Honey => "Honey",
            Self::Cherry => "Warm cherry",
            Self::Walnut => "Dark walnut",
            Self::Smoked => "Smoked gray",
        }
    }

    pub fn gpu_code(self) -> f32 {
        match self {
            Self::Honey => 0.0,
            Self::Cherry => 1.0,
            Self::Walnut => 2.0,
            Self::Smoked => 3.0,
        }
    }
}

impl WoodSpecies {
    pub const ALL: [Self; 4] = [Self::Oak, Self::Walnut, Self::Pine, Self::Maple];

    pub fn label(self) -> &'static str {
        match self {
            Self::Oak => "Oak",
            Self::Walnut => "Walnut",
            Self::Pine => "Pine",
            Self::Maple => "Maple",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WoodSettings {
    pub species: WoodSpecies,
    pub ring_spacing: f32,
    pub ring_contrast: f32,
    pub pores: f32,
    pub figure: f32,
    pub cut_angle: f32,
    pub ring_relief: f32,
    pub ring_variation: f32,
    pub bump: f32,
    pub fiber_relief: f32,
    pub fiber_pigment: f32,
    pub fiber_directionality: f32,
    pub scale_falloff: f32,
    pub sanding_grit: f32,
    pub sanding_angle: f32,
    pub knots: f32,
    pub end_checks: f32,
    pub stain_color: WoodStain,
    pub stain_load: f32,
    pub coat: f32,
    pub coat_sheen: f32,
    pub coat_amber: f32,
}

impl Default for WoodSettings {
    fn default() -> Self {
        Self::preset(WoodSpecies::Oak)
    }
}

impl WoodSettings {
    pub fn preset(species: WoodSpecies) -> Self {
        let (ring_spacing, ring_contrast, pores, figure, knots, coat) = match species {
            WoodSpecies::Oak => (0.060, 0.52, 0.72, 0.48, 0.42, 0.50),
            WoodSpecies::Walnut => (0.055, 0.38, 0.38, 0.58, 0.28, 0.58),
            WoodSpecies::Pine => (0.080, 0.60, 0.05, 0.38, 0.78, 0.20),
            WoodSpecies::Maple => (0.045, 0.24, 0.08, 0.82, 0.18, 0.62),
        };
        Self {
            species,
            ring_spacing,
            ring_contrast,
            pores,
            figure,
            cut_angle: 0.15,
            ring_relief: 0.18,
            ring_variation: 0.62,
            bump: 0.65,
            fiber_relief: 0.62,
            fiber_pigment: 0.90,
            fiber_directionality: 11.0,
            scale_falloff: 0.85,
            sanding_grit: 0.48,
            sanding_angle: 0.0,
            knots,
            end_checks: 0.35,
            stain_color: WoodStain::Honey,
            stain_load: 0.0,
            coat,
            coat_sheen: 0.46,
            coat_amber: 0.25,
        }
    }
}

impl MaterialKind {
    pub const ALL: [Self; 6] = [
        Self::Transparent,
        Self::Metallic,
        Self::Solid,
        Self::Wood,
        Self::Brick,
        Self::Diagnostic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Transparent => "Transparent",
            Self::Metallic => "Metallic",
            Self::Solid => "Solid",
            Self::Wood => "Wood",
            Self::Brick => "Brick",
            Self::Diagnostic => "Diagnostic",
        }
    }

    pub fn gpu_code(self) -> u32 {
        match self {
            Self::Solid => 0,
            Self::Wood => 1,
            Self::Brick => 5,
            Self::Transparent => 2,
            Self::Metallic => 3,
            Self::Diagnostic => 4,
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
    #[serde(default)]
    pub wood: WoodSettings,
    #[serde(default)]
    pub brick: BrickSettings,
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
            name: material.display_name().to_string(),
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
            wood: WoodSettings::default(),
            brick: BrickSettings::default(),
        }
    }
}

impl Material {
    pub fn display_name(self) -> &'static str {
        match self.kind {
            MaterialKind::Wood => self.wood.species.label(),
            kind => kind.label(),
        }
    }

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
            MaterialKind::Diagnostic => Self {
                kind,
                color: Vec4::new(0.2, 0.65, 0.9, 1.0),
                ..Self::default()
            },
            MaterialKind::Wood => Self {
                kind,
                color: Vec4::new(0.64, 0.39, 0.20, 1.0),
                roughness: 0.65,
                ..Self::default()
            },
            MaterialKind::Brick => Self {
                kind,
                color: Vec4::new(0.51, 0.19, 0.14, 1.0),
                roughness: 0.88,
                ..Self::default()
            },
        }
    }

    pub fn wood_preset(species: WoodSpecies) -> Self {
        let color = match species {
            WoodSpecies::Oak => Vec4::new(0.64, 0.39, 0.20, 1.0),
            WoodSpecies::Walnut => Vec4::new(0.32, 0.17, 0.09, 1.0),
            WoodSpecies::Pine => Vec4::new(0.78, 0.59, 0.34, 1.0),
            WoodSpecies::Maple => Vec4::new(0.82, 0.69, 0.49, 1.0),
        };
        Self {
            color,
            wood: WoodSettings::preset(species),
            ..Self::preset(MaterialKind::Wood)
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Repetition {
    pub enabled: bool,
    pub count: [u32; 3],
    pub spacing: Vec3,
}

#[derive(Deserialize)]
struct StoredRepetition {
    enabled: bool,
    #[serde(default)]
    axes: Option<[bool; 3]>,
    count: [u32; 3],
    spacing: Vec3,
}

impl<'de> Deserialize<'de> for Repetition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredRepetition::deserialize(deserializer)?;
        let count = stored.axes.map_or(stored.count, |axes| {
            std::array::from_fn(|axis| if axes[axis] { stored.count[axis] } else { 1 })
        });
        Ok(Self {
            enabled: stored.enabled,
            count,
            spacing: stored.spacing,
        })
    }
}

impl Default for Repetition {
    fn default() -> Self {
        Self {
            enabled: false,
            count: [3, 1, 1],
            spacing: Vec3::splat(0.8),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mirror {
    pub axes: [bool; 3],
}

impl Default for Mirror {
    fn default() -> Self {
        Self {
            axes: [true, false, false],
        }
    }
}

impl Mirror {
    pub fn fold_point(self, point: Vec3, matrix: Mat4) -> Vec3 {
        let mut local = matrix.inverse().transform_point3(point);
        for axis in 0..3 {
            if self.axes[axis] {
                local[axis] = local[axis].abs();
            }
        }
        matrix.transform_point3(local)
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

pub const MAX_POLYGON_PRISM_VERTICES: usize = 32;

#[derive(Clone, Serialize, Deserialize)]
pub struct PolygonPrismParams {
    pub vertices: Vec<Vec2>,
    pub half_depth: f32,
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
    PolygonPrismParams(PolygonPrismParams),
}

/// A regular three-dimensional cage in the owning group's local space.
/// Only control-point offsets are stored; undeformed points are implicit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lattice {
    pub resolution: u8,
    pub min: Vec3,
    pub max: Vec3,
    pub offsets: Vec<Vec3>,
    #[serde(default)]
    pub shape_keys: Vec<LatticeShapeKey>,
    #[serde(default)]
    pub current_shape_key: Option<usize>,
    #[serde(default)]
    pub shape_position: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatticeShapeKey {
    pub name: String,
    pub offsets: Vec<Vec3>,
}

impl Lattice {
    pub fn new(min: Vec3, max: Vec3, resolution: u8) -> Self {
        let resolution = resolution.clamp(2, 9);
        Self {
            resolution,
            min,
            max,
            offsets: vec![
                Vec3::ZERO;
                resolution as usize * resolution as usize * resolution as usize
            ],
            shape_keys: Vec::new(),
            current_shape_key: Some(0),
            shape_position: 0.0,
        }
    }

    pub fn select_shape_key(&mut self, index: usize) {
        if index > self.shape_keys.len() {
            return;
        }
        self.current_shape_key = Some(index);
        self.apply_shape_position(index as f32);
    }

    pub fn add_shape_key(&mut self) -> usize {
        let index = self.shape_keys.len() + 1;
        self.shape_keys.push(LatticeShapeKey {
            name: format!("Shape {index}"),
            offsets: self.offsets.clone(),
        });
        self.select_shape_key(index);
        index
    }

    pub fn save_selected_shape_key(&mut self) {
        if let Some(index) = self.current_shape_key.filter(|index| *index > 0) {
            if let Some(key) = self.shape_keys.get_mut(index - 1) {
                key.offsets = self.offsets.clone();
            }
        }
    }

    pub fn apply_shape_position(&mut self, value: f32) {
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(0.0, self.shape_keys.len() as f32);
        let lower = value.floor() as usize;
        let upper = value.ceil() as usize;
        let amount = value - lower as f32;
        let count = self.offsets.len();
        let left = if lower == 0 {
            None
        } else {
            self.shape_keys.get(lower - 1).map(|key| &key.offsets)
        };
        let right = if upper == 0 {
            None
        } else {
            self.shape_keys.get(upper - 1).map(|key| &key.offsets)
        };
        if left.is_some_and(|offsets| offsets.len() != count)
            || right.is_some_and(|offsets| offsets.len() != count)
        {
            return;
        }
        for index in 0..count {
            let a = left.map_or(Vec3::ZERO, |offsets| offsets[index]);
            let b = right.map_or(Vec3::ZERO, |offsets| offsets[index]);
            self.offsets[index] = a.lerp(b, amount);
        }
        self.shape_position = value;
    }

    pub fn index(&self, x: usize, y: usize, z: usize) -> usize {
        let n = self.resolution as usize;
        x + n * (y + n * z)
    }

    pub fn position(&self, x: usize, y: usize, z: usize) -> Vec3 {
        let denominator = (self.resolution - 1) as f32;
        self.min + (self.max - self.min) * Vec3::new(x as f32, y as f32, z as f32) / denominator
    }

    pub fn is_surface_point(&self, x: usize, y: usize, z: usize) -> bool {
        let last = self.resolution as usize - 1;
        x == 0 || y == 0 || z == 0 || x == last || y == last || z == last
    }

    pub fn effective_offsets(&self) -> Vec<Vec3> {
        let n = self.resolution as usize;
        if self.offsets.len() != n * n * n {
            return vec![Vec3::ZERO; n * n * n];
        }
        let mut result = self.offsets.clone();
        let last = n - 1;
        for z in 1..last {
            for y in 1..last {
                for x in 1..last {
                    let tx = x as f32 / last as f32;
                    let ty = y as f32 / last as f32;
                    let tz = z as f32 / last as f32;
                    let along_x = self.offsets[self.index(0, y, z)]
                        .lerp(self.offsets[self.index(last, y, z)], tx);
                    let along_y = self.offsets[self.index(x, 0, z)]
                        .lerp(self.offsets[self.index(x, last, z)], ty);
                    let along_z = self.offsets[self.index(x, y, 0)]
                        .lerp(self.offsets[self.index(x, y, last)], tz);
                    result[self.index(x, y, z)] = (along_x + along_y + along_z) / 3.0;
                }
            }
        }
        result
    }

    pub fn displacement(&self, position: Vec3) -> Vec3 {
        let n = self.resolution as usize;
        if self.offsets.len() != n * n * n {
            return Vec3::ZERO;
        }
        let coordinates = ((position - self.min) / (self.max - self.min).max(Vec3::splat(0.0001)))
            .clamp(Vec3::ZERO, Vec3::ONE)
            * (n - 1) as f32;
        let lower = coordinates.floor().as_uvec3();
        let upper = (lower + glam::UVec3::ONE).min(glam::UVec3::splat((n - 1) as u32));
        let t = coordinates - lower.as_vec3();
        let offsets = self.effective_offsets();
        let sample =
            |x: u32, y: u32, z: u32| offsets[self.index(x as usize, y as usize, z as usize)];
        let a = sample(lower.x, lower.y, lower.z).lerp(sample(upper.x, lower.y, lower.z), t.x);
        let b = sample(lower.x, upper.y, lower.z).lerp(sample(upper.x, upper.y, lower.z), t.x);
        let c = sample(lower.x, lower.y, upper.z).lerp(sample(upper.x, lower.y, upper.z), t.x);
        let d = sample(lower.x, upper.y, upper.z).lerp(sample(upper.x, upper.y, upper.z), t.x);
        a.lerp(b, t.y).lerp(c.lerp(d, t.y), t.z)
    }

    pub fn resize(&mut self, resolution: u8) {
        let resolution = resolution.clamp(2, 9);
        if resolution == self.resolution {
            return;
        }
        let previous = self.clone();
        *self = Self::new(previous.min, previous.max, resolution);
        self.current_shape_key = previous.current_shape_key;
        self.shape_position = previous.shape_position;
        self.shape_keys = previous
            .shape_keys
            .iter()
            .map(|key| LatticeShapeKey {
                name: key.name.clone(),
                offsets: vec![
                    Vec3::ZERO;
                    resolution as usize * resolution as usize * resolution as usize
                ],
            })
            .collect();
        for z in 0..resolution as usize {
            for y in 0..resolution as usize {
                for x in 0..resolution as usize {
                    let index = self.index(x, y, z);
                    self.offsets[index] = previous.displacement(self.position(x, y, z));
                }
            }
        }
        let min = self.min;
        let max = self.max;
        let denominator = (resolution - 1) as f32;
        for (key, previous_key) in self.shape_keys.iter_mut().zip(&previous.shape_keys) {
            let mut previous_shape = previous.clone();
            previous_shape.offsets = previous_key.offsets.clone();
            for z in 0..resolution as usize {
                for y in 0..resolution as usize {
                    for x in 0..resolution as usize {
                        let index = x + resolution as usize * (y + resolution as usize * z);
                        let position = min
                            + (max - min) * Vec3::new(x as f32, y as f32, z as f32) / denominator;
                        key.offsets[index] = previous_shape.displacement(position);
                    }
                }
            }
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
        self.distance_with_matrix_at(point, matrix, true)
    }

    pub(crate) fn distance_with_matrix_without_repetition(&self, point: Vec3, matrix: Mat4) -> f32 {
        self.distance_with_matrix_at(point, matrix, false)
    }

    fn distance_with_matrix_at(&self, point: Vec3, matrix: Mat4, repeat: bool) -> f32 {
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

pub fn polygon_distance(point: Vec2, vertices: &[Vec2]) -> f32 {
    if vertices.len() < 3 {
        return f32::INFINITY;
    }
    let mut distance_squared = f32::INFINITY;
    let mut inside = false;
    for index in 0..vertices.len() {
        let a = vertices[index];
        let b = vertices[(index + 1) % vertices.len()];
        let edge = b - a;
        let relative = point - a;
        let edge_length_squared = edge.length_squared();
        if edge_length_squared > 0.000_000_1 {
            let closest = a + edge * (relative.dot(edge) / edge_length_squared).clamp(0.0, 1.0);
            distance_squared = distance_squared.min(point.distance_squared(closest));
        }
        if (a.y > point.y) != (b.y > point.y) {
            let crossing_x = a.x + (point.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if point.x < crossing_x {
                inside = !inside;
            }
        }
    }
    distance_squared.sqrt() * if inside { -1.0 } else { 1.0 }
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

pub fn lattice_world_matrix(scene: &[SdfObject], id: uuid::Uuid) -> Mat4 {
    if has_boolean_children(scene, id) {
        group_world_matrix(scene, id)
    } else {
        object_world_matrix(scene, id)
    }
}

pub fn lattice_bounds(scene: &[SdfObject], root: uuid::Uuid) -> Option<(Vec3, Vec3)> {
    let root_inverse = lattice_world_matrix(scene, root).inverse();
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for object in scene {
        let mut ancestor = Some(object.uuid);
        let mut belongs = false;
        for _ in 0..scene.len() {
            let Some(id) = ancestor else {
                break;
            };
            if id == root {
                belongs = true;
                break;
            }
            ancestor = scene
                .iter()
                .find(|candidate| candidate.uuid == id)
                .and_then(|candidate| candidate.boolean_parent);
        }
        if !belongs {
            continue;
        }
        let extent = match &object.params {
            SdfParams::SphereParams(p) => Vec3::splat(p.radius),
            SdfParams::BoxParams(p) => p.box_q,
            SdfParams::CylinderParams {
                radius,
                half_height,
            } => Vec3::new(*radius, *half_height, *radius),
            SdfParams::TorusParams {
                major_radius,
                minor_radius,
            } => Vec3::new(
                major_radius + minor_radius,
                *minor_radius,
                major_radius + minor_radius,
            ),
            SdfParams::PolygonPrismParams(p) => {
                let planar = p
                    .vertices
                    .iter()
                    .fold(Vec2::ZERO, |size, point| size.max(point.abs()));
                Vec3::new(planar.x, planar.y, p.half_depth)
            }
        };
        let mut extent = extent;
        if object.repetition.enabled {
            for axis in 0..3 {
                if object.repetition.count[axis] > 1 {
                    extent[axis] += object.repetition.count[axis].saturating_sub(1) as f32
                        * object.repetition.spacing[axis].max(0.001)
                        * 0.5;
                }
            }
        }
        let matrix = root_inverse * object_world_matrix(scene, object.uuid);
        let center = matrix.transform_point3(Vec3::ZERO);
        let half = matrix.x_axis.truncate().abs() * extent.x
            + matrix.y_axis.truncate().abs() * extent.y
            + matrix.z_axis.truncate().abs() * extent.z;
        minimum = minimum.min(center - half);
        maximum = maximum.max(center + half);
    }
    if !minimum.is_finite() {
        return None;
    }
    let padding = (maximum - minimum).max(Vec3::splat(0.1)) * 0.1;
    Some((minimum - padding, maximum + padding))
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

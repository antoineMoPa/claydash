use glam::{EulerRot, Mat4, Quat, Vec2, Vec3, Vec4};
use observable_key_value_tree::{CanBeNone, ObservableKVTree};
use sdf_consts::{TYPE_BOX, TYPE_CYLINDER, TYPE_SPHERE, TYPE_TORUS};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorAxis {
    X,
    Y,
    Z,
}

impl VectorAxis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    pub fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorChannel {
    Red,
    Green,
    Blue,
    Alpha,
}

impl ColorChannel {
    pub const ALL: [Self; 4] = [Self::Red, Self::Green, Self::Blue, Self::Alpha];

    pub fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Alpha => "Alpha",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Red => 0,
            Self::Green => 1,
            Self::Blue => 2,
            Self::Alpha => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimatableProperty {
    Position(VectorAxis),
    Rotation(VectorAxis),
    Scale(VectorAxis),
    BoxHalfExtent(VectorAxis),
    SphereRadius,
    CylinderRadius,
    CylinderHalfHeight,
    TorusMajorRadius,
    TorusMinorRadius,
    MaterialColor(ColorChannel),
    MaterialRoughness,
    MaterialMetallic,
    MaterialReflectivity,
    MaterialRefractiveIndex,
    MaterialOpacity,
    OperandSoftness,
    RepetitionEnabled,
    RepetitionAxis(VectorAxis),
    RepetitionCount(VectorAxis),
    RepetitionSpacing(VectorAxis),
}

impl AnimatableProperty {
    pub fn label(self) -> String {
        match self {
            Self::Position(axis) => format!("Position {}", axis.label()),
            Self::Rotation(axis) => format!("Rotation {}", axis.label()),
            Self::Scale(axis) => format!("Scale {}", axis.label()),
            Self::BoxHalfExtent(axis) => format!("Half extent {}", axis.label()),
            Self::SphereRadius => "Radius".into(),
            Self::CylinderRadius => "Radius".into(),
            Self::CylinderHalfHeight => "Half height".into(),
            Self::TorusMajorRadius => "Major radius".into(),
            Self::TorusMinorRadius => "Tube radius".into(),
            Self::MaterialColor(channel) => format!("Color {}", channel.label()),
            Self::MaterialRoughness => "Roughness".into(),
            Self::MaterialMetallic => "Metallic".into(),
            Self::MaterialReflectivity => "Reflectivity".into(),
            Self::MaterialRefractiveIndex => "Refractive index".into(),
            Self::MaterialOpacity => "Opacity".into(),
            Self::OperandSoftness => "Softness".into(),
            Self::RepetitionEnabled => "Repetition enabled".into(),
            Self::RepetitionAxis(axis) => format!("Repeat {}", axis.label()),
            Self::RepetitionCount(axis) => format!("Count {}", axis.label()),
            Self::RepetitionSpacing(axis) => format!("Spacing {}", axis.label()),
        }
    }

    pub fn value(self, object: &SdfObject) -> Option<f32> {
        match self {
            Self::Position(axis) => Some(object.transform.translation[axis.index()]),
            Self::Rotation(axis) => {
                let (x, y, z) = object.transform.rotation.to_euler(EulerRot::XYZ);
                Some([x.to_degrees(), y.to_degrees(), z.to_degrees()][axis.index()])
            }
            Self::Scale(axis) => Some(object.transform.scale[axis.index()]),
            Self::BoxHalfExtent(axis) => match &object.params {
                SdfParams::BoxParams(params) => Some(params.box_q[axis.index()]),
                _ => None,
            },
            Self::SphereRadius => match &object.params {
                SdfParams::SphereParams(params) => Some(params.radius),
                _ => None,
            },
            Self::CylinderRadius => match &object.params {
                SdfParams::CylinderParams { radius, .. } => Some(*radius),
                _ => None,
            },
            Self::CylinderHalfHeight => match &object.params {
                SdfParams::CylinderParams { half_height, .. } => Some(*half_height),
                _ => None,
            },
            Self::TorusMajorRadius => match &object.params {
                SdfParams::TorusParams { major_radius, .. } => Some(*major_radius),
                _ => None,
            },
            Self::TorusMinorRadius => match &object.params {
                SdfParams::TorusParams { minor_radius, .. } => Some(*minor_radius),
                _ => None,
            },
            Self::MaterialColor(channel) => Some(object.material.color[channel.index()]),
            Self::MaterialRoughness => Some(object.material.roughness),
            Self::MaterialMetallic => Some(object.material.metallic),
            Self::MaterialReflectivity => Some(object.material.reflectivity),
            Self::MaterialRefractiveIndex => Some(object.material.refractive_index),
            Self::MaterialOpacity => Some(object.material.opacity),
            Self::OperandSoftness => Some(object.softness),
            Self::RepetitionEnabled => Some(if object.repetition.enabled { 1.0 } else { 0.0 }),
            Self::RepetitionAxis(axis) => Some(if object.repetition.axes[axis.index()] {
                1.0
            } else {
                0.0
            }),
            Self::RepetitionCount(axis) => Some(object.repetition.count[axis.index()] as f32),
            Self::RepetitionSpacing(axis) => Some(object.repetition.spacing[axis.index()]),
        }
    }

    pub fn apply(self, object: &mut SdfObject, value: f32) {
        match self {
            Self::Position(axis) => object.transform.translation[axis.index()] = value,
            Self::Rotation(axis) => {
                let (x, y, z) = object.transform.rotation.to_euler(EulerRot::XYZ);
                let mut degrees = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
                degrees[axis.index()] = value;
                object.transform.rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    degrees[0].to_radians(),
                    degrees[1].to_radians(),
                    degrees[2].to_radians(),
                );
            }
            Self::Scale(axis) => object.transform.scale[axis.index()] = value,
            Self::BoxHalfExtent(axis) => {
                if let SdfParams::BoxParams(params) = &mut object.params {
                    params.box_q[axis.index()] = value.max(0.01);
                }
            }
            Self::SphereRadius => {
                if let SdfParams::SphereParams(params) = &mut object.params {
                    params.radius = value.max(0.01);
                }
            }
            Self::CylinderRadius => {
                if let SdfParams::CylinderParams { radius, .. } = &mut object.params {
                    *radius = value.max(0.01);
                }
            }
            Self::CylinderHalfHeight => {
                if let SdfParams::CylinderParams { half_height, .. } = &mut object.params {
                    *half_height = value.max(0.01);
                }
            }
            Self::TorusMajorRadius => {
                if let SdfParams::TorusParams { major_radius, .. } = &mut object.params {
                    *major_radius = value.max(0.02);
                }
            }
            Self::TorusMinorRadius => {
                if let SdfParams::TorusParams { minor_radius, .. } = &mut object.params {
                    *minor_radius = value.max(0.01);
                }
            }
            Self::MaterialColor(channel) => {
                object.material.color[channel.index()] = value.clamp(0.0, 1.0);
                object.color = object.material.color;
            }
            Self::MaterialRoughness => object.material.roughness = value.clamp(0.0, 1.0),
            Self::MaterialMetallic => object.material.metallic = value.clamp(0.0, 1.0),
            Self::MaterialReflectivity => object.material.reflectivity = value.clamp(0.0, 1.0),
            Self::MaterialRefractiveIndex => {
                object.material.refractive_index = value.clamp(1.0, 2.5)
            }
            Self::MaterialOpacity => object.material.opacity = value.clamp(0.02, 1.0),
            Self::OperandSoftness => object.softness = value.clamp(0.0, 0.5),
            Self::RepetitionEnabled => object.repetition.enabled = value >= 0.5,
            Self::RepetitionAxis(axis) => object.repetition.axes[axis.index()] = value >= 0.5,
            Self::RepetitionCount(axis) => {
                object.repetition.count[axis.index()] = value.round().clamp(1.0, 32.0) as u32
            }
            Self::RepetitionSpacing(axis) => {
                object.repetition.spacing[axis.index()] = value.clamp(0.01, 20.0)
            }
        }
    }

    pub fn uses_step_interpolation(self) -> bool {
        matches!(
            self,
            Self::RepetitionEnabled | Self::RepetitionAxis(_) | Self::RepetitionCount(_)
        )
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimationBinding {
    pub object: uuid::Uuid,
    pub property: AnimatableProperty,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyframeInterpolation {
    Constant,
    Linear,
    #[default]
    Bezier,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct BezierHandle {
    /// Frame offset from the keyframe. Incoming handles are negative.
    pub frame_offset: f32,
    /// Value offset from the keyframe.
    pub value_offset: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Keyframe {
    pub frame: u32,
    pub value: f32,
    pub interpolation: KeyframeInterpolation,
    /// An absent handle uses an automatic one-third control point.
    pub incoming_handle: Option<BezierHandle>,
    /// An absent handle uses an automatic one-third control point.
    pub outgoing_handle: Option<BezierHandle>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnimationTrack {
    pub binding: AnimationBinding,
    pub keyframes: Vec<Keyframe>,
}

fn default_animation_fps() -> f32 {
    24.0
}

fn default_animation_end_frame() -> u32 {
    250
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnimationData {
    #[serde(default = "default_animation_fps")]
    pub fps: f32,
    #[serde(default)]
    pub start_frame: u32,
    #[serde(default = "default_animation_end_frame")]
    pub end_frame: u32,
    #[serde(default)]
    pub tracks: Vec<AnimationTrack>,
}

impl Default for AnimationData {
    fn default() -> Self {
        Self {
            fps: default_animation_fps(),
            start_frame: 0,
            end_frame: default_animation_end_frame(),
            tracks: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Material {
    pub kind: MaterialKind,
    pub color: Vec4,
    pub roughness: f32,
    pub metallic: f32,
    pub reflectivity: f32,
    pub refractive_index: f32,
    pub opacity: f32,
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
            },
            MaterialKind::Metallic => Self {
                kind,
                color: Vec4::new(0.62, 0.66, 0.72, 1.0),
                roughness: 0.24,
                metallic: 1.0,
                reflectivity: 0.82,
                refractive_index: 1.0,
                opacity: 1.0,
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

#[derive(Clone, Copy, Serialize, Deserialize)]
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

    pub fn distance(&self, point: Vec3) -> f32 {
        let local = (self.transform.matrix().inverse() * point.extend(1.0)).truncate();
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
        distance * self.transform.scale.abs().min_element()
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

#[derive(Clone, Serialize, Deserialize)]
pub enum ClaydashValue {
    Animation(AnimationData),
    Material(Material),
    BooleanPick(BooleanPick),
    Uuid(uuid::Uuid),
    VecUuid(Vec<uuid::Uuid>),
    F32(f32),
    Vec2(Vec2),
    Vec4(Vec4),
    Transform(Transform),
    VecSDFObject(Vec<SdfObject>),
    EditorState(EditorState),
    SelectionScope(SelectionScope),
    Bool(bool),
    #[serde(skip)]
    Fn(fn(&mut ObservableKVTree<ClaydashValue>)),
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionScope {
    #[default]
    Group,
    Exact,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct BooleanPick {
    pub target: uuid::Uuid,
    pub operation: BooleanOperation,
}

impl Default for ClaydashValue {
    fn default() -> Self {
        Self::None
    }
}

impl CanBeNone<ClaydashValue> for ClaydashValue {
    fn none() -> Self {
        Self::None
    }
}

pub type DataTree = ObservableKVTree<ClaydashValue>;

pub fn picked_material(tree: &DataTree) -> Material {
    match tree.get_path("editor.material") {
        ClaydashValue::Material(material) => material,
        _ => {
            let mut material = Material::default();
            if let ClaydashValue::Vec4(color) = tree.get_path("editor.color") {
                material.color = color;
            }
            material
        }
    }
}

pub fn boolean_distance(a: f32, b: f32, operation: BooleanOperation, softness: f32) -> f32 {
    let b = if operation == BooleanOperation::Subtract {
        -b
    } else {
        b
    };
    let hard = if operation == BooleanOperation::Union {
        a.min(b)
    } else {
        a.max(b)
    };
    if softness <= 0.0 {
        return hard;
    }
    let h = (softness - (a - b).abs()).max(0.0) / softness;
    let blend = softness * h * h * 0.25;
    if operation == BooleanOperation::Union {
        hard - blend
    } else {
        hard + blend
    }
}

pub fn objects(tree: &DataTree) -> Vec<SdfObject> {
    match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(value) => value,
        _ => vec![],
    }
}

pub fn selected(tree: &DataTree) -> Vec<uuid::Uuid> {
    match tree.get_path("scene.selected_uuids") {
        ClaydashValue::VecUuid(value) => value,
        _ => vec![],
    }
}

pub fn objects_ref(tree: &DataTree) -> &[SdfObject] {
    match tree.get_path_ref("scene.sdf_objects") {
        Some(ClaydashValue::VecSDFObject(value)) => value,
        _ => &[],
    }
}

pub fn selected_ref(tree: &DataTree) -> &[uuid::Uuid] {
    match tree.get_path_ref("scene.selected_uuids") {
        Some(ClaydashValue::VecUuid(value)) => value,
        _ => &[],
    }
}

pub fn selection_scope(tree: &DataTree) -> SelectionScope {
    match tree.get_path("scene.selection_scope") {
        ClaydashValue::SelectionScope(scope) => scope,
        _ => SelectionScope::Group,
    }
}

pub fn set_objects(tree: &mut DataTree, value: Vec<SdfObject>) {
    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(value));
}

pub fn set_objects_transient(tree: &mut DataTree, value: Vec<SdfObject>) {
    tree.set_transient_path("scene.sdf_objects", ClaydashValue::VecSDFObject(value));
}

pub fn set_selected(tree: &mut DataTree, value: Vec<uuid::Uuid>) {
    tree.set_transient_path(
        "scene.selection_scope",
        ClaydashValue::SelectionScope(SelectionScope::Group),
    );
    tree.set_transient_path("scene.selected_uuids", ClaydashValue::VecUuid(value));
}

pub fn set_selected_exact(tree: &mut DataTree, value: Vec<uuid::Uuid>) {
    tree.set_transient_path(
        "scene.selection_scope",
        ClaydashValue::SelectionScope(SelectionScope::Exact),
    );
    tree.set_transient_path("scene.selected_uuids", ClaydashValue::VecUuid(value));
}

/// Deterministic visual QA scene: materials above, boolean operations below.
#[cfg(not(target_arch = "wasm32"))]
pub fn ui_preview_scene() -> Vec<SdfObject> {
    let mut scene = Vec::new();
    for (index, kind) in [
        MaterialKind::Solid,
        MaterialKind::Metallic,
        MaterialKind::Transparent,
    ]
    .into_iter()
    .enumerate()
    {
        let mut sphere = SdfObject::create(TYPE_SPHERE);
        sphere.name = kind.label().into();
        sphere.params = SdfParams::SphereParams(SphereParams { radius: 0.48 });
        sphere.transform.translation = Vec3::new((index as f32 - 1.0) * 1.35, 0.7, 0.0);
        sphere.material = Material::preset(kind);
        sphere.color = sphere.material.color;
        scene.push(sphere);
    }
    let mut backdrop = SdfObject::create(TYPE_BOX);
    backdrop.name = "Orange bar behind glass".into();
    backdrop.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::new(2.1, 0.12, 0.12),
    });
    backdrop.transform.translation = Vec3::new(0.0, 0.7, -0.9);
    backdrop.material.color = Vec4::new(1.0, 0.23, 0.025, 1.0);
    backdrop.color = backdrop.material.color;
    scene.push(backdrop);
    for (index, operation) in [
        BooleanOperation::Union,
        BooleanOperation::Subtract,
        BooleanOperation::Intersect,
    ]
    .into_iter()
    .enumerate()
    {
        let mut target = SdfObject::create(TYPE_BOX);
        target.name = format!("{} target", operation.label());
        target.params = SdfParams::BoxParams(BoxParams {
            box_q: Vec3::splat(0.4),
        });
        target.transform.translation = Vec3::new((index as f32 - 1.0) * 1.35, -0.65, 0.0);
        target.material.color = Vec4::new(0.15, 0.6, 0.8, 1.0);
        target.color = target.material.color;
        let mut operand = SdfObject::create(TYPE_SPHERE);
        operand.name = format!("{} operand", operation.label());
        operand.params = SdfParams::SphereParams(SphereParams { radius: 0.43 });
        operand.transform.translation = target.transform.translation + Vec3::new(0.18, 0.16, 0.35);
        operand.boolean_parent = Some(target.uuid);
        operand.operation = operation;
        operand.material = target.material;
        operand.color = target.color;
        scene.extend([target, operand]);
    }
    scene
}

/// Evaluate each subtree before combining it with its parent. Parent links are
/// independent of storage order, and root objects always union with one another.
pub fn scene_sample(point: Vec3, scene: &[SdfObject]) -> Option<(f32, uuid::Uuid)> {
    fn subtree(point: Vec3, scene: &[SdfObject], index: usize, depth: usize) -> (f32, uuid::Uuid) {
        let object = &scene[index];
        let mut result = (object.distance(point), object.uuid);
        if depth >= scene.len() {
            return result;
        }
        for (child_index, child) in scene.iter().enumerate() {
            if child.boolean_parent != Some(object.uuid) {
                continue;
            }
            let candidate = subtree(point, scene, child_index, depth + 1);
            let distance = boolean_distance(result.0, candidate.0, child.operation, child.softness);
            match child.operation {
                BooleanOperation::Union if candidate.0 < result.0 => result = candidate,
                BooleanOperation::Subtract => result.0 = result.0.max(-candidate.0),
                BooleanOperation::Intersect if candidate.0 > result.0 => result = candidate,
                _ => {}
            }
            result.0 = distance;
        }
        result
    }
    scene
        .iter()
        .enumerate()
        .filter(|(_, object)| object.boolean_parent.is_none())
        .map(|(index, _)| subtree(point, scene, index, 0))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn renderer_stress_scene() -> Vec<SdfObject> {
    use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

    let layers = if std::env::args().any(|arg| arg == "--benchmark-1024") {
        16
    } else {
        4
    };
    let mut objects = Vec::with_capacity(layers * 64);
    for z in 0..layers {
        for y in 0..8 {
            for x in 0..8 {
                let object_type = if (x + y + z) % 2 == 0 {
                    TYPE_SPHERE
                } else {
                    TYPE_BOX
                };
                let mut object = SdfObject::create(object_type);
                object.transform.translation = Vec3::new(
                    (x as f32 - 3.5) * 0.42,
                    (y as f32 - 3.5) * 0.42,
                    (z as f32 - (layers as f32 - 1.0) * 0.5) * 0.42,
                );
                object.transform.rotation = Quat::from_euler(
                    glam::EulerRot::XYZ,
                    x as f32 * 0.11,
                    y as f32 * 0.07,
                    z as f32 * 0.17,
                );
                object.transform.scale = Vec3::new(
                    0.8 + (x % 3) as f32 * 0.14,
                    0.8 + (y % 3) as f32 * 0.14,
                    0.8 + (z % 3) as f32 * 0.14,
                );
                object.color = Vec4::new(
                    0.25 + x as f32 * 0.07,
                    0.2 + y as f32 * 0.06,
                    0.35 + z as f32 * 0.14,
                    1.0,
                );
                objects.push(object);
            }
        }
    }
    objects
}

/// Deterministic fixtures for timing and pixel comparisons with every material.
#[cfg(not(target_arch = "wasm32"))]
pub fn renderer_benchmark_scenes() -> Vec<(String, Vec<SdfObject>)> {
    let mut cases = Vec::new();
    for kind in MaterialKind::ALL {
        let mut scene = renderer_stress_scene();
        for object in &mut scene {
            object.material = Material::preset(kind);
            if kind == MaterialKind::Wood {
                object.color = object.material.color;
            }
        }
        let name = match kind {
            MaterialKind::Transparent => "transparent",
            MaterialKind::Metallic => "metallic",
            MaterialKind::Solid => "solid",
            MaterialKind::Wood => "wood",
        };
        cases.push((name.into(), scene));
    }
    let mut mixed = renderer_stress_scene();
    for (i, object) in mixed.iter_mut().enumerate() {
        let template = SdfObject::create(PrimitiveKind::ALL[i % 4].object_type());
        object.object_type = template.object_type;
        object.params = template.params;
        object.material = Material::preset(MaterialKind::ALL[i % 3]);
    }
    cases.push(("mixed".into(), mixed.clone()));
    for pair in mixed.chunks_mut(2) {
        pair[1].boolean_parent = Some(pair[0].uuid);
        pair[1].operation = BooleanOperation::Subtract;
    }
    cases.push(("booleans".into(), mixed.clone()));
    for group in mixed.chunks_mut(4) {
        group[0].boolean_parent = None;
        for i in 1..group.len() {
            group[i].boolean_parent = Some(group[i - 1].uuid);
            group[i].operation = if i == 2 {
                BooleanOperation::Intersect
            } else {
                BooleanOperation::Subtract
            };
        }
    }
    cases.push(("nested".into(), mixed));
    let mut repeated: Vec<_> = renderer_stress_scene().into_iter().take(64).collect();
    for (i, object) in repeated.iter_mut().enumerate() {
        let template = SdfObject::create(PrimitiveKind::ALL[i % 4].object_type());
        object.object_type = template.object_type;
        object.params = template.params;
        object.material = Material::preset(MaterialKind::ALL[i % 3]);
        object.repetition.enabled = true;
        object.repetition.axes = [true; 3];
        object.repetition.count = [3; 3];
        object.repetition.spacing = Vec3::splat(1.5);
    }
    cases.push(("repeated".into(), repeated));
    cases.push(("preview".into(), ui_preview_scene()));
    cases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_kind_maps_every_gpu_type_explicitly() {
        for kind in PrimitiveKind::ALL {
            assert_eq!(PrimitiveKind::from_object_type(kind.object_type()), kind);
        }
    }

    #[test]
    fn finite_domain_repetition_creates_pickable_copies() {
        let mut sphere = SdfObject::create_kind(PrimitiveKind::Sphere);
        sphere.repetition.enabled = true;
        sphere.repetition.axes = [true, false, false];
        sphere.repetition.count = [3, 1, 1];
        sphere.repetition.spacing = Vec3::splat(1.0);

        assert!(sphere.distance(Vec3::X) < 0.0);
        assert!(sphere.distance(Vec3::X * 2.0) > 0.5);
    }

    #[test]
    fn material_presets_expose_distinct_surface_properties() {
        let transparent = Material::preset(MaterialKind::Transparent);
        let metallic = Material::preset(MaterialKind::Metallic);
        let solid = Material::preset(MaterialKind::Solid);

        assert!(transparent.opacity < solid.opacity);
        assert!(metallic.metallic > solid.metallic);
        assert!(transparent.refractive_index > 1.0);
    }

    #[test]
    fn softness_blends_all_operations_and_zero_preserves_hard_edges() {
        assert_eq!(
            boolean_distance(0.0, 0.0, BooleanOperation::Union, 0.2),
            -0.05
        );
        for operation in [BooleanOperation::Subtract, BooleanOperation::Intersect] {
            assert_eq!(boolean_distance(0.0, 0.0, operation, 0.2), 0.05);
        }
        for operation in [
            BooleanOperation::Union,
            BooleanOperation::Subtract,
            BooleanOperation::Intersect,
        ] {
            let b = if operation == BooleanOperation::Subtract {
                -0.7
            } else {
                0.7
            };
            let hard = if operation == BooleanOperation::Union {
                (-0.1_f32).min(b)
            } else {
                (-0.1_f32).max(b)
            };
            assert_eq!(boolean_distance(-0.1, 0.7, operation, 0.0), hard);
            assert_eq!(boolean_distance(-0.1, 0.7, operation, 0.05), hard);
        }
    }

    #[test]
    fn new_objects_are_soft_but_old_documents_keep_their_geometry() {
        let object = SdfObject::create_kind(PrimitiveKind::Sphere);
        assert_eq!(object.softness, 0.05);
        let mut value = serde_json::to_value(&object).unwrap();
        value.as_object_mut().unwrap().remove("softness");
        let old: SdfObject = serde_json::from_value(value).unwrap();
        assert_eq!(old.softness, 0.0);
    }

    #[test]
    fn boolean_cut_does_not_remove_an_unrelated_root() {
        let mut target = SdfObject::create(TYPE_BOX);
        target.transform.scale = Vec3::splat(4.0);
        let mut cutter = SdfObject::create(TYPE_SPHERE);
        cutter.operation = BooleanOperation::Subtract;
        cutter.boolean_parent = Some(target.uuid);
        let island = SdfObject::create(TYPE_SPHERE);
        assert!(
            scene_sample(Vec3::ZERO, &[target.clone(), cutter.clone()])
                .unwrap()
                .0
                > 0.0
        );
        assert!(
            scene_sample(Vec3::ZERO, &[island, cutter, target])
                .unwrap()
                .0
                < 0.0
        );
    }

    #[test]
    fn nested_cutter_group_is_evaluated_before_subtraction() {
        let mut target = SdfObject::create(TYPE_BOX);
        target.transform.scale = Vec3::splat(4.0);
        let mut cutter = SdfObject::create(TYPE_SPHERE);
        cutter.boolean_parent = Some(target.uuid);
        cutter.operation = BooleanOperation::Subtract;
        let mut cutter_hole = SdfObject::create(TYPE_SPHERE);
        cutter_hole.transform.scale = Vec3::splat(0.4);
        cutter_hole.boolean_parent = Some(cutter.uuid);
        cutter_hole.operation = BooleanOperation::Subtract;
        let scene = [cutter_hole, target, cutter];
        assert!(scene_sample(Vec3::ZERO, &scene).unwrap().0 < 0.0);
        assert!(scene_sample(Vec3::X * 0.2, &scene).unwrap().0 > 0.0);
    }
}

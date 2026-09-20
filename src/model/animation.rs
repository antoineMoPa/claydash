use glam::{EulerRot, Quat};
use serde::{Deserialize, Serialize};

use super::{SdfObject, SdfParams};

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
    GroupPosition(VectorAxis),
    GroupRotation(VectorAxis),
    GroupScale(VectorAxis),
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
            Self::GroupPosition(axis) => format!("Group position {}", axis.label()),
            Self::GroupRotation(axis) => format!("Group rotation {}", axis.label()),
            Self::GroupScale(axis) => format!("Group scale {}", axis.label()),
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
            Self::GroupPosition(axis) => Some(object.group_transform.translation[axis.index()]),
            Self::GroupRotation(axis) => {
                let (x, y, z) = object.group_transform.rotation.to_euler(EulerRot::XYZ);
                Some([x.to_degrees(), y.to_degrees(), z.to_degrees()][axis.index()])
            }
            Self::GroupScale(axis) => Some(object.group_transform.scale[axis.index()]),
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
            Self::GroupPosition(axis) => object.group_transform.translation[axis.index()] = value,
            Self::GroupRotation(axis) => {
                let (x, y, z) = object.group_transform.rotation.to_euler(EulerRot::XYZ);
                let mut degrees = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
                degrees[axis.index()] = value;
                object.group_transform.rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    degrees[0].to_radians(),
                    degrees[1].to_radians(),
                    degrees[2].to_radians(),
                );
            }
            Self::GroupScale(axis) => object.group_transform.scale[axis.index()] = value,
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

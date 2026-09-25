use glam::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

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

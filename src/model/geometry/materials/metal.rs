use super::{Material, MaterialKind};
use glam::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MetalSpecies {
    #[default]
    Steel,
    Iron,
    Aluminum,
    Copper,
    Bronze,
    Gold,
    Silver,
    Chrome,
    Lead,
}
impl MetalSpecies {
    pub const ALL: [Self; 9] = [
        Self::Steel,
        Self::Iron,
        Self::Aluminum,
        Self::Copper,
        Self::Bronze,
        Self::Gold,
        Self::Silver,
        Self::Chrome,
        Self::Lead,
    ];
    pub fn label(self) -> &'static str {
        [
            "Steel", "Iron", "Aluminum", "Copper", "Bronze", "Gold", "Silver", "Chrome", "Lead",
        ][self as usize]
    }
    /// Linear RGB conductor reflectance proxies, matching the material study.
    pub fn f0(self) -> Vec3 {
        Vec3::from_array(
            [
                [0.56, 0.57, 0.58],
                [0.56, 0.57, 0.58],
                [0.913, 0.922, 0.924],
                [0.955, 0.638, 0.538],
                [0.70, 0.46, 0.22],
                [1.0, 0.766, 0.336],
                [0.972, 0.960, 0.915],
                [0.55, 0.556, 0.554],
                [0.43, 0.46, 0.49],
            ][self as usize],
        )
    }
    pub fn oxide(self) -> Vec3 {
        Vec3::from_array(
            [
                [0.12, 0.026, 0.008],
                [0.085, 0.022, 0.006],
                [0.42, 0.44, 0.46],
                [0.035, 0.24, 0.16],
                [0.055, 0.20, 0.12],
                [0.24, 0.16, 0.04],
                [0.035, 0.03, 0.026],
                [0.18, 0.20, 0.20],
                [0.17, 0.18, 0.20],
            ][self as usize],
        )
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MetalFinish {
    #[default]
    Polished,
    Brushed,
    Satin,
    Hammered,
    Cast,
}
impl MetalFinish {
    pub const ALL: [Self; 5] = [
        Self::Polished,
        Self::Brushed,
        Self::Satin,
        Self::Hammered,
        Self::Cast,
    ];
    pub fn label(self) -> &'static str {
        ["Polished", "Brushed", "Satin", "Hammered", "Cast / pitted"][self as usize]
    }
    pub fn defaults(self) -> (f32, f32, f32) {
        [
            (0.12, 0.0, 0.25),
            (0.29, 0.75, 0.60),
            (0.43, 0.30, 0.40),
            (0.28, 0.0, 0.70),
            (0.63, 0.05, 0.90),
        ][self as usize]
    }
    pub fn gpu_code(self) -> f32 {
        self as u8 as f32
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MetalTangent {
    #[default]
    Linear,
    Radial,
    Wire,
}
impl MetalTangent {
    pub const ALL: [Self; 3] = [Self::Linear, Self::Radial, Self::Wire];
    pub fn label(self) -> &'static str {
        ["Linear", "Radial", "Wire axis (local Z)"][self as usize]
    }
    pub fn gpu_code(self) -> f32 {
        self as u8 as f32
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MetalSettings {
    pub species: MetalSpecies,
    pub finish: MetalFinish,
    pub tangent: MetalTangent,
    pub anisotropy: f32,
    pub brush_angle: f32,
    /// Spatial frequency multiplier: 1 is the console-scale baseline; larger is finer.
    pub texture_scale: f32,
    pub relief_strength: f32,
    pub scratches: f32,
    pub oxidation: f32,
    pub paint_coverage: f32,
    pub paint_color: Vec3,
    pub paint_roughness: f32,
    /// Grayscale relief from the object's existing image stencil, zero keeps it a color decal.
    pub image_relief: f32,
}
impl Default for MetalSettings {
    fn default() -> Self {
        Self {
            species: MetalSpecies::Steel,
            finish: MetalFinish::Polished,
            tangent: MetalTangent::Linear,
            anisotropy: 0.0,
            brush_angle: 0.0,
            texture_scale: 1.0,
            relief_strength: 0.25,
            scratches: 0.16,
            oxidation: 0.0,
            paint_coverage: 0.0,
            paint_color: Vec3::new(0.48, 0.025, 0.018),
            paint_roughness: 0.55,
            image_relief: 0.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MetalStudy {
    MachinedSteel,
    GoldBullion,
    PolishedChrome,
    BrushedAluminum,
    CopperWire,
    RustedIron,
    ChippedPaint,
    HammeredBronze,
}
impl MetalStudy {
    pub const ALL: [Self; 8] = [
        Self::MachinedSteel,
        Self::GoldBullion,
        Self::PolishedChrome,
        Self::BrushedAluminum,
        Self::CopperWire,
        Self::RustedIron,
        Self::ChippedPaint,
        Self::HammeredBronze,
    ];
    pub fn label(self) -> &'static str {
        [
            "Machined steel",
            "Gold bullion",
            "Polished chrome",
            "Brushed aluminum",
            "Copper wire",
            "Rusted cast iron",
            "Chipped red paint",
            "Hammered bronze",
        ][self as usize]
    }
    pub fn material(self) -> Material {
        let (species, finish, tangent, oxidation, paint_coverage, scratches) = [
            (
                MetalSpecies::Steel,
                MetalFinish::Brushed,
                MetalTangent::Linear,
                0.0,
                0.0,
                0.16,
            ),
            (
                MetalSpecies::Gold,
                MetalFinish::Polished,
                MetalTangent::Linear,
                0.0,
                0.0,
                0.20,
            ),
            (
                MetalSpecies::Chrome,
                MetalFinish::Polished,
                MetalTangent::Linear,
                0.0,
                0.0,
                0.0,
            ),
            (
                MetalSpecies::Aluminum,
                MetalFinish::Brushed,
                MetalTangent::Linear,
                0.0,
                0.0,
                0.08,
            ),
            (
                MetalSpecies::Copper,
                MetalFinish::Brushed,
                MetalTangent::Wire,
                0.05,
                0.0,
                0.15,
            ),
            (
                MetalSpecies::Iron,
                MetalFinish::Cast,
                MetalTangent::Radial,
                0.66,
                0.0,
                0.30,
            ),
            (
                MetalSpecies::Iron,
                MetalFinish::Satin,
                MetalTangent::Linear,
                0.60,
                0.60,
                0.60,
            ),
            (
                MetalSpecies::Bronze,
                MetalFinish::Hammered,
                MetalTangent::Linear,
                0.25,
                0.0,
                0.12,
            ),
        ][self as usize];
        let mut material = Material::metal_preset(species);
        material.apply_metal_finish(finish);
        material.metal.tangent = tangent;
        material.metal.oxidation = oxidation;
        material.metal.paint_coverage = paint_coverage;
        material.metal.scratches = scratches;
        material
    }
}
impl Material {
    pub fn metal_preset(species: MetalSpecies) -> Self {
        Self {
            kind: MaterialKind::Metal,
            color: Vec4::ONE,
            roughness: 0.12,
            metallic: 1.0,
            reflectivity: 1.0,
            refractive_index: 1.0,
            metal: MetalSettings {
                species,
                ..MetalSettings::default()
            },
            ..Self::default()
        }
    }
    pub fn apply_metal_finish(&mut self, finish: MetalFinish) {
        let (roughness, anisotropy, relief) = finish.defaults();
        self.metal.finish = finish;
        self.roughness = roughness;
        self.metal.anisotropy = anisotropy;
        self.metal.relief_strength = relief;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metal_settings_round_trip_and_legacy_metallic_remains_unchanged() {
        for study in MetalStudy::ALL {
            let mut material = study.material();
            material.metal.brush_angle = 0.71;
            material.metal.image_relief = 0.28;
            material.color = Vec4::new(0.8, 0.7, 0.6, 1.0);
            let json = serde_json::to_string(&material).unwrap();
            assert_eq!(serde_json::from_str::<Material>(&json).unwrap(), material);
        }
        let legacy = Material::preset(MaterialKind::Metallic);
        let mut json = serde_json::to_value(legacy).unwrap();
        json.as_object_mut().unwrap().remove("metal");
        assert_eq!(serde_json::from_value::<Material>(json).unwrap(), legacy);
        assert_eq!(MetalSpecies::ALL.len(), 9);
    }
}

use glam::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

pub const FABRIC_DEFAULT_TEXTURE_SCALE: f32 = 4.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FabricPreset {
    #[default]
    Jersey,
    Fleece,
    WovenWafflePique,
    HeatherJersey,
    HeatherFleece,
    TaslanLike,
    MicroPlain,
    BrushedJersey,
    BrushedFleece,
    CompactStretchWeave,
}
impl FabricPreset {
    pub const ALL: [Self; 10] = [
        Self::Jersey,
        Self::Fleece,
        Self::WovenWafflePique,
        Self::HeatherJersey,
        Self::HeatherFleece,
        Self::TaslanLike,
        Self::MicroPlain,
        Self::BrushedJersey,
        Self::BrushedFleece,
        Self::CompactStretchWeave,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Jersey => "Jersey",
            Self::Fleece => "Fleece",
            Self::WovenWafflePique => "Woven waffle piqué",
            Self::HeatherJersey => "Heather jersey",
            Self::HeatherFleece => "Heather fleece",
            Self::TaslanLike => "Taslan-like weave",
            Self::MicroPlain => "Micro plain weave",
            Self::BrushedJersey => "Brushed jersey",
            Self::BrushedFleece => "Brushed fleece",
            Self::CompactStretchWeave => "Spandex-like compact weave",
        }
    }
    pub fn color(self) -> Vec4 {
        match self {
            Self::Jersey => Vec4::new(0.63, 0.58, 0.53, 1.0),
            Self::Fleece => Vec4::new(0.67, 0.69, 0.68, 1.0),
            Self::WovenWafflePique => Vec4::new(0.76, 0.73, 0.65, 1.0),
            Self::HeatherJersey => Vec4::new(0.14, 0.32, 0.73, 1.0),
            Self::HeatherFleece => Vec4::new(0.38, 0.40, 0.42, 1.0),
            Self::TaslanLike => Vec4::new(0.35, 0.40, 0.43, 1.0),
            Self::MicroPlain => Vec4::new(0.70, 0.70, 0.68, 1.0),
            Self::BrushedJersey => Vec4::new(0.85, 0.81, 0.73, 1.0),
            Self::BrushedFleece => Vec4::new(0.88, 0.87, 0.83, 1.0),
            Self::CompactStretchWeave => Vec4::new(0.29, 0.31, 0.34, 1.0),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FabricConstruction {
    #[default]
    JerseyKnit,
    PileKnit,
    WaffleWeave,
    PlainWeave,
}
impl FabricConstruction {
    pub const ALL: [Self; 4] = [
        Self::JerseyKnit,
        Self::PileKnit,
        Self::WaffleWeave,
        Self::PlainWeave,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::JerseyKnit => "Jersey knit",
            Self::PileKnit => "Pile knit",
            Self::WaffleWeave => "Waffle weave",
            Self::PlainWeave => "Plain weave",
        }
    }
    pub fn gpu_code(self) -> f32 {
        match self {
            Self::JerseyKnit => 0.0,
            Self::PileKnit => 1.0,
            Self::WaffleWeave => 2.0,
            Self::PlainWeave => 3.0,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FabricFinish {
    #[default]
    None,
    Brushed,
    Napped,
    AirTextured,
}
impl FabricFinish {
    pub const ALL: [Self; 4] = [Self::None, Self::Brushed, Self::Napped, Self::AirTextured];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Brushed => "Brushed",
            Self::Napped => "Napped",
            Self::AirTextured => "Air-textured yarn proxy",
        }
    }
    pub fn gpu_code(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Brushed => 1.0,
            Self::Napped => 2.0,
            Self::AirTextured => 3.0,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FabricColoration {
    #[default]
    Solid,
    Heather,
}
impl FabricColoration {
    pub const ALL: [Self; 2] = [Self::Solid, Self::Heather];
    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid dye",
            Self::Heather => "Heather yarns",
        }
    }
    pub fn gpu_code(self) -> f32 {
        match self {
            Self::Solid => 0.0,
            Self::Heather => 1.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FabricSettings {
    pub preset: FabricPreset,
    pub construction: FabricConstruction,
    pub finish: FabricFinish,
    pub coloration: FabricColoration,
    pub pitch_x: f32,
    pub pitch_y: f32,
    pub relief_depth: f32,
    pub normal_gain: f32,
    pub fiber_detail: f32,
    pub strand_length: f32,
    pub nap_length: f32,
    pub orientation: f32,
    pub sheen_weight: f32,
    pub sheen_spread: f32,
    pub fiber_alignment: f32,
    pub light_yarn_color: Vec3,
    pub light_yarn_fraction: f32,
}
impl Default for FabricSettings {
    fn default() -> Self {
        Self::preset(FabricPreset::Jersey)
    }
}
impl FabricSettings {
    pub fn preset(preset: FabricPreset) -> Self {
        let (
            construction,
            finish,
            coloration,
            pitch_x,
            pitch_y,
            relief_depth,
            normal_gain,
            fiber_detail,
            strand_length,
            nap_length,
            sheen_weight,
            sheen_spread,
            fiber_alignment,
            light_yarn_fraction,
        ) = match preset {
            FabricPreset::Jersey => (
                FabricConstruction::JerseyKnit,
                FabricFinish::None,
                FabricColoration::Solid,
                0.045,
                0.052,
                0.008,
                0.72,
                0.58,
                2.0,
                0.0,
                0.30,
                0.70,
                0.62,
                0.0,
            ),
            FabricPreset::Fleece => (
                FabricConstruction::PileKnit,
                FabricFinish::Napped,
                FabricColoration::Solid,
                0.060,
                0.064,
                0.013,
                0.82,
                0.95,
                2.0,
                0.72,
                0.62,
                0.84,
                0.20,
                0.0,
            ),
            FabricPreset::WovenWafflePique => (
                FabricConstruction::WaffleWeave,
                FabricFinish::None,
                FabricColoration::Solid,
                0.072,
                0.072,
                0.016,
                0.85,
                0.66,
                2.0,
                0.0,
                0.24,
                0.72,
                0.32,
                0.0,
            ),
            FabricPreset::HeatherJersey => (
                FabricConstruction::JerseyKnit,
                FabricFinish::None,
                FabricColoration::Heather,
                0.043,
                0.052,
                0.008,
                0.72,
                0.62,
                4.0,
                0.0,
                0.34,
                0.72,
                0.60,
                0.32,
            ),
            FabricPreset::HeatherFleece => (
                FabricConstruction::PileKnit,
                FabricFinish::Napped,
                FabricColoration::Heather,
                0.060,
                0.064,
                0.012,
                0.76,
                0.86,
                3.6,
                0.68,
                0.63,
                0.86,
                0.25,
                0.29,
            ),
            FabricPreset::TaslanLike => (
                FabricConstruction::PlainWeave,
                FabricFinish::AirTextured,
                FabricColoration::Solid,
                0.031,
                0.034,
                0.008,
                0.70,
                0.72,
                2.0,
                0.0,
                0.78,
                0.48,
                0.70,
                0.0,
            ),
            FabricPreset::MicroPlain => (
                FabricConstruction::PlainWeave,
                FabricFinish::None,
                FabricColoration::Solid,
                0.018,
                0.019,
                0.004,
                0.55,
                0.40,
                2.0,
                0.0,
                0.42,
                0.64,
                0.52,
                0.0,
            ),
            FabricPreset::BrushedJersey => (
                FabricConstruction::JerseyKnit,
                FabricFinish::Brushed,
                FabricColoration::Solid,
                0.046,
                0.055,
                0.009,
                0.64,
                0.76,
                2.0,
                0.38,
                0.56,
                0.81,
                0.42,
                0.0,
            ),
            FabricPreset::BrushedFleece => (
                FabricConstruction::PileKnit,
                FabricFinish::Brushed,
                FabricColoration::Solid,
                0.063,
                0.066,
                0.015,
                0.70,
                0.94,
                2.0,
                0.88,
                0.72,
                0.89,
                0.18,
                0.0,
            ),
            FabricPreset::CompactStretchWeave => (
                FabricConstruction::PlainWeave,
                FabricFinish::None,
                FabricColoration::Solid,
                0.022,
                0.022,
                0.003,
                0.42,
                0.30,
                2.0,
                0.0,
                0.82,
                0.39,
                0.72,
                0.0,
            ),
        };
        Self {
            preset,
            construction,
            finish,
            coloration,
            // The former scale 4 pitch is now the scale 1 starting pitch.
            pitch_x: pitch_x / FABRIC_DEFAULT_TEXTURE_SCALE,
            pitch_y: pitch_y / FABRIC_DEFAULT_TEXTURE_SCALE,
            relief_depth,
            normal_gain,
            fiber_detail,
            strand_length,
            nap_length,
            orientation: 0.0,
            sheen_weight,
            sheen_spread,
            fiber_alignment,
            light_yarn_color: match preset {
                FabricPreset::HeatherJersey => Vec3::new(0.95, 0.94, 0.91),
                FabricPreset::HeatherFleece => Vec3::new(0.83, 0.84, 0.85),
                _ => Vec3::new(0.73, 0.74, 0.75),
            },
            light_yarn_fraction,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_starts_at_the_former_scale_four_pitch() {
        for (preset, old_pitch_x, old_pitch_y) in [
            (FabricPreset::Jersey, 0.045, 0.052),
            (FabricPreset::Fleece, 0.060, 0.064),
            (FabricPreset::WovenWafflePique, 0.072, 0.072),
            (FabricPreset::HeatherJersey, 0.043, 0.052),
            (FabricPreset::HeatherFleece, 0.060, 0.064),
            (FabricPreset::TaslanLike, 0.031, 0.034),
            (FabricPreset::MicroPlain, 0.018, 0.019),
            (FabricPreset::BrushedJersey, 0.046, 0.055),
            (FabricPreset::BrushedFleece, 0.063, 0.066),
            (FabricPreset::CompactStretchWeave, 0.022, 0.022),
        ] {
            let settings = FabricSettings::preset(preset);
            assert!((settings.pitch_x * FABRIC_DEFAULT_TEXTURE_SCALE - old_pitch_x).abs() < 1e-6);
            assert!((settings.pitch_y * FABRIC_DEFAULT_TEXTURE_SCALE - old_pitch_y).abs() < 1e-6);
        }
    }
}

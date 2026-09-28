use serde::{Deserialize, Serialize};

/// A saved render choice for an object or Boolean group. Derived bake data
/// does not belong in the scene document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupRenderRepresentation {
    BoxDepthAtlas,
    SphereDepthAtlas,
    GaussianSplats,
    NeuralSdf,
    #[default]
    #[serde(other)]
    ExactSdf,
}

impl GroupRenderRepresentation {
    pub const ALL: [Self; 5] = [
        Self::ExactSdf,
        Self::BoxDepthAtlas,
        Self::SphereDepthAtlas,
        Self::GaussianSplats,
        Self::NeuralSdf,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ExactSdf => "Exact SDF (current)",
            Self::BoxDepthAtlas => "Box depth + texture atlas",
            Self::SphereDepthAtlas => "Sphere depth + texture atlas",
            Self::GaussianSplats => "Gaussian splats",
            Self::NeuralSdf => "Neural SDF",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ExactSdf => "Editable source geometry with the current exact renderer.",
            Self::BoxDepthAtlas => "Capture depth and appearance from six box faces.",
            Self::SphereDepthAtlas => {
                "Capture depth and appearance with inward rays from a sphere."
            }
            Self::NeuralSdf => "Fit a configurable neural field to a sampled distance grid.",
            Self::GaussianSplats => "Approximate the group with soft Gaussian surface samples.",
        }
    }

    pub fn limitation(self) -> &'static str {
        match self {
            Self::ExactSdf => "Full source detail; repeated group queries can be expensive.",
            Self::BoxDepthAtlas => {
                "A single depth layer misses hidden surfaces and close parallax."
            }
            Self::SphereDepthAtlas => {
                "A single radial layer misses hidden surfaces and close parallax."
            }
            Self::NeuralSdf => "Approximate geometry and material boundaries; thin details may disappear. Trains automatically; Exact SDF is shown while training.",
            Self::GaussianSplats => {
                "Fast splats for opaque solid scenes; other materials use ray composition. Thin details may be lost."
            }
        }
    }

    pub fn is_exact(&self) -> bool {
        *self == Self::ExactSdf
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeuralActivation {
    #[default]
    Relu,
    Softplus,
}
impl NeuralActivation {
    pub const ALL: [Self; 2] = [Self::Relu, Self::Softplus];
    pub fn label(self) -> &'static str {
        match self {
            Self::Relu => "ReLU (faceted)",
            Self::Softplus => "Softplus (smooth)",
        }
    }
    pub fn shader_id(self) -> u32 {
        match self {
            Self::Relu => 0,
            Self::Softplus => 1,
        }
    }
    pub fn evaluate(self, value: f32) -> f32 {
        match self {
            Self::Relu => value.max(0.0),
            Self::Softplus => value.max(0.0) + (-10.0 * value.abs()).exp().ln_1p() / 10.0,
        }
    }
    pub fn slope_from_output(self, output: f32) -> f32 {
        match self {
            Self::Relu => {
                if output > 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Softplus => -(-10.0 * output).exp_m1(),
        }
    }
}

/// Saved choices, never the fitted model itself.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NeuralTrainingSettings {
    pub activation: NeuralActivation,
    pub samples_per_side: u32,
    pub layers: u32,
    pub width: u32,
    pub epochs: u32,
    pub learning_rate: f32,
    pub seed: u32,
}
impl Default for NeuralTrainingSettings {
    fn default() -> Self {
        Self {
            activation: NeuralActivation::default(),
            samples_per_side: 32,
            layers: 1,
            width: 8,
            epochs: 32,
            learning_rate: 0.001,
            seed: 0xabc123,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeuralModelPreset {
    Preview,
    Balanced,
    Detailed,
    High,
}
impl NeuralModelPreset {
    pub const ALL: [Self; 4] = [Self::Preview, Self::Balanced, Self::Detailed, Self::High];
    pub fn label(self) -> &'static str {
        match self {
            Self::Preview => "Preview · 1 layer",
            Self::Balanced => "Balanced · 2 layers",
            Self::Detailed => "Detailed · 3 layers",
            Self::High => "High detail · 4 layers",
        }
    }
    pub fn settings(self) -> NeuralTrainingSettings {
        let (layers, width, epochs) = match self {
            Self::Preview => (1, 8, 32),
            Self::Balanced => (2, 12, 64),
            Self::Detailed => (3, 16, 96),
            Self::High => (4, 24, 128),
        };
        NeuralTrainingSettings {
            layers,
            width,
            epochs,
            activation: NeuralActivation::Softplus,
            ..Default::default()
        }
    }
    pub fn matching(settings: NeuralTrainingSettings) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.settings() == settings)
    }
}

impl NeuralTrainingSettings {
    pub fn is_valid(&self) -> bool {
        (8..=1024).contains(&self.samples_per_side)
            && (1..=4).contains(&self.layers)
            && (4..=32).contains(&self.width)
            && (1..=512).contains(&self.epochs)
            && self.learning_rate.is_finite()
            && (0.00001..=0.1).contains(&self.learning_rate)
    }
    pub fn parameter_count(&self) -> Option<usize> {
        self.is_valid().then(|| {
            let w = self.width as usize;
            4 * w + (self.layers as usize - 1) * w * (w + 1) + w + 1
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StoredNeuralSdfSettings")]
pub struct NeuralSdfSettings {
    pub training: NeuralTrainingSettings,
    /// Half a grid interval by default; independent of the fitted field.
    pub hit_distance_cells: f32,
}
impl Default for NeuralSdfSettings {
    fn default() -> Self {
        Self {
            training: Default::default(),
            hit_distance_cells: 0.5,
        }
    }
}
impl NeuralSdfSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn hit_distance_is_valid(&self) -> bool {
        self.hit_distance_cells.is_finite() && (0.001..=4.0).contains(&self.hit_distance_cells)
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct StoredNeuralSdfSettings {
    training: NeuralTrainingSettings,
    hit_distance_cells: f32,
}
impl Default for StoredNeuralSdfSettings {
    fn default() -> Self {
        let defaults = NeuralSdfSettings::default();
        Self {
            training: defaults.training,
            hit_distance_cells: defaults.hit_distance_cells,
        }
    }
}
impl TryFrom<StoredNeuralSdfSettings> for NeuralSdfSettings {
    type Error = &'static str;
    fn try_from(value: StoredNeuralSdfSettings) -> Result<Self, Self::Error> {
        let settings = Self {
            training: value.training,
            hit_distance_cells: value.hit_distance_cells,
        };
        if !settings.training.is_valid() || !settings.hit_distance_is_valid() {
            return Err("Neural SDF settings are outside supported ranges");
        }
        Ok(settings)
    }
}

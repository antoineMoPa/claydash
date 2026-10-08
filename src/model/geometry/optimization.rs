use serde::{Deserialize, Serialize};

/// A saved render choice for an object or Boolean group. Neural fields can
/// carry a versioned cache alongside the editable source geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupRenderRepresentation {
    BoxDepthAtlas,
    BoxAccelerator,
    SphereDepthAtlas,
    SphereAccelerator,
    GaussianSplats,
    PoissonMesh,
    NeuralSdf,
    #[default]
    #[serde(other)]
    ExactSdf,
}

impl GroupRenderRepresentation {
    pub const ALL: [Self; 8] = [
        Self::ExactSdf,
        Self::BoxDepthAtlas,
        Self::SphereDepthAtlas,
        Self::SphereAccelerator,
        Self::BoxAccelerator,
        Self::GaussianSplats,
        Self::PoissonMesh,
        Self::NeuralSdf,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ExactSdf => "Exact SDF",
            Self::BoxDepthAtlas => "Box depth + texture atlas",
            Self::BoxAccelerator => "Box accelerator",
            Self::SphereDepthAtlas => "Sphere depth + texture atlas",
            Self::SphereAccelerator => "Sphere accelerator",
            Self::GaussianSplats => "Gaussian splats",
            Self::PoissonMesh => "Mesh (Poisson)",
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
            Self::BoxAccelerator => {
                "Bake six box faces, then refine only the map-selected source surface."
            }
            Self::SphereAccelerator => {
                "Bake sphere depth, then refine only the map-selected source surface."
            }
            Self::NeuralSdf => {
                "Fit a ray-conditioned neural field to random positions and unit ray directions."
            }
            Self::GaussianSplats => "Approximate the group with soft Gaussian surface samples.",
            Self::PoissonMesh => "Reconstruct a triangle mesh from implicit oriented surface samples. Click Recompute to build it.",
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
            Self::BoxAccelerator => "Fast, approximate refinement; artifacts at missing surfaces and Boolean seams are accepted.",
            Self::SphereAccelerator => "Refines captured surfaces; features missing from the depth capture may still be missed.",
            Self::NeuralSdf => "Approximate geometry and material boundaries; thin details may disappear. Trains automatically; Exact SDF is shown while training.",
            Self::GaussianSplats => {
                "Fast splats for opaque solid scenes; other materials use ray composition. Thin details may be lost."
            }
            Self::PoissonMesh => "Thin features may be lost at the chosen sampling resolution.",
        }
    }

    pub fn is_depth_accelerator(self) -> bool {
        matches!(self, Self::SphereAccelerator | Self::BoxAccelerator)
    }

    pub fn is_exact(&self) -> bool {
        *self == Self::ExactSdf
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeuralActivation {
    Relu,
    #[default]
    Softplus,
}
impl NeuralActivation {
    pub const ALL: [Self; 2] = [Self::Relu, Self::Softplus];
    pub fn label(self) -> &'static str {
        match self {
            Self::Relu => "ReLU",
            Self::Softplus => "Softplus",
        }
    }
    pub fn shader_id(self) -> u32 {
        match self {
            Self::Relu => 0,
            Self::Softplus => 1,
        }
    }
    #[cfg(test)]
    pub fn evaluate(self, value: f32) -> f32 {
        match self {
            Self::Relu => value.max(0.0),
            Self::Softplus => value.max(0.0) + (-10.0 * value.abs()).exp().ln_1p() / 10.0,
        }
    }
    #[cfg(test)]
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

/// Target for the three-component, ray-conditioned neural output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NeuralDistanceTarget {
    RayHit,
    #[default]
    SignedSdf,
}
impl NeuralDistanceTarget {
    pub fn shader_id(self) -> u32 {
        match self {
            Self::RayHit => 1,
            Self::SignedSdf => 0,
        }
    }
}

/// Saved choices, never the fitted model itself.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StoredNeuralTrainingSettings")]
pub struct NeuralTrainingSettings {
    pub activation: NeuralActivation,
    pub distance_target: NeuralDistanceTarget,
    pub samples: u32,
    pub layers: u32,
    pub width: u32,
    pub epochs: u32,
    pub learning_rate: f32,
    pub seed: u32,
    pub raymarch_last_segment: bool,
    /// World-space outward shift of training targets before exact refinement.
    pub distance_offset: f32,
}
impl Default for NeuralTrainingSettings {
    fn default() -> Self {
        Self {
            activation: NeuralActivation::default(),
            distance_target: NeuralDistanceTarget::SignedSdf,
            samples: 32_768,
            layers: 2,
            width: 24,
            epochs: 32,
            learning_rate: 0.02,
            seed: 0xabc123,
            raymarch_last_segment: false,
            distance_offset: 0.3,
        }
    }
}
// Explicitly map legacy grid settings to the same total number of samples.
#[derive(Deserialize)]
#[serde(default)]
struct StoredNeuralTrainingSettings {
    activation: NeuralActivation,
    distance_target: NeuralDistanceTarget,
    samples: Option<u32>,
    samples_per_side: Option<u32>,
    layers: u32,
    width: u32,
    epochs: u32,
    learning_rate: f32,
    seed: u32,
    raymarch_last_segment: bool,
    distance_offset: f32,
}
impl Default for StoredNeuralTrainingSettings {
    fn default() -> Self {
        let settings = NeuralTrainingSettings::default();
        Self {
            activation: settings.activation,
            distance_target: settings.distance_target,
            samples: None,
            samples_per_side: None,
            layers: settings.layers,
            width: settings.width,
            epochs: settings.epochs,
            learning_rate: settings.learning_rate,
            seed: settings.seed,
            raymarch_last_segment: settings.raymarch_last_segment,
            distance_offset: settings.distance_offset,
        }
    }
}
impl TryFrom<StoredNeuralTrainingSettings> for NeuralTrainingSettings {
    type Error = &'static str;
    fn try_from(stored: StoredNeuralTrainingSettings) -> Result<Self, Self::Error> {
        let samples = match (stored.samples, stored.samples_per_side) {
            (Some(samples), _) => samples,
            (None, Some(side)) if (8..=1024).contains(&side) => side.pow(3),
            (None, Some(_)) => return Err("Invalid legacy neural sample resolution"),
            (None, None) => Self::default().samples,
        };
        let settings = Self {
            activation: stored.activation,
            // Older documents may request next-hit targets. Keep loading them, but
            // train every current model with the signed-SDF vector target.
            distance_target: NeuralDistanceTarget::SignedSdf,
            samples,
            layers: stored.layers,
            width: stored.width,
            epochs: stored.epochs,
            learning_rate: stored.learning_rate,
            seed: stored.seed,
            raymarch_last_segment: stored.raymarch_last_segment,
            distance_offset: stored.distance_offset,
        };
        settings
            .is_valid()
            .then_some(settings)
            .ok_or("Invalid neural training settings")
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
        Self::ALL.into_iter().find(|preset| {
            let choice = preset.settings();
            settings.activation == choice.activation
                && settings.samples == choice.samples
                && settings.layers == choice.layers
                && settings.width == choice.width
                && settings.epochs == choice.epochs
                && settings.learning_rate == choice.learning_rate
                && settings.seed == choice.seed
        })
    }
}

impl NeuralTrainingSettings {
    pub const MAX_LAYERS: u32 = 8;
    pub const MAX_WIDTH: u32 = 1024;
    pub const MAX_SAMPLES: u32 = 1 << 30;

    pub fn is_valid(&self) -> bool {
        (512..=Self::MAX_SAMPLES).contains(&self.samples)
            && (1..=Self::MAX_LAYERS).contains(&self.layers)
            && (4..=Self::MAX_WIDTH).contains(&self.width)
            && (1..=512).contains(&self.epochs)
            && self.learning_rate.is_finite()
            && (0.00001..=0.1).contains(&self.learning_rate)
            && self.distance_offset.is_finite()
            && (0.0..=10.0).contains(&self.distance_offset)
    }
    pub fn parameter_count(&self) -> Option<usize> {
        self.is_valid().then(|| {
            let w = self.width as usize;
            7 * w + (self.layers as usize - 1) * w * (w + 1) + 3 * (w + 1)
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StoredNeuralSdfSettings")]
pub struct NeuralSdfSettings {
    pub training: NeuralTrainingSettings,
    /// Hit tolerance in equivalent sample-spacing units; independent of the fitted field.
    pub hit_distance_cells: f32,
}
impl Default for NeuralSdfSettings {
    fn default() -> Self {
        Self {
            training: Default::default(),
            hit_distance_cells: 0.02,
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

/// Versioned derived data. Invalid or stale fields are ignored by the renderer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedNeuralField {
    pub version: u32,
    pub source_key: u64,
    /// Settings used for this fit, which may differ from pending UI edits.
    pub training: NeuralTrainingSettings,
    pub weights: Vec<f32>,
    pub half_extent: f32,
    pub owners: Vec<uuid::Uuid>,
    pub rms_error: f32,
    pub max_error: f32,
    pub bake_ms: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedBoxDepthAtlas {
    pub local_min: glam::Vec3,
    pub local_max: glam::Vec3,
    pub resolution: u32,
    pub layers: usize,
    /// Depth and RGB, with negative depth for a missed ray.
    pub texels: Vec<[f32; 4]>,
    pub owners: Vec<Option<uuid::Uuid>>,
    pub normals: Vec<glam::Vec3>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSphereDepthAtlas {
    pub radius: f32,
    pub width: u32,
    pub height: u32,
    pub texels: Vec<[f32; 4]>,
    pub owners: Vec<Option<uuid::Uuid>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SavedDepthAtlas {
    Box(std::sync::Arc<SavedBoxDepthAtlas>),
    Sphere(std::sync::Arc<SavedSphereDepthAtlas>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedGroupCapture {
    pub version: u32,
    pub source_key: u64,
    pub capture: SavedDepthAtlas,
}

/// World-space distance from the captured surface at which exact refinement begins.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StoredDepthAcceleratorSettings")]
pub struct DepthAcceleratorSettings {
    pub configurable_epsilon: f32,
}
impl Default for DepthAcceleratorSettings {
    fn default() -> Self {
        Self {
            configurable_epsilon: 0.1,
        }
    }
}
impl DepthAcceleratorSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn is_valid(&self) -> bool {
        self.configurable_epsilon.is_finite()
            && (0.0001..=1000.0).contains(&self.configurable_epsilon)
    }
}
#[derive(Deserialize)]
#[serde(default)]
struct StoredDepthAcceleratorSettings {
    configurable_epsilon: f32,
}
impl Default for StoredDepthAcceleratorSettings {
    fn default() -> Self {
        Self {
            configurable_epsilon: DepthAcceleratorSettings::default().configurable_epsilon,
        }
    }
}
impl TryFrom<StoredDepthAcceleratorSettings> for DepthAcceleratorSettings {
    type Error = &'static str;
    fn try_from(stored: StoredDepthAcceleratorSettings) -> Result<Self, Self::Error> {
        let settings = Self {
            configurable_epsilon: stored.configurable_epsilon,
        };
        settings
            .is_valid()
            .then_some(settings)
            .ok_or("Invalid depth accelerator distance")
    }
}

pub type SphereAcceleratorSettings = DepthAcceleratorSettings;
pub type BoxAcceleratorSettings = DepthAcceleratorSettings;

// Read the initial preset-based representation as well as numeric resolutions.
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredSplatResolution {
    Samples(u32),
    Preset(LegacySplatResolution),
}
#[derive(Deserialize)]
enum LegacySplatResolution {
    Low,
    Medium,
    High,
    Ultra,
}
fn deserialize_splat_resolution<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u32, D::Error> {
    Ok(match StoredSplatResolution::deserialize(deserializer)? {
        StoredSplatResolution::Samples(samples) => samples,
        StoredSplatResolution::Preset(preset) => match preset {
            LegacySplatResolution::Low => 16,
            LegacySplatResolution::Medium => 32,
            LegacySplatResolution::High => 64,
            LegacySplatResolution::Ultra => 96,
        },
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GaussianSplatSettings {
    #[serde(deserialize_with = "deserialize_splat_resolution")]
    pub resolution: u32,
}
impl Default for GaussianSplatSettings {
    fn default() -> Self {
        Self { resolution: 64 }
    }
}
impl GaussianSplatSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn is_valid(self) -> bool {
        self.resolution > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PoissonMeshSettings {
    /// Cells along each side of the reconstruction grid.
    pub resolution: u32,
}

impl Default for PoissonMeshSettings {
    fn default() -> Self { Self { resolution: 64 } }
}

impl PoissonMeshSettings {
    pub fn is_default(&self) -> bool { *self == Self::default() }
}

#[cfg(test)]
mod splat_settings_tests {
    use super::*;
    #[test]
    fn numeric_resolution_and_legacy_presets_round_trip() {
        for resolution in [1, 97, 129, 256, 1024, u32::MAX] {
            let settings = GaussianSplatSettings { resolution };
            let json = serde_json::to_string(&settings).unwrap();
            assert!(json.contains(&format!("\"resolution\":{resolution}")));
            assert_eq!(
                serde_json::from_str::<GaussianSplatSettings>(&json).unwrap(),
                settings
            );
        }
        for (preset, resolution) in [("Low", 16), ("Medium", 32), ("High", 64), ("Ultra", 96)] {
            let json = format!("{{\"resolution\":\"{preset}\"}}");
            assert_eq!(
                serde_json::from_str::<GaussianSplatSettings>(&json)
                    .unwrap()
                    .resolution,
                resolution
            );
        }
        assert_eq!(
            serde_json::from_str::<GaussianSplatSettings>("{}").unwrap(),
            GaussianSplatSettings::default()
        );
    }
}

#[cfg(test)]
mod poisson_mesh_settings_tests {
    use super::*;

    #[test]
    fn mesh_resolution_defaults_for_older_documents_and_round_trips() {
        let default: PoissonMeshSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(default.resolution, 64);
        let edited = PoissonMeshSettings { resolution: 96 };
        assert_eq!(serde_json::from_str::<PoissonMeshSettings>(
            &serde_json::to_string(&edited).unwrap()).unwrap(), edited);
    }
}

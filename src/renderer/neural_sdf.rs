//! Tiny, deterministic neural distance bake. Positions and distances are explicitly
//! mapped to a cube; completed fields can be persisted in the document.
use super::*;
use crate::model::{lattice_bounds, lattice_world_matrix, PreparedSubtreeSampler};
use std::sync::Arc;

mod gpu;
pub(super) use gpu::GpuTrainingJob;

#[cfg(test)]
pub(super) const GRID: usize = 32;
#[cfg(test)]
pub(super) const SAMPLES: usize = GRID * GRID * GRID;
const MATERIAL_GRID: usize = 32;
const MATERIAL_SAMPLES: usize = MATERIAL_GRID * MATERIAL_GRID * MATERIAL_GRID;
const BATCH: usize = 256;
use crate::model::NeuralTrainingSettings;

pub(super) fn payload_records(settings: NeuralTrainingSettings) -> Option<usize> {
    settings.parameter_count().map(|_| {
        let width = settings.width as usize;
        2 + width
            + (settings.layers as usize - 1) * width * (width + 1).div_ceil(4)
            + (width + 1).div_ceil(4)
            + MATERIAL_SAMPLES
    })
}

#[derive(Clone, Debug)]
pub(super) struct Network {
    pub weights: Vec<f32>,
    activation: crate::model::NeuralActivation,
    layers: usize,
    width: usize,
    sample_resolution: f32,
}
#[cfg(test)]
struct NetworkScratch {
    activations: Vec<f32>,
    delta: Vec<f32>,
    previous: Vec<f32>,
}
#[cfg(test)]
impl NetworkScratch {
    fn new(network: &Network) -> Self {
        Self {
            activations: vec![0.0; network.layers * network.width],
            delta: vec![0.0; network.width],
            previous: vec![0.0; network.width],
        }
    }
}

fn random_u32(state: &mut u32) -> u32 {
    // An LCG supports every seed, including zero.
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    *state
}
impl Network {
    fn new(settings: NeuralTrainingSettings) -> Self {
        let width = settings.width as usize;
        let layers = settings.layers as usize;
        let mut network = Self {
            weights: vec![
                0.0;
                settings
                    .parameter_count()
                    .expect("validated network settings")
            ],
            sample_resolution: (settings.samples as f32).cbrt(),
            activation: settings.activation,
            layers,
            width,
        };
        let mut rng = settings.seed;
        for layer in 0..layers {
            let inputs = if layer == 0 { 3 } else { width };
            let offset = network.layer_offset(layer);
            for row in 0..width {
                for col in 0..inputs {
                    let random = random_u32(&mut rng) as f64 / u32::MAX as f64;
                    network.weights[offset + row * (inputs + 1) + col] =
                        (random as f32 * 2.0 - 1.0) * (2.0 / inputs as f32).sqrt();
                }
                network.weights[offset + row * (inputs + 1) + inputs] = 0.1;
            }
        }
        // Start the default small network with octant coverage. Seed controls
        // small perturbations and sample order, including on wider networks.
        for i in 0..width {
            for axis in 0..3 {
                network.weights[i * 4 + axis] = if i & (1 << axis) == 0 { -0.5 } else { 0.5 }
                    + network.weights[i * 4 + axis] * 0.05;
            }
        }
        let out = network.output_offset();
        for i in 0..width {
            network.weights[out + i] = 2.0 / width as f32;
        }
        network.weights[out + width] = -0.5;
        network
    }
    fn layer_offset(&self, layer: usize) -> usize {
        if layer == 0 {
            0
        } else {
            self.width * 4 + (layer - 1) * self.width * (self.width + 1)
        }
    }
    fn output_offset(&self) -> usize {
        self.width * 4 + (self.layers - 1) * self.width * (self.width + 1)
    }
    #[cfg(test)]
    fn forward(&self, p: Vec3, activations: &mut [f32]) -> f32 {
        for layer in 0..self.layers {
            let inputs = if layer == 0 { 3 } else { self.width };
            for row in 0..self.width {
                let offset = self.layer_offset(layer) + row * (inputs + 1);
                let mut sum = self.weights[offset + inputs];
                for col in 0..inputs {
                    let value = if layer == 0 {
                        p[col]
                    } else {
                        activations[(layer - 1) * self.width + col]
                    };
                    sum += self.weights[offset + col] * value;
                }
                activations[layer * self.width + row] = self.activation.evaluate(sum);
            }
        }
        let offset = self.output_offset();
        let mut sum = self.weights[offset + self.width];
        for i in 0..self.width {
            sum += self.weights[offset + i] * activations[(self.layers - 1) * self.width + i];
        }
        sum
    }
    #[cfg(test)]
    pub fn evaluate(&self, p: Vec3) -> f32 {
        self.forward(p, &mut vec![0.0; self.layers * self.width])
    }
    #[cfg(test)]
    fn gradient(&self, p: Vec3, target: f32, gradient: &mut [f32]) {
        self.gradient_with_scratch(p, target, gradient, &mut NetworkScratch::new(self));
    }
    #[cfg(test)]
    fn gradient_with_scratch(
        &self,
        p: Vec3,
        target: f32,
        gradient: &mut [f32],
        scratch: &mut NetworkScratch,
    ) {
        let error = 2.0 * (self.forward(p, &mut scratch.activations) - target);
        let out = self.output_offset();
        gradient[out + self.width] += error;
        for i in 0..self.width {
            gradient[out + i] += error * scratch.activations[(self.layers - 1) * self.width + i];
            scratch.delta[i] = error * self.weights[out + i];
        }
        for layer in (0..self.layers).rev() {
            let inputs = if layer == 0 { 3 } else { self.width };
            scratch.previous.fill(0.0);
            for row in 0..self.width {
                let local_delta = scratch.delta[row]
                    * self
                        .activation
                        .slope_from_output(scratch.activations[layer * self.width + row]);
                let offset = self.layer_offset(layer) + row * (inputs + 1);
                gradient[offset + inputs] += local_delta;
                for col in 0..inputs {
                    let value = if layer == 0 {
                        p[col]
                    } else {
                        scratch.activations[(layer - 1) * self.width + col]
                    };
                    gradient[offset + col] += local_delta * value;
                    scratch.previous[col] += self.weights[offset + col] * local_delta;
                }
            }
            std::mem::swap(&mut scratch.delta, &mut scratch.previous);
        }
    }
    pub fn width(&self) -> u32 {
        self.width as u32
    }
    pub fn gpu_records(&self, hit_distance_cells: f32) -> Vec<[f32; 4]> {
        let mut records = vec![
            [0.0; 4],
            [
                self.lipschitz(),
                hit_distance_cells,
                self.activation.shader_id() as f32,
                self.sample_resolution,
            ],
        ];
        // Pad each affine row to vec4 alignment so hidden layers can use dot products.
        for layer in 0..self.layers {
            let inputs = if layer == 0 { 3 } else { self.width };
            let offset = self.layer_offset(layer);
            for row in 0..self.width {
                for chunk in self.weights
                    [offset + row * (inputs + 1)..offset + (row + 1) * (inputs + 1)]
                    .chunks(4)
                {
                    let mut packed = [0.0; 4];
                    packed[..chunk.len()].copy_from_slice(chunk);
                    records.push(packed);
                }
            }
        }
        let output_offset = (records.len() - 2) * 4;
        for chunk in self.weights[self.output_offset()..].chunks(4) {
            let mut packed = [0.0; 4];
            packed[..chunk.len()].copy_from_slice(chunk);
            records.push(packed);
        }
        records[0] = [
            self.layers as f32,
            self.width as f32,
            output_offset as f32,
            records.len() as f32,
        ];
        records
    }
    pub fn payload_records(&self) -> usize {
        2 + self.width
            + (self.layers - 1) * self.width * (self.width + 1).div_ceil(4)
            + (self.width + 1).div_ceil(4)
            + MATERIAL_SAMPLES
    }
    pub fn lipschitz(&self) -> f32 {
        let out = self.output_offset();
        if self.layers == 1 && self.width <= 8 {
            let mut bound = 1.0_f32;
            for mask in 0..(1 << self.width) {
                let mut gradient = Vec3::ZERO;
                for i in 0..self.width {
                    if mask & (1 << i) != 0 {
                        gradient += Vec3::from_slice(&self.weights[i * 4..i * 4 + 3])
                            * self.weights[out + i];
                    }
                }
                bound = bound.max(gradient.length());
            }
            return bound * 1.00001;
        }
        // Propagate component-wise absolute derivative bounds. ReLU slopes
        // are at most one, so this remains safe for deeper/wider networks.
        let mut bounds = vec![Vec3::ZERO; self.width];
        let mut next = vec![Vec3::ZERO; self.width];
        for layer in 0..self.layers {
            let inputs = if layer == 0 { 3 } else { self.width };
            next.fill(Vec3::ZERO);
            for row in 0..self.width {
                let offset = self.layer_offset(layer) + row * (inputs + 1);
                if layer == 0 {
                    next[row] = Vec3::from_slice(&self.weights[offset..offset + 3]).abs();
                } else {
                    for col in 0..inputs {
                        next[row] += bounds[col] * self.weights[offset + col].abs();
                    }
                }
            }
            std::mem::swap(&mut bounds, &mut next);
        }
        let mut gradient = Vec3::ZERO;
        for i in 0..self.width {
            gradient += bounds[i] * self.weights[out + i].abs();
        }
        gradient.length().max(1.0) * 1.00001
    }
}

#[cfg(test)]
pub(super) fn grid_position(index: usize) -> Vec3 {
    sample_position(index, GRID)
}

#[cfg(test)]
fn sample_position(index: usize, grid: usize) -> Vec3 {
    Vec3::new(
        (index % grid) as f32,
        ((index / grid) % grid) as f32,
        (index / (grid * grid)) as f32,
    ) * (2.0 / (grid - 1) as f32)
        - Vec3::ONE
}

#[cfg(test)]
fn sample_hash(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846ca68b);
    value ^ (value >> 16)
}
#[cfg(test)]
fn drop_training_sample(index: usize, seed: u32, distance: f32) -> bool {
    distance > 0.4 && sample_hash(index as u32 ^ seed ^ 0xa511e9b3) & 1 != 0
}

#[cfg(test)]
fn random_sample_position(index: usize, seed: u32) -> Vec3 {
    let base = (index as u32).wrapping_mul(3) ^ seed;
    let coordinate =
        |axis: u32| (sample_hash(base.wrapping_add(axis)) >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0;
    Vec3::new(coordinate(0), coordinate(1), coordinate(2))
}

#[derive(Clone, Debug)]
pub(super) struct NeuralField {
    pub network: Network,
    pub half_extent: f32,
    pub owners: Vec<uuid::Uuid>,
    pub rms_error: f32,
    pub max_error: f32,
    pub bake_ms: f64,
}

impl NeuralField {
    pub fn saved(
        &self,
        source_key: u64,
        training: NeuralTrainingSettings,
    ) -> crate::model::SavedNeuralField {
        crate::model::SavedNeuralField {
            version: 1,
            source_key,
            training,
            weights: self.network.weights.clone(),
            half_extent: self.half_extent,
            owners: self.owners.clone(),
            rms_error: self.rms_error,
            max_error: self.max_error,
            bake_ms: self.bake_ms,
        }
    }

    pub fn from_saved(
        saved: &crate::model::SavedNeuralField,
        key: u64,
        source: &[SdfObject],
    ) -> Option<Self> {
        let settings = saved.training;
        let ids: std::collections::HashSet<_> = source.iter().map(|object| object.uuid).collect();
        if saved.version != 1
            || saved.source_key != key
            || settings.parameter_count()? != saved.weights.len()
            || saved.weights.iter().any(|weight| !weight.is_finite())
            || !saved.half_extent.is_finite()
            || saved.half_extent <= 0.0
            || saved.owners.len() != MATERIAL_SAMPLES
            || saved.owners.iter().any(|id| !ids.contains(id))
            || !saved.rms_error.is_finite()
            || saved.rms_error < 0.0
            || !saved.max_error.is_finite()
            || saved.max_error < 0.0
            || !saved.bake_ms.is_finite()
            || saved.bake_ms < 0.0
        {
            return None;
        }
        Some(Self {
            network: Network {
                weights: saved.weights.clone(),
                activation: settings.activation,
                layers: settings.layers as usize,
                width: settings.width as usize,
                sample_resolution: (settings.samples as f32).cbrt(),
            },
            half_extent: saved.half_extent,
            owners: saved.owners.clone(),
            rms_error: saved.rms_error,
            max_error: saved.max_error,
            bake_ms: saved.bake_ms,
        })
    }
}

// Shuffle small sample sets; large sets use a bijective affine index mapping
// instead of allocating billions of sample indices.
enum SampleOrder {
    Shuffled(Vec<usize>),
    Mapped {
        count: usize,
        offset: usize,
        stride: usize,
    },
}
impl SampleOrder {
    fn new(count: usize) -> Self {
        if count <= MATERIAL_SAMPLES {
            Self::Shuffled((0..count).collect())
        } else {
            Self::Mapped {
                count,
                offset: 0,
                stride: 1,
            }
        }
    }
    fn shuffle(&mut self, rng: &mut u32) {
        match self {
            Self::Shuffled(indices) => {
                for i in (1..indices.len()).rev() {
                    let j = random_u32(rng) as usize % (i + 1);
                    indices.swap(i, j);
                }
            }
            Self::Mapped {
                count,
                offset,
                stride,
            } => {
                *offset = random_u32(rng) as usize % *count;
                *stride = random_u32(rng) as usize % *count;
                loop {
                    let (mut a, mut b) = (*stride, *count);
                    while b != 0 {
                        (a, b) = (b, a % b);
                    }
                    if a == 1 {
                        break;
                    }
                    *stride = (*stride + 1) % *count;
                }
            }
        }
    }
    fn index(&self, position: usize) -> usize {
        match self {
            Self::Shuffled(indices) => indices[position],
            Self::Mapped {
                count,
                offset,
                stride,
            } => ((*offset as u64 + position as u64 * *stride as u64) % *count as u64) as usize,
        }
    }
}

// Host state for GPU initialization, sample scheduling, and readback assembly.
struct TrainingState {
    world: glam::Mat4,
    center: Vec3,
    half_extent: f32,
    distance_unit: f32,
    owners: Vec<uuid::Uuid>,
    network: Network,
    settings: NeuralTrainingSettings,
    sample_order: SampleOrder,
    update: usize,
    rng: u32,
    best: Network,
    best_loss: f32,
    best_max_error: f32,
    best_min: f32,
    best_max: f32,
    started: web_time::Instant,
}
impl TrainingState {
    pub fn new(source: Arc<Vec<SdfObject>>, root: uuid::Uuid) -> Option<Self> {
        let settings = source
            .iter()
            .find(|object| object.uuid == root)?
            .neural_sdf
            .training;
        if !settings.is_valid()
            || payload_records(settings)? > super::box_depth_atlas::MAX_BOX_DEPTH_TEXELS
        {
            return None;
        }
        let samples = settings.samples as usize;
        let network = Network::new(settings);
        let sampler = PreparedSubtreeSampler::new(&source, root)?;
        let (minimum, maximum) = lattice_bounds(&source, root)?;
        let world = lattice_world_matrix(&source, root);
        let extent = sampler.deformation_extent(world.inverse());
        let half_extent = ((maximum - minimum) * 0.5 + extent).max_element() * 1.1;
        let scale = world
            .x_axis
            .truncate()
            .length()
            .min(world.y_axis.truncate().length())
            .min(world.z_axis.truncate().length());
        let distance_unit = half_extent * scale;
        if !minimum.is_finite()
            || !maximum.is_finite()
            || !world.is_finite()
            || !distance_unit.is_finite()
            || distance_unit <= 1e-6
        {
            return None;
        }
        drop(sampler);
        Some(Self {
            world,
            center: (minimum + maximum) * 0.5,
            half_extent,
            distance_unit,
            owners: Vec::with_capacity(MATERIAL_SAMPLES),
            network: network.clone(),
            settings,
            sample_order: SampleOrder::new(samples),
            update: 0,
            rng: settings.seed,
            best: network,
            best_loss: f32::INFINITY,
            best_max_error: 0.0,
            best_min: f32::INFINITY,
            best_max: f32::NEG_INFINITY,
            started: web_time::Instant::now(),
        })
    }
    pub fn finish(self) -> Result<NeuralField, &'static str> {
        let samples = self.settings.samples as usize;
        if self.best_min >= 0.0 || self.best_max <= 0.0 || !self.best_loss.is_finite() {
            return Err("Fit contains no surface crossing");
        }
        Ok(NeuralField {
            network: self.best,
            half_extent: self.half_extent,
            owners: self.owners,
            rms_error: (self.best_loss / samples as f32).sqrt() * self.distance_unit,
            max_error: self.best_max_error * self.distance_unit,
            bake_ms: self.started.elapsed().as_secs_f64() * 1000.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BooleanOperation, GroupRenderRepresentation, PrimitiveKind};

    #[test]
    fn wide_neural_networks_have_correct_gradients_and_payloads() {
        for (layers, width) in [(2, 33), (3, 65), (4, 128), (8, 33), (2, 1024)] {
            let settings = NeuralTrainingSettings {
                layers,
                width,
                activation: crate::model::NeuralActivation::Softplus,
                ..Default::default()
            };
            let network = Network::new(settings);
            let p = Vec3::new(0.37, -0.21, 0.63);
            let target = 0.25;
            let mut gradient = vec![0.0; network.weights.len()];
            network.gradient(p, target, &mut gradient);
            for index in [
                0,
                3,
                width as usize * 4,
                network.output_offset(),
                network.weights.len() - 1,
            ] {
                let mut high = network.clone();
                let mut low = network.clone();
                high.weights[index] += 0.001;
                low.weights[index] -= 0.001;
                let numerical = ((high.evaluate(p) - target).powi(2)
                    - (low.evaluate(p) - target).powi(2))
                    / 0.002;
                assert!(
                    (gradient[index] - numerical).abs() < 0.005,
                    "layers {layers}, width {width}, index {index}"
                );
            }
            assert!(network.lipschitz().is_finite());
            assert_eq!(
                network.gpu_records(0.5).len() + MATERIAL_SAMPLES,
                network.payload_records()
            );
            assert_eq!(payload_records(settings), Some(network.payload_records()));
        }
    }

    #[test]
    fn neural_far_sample_dropout_keeps_near_surface_and_half_far_points() {
        let mut dropped = 0;
        for index in 0..32_768 {
            for distance in [-1.0, 0.0, 0.399, 0.4] {
                assert!(!drop_training_sample(index, 42, distance));
            }
            let drop = drop_training_sample(index, 42, 0.401);
            assert_eq!(drop, drop_training_sample(index, 42, 10.0));
            dropped += usize::from(drop);
        }
        assert!((15_800..17_000).contains(&dropped));
    }

    #[test]
    fn neural_random_samples_are_uniform_continuous_and_reproducible() {
        let count = 32_768;
        let mut sum = Vec3::ZERO;
        let mut squares = Vec3::ZERO;
        let mut octants = [0usize; 8];
        let mut off_grid = 0;
        for index in 0..count {
            let p = random_sample_position(index, 42);
            assert_eq!(p, random_sample_position(index, 42));
            assert!(p.cmpge(-Vec3::ONE).all() && p.cmplt(Vec3::ONE).all());
            sum += p;
            squares += p * p;
            let octant = usize::from(p.x >= 0.0)
                | (usize::from(p.y >= 0.0) << 1)
                | (usize::from(p.z >= 0.0) << 2);
            octants[octant] += 1;
            let old_cell = (p + Vec3::ONE) * 15.5;
            if (old_cell - old_cell.round()).abs().min_element() > 0.0001 {
                off_grid += 1;
            }
        }
        assert!((sum / count as f32).abs().max_element() < 0.02);
        assert!(
            (squares / count as f32 - Vec3::splat(1.0 / 3.0))
                .abs()
                .max_element()
                < 0.02
        );
        assert!(octants.iter().all(|&count| (3800..4400).contains(&count)));
        assert!(off_grid > 32_000);
        assert_ne!(random_sample_position(0, 42), random_sample_position(0, 43));
    }

    #[test]
    fn neural_gradients_match_finite_differences() {
        for (layers, width, activation) in [
            (1, 8, crate::model::NeuralActivation::Relu),
            (2, 5, crate::model::NeuralActivation::Relu),
            (2, 8, crate::model::NeuralActivation::Relu),
            (4, 32, crate::model::NeuralActivation::Relu),
            (1, 8, crate::model::NeuralActivation::Softplus),
            (2, 5, crate::model::NeuralActivation::Softplus),
            (2, 8, crate::model::NeuralActivation::Softplus),
            (4, 32, crate::model::NeuralActivation::Softplus),
        ] {
            let network = Network::new(NeuralTrainingSettings {
                layers,
                width,
                activation,
                ..Default::default()
            });

            let p = Vec3::new(0.37, -0.21, 0.63);
            let target = 0.25;
            let mut gradient = vec![0.0; network.weights.len()];
            network.gradient(p, target, &mut gradient);
            for i in 0..network.weights.len() {
                let mut low = network.clone();
                let mut high = network.clone();
                low.weights[i] -= 0.0001;
                high.weights[i] += 0.0001;
                let numerical = ((high.evaluate(p) - target).powi(2)
                    - (low.evaluate(p) - target).powi(2))
                    / 0.0002;
                assert!((numerical - gradient[i]).abs() < 0.001, "parameter {i}");
            }
        }
    }

    #[test]
    fn neural_proxy_preserves_owners_and_removes_baked_repetition() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::NeuralSdf;
        root.repetition.enabled = true;
        root.repetition.count = [2, 1, 1];
        root.repetition.spacing = Vec3::splat(0.2);
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        child.operation = BooleanOperation::Union;
        child.transform.translation.x = 0.2;
        let child_id = child.uuid;
        let source = vec![root, child];
        let pending =
            super::super::group_capture::prepare_group_scene(&source, &mut Default::default());
        assert_eq!(pending.objects.len(), 2);
        let mut state = TrainingState::new(Arc::new(source.clone()), source[0].uuid).unwrap();
        state.best_loss = 0.01;
        state.best_min = -0.1;
        state.best_max = 0.1;
        state.owners = (0..MATERIAL_SAMPLES)
            .map(|i| source[i % source.len()].uuid)
            .collect();
        let field = state.finish().unwrap();
        assert!(field.owners.contains(&source[0].uuid));
        assert!(field.owners.contains(&child_id));
        let ready = [(source[0].uuid, Arc::new(field))].into_iter().collect();
        let prepared = super::super::group_capture::prepare_group_scene_with_neural(
            &source,
            &mut Default::default(),
            &ready,
            &Default::default(),
        );
        assert_eq!(prepared.objects.len(), 1);
        assert!(!prepared.objects[0].repetition.enabled);
        assert!(prepared.neural_fields.contains_key(&source[0].uuid));
    }
    #[test]
    fn neural_sample_order_covers_grid_and_seed_changes_initialization() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.neural_sdf.training.epochs = 1;
        object.neural_sdf.training.layers = 2;
        object.neural_sdf.training.width = 5;
        let mut job = TrainingState::new(Arc::new(vec![object.clone()]), object.uuid).unwrap();
        let first = job.network.weights.clone();
        job.sample_order.shuffle(&mut job.rng);
        let mut indices: Vec<_> = (0..job.settings.samples as usize)
            .map(|i| job.sample_order.index(i))
            .collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..SAMPLES).collect::<Vec<_>>());
        object.neural_sdf.training.seed = 0;
        let other = TrainingState::new(Arc::new(vec![object.clone()]), object.uuid).unwrap();
        assert_ne!(first, other.network.weights);
        assert_eq!(other.network.layers, 2);
        assert_eq!(other.network.width, 5);
    }
    #[test]
    fn neural_mapped_sample_order_visits_every_sample_once() {
        let count = 33_usize.pow(3);
        let mut order = SampleOrder::new(count);
        for seed in [0, 42, u32::MAX] {
            let mut rng = seed;
            order.shuffle(&mut rng);
            let mut indices: Vec<_> = (0..count).map(|i| order.index(i)).collect();
            indices.sort_unstable();
            assert_eq!(indices, (0..count).collect::<Vec<_>>());
        }
    }
    #[test]
    fn neural_large_sample_count_starts_without_sample_allocation() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.neural_sdf.training.samples = 1073741824;
        assert!(object.neural_sdf.training.is_valid());
        let job = TrainingState::new(Arc::new(vec![object.clone()]), object.uuid).unwrap();
        assert!(matches!(job.sample_order, SampleOrder::Mapped { .. }));
        assert!(job.owners.is_empty());
    }

    #[test]
    fn neural_invalid_fit_falls_back() {
        let object = SdfObject::create_kind(PrimitiveKind::Sphere);
        let mut job = TrainingState::new(Arc::new(vec![object.clone()]), object.uuid).unwrap();
        job.best_min = 0.0;
        job.best_max = 0.0;
        assert!(job.finish().is_err());
    }

    #[test]
    fn neural_grid_mapping_and_lipschitz_bound() {
        assert_eq!(grid_position(0), -Vec3::ONE);
        assert_eq!(grid_position(SAMPLES - 1), Vec3::ONE);
        let network = Network::new(NeuralTrainingSettings::default());
        let bound = network.lipschitz();
        for i in 1..SAMPLES {
            let a = grid_position(i - 1);
            let b = grid_position(i);
            assert!(
                (network.evaluate(a) - network.evaluate(b)).abs() <= bound * a.distance(b) + 1e-6
            );
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod gpu_tests {
    use super::*;
    use wgpu::util::DeviceExt;

    #[test]
    #[ignore = "requires a GPU adapter; run explicitly for neural renderer changes"]
    fn neural_gpu_inference_matches_cpu() {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .unwrap();
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor::default())
                .await
                .unwrap();
            for (layers, width, activation) in [
                (1, 8, crate::model::NeuralActivation::Relu),
                (2, 5, crate::model::NeuralActivation::Relu),
                (2, 8, crate::model::NeuralActivation::Relu),
                (4, 32, crate::model::NeuralActivation::Relu),
                (8, 33, crate::model::NeuralActivation::Softplus),
                (1, 8, crate::model::NeuralActivation::Softplus),
                (2, 5, crate::model::NeuralActivation::Softplus),
                (2, 8, crate::model::NeuralActivation::Softplus),
                (4, 32, crate::model::NeuralActivation::Softplus),
                (2, 65, crate::model::NeuralActivation::Relu),
                (3, 128, crate::model::NeuralActivation::Softplus),
                (4, 256, crate::model::NeuralActivation::Softplus),
                (1, 1024, crate::model::NeuralActivation::Relu),
                (2, 1024, crate::model::NeuralActivation::Relu),
            ] {
                let network = Network::new(NeuralTrainingSettings {
                    layers,
                    width,
                    activation,
                    ..Default::default()
                });
                let sample_count = if width > 256 {
                    64
                } else if width > 32 {
                    256
                } else {
                    SAMPLES
                };
                let records = network.gpu_records(0.5);
                assert_eq!(records.len() + SAMPLES, network.payload_records());
                assert_eq!(
                    payload_records(NeuralTrainingSettings {
                        layers,
                        width,
                        ..Default::default()
                    }),
                    Some(network.payload_records())
                );
                let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("neural weights"),
                    contents: bytemuck::cast_slice(&records),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let output = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("neural values"),
                    size: (sample_count * 16) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("neural readback"),
                    size: output.size(),
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                // Execute the actual renderer inference function, not a test reimplementation.
                let renderer = include_str!("../../assets/shaders/sdf.wgsl");
                let start = renderer.find("fn neural_activation(").unwrap();
                let end = renderer[start..].find("fn neural_shape(").unwrap() + start;
                let source = format!(
                    r#"
                const NEURAL_WIDTH: u32 = {width}u;
                struct Object {{ box_depth_max: vec4<f32>, box_depth_meta: vec4<u32> }}
                @group(0) @binding(0) var<storage, read> box_depth_texels: array<vec4<f32>>;
                @group(0) @binding(1) var<storage, read_write> result: array<vec4<f32>>;
                {}
                @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
                    let i = id.x;
                    let p = vec3(f32(i % 32u), f32((i / 32u) % 32u), f32(i / 1024u)) * (2.0 / 31.0) - vec3(1.0);
                    let object = Object(vec4(2.0), vec4<u32>(0u));
                    result[i] = vec4(neural_gradient(p * 2.0, object), neural_value(p * 2.0, object));
                }}
            "#,
                    &renderer[start..end]
                );
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("neural inference parity"),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                });
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: None,
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: input.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: output.as_entire_binding(),
                        },
                    ],
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(sample_count as u32 / 64, 1, 1);
                }
                encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output.size());
                queue.submit([encoder.finish()]);
                let (sender, receiver) = std::sync::mpsc::channel();
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        sender.send(result).unwrap();
                    });
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(std::time::Duration::from_secs(15)),
                    })
                    .unwrap();
                receiver.recv().unwrap().unwrap();
                let bytes = readback.slice(..).get_mapped_range().unwrap();
                for (i, value) in bytemuck::cast_slice::<u8, [f32; 4]>(&bytes)
                    .iter()
                    .enumerate()
                {
                    assert!(
                        (value[3] - network.evaluate(grid_position(i)) * 2.0).abs() < 1e-5,
                        "grid sample {i}"
                    );
                    if i % 257 == 0 {
                        let p = grid_position(i);
                        for axis in 0..3 {
                            let mut delta = Vec3::ZERO;
                            delta[axis] = 0.0001;
                            let derivative = (network.evaluate(p + delta)
                                - network.evaluate(p - delta))
                                / 0.0002;
                            assert!(
                                (value[axis] - derivative).abs() < 0.02,
                                "GPU gradient: layers {layers}, width {width}, sample {i}"
                            );
                        }
                    }
                }
            }
        });
    }
}

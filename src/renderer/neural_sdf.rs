//! Tiny, deterministic neural distance bake. Positions and distances are explicitly
//! mapped to a cube; no trained data is serialized into the document.
use super::*;
use crate::model::{lattice_bounds, lattice_world_matrix, PreparedSubtreeSampler};
use std::sync::Arc;

#[cfg(test)]
pub(super) const GRID: usize = 32;
#[cfg(test)]
pub(super) const SAMPLES: usize = GRID * GRID * GRID;
const MATERIAL_GRID: usize = 32;
const MATERIAL_SAMPLES: usize = MATERIAL_GRID * MATERIAL_GRID * MATERIAL_GRID;
const BATCH: usize = 256;
const MAX_WIDTH: usize = 32;
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
    grid: usize,
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
            grid: settings.samples_per_side as usize,
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
    fn forward(&self, p: Vec3, activations: &mut [[f32; MAX_WIDTH]; 4]) -> f32 {
        for layer in 0..self.layers {
            let inputs = if layer == 0 { 3 } else { self.width };
            for row in 0..self.width {
                let offset = self.layer_offset(layer) + row * (inputs + 1);
                let mut sum = self.weights[offset + inputs];
                for col in 0..inputs {
                    let value = if layer == 0 {
                        p[col]
                    } else {
                        activations[layer - 1][col]
                    };
                    sum += self.weights[offset + col] * value;
                }
                activations[layer][row] = self.activation.evaluate(sum);
            }
        }
        let offset = self.output_offset();
        let mut sum = self.weights[offset + self.width];
        for i in 0..self.width {
            sum += self.weights[offset + i] * activations[self.layers - 1][i];
        }
        sum
    }
    pub fn evaluate(&self, p: Vec3) -> f32 {
        self.forward(p, &mut [[0.0; MAX_WIDTH]; 4])
    }
    fn gradient(&self, p: Vec3, target: f32, gradient: &mut [f32]) {
        let mut activations = [[0.0; MAX_WIDTH]; 4];
        let error = 2.0 * (self.forward(p, &mut activations) - target);
        let out = self.output_offset();
        gradient[out + self.width] += error;
        let mut delta = [0.0; MAX_WIDTH];
        for i in 0..self.width {
            gradient[out + i] += error * activations[self.layers - 1][i];
            delta[i] = error * self.weights[out + i];
        }
        for layer in (0..self.layers).rev() {
            let inputs = if layer == 0 { 3 } else { self.width };
            let mut previous = [0.0; MAX_WIDTH];
            for row in 0..self.width {
                let local_delta =
                    delta[row] * self.activation.slope_from_output(activations[layer][row]);
                let offset = self.layer_offset(layer) + row * (inputs + 1);
                gradient[offset + inputs] += local_delta;
                for col in 0..inputs {
                    let value = if layer == 0 {
                        p[col]
                    } else {
                        activations[layer - 1][col]
                    };
                    gradient[offset + col] += local_delta * value;
                    previous[col] += self.weights[offset + col] * local_delta;
                }
            }
            delta = previous;
        }
    }
    pub fn gpu_records(&self, hit_distance_cells: f32) -> Vec<[f32; 4]> {
        let mut records = vec![
            [0.0; 4],
            [
                self.lipschitz(),
                hit_distance_cells,
                self.activation.shader_id() as f32,
                self.grid as f32,
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
        let mut bounds = [Vec3::ZERO; MAX_WIDTH];
        for layer in 0..self.layers {
            let inputs = if layer == 0 { 3 } else { self.width };
            let mut next = [Vec3::ZERO; MAX_WIDTH];
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
            bounds = next;
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

fn sample_position(index: usize, grid: usize) -> Vec3 {
    Vec3::new(
        (index % grid) as f32,
        ((index / grid) % grid) as f32,
        (index / (grid * grid)) as f32,
    ) * (2.0 / (grid - 1) as f32)
        - Vec3::ONE
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

// Preserve calibrated shuffles for small grids; large grids use a bijective
// affine index mapping instead of allocating billions of sample indices.
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

pub(super) struct TrainingJob {
    source: Arc<Vec<SdfObject>>,
    root: uuid::Uuid,
    world: glam::Mat4,
    center: Vec3,
    half_extent: f32,
    distance_unit: f32,
    owners: Vec<uuid::Uuid>,
    network: Network,
    first: Vec<f32>,
    second: Vec<f32>,
    gradient: Vec<f32>,
    settings: NeuralTrainingSettings,
    sample_order: SampleOrder,
    update: usize,
    batch_fill: usize,
    rng: u32,
    best: Network,
    best_loss: f32,
    validation_cursor: usize,
    validation_loss: f32,
    validation_max_error: f32,
    best_max_error: f32,
    validating: bool,
    training_complete: bool,
    validation_min: f32,
    validation_max: f32,
    best_min: f32,
    best_max: f32,
    started: web_time::Instant,
}
impl TrainingJob {
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
        let samples = (settings.samples_per_side as usize).pow(3);
        let network = Network::new(settings);
        let parameters = network.weights.len();
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
            source,
            root,
            world,
            center: (minimum + maximum) * 0.5,
            half_extent,
            distance_unit,
            owners: Vec::with_capacity(MATERIAL_SAMPLES),
            network: network.clone(),
            first: vec![0.0; parameters],
            second: vec![0.0; parameters],
            gradient: vec![0.0; parameters],
            settings,
            sample_order: SampleOrder::new(samples),
            update: 0,
            batch_fill: 0,
            rng: settings.seed,
            best: network,
            best_loss: f32::INFINITY,
            validation_cursor: 0,
            validation_loss: 0.0,
            validation_max_error: 0.0,
            best_max_error: 0.0,
            validating: false,
            training_complete: false,
            validation_min: f32::INFINITY,
            validation_max: f32::NEG_INFINITY,
            best_min: f32::INFINITY,
            best_max: f32::NEG_INFINITY,
            started: web_time::Instant::now(),
        })
    }
    /// A small bounded work unit. Sampling preparation is shared by a slice.
    pub fn advance(&mut self, budget: std::time::Duration) -> Result<bool, &'static str> {
        let samples = (self.settings.samples_per_side as usize).pow(3);
        let grid = self.settings.samples_per_side as usize;
        let batches = samples.div_ceil(BATCH);
        let start = web_time::Instant::now();
        let mut sampler =
            PreparedSubtreeSampler::new(&self.source, self.root).ok_or("Invalid source")?;
        loop {
            if self.training_complete {
                let end = (self.owners.len() + 16).min(MATERIAL_SAMPLES);
                for i in self.owners.len()..end {
                    let p = sample_position(i, MATERIAL_GRID);
                    let (_, owner) = sampler.sample(
                        self.world
                            .transform_point3(self.center + p * self.half_extent),
                    );
                    self.owners.push(owner);
                }
                if self.owners.len() == MATERIAL_SAMPLES {
                    return Ok(true);
                }
            } else if self.validating {
                let end = (self.validation_cursor + 16).min(samples);
                for i in self.validation_cursor..end {
                    let p = sample_position(i, grid);
                    let (distance, _) = sampler.sample(
                        self.world
                            .transform_point3(self.center + p * self.half_extent),
                    );
                    if !distance.is_finite() {
                        return Err("Nonfinite source distance");
                    }
                    let value = self.network.evaluate(p);
                    self.validation_min = self.validation_min.min(value);
                    self.validation_max = self.validation_max.max(value);
                    let error = value - distance / self.distance_unit;
                    self.validation_loss += error * error;
                    self.validation_max_error = self.validation_max_error.max(error.abs());
                }
                self.validation_cursor = end;
                if end == samples {
                    if self.validation_loss < self.best_loss {
                        self.best_loss = self.validation_loss;
                        self.best_max_error = self.validation_max_error;
                        self.best_min = self.validation_min;
                        self.best_max = self.validation_max;
                        self.best = self.network.clone();
                    }
                    self.validating = false;
                    self.validation_cursor = 0;
                    self.validation_loss = 0.0;
                    self.validation_max_error = 0.0;
                    self.validation_min = f32::INFINITY;
                    self.validation_max = f32::NEG_INFINITY;
                    self.training_complete = self.update == self.settings.epochs as usize * batches;
                }
            } else {
                if self.batch_fill == 0 {
                    self.gradient.fill(0.0);
                }
                let batch_start = (self.update % batches) * BATCH;
                if batch_start == 0 && self.batch_fill == 0 {
                    self.sample_order.shuffle(&mut self.rng);
                }
                let batch_size = BATCH.min(samples - batch_start);
                let chunk_start = batch_start + self.batch_fill;
                let chunk_size = 16.min(batch_size - self.batch_fill);
                for position in chunk_start..chunk_start + chunk_size {
                    let index = self.sample_order.index(position);
                    let p = sample_position(index, grid);
                    let distance = sampler
                        .sample(
                            self.world
                                .transform_point3(self.center + p * self.half_extent),
                        )
                        .0;
                    if !distance.is_finite() {
                        return Err("Nonfinite source distance");
                    }
                    self.network
                        .gradient(p, distance / self.distance_unit, &mut self.gradient);
                }
                self.batch_fill += chunk_size;
                if self.batch_fill < batch_size {
                    if start.elapsed() >= budget {
                        return Ok(false);
                    }
                    continue;
                }
                self.batch_fill = 0;
                self.update += 1;
                let correction1 = 1.0 - 0.9_f32.powi(self.update as i32);
                let correction2 = 1.0 - 0.999_f32.powi(self.update as i32);
                for (i, gradient) in self.gradient.iter().enumerate() {
                    let g = gradient / batch_size as f32;
                    self.first[i] = 0.9 * self.first[i] + 0.1 * g;
                    self.second[i] = 0.999 * self.second[i] + 0.001 * g * g;
                    self.network.weights[i] -= self.settings.learning_rate
                        * (self.first[i] / correction1)
                        / ((self.second[i] / correction2).sqrt() + 1e-8);
                }
                if !self.network.weights.iter().all(|v| v.is_finite()) {
                    return Err("Nonfinite trained weights");
                }
                self.validating = self.update % batches == 0;
            }
            if start.elapsed() >= budget {
                return Ok(false);
            }
        }
    }
    pub fn finish(self) -> Result<NeuralField, &'static str> {
        let samples = (self.settings.samples_per_side as usize).pow(3);
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
    use std::time::Duration;

    fn train(source: Vec<SdfObject>) -> NeuralField {
        let id = source[0].uuid;
        let mut job = TrainingJob::new(Arc::new(source), id).unwrap();
        while !job.advance(Duration::from_millis(4)).unwrap() {}
        job.finish().unwrap()
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
    fn neural_training_reduces_loss_and_is_deterministic() {
        let object = SdfObject::create_kind(PrimitiveKind::Sphere);
        let source = vec![object];
        let initial = TrainingJob::new(Arc::new(source.clone()), source[0].uuid).unwrap();
        let mut sampler = PreparedSubtreeSampler::new(&source, source[0].uuid).unwrap();
        let initial_loss: f32 = (0..SAMPLES)
            .map(|i| {
                let p = grid_position(i);
                let target = sampler
                    .sample(
                        initial
                            .world
                            .transform_point3(initial.center + p * initial.half_extent),
                    )
                    .0
                    / initial.distance_unit;
                (initial.network.evaluate(p) - target).powi(2)
            })
            .sum::<f32>()
            / SAMPLES as f32;
        let first = train(source.clone());
        let second = train(source);
        assert_eq!(first.network.weights, second.network.weights);
        assert!(first.rms_error / initial.distance_unit < initial_loss.sqrt() * 0.7);
        assert!(first.network.evaluate(Vec3::ZERO) < 0.0);
        assert!(first.network.evaluate(Vec3::ONE) > 0.0);
        eprintln!(
            "Neural sphere: {:.1} ms, RMS {:.5}, max {:.5}, GPU payload {} bytes, L {:.3}",
            first.bake_ms,
            first.rms_error,
            first.max_error,
            first.network.payload_records() * 16,
            first.network.lipschitz()
        );
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
        let field = train(source.clone());
        assert!(field.owners.contains(&source[0].uuid));
        assert!(field.owners.contains(&child_id));
        let ready = [(source[0].uuid, Arc::new(field))].into_iter().collect();
        let prepared = super::super::group_capture::prepare_group_scene_with_neural(
            &source,
            &mut Default::default(),
            &ready,
        );
        assert_eq!(prepared.objects.len(), 1);
        assert!(!prepared.objects[0].repetition.enabled);
        assert!(prepared.neural_fields.contains_key(&source[0].uuid));
    }
    #[test]
    fn neural_epochs_cover_grid_and_seed_changes_initialization() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.neural_sdf.training.epochs = 1;
        object.neural_sdf.training.layers = 2;
        object.neural_sdf.training.width = 5;
        let mut job = TrainingJob::new(Arc::new(vec![object.clone()]), object.uuid).unwrap();
        let first = job.network.weights.clone();
        while !job.advance(Duration::from_millis(4)).unwrap() {}
        assert_eq!(job.update, SAMPLES / BATCH);
        let mut indices: Vec<_> = (0..(job.settings.samples_per_side as usize).pow(3))
            .map(|i| job.sample_order.index(i))
            .collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..SAMPLES).collect::<Vec<_>>());
        object.neural_sdf.training.seed = 0;
        let other = TrainingJob::new(Arc::new(vec![object.clone()]), object.uuid).unwrap();
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
    fn neural_large_grid_starts_without_grid_allocation() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.neural_sdf.training.samples_per_side = 1024;
        assert!(object.neural_sdf.training.is_valid());
        let mut job = TrainingJob::new(Arc::new(vec![object.clone()]), object.uuid).unwrap();
        assert!(matches!(job.sample_order, SampleOrder::Mapped { .. }));
        assert!(!job.advance(Duration::ZERO).unwrap());
        assert_eq!(job.batch_fill, 16);
        assert!(job.owners.is_empty());
    }
    #[test]
    fn neural_odd_resolution_covers_partial_batches_and_payload() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.neural_sdf.training.samples_per_side = 9;
        object.neural_sdf.training.epochs = 2;
        let settings = object.neural_sdf.training;
        let mut job = TrainingJob::new(Arc::new(vec![object.clone()]), object.uuid).unwrap();
        while !job.advance(Duration::from_millis(1)).unwrap() {}
        assert_eq!(job.update, 2 * 729_usize.div_ceil(BATCH));
        assert_eq!(job.owners.len(), MATERIAL_SAMPLES);
        let mut indices: Vec<_> = (0..(job.settings.samples_per_side as usize).pow(3))
            .map(|i| job.sample_order.index(i))
            .collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..729).collect::<Vec<_>>());
        assert_eq!(sample_position(0, 9), -Vec3::ONE);
        assert_eq!(sample_position(728, 9), Vec3::ONE);
        let field = job.finish().unwrap();
        assert_eq!(field.owners.len(), MATERIAL_SAMPLES);
        let records = field.network.gpu_records(0.5);
        assert_eq!(records[1][3], 9.0);
        assert_eq!(
            Some(records.len() + MATERIAL_SAMPLES),
            payload_records(settings)
        );
        assert_eq!(
            records.len() + MATERIAL_SAMPLES,
            field.network.payload_records()
        );
    }
    #[test]
    #[ignore = "trains all four presets; run explicitly for preset changes"]
    fn neural_duck_presets_fit_increasing_quality() {
        let document: serde_json::Value = serde_json::from_str(crate::duck::DEFAULT_DUCK).unwrap();
        let source: Vec<SdfObject> = serde_json::from_value(
            document["subtree"]["sdf_objects"]["value"]["VecSDFObject"].clone(),
        )
        .unwrap();
        let mut previous = f32::INFINITY;
        for preset in crate::model::NeuralModelPreset::ALL {
            let mut scene = source.clone();
            scene[0].neural_sdf.training = preset.settings();
            let field = train(scene);
            eprintln!(
                "{}: RMS {}, max {}, L {}, bake {}ms",
                preset.label(),
                field.rms_error,
                field.max_error,
                field.network.lipschitz(),
                field.bake_ms
            );
            assert!(
                field.rms_error < previous,
                "{} must improve the duck fit",
                preset.label()
            );
            previous = field.rms_error;
            let mut hits = 0;
            for y in 0..8 {
                for x in 0..8 {
                    let origin = Vec3::new(
                        (x as f32 + 0.5) / 4.0 - 1.0,
                        (y as f32 + 0.5) / 4.0 - 1.0,
                        1.0,
                    );
                    let mut t = 0.0;
                    for _ in 0..128 {
                        let p = origin - Vec3::Z * t;
                        let q = p.abs() - Vec3::ONE;
                        let cube = q.max(Vec3::ZERO).length() + q.max_element().min(0.0);
                        let distance = field.network.evaluate(p).max(cube);
                        if distance.abs() <= 1.0 / 31.0 {
                            hits += 1;
                            break;
                        }
                        t += distance.abs() * 0.8;
                        if t > 2.0 {
                            break;
                        }
                    }
                }
            }
            eprintln!(
                "{}: {hits} visible grid rays / 64 with {} steps",
                preset.label(),
                128
            );
            assert!(
                hits >= 8,
                "{} must render a visible surface",
                preset.label()
            );
        }
    }
    #[test]
    fn neural_duck_two_layer_distance_steps_preserve_visible_hits() {
        let document: serde_json::Value = serde_json::from_str(crate::duck::DEFAULT_DUCK).unwrap();
        let source: Vec<SdfObject> = serde_json::from_value(
            document["subtree"]["sdf_objects"]["value"]["VecSDFObject"].clone(),
        )
        .unwrap();
        for width in [8, 16, 32] {
            let mut scene = source.clone();
            scene[0].neural_sdf.training.layers = 2;
            scene[0].neural_sdf.training.width = width;
            let field = train(scene);
            let bound = field.network.lipschitz();
            let mut hits = [0; 2];
            for (budget_index, budget) in [128, 4096].into_iter().enumerate() {
                for y in 0..16 {
                    for x in 0..16 {
                        let origin = Vec3::new(
                            (x as f32 + 0.5) / 8.0 - 1.0,
                            (y as f32 + 0.5) / 8.0 - 1.0,
                            1.0,
                        );
                        let mut t = 0.0;
                        for _ in 0..budget {
                            let p = origin - Vec3::Z * t;
                            let q = p.abs() - Vec3::ONE;
                            let cube = q.max(Vec3::ZERO).length() + q.max_element().min(0.0);
                            let distance = field.network.evaluate(p).max(cube);
                            if distance.abs() <= 1.0 / 31.0 {
                                hits[budget_index] += 1;
                                break;
                            }
                            t += distance.abs() * 0.8;
                            if t > 2.0 {
                                break;
                            }
                        }
                    }
                }
            }
            eprintln!(
                "Duck width {width}: bound {bound}, budget hits {}, reference hits {}",
                hits[0], hits[1]
            );
            assert!(hits[1] > 0);
            assert_eq!(
                hits[0], hits[1],
                "width {width}: step budget must retain reference hits"
            );
        }
    }
    #[test]
    fn neural_sampling_yields_and_invalid_fit_falls_back() {
        let object = SdfObject::create_kind(PrimitiveKind::Sphere);
        let mut job = TrainingJob::new(Arc::new(vec![object.clone()]), object.uuid).unwrap();
        assert!(!job.advance(Duration::ZERO).unwrap());
        assert_eq!(job.batch_fill, 16);
        assert!(job.owners.is_empty()); // Training starts before any full-grid pass.
        while !job.advance(Duration::from_millis(4)).unwrap() {}
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
                    size: (SAMPLES * 16) as u64,
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
                    pass.dispatch_workgroups(SAMPLES as u32 / 64, 1, 1);
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

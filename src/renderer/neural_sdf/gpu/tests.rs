use super::*;
use crate::renderer::{inverse_affine_rows, GpuObject};

fn sphere_scene(source: &[SdfObject]) -> PackedTrainingScene {
    let ordered = crate::renderer::boolean_postorder(source);
    let root_index = ordered.len() - 1;
    let objects = ordered
        .iter()
        .map(|object| {
            let mut gpu = GpuObject::zeroed();
            gpu.meta = [
                0,
                object.object_type,
                object.operation.gpu_code(),
                object
                    .boolean_parent
                    .and_then(|id| ordered.iter().position(|object| object.uuid == id))
                    .map_or(-1, |index| index as i32),
            ];
            gpu.inverse_rows = inverse_affine_rows(object.transform.matrix().inverse());
            gpu.group_inverse_rows = inverse_affine_rows(glam::Mat4::IDENTITY);
            gpu.params = [
                match &object.params {
                    SdfParams::SphereParams(params) => params.radius,
                    _ => panic!("sphere fixture"),
                },
                0.0,
                0.0,
                1.0,
            ];
            gpu.scale = [1.0, 1.0, 1.0, 0.0];
            gpu.repeat_count = [1, 1, 1, 0];
            gpu.component = [0, root_index as u32, object.softness.to_bits(), 0];
            gpu.modifier[1] = 1.0f32.to_bits();
            gpu
        })
        .collect();
    PackedTrainingScene {
        objects,
        bvh: Vec::new(),
        polygon_points: Vec::new(),
        material_headers: Vec::new(),
        material_params: Vec::new(),
        lattice_points: Vec::new(),
        modifier_params: Vec::new(),
        lattice_tiles: Vec::new(),
        ids: ordered.iter().map(|object| object.uuid).collect(),
        start: 0,
        root: root_index as u32,
        capacity: source.len().next_power_of_two() as u32,
    }
}

#[test]
fn gpu_training_shaders_validate_for_portable_webgpu() {
    for source in [
        source_shader(2),
        model_shader(NeuralTrainingSettings::default()),
        model_shader(NeuralTrainingSettings {
            width: 65,
            layers: 8,
            ..Default::default()
        }),
    ] {
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        let info = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("portable training shader");
        for (index, entry) in module
            .entry_points
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.stage == wgpu::naga::ShaderStage::Compute)
        {
            let count = module
                .global_variables
                .iter()
                .filter(|(handle, variable)| {
                    matches!(variable.space, wgpu::naga::AddressSpace::Storage { .. })
                        && !info.get_entry_point(index)[*handle].is_empty()
                })
                .count();
            assert!(count <= 8, "{} uses {count} storage buffers", entry.name);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires a GPU adapter"]
fn gpu_neural_accelerator_samples_shift_training_targets_outward() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        for target in [crate::model::NeuralDistanceTarget::SignedSdf] {
            let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
            object.neural_sdf.training = NeuralTrainingSettings {
                distance_target: target,
                layers: 1,
                width: 8,
                samples: 512,
                epochs: 1,
                raymarch_last_segment: true,
                distance_offset: 0.3,
                ..Default::default()
            };
            let id = object.uuid;
            let source = Arc::new(vec![object]);
            let packed = sphere_scene(&source);
            let mut gpu =
                GpuTrainingJob::new(device.clone(), queue.clone(), source.clone(), id, packed)
                    .unwrap();
            assert_eq!(gpu.params.distance_target[0], 0.3);
            let state = TrainingState::new(source.clone(), id).unwrap();
            gpu.advance(std::time::Duration::ZERO).unwrap();
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(30)),
                })
                .unwrap();
            let values = read_buffer(&device, &queue, &gpu.samples);
            let mut sampler = PreparedSubtreeSampler::new(&source, id).unwrap();
            for sample in values.chunks_exact(16).take(32) {
                let point = Vec3::from_slice(sample);
                let world = state
                    .world
                    .transform_point3(state.center + point * state.half_extent);
                let expected = (sampler.sample(world).0 - 0.3) / state.distance_unit;
                assert!((sample[3] - expected).abs() < 1e-5);
                let ray = Vec3::from_slice(&sample[8..]);
                let vector = Vec3::from_slice(&sample[12..]);
                assert!((ray.length() - 1.0).abs() < 1e-6);
                let distance = match target {
                    crate::model::NeuralDistanceTarget::SignedSdf => expected,
                    crate::model::NeuralDistanceTarget::RayHit => {
                        let radius = match &source[0].params {
                            SdfParams::SphereParams(params) => params.radius + 0.3,
                            _ => unreachable!(),
                        };
                        let radius = radius / state.half_extent;
                        let b = point.dot(ray);
                        let discriminant = b * b - point.length_squared() + radius * radius;
                        let mut travel = f32::INFINITY;
                        if discriminant >= 0.0 {
                            for t in [-b - discriminant.sqrt(), -b + discriminant.sqrt()] {
                                if t >= 0.0 {
                                    travel = travel.min(t);
                                }
                            }
                        }
                        if !travel.is_finite() {
                            travel = (0..3)
                                .map(|axis| {
                                    let edge = if ray[axis] >= 0.0 { 1.0 } else { -1.0 };
                                    (edge - point[axis]) / ray[axis]
                                })
                                .fold(f32::INFINITY, f32::min);
                        }
                        if expected < 0.0 {
                            -travel
                        } else {
                            travel
                        }
                    }
                };
                assert!(
                    (vector - ray * distance).length() < 0.002,
                    "{target:?}: point {point}, ray {ray}, actual {vector}, expected {distance}"
                );
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires a GPU adapter"]
fn gpu_sampling_backprop_adam_and_training_match_reference() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let defaults = NeuralTrainingSettings::default();
        for (layers, width, activation, epochs, distance_target) in [
            (
                defaults.layers,
                defaults.width,
                defaults.activation,
                256,
                crate::model::NeuralDistanceTarget::SignedSdf,
            ),
            (
                2,
                24,
                crate::model::NeuralActivation::Relu,
                256,
                crate::model::NeuralDistanceTarget::SignedSdf,
            ),
            (
                1,
                8,
                crate::model::NeuralActivation::Relu,
                256,
                crate::model::NeuralDistanceTarget::SignedSdf,
            ),
            (
                2,
                33,
                crate::model::NeuralActivation::Softplus,
                256,
                crate::model::NeuralDistanceTarget::SignedSdf,
            ),
            (
                8,
                5,
                crate::model::NeuralActivation::Softplus,
                256,
                crate::model::NeuralDistanceTarget::SignedSdf,
            ),
        ] {
            let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
            object.neural_sdf.training = NeuralTrainingSettings {
                distance_target,
                layers,
                width,
                activation,
                samples: 731,
                epochs,
                ..Default::default()
            };
            let id = object.uuid;
            let mut objects = vec![object];
            if layers > 1 {
                let mut child = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
                child.boolean_parent = Some(id);
                child.transform.translation.x = 0.2;
                objects.push(child);
            }
            if layers == 8 {
                // A larger world-space source exercises rejection above 0.4 scene units.
                if let SdfParams::SphereParams(params) = &mut objects[0].params {
                    params.radius = 2.0;
                }
            }
            let source = Arc::new(objects);
            let packed = sphere_scene(&source);
            let owner_ids = packed.ids.clone();
            let mut gpu =
                GpuTrainingJob::new(device.clone(), queue.clone(), source.clone(), id, packed)
                    .unwrap();
            let mut reference = TrainingState::new(source.clone(), id).unwrap();
            // The first dispatch samples the exact SDF, backpropagates, and performs one Adam step.
            gpu.advance(std::time::Duration::ZERO).unwrap();
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(30)),
                })
                .unwrap();
            reference.sample_order.shuffle(&mut reference.rng);
            let mut sampler = PreparedSubtreeSampler::new(&source, id).unwrap();
            let mut gradient = vec![0.0; reference.network.weights.len()];
            let mut scratch = NetworkScratch::new(&reference.network);
            let sampled = read_buffer(&device, &queue, &gpu.samples);
            let mut kept = 0;
            for position in 0..BATCH {
                let p = random_sample_position(
                    reference.sample_order.index(position),
                    reference.settings.seed,
                );
                let target = sampler
                    .sample(
                        reference
                            .world
                            .transform_point3(reference.center + p * reference.half_extent),
                    )
                    .0
                    / reference.distance_unit;
                if drop_training_sample(
                    reference.sample_order.index(position),
                    reference.settings.seed,
                    target * reference.distance_unit,
                ) {
                    continue;
                }
                kept += 1;
                reference.network.gradient_with_scratch(
                    p,
                    Vec3::from_slice(&sampled[position * 16 + 8..]),
                    Vec3::from_slice(&sampled[position * 16 + 12..]),
                    &mut gradient,
                    &mut scratch,
                );
            }
            if layers == 8 {
                assert!(kept > 0 && kept < BATCH);
            }
            let states = read_buffer(&device, &queue, &gpu.weights);
            for (index, gradient) in gradient.iter().enumerate() {
                let g = *gradient / kept as f32;
                let m = 0.1 * g;
                let v = 0.001 * g * g;
                let expected = reference.network.weights[index]
                    - reference.settings.learning_rate * (m / (1.0 - 0.9f32))
                        / ((v / (1.0 - 0.999f32)).sqrt() + 1e-8);
                assert!(
                    (states[index * 4] - expected).abs() < 0.00002,
                    "Adam layers {layers}, width {width}, parameter {index}"
                );
                assert!(
                    (states[index * 4 + 1] - m).abs() < 0.00002,
                    "backprop parameter {index}"
                );
            }
            let samples = read_buffer(&device, &queue, &gpu.samples);
            for (sample, values) in samples.chunks_exact(16).enumerate() {
                let p = Vec3::from_slice(values);
                assert_eq!(
                    p,
                    random_sample_position(
                        reference.sample_order.index(sample),
                        reference.settings.seed
                    )
                );
                let expected = sampler
                    .sample(
                        reference
                            .world
                            .transform_point3(reference.center + p * reference.half_extent),
                    )
                    .0
                    / reference.distance_unit;
                assert!(
                    (values[3] - expected).abs() < 1e-5,
                    "GPU SDF sample {sample}"
                );
                let expected_owner = sampler
                    .sample(
                        reference
                            .world
                            .transform_point3(reference.center + p * reference.half_extent),
                    )
                    .1;
                assert_eq!(owner_ids[values[4].to_bits() as usize], expected_owner);
                assert_eq!(
                    values[5].to_bits() != 0,
                    drop_training_sample(
                        reference.sample_order.index(sample),
                        reference.settings.seed,
                        expected * reference.distance_unit
                    )
                );
            }
            if layers == 8 {
                // Deep architecture sampling, gradients, and Adam were checked above.
                continue;
            }
            let started = std::time::Instant::now();
            let mut percent = gpu.progress_percent();
            while !gpu.advance(std::time::Duration::from_millis(4)).unwrap() {
                assert!(started.elapsed() < std::time::Duration::from_secs(60));
                assert!(gpu.progress_percent() >= percent);
                percent = gpu.progress_percent();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let stats = read_buffer(&device, &queue, &gpu.stats);
            eprintln!(
                "GPU fit layers {layers}, width {width}: {:.0} ms, loss {}, range {}..{}",
                started.elapsed().as_secs_f64() * 1000.0,
                stats[4],
                stats[6],
                stats[7]
            );
            assert_eq!(gpu.progress_percent(), 100);
            assert_eq!(
                gpu.job.update,
                gpu.job.settings.epochs as usize * 731_usize.div_ceil(BATCH)
            );
            let field = gpu.finish().unwrap();
            assert!(field.network.weights.iter().all(|value| value.is_finite()));
            assert_eq!(field.owners.len(), MATERIAL_SAMPLES);
            assert!(field.owners.contains(&id));
            if layers > 1 {
                assert!(field.owners.contains(&source[1].uuid));
            }
            assert!(field.rms_error.is_finite());
            if distance_target == crate::model::NeuralDistanceTarget::SignedSdf {
                assert!(field.rms_error < 0.1);
            }
            if distance_target == crate::model::NeuralDistanceTarget::SignedSdf {
                assert!(field.network.evaluate(Vec3::ZERO, Vec3::X).x < 0.0);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn read_buffer(device: &wgpu::Device, queue: &wgpu::Queue, source: &wgpu::Buffer) -> Vec<f32> {
    let readback = buffer(
        device,
        "training parity readback",
        source.size(),
        wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(source, 0, &readback, 0, source.size());
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .unwrap();
    receive.recv().unwrap().unwrap();
    let range = readback.slice(..).get_mapped_range().unwrap();
    bytemuck::cast_slice(&range).to_vec()
}

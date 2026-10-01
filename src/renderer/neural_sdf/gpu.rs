use super::*;
use crate::renderer::scene_upload::PackedTrainingScene;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use wgpu::util::DeviceExt;

const STAGING_BATCHES: usize = 8;

const SHARED: &str = r#"
struct TrainParams {
    world: mat4x4<f32>, cube: vec4<f32>, control: vec4<u32>,
    options: vec4<f32>, component: vec4<u32>, distance_target: vec4<f32>,
}
struct TrainSample { point: vec4<f32>, owner: vec4<u32> }
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    world: [[f32; 4]; 4],
    cube: [f32; 4],
    control: [u32; 4],
    options: [f32; 4],
    component: [u32; 4],
    distance_target: [f32; 4],
}

#[derive(Clone, Copy)]
enum Phase {
    Training,
    Validation,
    Ownership,
    Readback,
    Done,
}

pub(crate) struct GpuTrainingJob {
    device: wgpu::Device,
    queue: wgpu::Queue,
    job: TrainingState,
    ids: Vec<uuid::Uuid>,
    params: Params,
    params_buffer: Vec<wgpu::Buffer>,
    indices: Vec<wgpu::Buffer>,
    weights: wgpu::Buffer,
    samples: wgpu::Buffer,
    owners: wgpu::Buffer,
    stats: wgpu::Buffer,
    readback: wgpu::Buffer,
    sample_pipeline: wgpu::ComputePipeline,
    scene_bind: wgpu::BindGroup,
    sample_bind: Vec<wgpu::BindGroup>,
    model_binds: Vec<wgpu::BindGroup>,
    forward: Vec<wgpu::ComputePipeline>,
    backward: Vec<wgpu::ComputePipeline>,
    output_delta: wgpu::ComputePipeline,
    adam: wgpu::ComputePipeline,
    validate: wgpu::ComputePipeline,
    reduce: wgpu::ComputePipeline,
    checkpoint: wgpu::ComputePipeline,
    commit: wgpu::ComputePipeline,
    phase: Phase,
    cursor: usize,
    epoch: usize,
    completed: u64,
    submitted: u64,
    waiting: bool,
    submission: Option<wgpu::SubmissionIndex>,
    done: Arc<AtomicBool>,
    mapped: Arc<Mutex<Option<Result<Vec<f32>, &'static str>>>>,
}

fn storage(
    device: &wgpu::Device,
    label: &str,
    bytes: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let empty = [0u8; 1024];
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: if bytes.is_empty() { &empty } else { bytes },
        usage,
    })
}
fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}
fn dispatch(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind: &wgpu::BindGroup,
    count: usize,
) {
    let mut pass = encoder.begin_compute_pass(&Default::default());
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind, &[]);
    pass.dispatch_workgroups(count.div_ceil(64) as u32, 1, 1);
}

fn source_shader(capacity: u32) -> String {
    let source = crate::renderer::scene_bounds::specialized_neural_shader_source(
        &crate::renderer::scene_bounds::specialized_shader_source(
            &crate::renderer::material_gpu::shader_source(),
            capacity,
        ),
        4,
    );
    // Training reads editable primitives, never captured proxies. Reuse the
    // native branches of the renderer evaluator without binding derived atlases.
    let start = source.find("fn primitive_distance(").unwrap();
    let end = source[start..]
        .find("    } else if HAS_NEURAL_SDF")
        .unwrap()
        + start;
    let native = format!(
        "{}    }}\n    return distance;\n}}\n",
        source[start..end].replace(
            "fn primitive_distance(",
            "fn train_native_primitive_distance("
        )
    );
    let source = source.replace(
        "brick_geometry_visible(object)",
        "train_brick_geometry_enabled(object)",
    );
    let source = source.replace(
        "fabric_geometry_visible(object)",
        "train_fabric_geometry_enabled(object)",
    );
    let native = format!("{native}\nfn train_brick_geometry_enabled(object: Object) -> bool {{\n    let header = material_headers[object.component.w];\n    return header.kind == MATERIAL_BRICK && material_params[header.offset + BRICK_RELIEF].x >= 0.001;\n}}\n");
    let native = format!("{native}\nfn train_fabric_geometry_enabled(object: Object) -> bool {{\n    if object.state.y != 1 && object.state.y != 2 {{ return false; }}\n    let header = material_headers[object.component.w];\n    return header.kind == MATERIAL_FABRIC && material_params[header.offset + FABRIC_PITCH].z >= 0.0005;\n}}\n");
    let source = source.replace(
        "let distance = primitive_distance(local, object);",
        "let distance = train_native_primitive_distance(local, object);",
    );
    format!(
        "{source}\n{native}\n{SHARED}\n{}",
        include_str!("../../../assets/shaders/neural_sampling.wgsl")
    )
}
fn model_shader(settings: NeuralTrainingSettings) -> String {
    format!("{SHARED}\nconst TRAIN_WIDTH: u32 = {}u;\nconst TRAIN_LAYERS: u32 = {}u;\nconst TRAIN_ACTIVATION: u32 = {}u;\nconst TRAIN_PARAMETERS: u32 = {}u;\n{}",
        settings.width, settings.layers, settings.activation.shader_id(), settings.parameter_count().unwrap(),
        include_str!("../../../assets/shaders/neural_training.wgsl"))
}

impl GpuTrainingJob {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        source: Arc<Vec<SdfObject>>,
        root: uuid::Uuid,
        packed: PackedTrainingScene,
    ) -> Result<Self, &'static str> {
        let state = TrainingState::new(source, root).ok_or("Invalid source bounds")?;
        let settings = state.settings;
        let rw = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let states: Vec<_> = state
            .network
            .weights
            .iter()
            .map(|weight| [*weight, 0.0, 0.0, *weight])
            .collect();
        let weights = storage(
            &device,
            "neural training weights and Adam moments",
            bytemuck::cast_slice(&states),
            rw,
        );
        let samples = buffer(&device, "neural training batch", (BATCH * 32) as u64, rw);
        let owners = buffer(
            &device,
            "neural training material samples",
            (MATERIAL_SAMPLES * 32) as u64,
            rw,
        );
        let scratch_size = (BATCH * settings.layers as usize * settings.width as usize * 8) as u64;
        if scratch_size > device.limits().max_storage_buffer_binding_size as u64
            || weights.size() > device.limits().max_storage_buffer_binding_size as u64
        {
            return Err("GPU training buffers exceed device limits");
        }
        let scratch = buffer(
            &device,
            "neural training activations and deltas",
            scratch_size,
            rw,
        );
        let mut initial_stats = vec![[0.0f32; 4]; BATCH + 3];
        initial_stats[1] = [1e30, 0.0, 1e30, -1e30];
        let stats = storage(
            &device,
            "neural training validation statistics",
            bytemuck::cast_slice(&initial_stats),
            rw,
        );
        let params = Params {
            world: state.world.to_cols_array_2d(),
            cube: state.center.extend(state.half_extent).to_array(),
            control: [0, 0, 0, 0],
            options: [state.distance_unit, settings.learning_rate, 1.0, 1.0],
            component: [packed.start, packed.root, settings.seed, 0],
            distance_target: [if settings.raymarch_last_segment { settings.distance_offset } else { 0.0 }, 0.0, 0.0, 0.0],
        };
        let params_buffer: Vec<_> = (0..STAGING_BATCHES)
            .map(|_| {
                storage(
                    &device,
                    "neural training parameters",
                    bytemuck::bytes_of(&params),
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                )
            })
            .collect();
        let indices: Vec<_> = (0..STAGING_BATCHES)
            .map(|_| {
                buffer(
                    &device,
                    "neural training sample indices",
                    (BATCH * 4) as u64,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                )
            })
            .collect();
        let source = source_shader(packed.capacity);
        let module = wgpu::naga::front::wgsl::parse_str(&source).map_err(|error| {
            eprintln!("{}", error.emit_to_string(&source));
            "Invalid GPU sampling shader"
        })?;
        let info = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|error| {
            eprintln!("GPU sampling: {error:?}");
            "Invalid GPU sampling shader"
        })?;
        let entry = module
            .entry_points
            .iter()
            .position(|entry| entry.name == "train_sample_sdf")
            .unwrap();
        let used = info.get_entry_point(entry);
        let bindings: Vec<_> = module
            .global_variables
            .iter()
            .filter_map(|(handle, variable)| {
                variable
                    .binding
                    .as_ref()
                    .filter(|binding| binding.group == 0 && !used[handle].is_empty())
                    .map(|binding| binding.binding)
            })
            .collect();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("exact SDF training sampler"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let sample_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("sample exact source distances for training"),
            layout: None,
            module: &shader,
            entry_point: Some("train_sample_sdf"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[
                    ("HAS_BOOLEANS", 1.0),
                    ("USE_BVH", 0.0),
                    ("HAS_NEURAL_SDF", 0.0),
                ],
                ..Default::default()
            },
            cache: None,
        });
        let scene_objects = storage(
            &device,
            "training source objects",
            bytemuck::cast_slice(&packed.objects),
            rw,
        );
        let bvh = storage(
            &device,
            "training source BVH",
            bytemuck::cast_slice(&packed.bvh),
            rw,
        );
        let points = storage(
            &device,
            "training source polygon and text points",
            bytemuck::cast_slice(&packed.polygon_points),
            rw,
        );
        let lattice_points = storage(
            &device,
            "training source lattice control points",
            bytemuck::cast_slice(&packed.lattice_points),
            rw,
        );
        let modifiers = storage(
            &device,
            "training source modifier parameters",
            bytemuck::cast_slice(&packed.modifier_params),
            rw,
        );
        let material_headers = storage(
            &device,
            "training source material headers",
            bytemuck::cast_slice(&packed.material_headers),
            rw,
        );
        let material_params = storage(
            &device,
            "training source material parameters",
            bytemuck::cast_slice(&packed.material_params),
            rw,
        );
        let dummy = storage(&device, "unused derived capture data", &[], rw);
        let rows = (packed.lattice_tiles.len() as u32)
            .div_ceil(crate::renderer::LATTICE_ATLAS_TILES_PER_ROW)
            .max(1);
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("training source lattice atlas"),
            size: wgpu::Extent3d {
                width: crate::renderer::LATTICE_ATLAS_WIDTH,
                height: crate::renderer::LATTICE_ATLAS_TILE_PITCH * rows,
                depth_or_array_layers: crate::renderer::modifier_gpu::INVERSE_GRID_RESOLUTION
                    as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (tile, resolution, offsets) in packed.lattice_tiles {
            let texels: Vec<u16> = offsets
                .iter()
                .flat_map(|offset| {
                    [offset.x, offset.y, offset.z, 0.0]
                        .map(|value| half::f16::from_f32(value).to_bits())
                })
                .collect();
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: tile % crate::renderer::LATTICE_ATLAS_TILES_PER_ROW
                            * crate::renderer::LATTICE_ATLAS_TILE_PITCH,
                        y: tile / crate::renderer::LATTICE_ATLAS_TILES_PER_ROW
                            * crate::renderer::LATTICE_ATLAS_TILE_PITCH,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&texels),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(resolution * 8),
                    rows_per_image: Some(resolution),
                },
                wgpu::Extent3d {
                    width: resolution,
                    height: resolution,
                    depth_or_array_layers: resolution,
                },
            );
        }
        let view = atlas.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let entries: Result<Vec<_>, _> = bindings
            .into_iter()
            .map(|binding| {
                let resource = match binding {
                    1 => scene_objects.as_entire_binding(),
                    2 => bvh.as_entire_binding(),
                    3 => material_headers.as_entire_binding(),
                    4 => material_params.as_entire_binding(),
                    5 => points.as_entire_binding(),
                    6 => lattice_points.as_entire_binding(),
                    8 => modifiers.as_entire_binding(),
                    13 => dummy.as_entire_binding(),
                    9 => wgpu::BindingResource::TextureView(&view),
                    10 => wgpu::BindingResource::Sampler(&sampler),
                    _ => return Err("Unexpected GPU sampling resource"),
                };
                Ok(wgpu::BindGroupEntry { binding, resource })
            })
            .collect();
        let scene_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("private exact training scene"),
            layout: &sample_pipeline.get_bind_group_layout(0),
            entries: &entries?,
        });
        let sample_bind = (0..STAGING_BATCHES)
            .map(|slot| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("neural sample outputs"),
                    layout: &sample_pipeline.get_bind_group_layout(1),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: params_buffer[slot].as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: samples.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: indices[slot].as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        let model_source = model_shader(settings);
        let model = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("neural backpropagation and Adam"),
            source: wgpu::ShaderSource::Wgsl(model_source.into()),
        });
        // Explicit layout keeps a single bind group compatible with every training pass.
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("neural optimizer buffers"),
            entries: &(0..5)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: if binding == 0 {
                            wgpu::BufferBindingType::Uniform
                        } else {
                            wgpu::BufferBindingType::Storage {
                                read_only: binding == 2,
                            }
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                })
                .collect::<Vec<_>>(),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("neural optimizer layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let make = |entry, layer| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &model,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("TRAIN_LAYER", layer as f64)],
                    ..Default::default()
                },
                cache: None,
            })
        };
        let forward = (0..settings.layers)
            .map(|layer| make("train_forward", layer))
            .collect();
        let backward = (0..settings.layers.saturating_sub(1))
            .map(|layer| make("train_backward", layer))
            .collect();
        let model_binds = (0..STAGING_BATCHES)
            .map(|slot| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("neural optimizer data"),
                    layout: &layout,
                    entries: &[&params_buffer[slot], &weights, &samples, &scratch, &stats]
                        .iter()
                        .enumerate()
                        .map(|(binding, buffer)| wgpu::BindGroupEntry {
                            binding: binding as u32,
                            resource: buffer.as_entire_binding(),
                        })
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        let readback_size = weights.size() + 32 + owners.size();
        let readback = buffer(
            &device,
            "completed neural field readback",
            readback_size,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        );
        let (output_delta, adam, validate, reduce, checkpoint, commit) = (
            make("train_output_delta", 0),
            make("train_adam", 0),
            make("train_validate", 0),
            make("train_reduce", 0),
            make("train_checkpoint", 0),
            make("train_commit_stats", 0),
        );
        Ok(Self {
            device,
            queue,
            job: state,
            ids: packed.ids,
            params,
            params_buffer,
            indices,
            weights,
            samples,
            owners,
            stats,
            readback,
            sample_pipeline,
            scene_bind,
            sample_bind,
            model_binds,
            forward,
            backward,
            output_delta,
            adam,
            validate,
            reduce,
            checkpoint,
            commit,
            phase: Phase::Training,
            cursor: 0,
            epoch: 0,
            completed: 0,
            submitted: 0,
            waiting: false,
            submission: None,
            done: Arc::new(AtomicBool::new(false)),
            mapped: Arc::new(Mutex::new(None)),
        })
    }

    pub fn progress_percent(&self) -> u32 {
        let count = self.job.settings.samples as u64;
        let total = 2 * self.job.settings.epochs as u64 * count + MATERIAL_SAMPLES as u64;
        if matches!(self.phase, Phase::Done) {
            100
        } else {
            ((self.completed * 100) / total).min(99) as u32
        }
    }

    pub fn advance(&mut self, budget: std::time::Duration) -> Result<bool, &'static str> {
        let started = web_time::Instant::now();
        loop {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let poll = if self.waiting || matches!(self.phase, Phase::Readback) {
                    wgpu::PollType::Wait {
                        submission_index: self.submission.clone(),
                        timeout: Some(std::time::Duration::from_millis(4)),
                    }
                } else {
                    wgpu::PollType::Poll
                };
                match self.device.poll(poll) {
                    Ok(_) => {}
                    Err(wgpu::PollError::Timeout) => return Ok(false),
                    Err(_) => return Err("GPU training device stopped"),
                }
            }
            if matches!(self.phase, Phase::Done) {
                return Ok(true);
            }
            if matches!(self.phase, Phase::Readback) {
                if self.mapped.lock().unwrap().is_none() {
                    return Ok(false);
                }
                self.phase = Phase::Done;
                return Ok(true);
            }
            if self.waiting {
                if !self.done.load(Ordering::Relaxed) {
                    return Ok(false);
                }
                self.waiting = false;
                self.completed += self.submitted;
            }
            let total = self.job.settings.samples as usize;
            let phase_total = if matches!(self.phase, Phase::Ownership) {
                MATERIAL_SAMPLES
            } else {
                total
            };
            if self.cursor == phase_total {
                self.cursor = 0;
                match self.phase {
                    Phase::Training => self.phase = Phase::Validation,
                    Phase::Validation => {
                        self.epoch += 1;
                        self.phase = if self.epoch == self.job.settings.epochs as usize {
                            Phase::Ownership
                        } else {
                            Phase::Training
                        };
                    }
                    Phase::Ownership => {
                        self.begin_readback();
                        return Ok(false);
                    }
                    _ => unreachable!(),
                }
                continue;
            }
            let mut encoder = self.device.create_command_encoder(&Default::default());
            self.submitted = 0;
            let batch_count = (16384 / self.job.network.weights.len()).clamp(1, STAGING_BATCHES);
            for slot in 0..batch_count {
                let count = BATCH.min(phase_total - self.cursor);
                let indices: Vec<u32> = if matches!(self.phase, Phase::Training) {
                    if self.cursor == 0 {
                        self.job.sample_order.shuffle(&mut self.job.rng);
                    }
                    (self.cursor..self.cursor + count)
                        .map(|index| self.job.sample_order.index(index) as u32)
                        .collect()
                } else {
                    (self.cursor..self.cursor + count)
                        .map(|index| index as u32)
                        .collect()
                };
                self.params.component[3] = u32::from(matches!(self.phase, Phase::Training));
                self.params.control = [
                    count as u32,
                    if matches!(self.phase, Phase::Ownership) {
                        MATERIAL_GRID as u32
                    } else {
                        0
                    },
                    self.job.update as u32 + 1,
                    u32::from(self.cursor == 0),
                ];
                self.params.options[2] = 1.0 - 0.9f32.powi(self.job.update as i32 + 1);
                self.params.options[3] = 1.0 - 0.999f32.powi(self.job.update as i32 + 1);
                self.queue.write_buffer(
                    &self.params_buffer[slot],
                    0,
                    bytemuck::bytes_of(&self.params),
                );
                self.queue
                    .write_buffer(&self.indices[slot], 0, bytemuck::cast_slice(&indices));
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&self.sample_pipeline);
                    pass.set_bind_group(0, &self.scene_bind, &[]);
                    pass.set_bind_group(1, &self.sample_bind[slot], &[]);
                    pass.dispatch_workgroups(count.div_ceil(64) as u32, 1, 1);
                }
                if matches!(self.phase, Phase::Ownership) {
                    encoder.copy_buffer_to_buffer(
                        &self.samples,
                        0,
                        &self.owners,
                        (self.cursor * 32) as u64,
                        (count * 32) as u64,
                    );
                } else {
                    let bind = &self.model_binds[slot];
                    for pipeline in &self.forward {
                        dispatch(&mut encoder, pipeline, bind, count * self.job.network.width);
                    }
                    if matches!(self.phase, Phase::Training) {
                        dispatch(&mut encoder, &self.output_delta, bind, count);
                        for pipeline in self.backward.iter().rev() {
                            dispatch(&mut encoder, pipeline, bind, count * self.job.network.width);
                        }
                        dispatch(
                            &mut encoder,
                            &self.adam,
                            bind,
                            self.job.network.weights.len(),
                        );
                        self.job.update += 1;
                    } else {
                        dispatch(&mut encoder, &self.validate, bind, count);
                        dispatch(&mut encoder, &self.reduce, bind, 1);
                        if self.cursor + count == total {
                            dispatch(
                                &mut encoder,
                                &self.checkpoint,
                                bind,
                                self.job.network.weights.len(),
                            );
                            dispatch(&mut encoder, &self.commit, bind, 1);
                        }
                    }
                }
                self.cursor += count;
                self.submitted += count as u64;
                if self.cursor == phase_total || started.elapsed() >= budget {
                    break;
                }
            }
            self.submission = Some(self.queue.submit([encoder.finish()]));
            self.done.store(false, Ordering::Relaxed);
            let done = self.done.clone();
            self.queue
                .on_submitted_work_done(move || done.store(true, Ordering::Relaxed));
            self.waiting = true;
            if started.elapsed() >= budget {
                return Ok(false);
            }
        }
    }

    fn begin_readback(&mut self) {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.weights, 0, &self.readback, 0, self.weights.size());
        encoder.copy_buffer_to_buffer(&self.stats, 0, &self.readback, self.weights.size(), 32);
        encoder.copy_buffer_to_buffer(
            &self.owners,
            0,
            &self.readback,
            self.weights.size() + 32,
            self.owners.size(),
        );
        self.submission = Some(self.queue.submit([encoder.finish()]));
        let mapped = self.mapped.clone();
        let readback = self.readback.clone();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let result = result
                    .map_err(|_| "Could not read GPU trained field")
                    .and_then(|_| {
                        let range = readback
                            .slice(..)
                            .get_mapped_range()
                            .map_err(|_| "Could not access GPU trained field")?;
                        Ok(bytemuck::cast_slice::<u8, f32>(&range).to_vec())
                    });
                *mapped.lock().unwrap() = Some(result);
            });
        self.phase = Phase::Readback;
    }

    pub fn finish(mut self) -> Result<NeuralField, &'static str> {
        let values = self
            .mapped
            .lock()
            .unwrap()
            .take()
            .ok_or("GPU training is not complete")??;
        let parameters = self.job.network.weights.len();
        self.job.best.weights = values[..parameters * 4]
            .chunks_exact(4)
            .map(|state| state[3])
            .collect();
        if self
            .job
            .best
            .weights
            .iter()
            .any(|weight| !weight.is_finite())
        {
            return Err("Nonfinite GPU trained weights");
        }
        let stats = &values[parameters * 4 + 4..parameters * 4 + 8];
        if !stats[0].is_finite() || stats[0] >= 1e29 {
            return Err("Invalid GPU source samples or fit");
        }
        self.job.best_loss = stats[0];
        self.job.best_max_error = stats[1];
        self.job.best_min = stats[2];
        self.job.best_max = stats[3];
        self.job.owners = values[parameters * 4 + 8..]
            .chunks_exact(8)
            .map(|sample| {
                self.ids
                    .get(sample[4].to_bits() as usize)
                    .copied()
                    .ok_or("Invalid GPU material owner")
            })
            .collect::<Result<_, _>>()?;
        self.readback.unmap();
        self.job.finish()
    }
}

#[cfg(test)]
mod tests {
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
            let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
            object.neural_sdf.training = NeuralTrainingSettings {
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
            let mut gpu = GpuTrainingJob::new(device.clone(), queue.clone(), source.clone(), id, packed)
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
            for sample in values.chunks_exact(8).take(32) {
                let point = Vec3::from_slice(sample);
                let world = state
                    .world
                    .transform_point3(state.center + point * state.half_extent);
                let expected = (sampler.sample(world).0 - 0.3) / state.distance_unit;
                assert!((sample[3] - expected).abs() < 1e-5);
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
            for (layers, width, activation, epochs) in [
                (
                    defaults.layers,
                    defaults.width,
                    defaults.activation,
                    defaults.epochs,
                ),
                (1, 8, crate::model::NeuralActivation::Relu, 32),
                (2, 33, crate::model::NeuralActivation::Softplus, 256),
                (8, 5, crate::model::NeuralActivation::Softplus, 256),
            ] {
                let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
                object.neural_sdf.training = NeuralTrainingSettings {
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
                    reference
                        .network
                        .gradient_with_scratch(p, target, &mut gradient, &mut scratch);
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
                for (sample, values) in samples.chunks_exact(8).enumerate() {
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
                assert!(field.rms_error.is_finite() && field.rms_error < 0.1);
                assert!(field.network.evaluate(Vec3::ZERO) < 0.0);
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
}

//! Cache completed images and refine expensive scenes in bounded GPU batches.
//! UI rendering stays at native resolution throughout camera and object edits.
use std::sync::{Arc, Mutex};

mod encoding;

const TILE: u32 = 32;
const INITIAL_PIXELS: u32 = 48 * 1024;
const EDIT_TARGET_MS: f64 = 6.0;
const PLAYBACK_TARGET_MS: f64 = 12.0;
const REFINEMENT_PAUSE: std::time::Duration = std::time::Duration::from_millis(4);

#[derive(Clone, Debug, PartialEq)]
pub struct ViewKey {
    pub matrix: [[f32; 4]; 4],
    pub position: [f32; 3],
    pub projection: u32,
    /// Geometry/material/selection revision, followed by world lighting revision.
    pub versions: [i32; 2],
    pub size: [u32; 2],
    pub refine: bool,
}

struct Targets {
    preview: wgpu::TextureView,
    refined: wgpu::TextureView,
    preview_depth: wgpu::TextureView,
    refined_depth: wgpu::TextureView,
    composite: wgpu::BindGroup,
    deferred: Option<DeferredBuffers>,
    preview_size: [u32; 2],
}

struct DeferredBuffers {
    preview: DeferredTargets,
    native: Option<DeferredTargets>,
}

struct DeferredTargets {
    buffers: [wgpu::TextureView; 4],
    bind_group: wgpu::BindGroup,
}

impl DeferredTargets {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, size: [u32; 2]) -> Self {
        let buffers = [
            "deferred position",
            "deferred normal",
            "deferred albedo",
            "deferred optics",
        ]
        .map(|label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deferred geometry bind group"),
            layout,
            entries: &buffers
                .iter()
                .enumerate()
                .map(|(binding, view)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: wgpu::BindingResource::TextureView(view),
                })
                .collect::<Vec<_>>(),
        });
        Self {
            buffers,
            bind_group,
        }
    }
}

impl DeferredBuffers {
    fn for_work(&self, work: Work) -> &DeferredTargets {
        match work {
            Work::Preview => &self.preview,
            Work::Refine { .. } | Work::Interleave { .. } | Work::Relight { native: true } => {
                self.native.as_ref().expect("native deferred targets")
            }
            Work::Relight { native: false } => &self.preview,
            Work::Cached => unreachable!(),
        }
    }
}

fn same_geometry(a: &ViewKey, b: &ViewKey) -> bool {
    a.matrix == b.matrix
        && a.position == b.position
        && a.projection == b.projection
        && a.versions[0] == b.versions[0]
        && a.size == b.size
        && a.refine == b.refine
}

// Shade each newly completed region immediately. Once all geometry is ready,
// refresh the whole image so screen-space effects see complete neighbors.
fn deferred_shading_work(work: Work, size: [u32; 2]) -> Work {
    match work {
        Work::Preview | Work::Relight { .. } => Work::Preview,
        Work::Refine { end, .. } if end == tile_count(size) => Work::Preview,
        _ => work,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Work {
    Preview,
    Refine { first: u32, end: u32 },
    Interleave { first: u32, end: u32 },
    Relight { native: bool },
    Cached,
}

pub struct HybridPass<'a> {
    pub depth_pipeline: Option<&'a wgpu::RenderPipeline>,
    pub splat_pipeline: &'a wgpu::RenderPipeline,
    pub splat_bind_group: &'a wgpu::BindGroup,
    pub splat_buffer: &'a wgpu::Buffer,
    pub splat_count: u32,
    pub mesh_pipeline: &'a wgpu::RenderPipeline,
    pub mesh_bind_group: &'a wgpu::BindGroup,
    pub mesh_buffer: &'a wgpu::Buffer,
    pub mesh_vertex_count: u32,
}

pub struct DeferredPass<'a> {
    pub geometry_pipeline: &'a wgpu::RenderPipeline,
}

pub struct Viewport {
    key: Option<ViewKey>,
    targets: Option<Targets>,
    completed: u32,
    interleaved: bool,
    pixel_budget: u32,
    target_ms: f64,
    initial_budget: u32,
    pending: bool,
    pending_frames: u32,
    refinement_resume_at: Option<web_time::Instant>,
    timing: Arc<Mutex<Option<Option<f64>>>>,
    submitted_pixels: u32,
    submitted_budget: u32,
    query_set: Option<wgpu::QuerySet>,
    query_resolve: Option<wgpu::Buffer>,
    pipeline: wgpu::RenderPipeline,
    deferred_pipeline: wgpu::RenderPipeline,
    deferred_layout: wgpu::BindGroupLayout,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    progress: wgpu::Buffer,
    format: wgpu::TextureFormat,
}

impl Viewport {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        timestamps: bool,
        scene_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport composite layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport composite"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../assets/shaders/viewport.wgsl").into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport composite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let deferred_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deferred geometry textures"),
            entries: &[0, 1, 2, 3].map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }),
        });
        let deferred_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deferred lighting"),
            source: wgpu::ShaderSource::Wgsl(deferred_shader_source().into()),
        });
        let deferred_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("deferred lighting layout"),
                bind_group_layouts: &[Some(scene_layout), Some(&deferred_layout)],
                immediate_size: 0,
            });
        let deferred_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("deferred lighting pipeline"),
            layout: Some(&deferred_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &deferred_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &deferred_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            key: None,
            targets: None,
            completed: 0,
            interleaved: false,
            pixel_budget: INITIAL_PIXELS,
            target_ms: EDIT_TARGET_MS,
            initial_budget: INITIAL_PIXELS,
            pending: false,
            pending_frames: 0,
            refinement_resume_at: None,
            submitted_pixels: INITIAL_PIXELS,
            submitted_budget: INITIAL_PIXELS,
            timing: Arc::new(Mutex::new(None)),
            query_set: timestamps.then(|| {
                device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("viewport GPU time"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 2,
                })
            }),
            query_resolve: timestamps.then(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("viewport timestamp resolve"),
                    size: 16,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                })
            }),
            pipeline,
            deferred_pipeline,
            deferred_layout,
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("viewport upscale"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            progress: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("viewport progress"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            format,
        }
    }

    pub fn set_initial_budget(&mut self, pixels: u32) {
        if self.initial_budget != pixels {
            self.initial_budget = pixels;
            self.pixel_budget = self.pixel_budget.min(pixels);
        }
    }

    pub fn set_playback_budget(&mut self, playing: bool) {
        let target = if playing {
            PLAYBACK_TARGET_MS
        } else {
            EDIT_TARGET_MS
        };
        if self.target_ms != target {
            self.pixel_budget = ((self.pixel_budget as f64 * target / self.target_ms) as u32)
                .clamp(TILE * TILE, 4 * 1024 * 1024);
            self.target_ms = target;
        }
    }

    pub fn invalidate(&mut self) {
        self.key = None;
        self.completed = 0;
    }

    pub(crate) fn reset_for_scene(&mut self) {
        self.invalidate();
        self.targets = None;
        self.interleaved = false;
        self.pixel_budget = INITIAL_PIXELS;
        self.initial_budget = INITIAL_PIXELS;
        self.target_ms = EDIT_TARGET_MS;
        self.pending = false;
        self.pending_frames = 0;
        self.refinement_resume_at = None;
        self.submitted_pixels = 0;
        self.submitted_budget = 0;
        // Already-submitted GPU callbacks may still run. Give the new scene
        // its own mailbox so old timings cannot release or resize its batches.
        self.timing = Arc::new(Mutex::new(None));
    }

    #[cfg(all(not(target_arch = "wasm32"), unix))]
    pub(crate) fn diagnostics(&self) -> serde_json::Value {
        serde_json::json!({
            "has_view": self.key.is_some(),
            "size": self.key.as_ref().map(|key| key.size),
            "completed": self.completed,
            "total": self.key.as_ref().map(|key| if self.interleaved { 256 } else { tile_count(key.size) }),
            "refined": self.is_refined(),
            "pending_gpu": self.pending,
            "completion_received": self.timing.lock().unwrap().is_some(),
            "pixel_budget": self.pixel_budget,
            "preview_size": self.targets.as_ref().map(|targets| targets.preview_size),
            "interleaved": self.interleaved,
            "deferred": self.targets.as_ref().is_some_and(|targets| targets.deferred.is_some()),
        })
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        key: ViewKey,
        refine: bool,
        deferred: bool,
        interleaved: bool,
    ) -> Work {
        if self.interleaved != interleaved {
            self.key = None;
        }
        self.interleaved = interleaved;
        if !refine && !interleaved {
            self.completed = 0;
        }
        // Poll is nonblocking. WebGPU delivers map callbacks through its event loop.
        let _ = device.poll(wgpu::PollType::Poll);
        if let Some(milliseconds) = self.timing.lock().unwrap().take() {
            self.pending = false;
            // Leave a gap after completed GPU work for the compositor and
            // other applications. Only refinement observes this deadline;
            // camera/scene changes below can submit their preview immediately.
            self.refinement_resume_at = Some(web_time::Instant::now() + REFINEMENT_PAUSE);
            if let Some(ms) = milliseconds.filter(|_| self.submitted_pixels > 0) {
                self.pixel_budget = budget_after_sample(
                    self.pixel_budget,
                    self.submitted_budget,
                    self.submitted_pixels,
                    ms,
                    self.target_ms,
                );
            }
        }
        if self.pending {
            self.pending_frames += 1;
            // Timestamp queries are optional in WebGPU. Without them, prevent
            // multi-frame GPU backlog by reducing the next batch conservatively.
            if self.query_set.is_none() && self.pending_frames == 2 && self.submitted_pixels > 0 {
                self.pixel_budget = (self.pixel_budget / 2).max(TILE * TILE);
            }
            return Work::Cached;
        }
        self.pending_frames = 0;
        if self.key.as_ref() != Some(&key) {
            self.refinement_resume_at = None;
            // World lighting/effect revisions leave the primary-hit geometry intact.
            // Refresh the visible complete target; unfinished native tiles remain valid.
            if deferred
                && self
                    .key
                    .as_ref()
                    .is_some_and(|old| same_geometry(old, &key))
                && self
                    .targets
                    .as_ref()
                    .is_some_and(|targets| targets.deferred.is_some())
            {
                let native = self.completed == tile_count(key.size);
                self.key = Some(key);
                return Work::Relight { native };
            }
            // Object transforms invalidate the image, not the GPU's measured throughput.
            // Keep the learned budget while editing. More expensive scene classes
            // still lower it through set_initial_budget, and GPU timings adapt per batch.
            let mut preview_size = if interleaved {
                [1, 1]
            } else if refine {
                preview_size(key.size, self.pixel_budget)
            } else {
                key.size
            };
            if let Some(targets) = &self.targets {
                let previous = targets.preview_size[0] * targets.preview_size[1];
                let next = preview_size[0] * preview_size[1];
                if self.key.as_ref().is_some_and(|old| old.size == key.size)
                    && next >= previous * 4 / 5
                    && next <= previous * 6 / 5
                {
                    preview_size = targets.preview_size;
                }
            }
            let recreate = self
                .key
                .as_ref()
                .is_none_or(|previous| previous.size != key.size)
                || self.targets.as_ref().is_none_or(|targets| {
                    targets.preview_size != preview_size || targets.deferred.is_some() != deferred
                });
            if recreate {
                let texture = |size: [u32; 2], label| {
                    device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some(label),
                            size: wgpu::Extent3d {
                                width: size[0],
                                height: size[1],
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: self.format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::COPY_SRC,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                };
                let preview = texture(preview_size, "interactive scene");
                let refined = texture(key.size, "full resolution scene");
                let depth = |size: [u32; 2], label| {
                    device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some(label),
                            size: wgpu::Extent3d {
                                width: size[0],
                                height: size[1],
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Depth32Float,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                };
                let preview_depth = depth(preview_size, "interactive SDF depth");
                let refined_depth = depth(key.size, "native SDF depth");
                let deferred = deferred.then(|| DeferredBuffers {
                    preview: DeferredTargets::new(device, &self.deferred_layout, preview_size),
                    native: None,
                });
                let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("viewport composite"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&preview),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&refined),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: self.progress.as_entire_binding(),
                        },
                    ],
                });
                self.targets = Some(Targets {
                    preview,
                    refined,
                    preview_depth,
                    refined_depth,
                    composite,
                    deferred,
                    preview_size,
                });
            }
            self.key = Some(key.clone());
            self.completed = 0;
            if !interleaved {
                return Work::Preview;
            }
        }
        if !interleaved
            && self
                .targets
                .as_ref()
                .is_some_and(|targets| targets.preview_size == key.size)
        {
            return Work::Cached;
        }
        if !refine && !interleaved {
            return Work::Cached;
        }
        let total = if interleaved {
            256
        } else {
            tile_count(key.size)
        };
        if self.completed >= total {
            return Work::Cached;
        }
        if self.refinement_resume_at.is_some_and(|deadline| web_time::Instant::now() < deadline) {
            return Work::Cached;
        }
        if let Some(buffers) = self
            .targets
            .as_mut()
            .and_then(|targets| targets.deferred.as_mut())
        {
            buffers.native.get_or_insert_with(|| {
                DeferredTargets::new(device, &self.deferred_layout, key.size)
            });
        }
        if interleaved {
            let phase_pixels = key.size[0].div_ceil(16) * key.size[1].div_ceil(16);
            let count = (self.pixel_budget / phase_pixels.max(1)).clamp(1, 16);
            Work::Interleave {
                first: self.completed,
                end: (self.completed + count).min(total),
            }
        } else {
            let count = (self.pixel_budget / (TILE * TILE)).max(1);
            Work::Refine {
                first: self.completed,
                end: (self.completed + count).min(total),
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn preview_pixels(&self) -> u32 {
        if self.interleaved {
            return 0;
        }
        self.targets.as_ref().map_or(0, |targets| {
            targets.preview_size[0] * targets.preview_size[1]
        })
    }

    pub fn is_refined(&self) -> bool {
        self.key.as_ref().is_some_and(|key| {
            self.completed
                == if self.interleaved {
                    256
                } else {
                    tile_count(key.size)
                }
                || (!self.interleaved
                    && self
                        .targets
                        .as_ref()
                        .is_some_and(|targets| targets.preview_size == key.size))
        })
    }

    pub fn matches(&self, key: &ViewKey) -> bool {
        self.key.as_ref() == Some(key)
    }

    pub fn composite(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(
            0,
            &self.targets.as_ref().expect("prepared viewport").composite,
            &[],
        );
        pass.draw(0..3, 0..1);
    }
}

fn adjusted_budget(previous: u32, milliseconds: f64, target_ms: f64) -> u32 {
    if !milliseconds.is_finite() || milliseconds <= 0.0 {
        return previous;
    }
    // React quickly to expensive views, grow cautiously, and cap the minimum
    // batch at one tile. Timings come from the GPU, not the vsync interval.
    let ratio = (target_ms / milliseconds).max(0.25);
    ((previous as f64 * ratio) as u32).clamp(TILE * TILE, 4 * 1024 * 1024)
}

fn budget_after_sample(
    budget: u32,
    submitted_budget: u32,
    rendered_pixels: u32,
    milliseconds: f64,
    target_ms: f64,
) -> u32 {
    // The final refinement batch can be just a few edge pixels. Its fixed GPU
    // overhead says nothing about the cost of a full batch, so retain the
    // learned budget for the next camera or selection change.
    if rendered_pixels < submitted_budget / 2 {
        return budget;
    }
    adjusted_budget(rendered_pixels, milliseconds, target_ms).min(budget.saturating_mul(6) / 5)
}

fn preview_size(size: [u32; 2], budget: u32) -> [u32; 2] {
    let scale = (budget as f64 / (size[0] as f64 * size[1] as f64))
        .sqrt()
        .min(1.0);
    [
        (size[0] as f64 * scale).floor().max(1.0) as u32,
        (size[1] as f64 * scale).floor().max(1.0) as u32,
    ]
}
fn tile_count(size: [u32; 2]) -> u32 {
    size[0].div_ceil(TILE) * size[1].div_ceil(TILE)
}
fn tile_rect(size: [u32; 2], index: u32) -> [u32; 4] {
    let columns = size[0].div_ceil(TILE);
    let x = index % columns * TILE;
    let y = index / columns * TILE;
    [x, y, (size[0] - x).min(TILE), (size[1] - y).min(TILE)]
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod deferred_tests;

#[cfg(test)]
mod tests;

fn deferred_shader_source() -> String {
    include_str!("../assets/shaders/deferred.wgsl")
        .replace(
            "// SSAO_MODULE",
            include_str!("../assets/shaders/ssao.wgsl"),
        )
        .replace("// SSR_MODULE", include_str!("../assets/shaders/ssr.wgsl"))
        .replace(
            "// TRANSMISSION_MODULE",
            include_str!("../assets/shaders/deferred_transmission.wgsl"),
        )
}

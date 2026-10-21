//! Cache completed images and refine expensive scenes in bounded GPU batches.
//! UI rendering stays at native resolution throughout camera and object edits.

mod bloom;
mod encoding;
mod scheduler;
use scheduler::{Plan, Scheduler, Stage};

const TILE: u32 = 32;
const INITIAL_PIXELS: u32 = 48 * 1024;
// Below this the desktop preview becomes a handful of indistinct pixels.
// Keep coverage batches independently small to bound expensive source work.
const MIN_PREVIEW_PIXELS: u32 = 256;
const MIN_COVERAGE_PIXELS: u32 = 64;
// Leave half a 60 Hz frame for input/UI/compositing. A 4 ms target was
// nearly consumed by fixed GPU overhead and collapsed previews to a few pixels.
const EDIT_TARGET_MS: f64 = 8.0;
const PLAYBACK_TARGET_MS: f64 = 12.0;
const REFINEMENT_PAUSE: std::time::Duration = std::time::Duration::from_millis(16);
const SUBMISSION_RECEIPT: u64 = 0x434c_4159_4441_5348;

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
    contact: Option<ContactTarget>,
    preview: wgpu::TextureView,
    refined: wgpu::TextureView,
    preview_depth: wgpu::TextureView,
    refined_depth: wgpu::TextureView,
    composite: wgpu::BindGroup,
    deferred: Option<DeferredBuffers>,
    bloom: Option<bloom::BloomTargets>,
    preview_size: [u32; 2],
}

struct DeferredBuffers {
    preview: DeferredTargets,
    native: Option<DeferredTargets>,
}

pub(crate) struct ContactTarget {
    pub(crate) view: wgpu::TextureView,
    pub(crate) bind_group: wgpu::BindGroup,
}
impl ContactTarget {
    pub(crate) fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, size: [u32; 2]) -> Self {
        let view = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("primary contact probes"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        }).create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("primary contact cache"), layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
        });
        Self { view, bind_group }
    }
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
    /// Range of sparse samples in phase order, including partial phases.
    Interleave { first: u32, end: u32 },
    Relight { native: bool },
    Cached,
}

pub struct HybridPass<'a> {
    pub depth_pipeline: Option<&'a wgpu::RenderPipeline>,
    pub splat_pipeline: &'a wgpu::RenderPipeline,
    pub splat_bind_group: &'a wgpu::BindGroup,
    pub splat_buffer: &'a wgpu::Buffer,
    pub splat_first_count: u32,
    pub splat_extra_buffers: &'a [(wgpu::Buffer, u32)],
    pub splat_count: u32,
    pub mesh_pipeline: &'a wgpu::RenderPipeline,
    pub mesh_bind_group: &'a wgpu::BindGroup,
    pub mesh_buffer: &'a wgpu::Buffer,
    pub mesh_vertex_count: u32,
}

pub struct DeferredPass<'a> {
    pub geometry_pipeline: &'a wgpu::RenderPipeline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderingPath {
    Mesh,
    Deferred,
    Hybrid,
    Exact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activity { Preview, Ready, Gpu, Interaction, Optimization, Pacing, Complete }

pub struct Viewport {
    refinement_rect: [u32; 4],
    refinement_region_changed: bool,
    native_background: Option<wgpu::RenderPipeline>,
    key: Option<ViewKey>,
    targets: Option<Targets>,
    completed: u32,
    preview_ready: bool,
    activity: Activity,
    interleaved: bool,
    interacting: bool,
    refinement_paused: bool,
    rendering_path: Option<RenderingPath>,
    pixel_budget: u32,
    target_ms: f64,
    initial_budget: u32,
    preview_step: u8,
    preview_budget_override: Option<u32>,
    progressive_hybrid_preview: bool,
    refinement_resume_at: Option<web_time::Instant>,
    scheduler: Scheduler,
    repaint: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    query_set: Option<wgpu::QuerySet>,
    query_resolve: Option<wgpu::Buffer>,
    receipt_source: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    deferred_pipeline: wgpu::RenderPipeline,
    deferred_emission_pipeline: Option<wgpu::RenderPipeline>,
    deferred_bloom_pipeline: Option<wgpu::RenderPipeline>,
    deferred_pipeline_layout: wgpu::PipelineLayout,
    deferred_layout: wgpu::BindGroupLayout,
    contact_layout: wgpu::BindGroupLayout,
    contact_pipeline: Option<wgpu::RenderPipeline>,
    contact_active: bool,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    progress: wgpu::Buffer,
    format: wgpu::TextureFormat,
    bloom: bloom::Bloom,
}

impl Viewport {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        timestamps: bool,
        scene_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        use wgpu::util::DeviceExt;
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
                texture_entry(4),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
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
            source: wgpu::ShaderSource::Wgsl(bloom::composite_source().into()),
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
            refinement_rect: [0; 4],
            refinement_region_changed: false,
            native_background: None,
            targets: None,
            completed: 0,
            preview_ready: false,
            activity: Activity::Preview,
            interleaved: false,
            interacting: false,
            refinement_paused: false,
            rendering_path: None,
            pixel_budget: INITIAL_PIXELS,
            target_ms: EDIT_TARGET_MS,
            initial_budget: INITIAL_PIXELS,
            preview_step: 0,
            preview_budget_override: None,
            progressive_hybrid_preview: false,
            refinement_resume_at: None,
            scheduler: Scheduler::default(),
            repaint: None,
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
            receipt_source: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("scene submission receipt"),
                contents: bytemuck::bytes_of(&SUBMISSION_RECEIPT),
                usage: wgpu::BufferUsages::COPY_SRC,
            }),
            pipeline,
            deferred_pipeline,
            deferred_emission_pipeline: None,
            deferred_bloom_pipeline: None,
            deferred_pipeline_layout,
            deferred_layout,
            contact_layout: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("primary contact cache layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2, multisampled: false,
                    }, count: None,
                }],
            }),
            contact_pipeline: None,
            contact_active: false,
            bloom: bloom::Bloom::new(device),
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("viewport upscale"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            progress: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("viewport progress"),
                size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            format,
        }
    }

    pub(crate) fn contact_layout(&self) -> &wgpu::BindGroupLayout { &self.contact_layout }
    pub(crate) fn set_refinement_region(&mut self, rect: [u32; 4], background: Option<wgpu::RenderPipeline>) {
        self.refinement_region_changed |= self.refinement_rect != rect;
        self.refinement_rect = rect;
        self.native_background = background;
    }
    fn sparse_size(&self, size: [u32; 2]) -> [u32; 2] {
        if self.refinement_rect[2] > 0 && self.refinement_rect[3] > 0 {
            [self.refinement_rect[2], self.refinement_rect[3]]
        } else { size }
    }
    pub(crate) fn phase_samples(&self) -> u32 {
        self.key.as_ref().map_or(0, |key| sparse_phase_pixels(self.sparse_size(key.size)))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn contact_pipeline(&self) -> Option<&wgpu::RenderPipeline> { self.contact_pipeline.as_ref() }
    #[cfg(test)]
    pub(crate) fn fail_receipt_for_test(&mut self, device: &wgpu::Device) {
        self.receipt_source = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("failed GPU receipt injection"), size: 8,
            usage: wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false,
        });
    }
    pub(crate) fn set_contact_active(&mut self, active: bool) { self.contact_active = active; }
    pub(crate) fn set_contact_pipeline(&mut self, pipeline: Option<wgpu::RenderPipeline>) {
        self.contact_pipeline = pipeline;
    }

    pub fn set_initial_budget(&mut self, pixels: u32) {
        if self.initial_budget != pixels {
            self.initial_budget = pixels;
            self.pixel_budget = self.pixel_budget.min(pixels);
            self.scheduler.reset_budgets(pixels);
        }
    }

    #[cfg(test)]
    pub(crate) fn fixed_profile_budget(&mut self, pixels: u32) {
        self.scheduler.retire(self.target_ms);
        self.pixel_budget = pixels;
        for stage in [Stage::Preview, Stage::Coverage, Stage::Sparse, Stage::Tiles, Stage::Relight] {
            self.scheduler.set_budget(stage, pixels);
        }
    }

    pub fn set_playback_budget(&mut self, playing: bool) {
        let target = if playing {
            PLAYBACK_TARGET_MS
        } else {
            EDIT_TARGET_MS
        };
        if self.target_ms != target {
            self.scheduler.scale_budgets(target / self.target_ms);
            self.pixel_budget = ((self.pixel_budget as f64 * target / self.target_ms) as u32)
                .clamp(1, 4 * 1024 * 1024);
            self.target_ms = target;
        }
    }

    /// Keep previews responsive while a pointer or modal transform is active.
    /// Native refinement resumes with the same view and measured GPU budget.
    pub fn set_interacting(&mut self, interacting: bool) {
        self.interacting = interacting;
    }

    /// Keep scene-change previews available while optimization workers run.
    /// Completed tiles and timing estimates are retained until refinement resumes.
    pub fn set_refinement_paused(&mut self, paused: bool) {
        self.refinement_paused = paused;
    }

    pub fn set_rendering_path(&mut self, path: RenderingPath) {
        if self.rendering_path != Some(path) {
            // Raster mesh timings do not predict the cost of raymarching its
            // edited source. Restart conservatively even if object counts
            // selected the same initial budget for both representations.
            self.pixel_budget = self.pixel_budget.min(self.initial_budget);
            // Keep the pending GPU fence, but discard its old-path throughput
            // sample when prepare observes completion after this transition.
            self.scheduler.new_generation();
            self.scheduler.reset_budgets(self.initial_budget);
            self.rendering_path = Some(path);
        }
    }

    pub fn set_progressive_hybrid_preview(&mut self, enabled: bool) {
        self.progressive_hybrid_preview = enabled;
    }

    pub fn invalidate(&mut self) {
        self.scheduler.new_generation();
        self.key = None;
        self.completed = 0;
        self.preview_ready = false;
        self.activity = Activity::Preview;
    }

    pub(crate) fn invalidate_source_version(&mut self, version: i32) {
        // Uploading a thumbnail temporarily replaces the shared scene buffers,
        // but restoring them does not change the image being refined. Compare
        // the logical view revision, not the buffer cache's upload sentinel.
        if self.key.as_ref().is_some_and(|key| key.versions[0] != version) {
            self.invalidate();
        }
    }

    pub(crate) fn reset_for_scene(&mut self) {
        self.invalidate();
        self.refinement_rect = [0; 4];
        self.refinement_region_changed = false;
        self.native_background = None;
        self.targets = None;
        self.interleaved = false;
        self.interacting = false;
        self.refinement_paused = false;
        self.rendering_path = None;
        self.pixel_budget = INITIAL_PIXELS;
        self.initial_budget = INITIAL_PIXELS;
        self.preview_step = 0;
        self.preview_budget_override = None;
        self.progressive_hybrid_preview = false;
        self.target_ms = EDIT_TARGET_MS;
        self.refinement_resume_at = None;
        self.scheduler.reset_budgets(INITIAL_PIXELS);

    }

    #[cfg(all(not(target_arch = "wasm32"), unix))]
    pub(crate) fn diagnostics(&self) -> serde_json::Value {
        let timing = self.scheduler.current_gpu_ms();
        serde_json::json!({
            "has_view": self.key.is_some(),
            "size": self.key.as_ref().map(|key| key.size),
            "completed": self.completed,
            "total": self.key.as_ref().map(|key| if self.interleaved { sparse_sample_count(self.sparse_size(key.size)) } else { tile_count(key.size) }),
            "refined": self.is_refined(),
            "refinement_rect": self.refinement_rect,
            "pending_gpu": self.scheduler.busy(),
            "completion_received": self.scheduler.state() == "completed",
            "completed_gpu_ms": timing,
            "scheduler_state": self.scheduler.state(),
            "gpu_fault": self.scheduler.gpu_fault(),
            "activity": format!("{:?}", self.activity),
            "render_generation": self.scheduler.generation(),
            "stage": format!("{:?}", self.scheduler.stage()),
            "submitted_batches": self.scheduler.submitted_count,
            "completed_batches": self.scheduler.completed_count,
            "pixel_budget": self.pixel_budget,
            "preview_size": self.targets.as_ref().map(|targets| targets.preview_size),
            "interleaved": self.interleaved,
            "interacting": self.interacting,
            "refinement_paused": self.refinement_paused,
            "deferred": self.targets.as_ref().is_some_and(|targets| targets.deferred.is_some()),
        })
    }

    /// Avoid queuing presentation frames behind an unfinished scene batch.
    /// Polling delivers completion callbacks without waiting for the GPU.
    pub fn gpu_work_pending(&mut self, device: &wgpu::Device) -> bool {
        let _ = device.poll(wgpu::PollType::Poll);
        self.scheduler.retire(self.target_ms);
        if self.scheduler.busy() { self.activity = Activity::Gpu; }
        self.scheduler.busy()
    }

    pub fn set_repaint_callback(&mut self, repaint: impl Fn() + Send + Sync + 'static) {
        self.repaint = Some(std::sync::Arc::new(repaint));
    }
    pub fn gpu_fault(&self) -> Option<&'static str> { self.scheduler.gpu_fault() }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        key: ViewKey,
        refine: bool,
        deferred: bool,
        interleaved: bool,
    ) -> Work {
        if deferred && self.bloom.emission {
            let pipeline = if self.bloom.enabled {
                &mut self.deferred_bloom_pipeline
            } else {
                &mut self.deferred_emission_pipeline
            };
            pipeline.get_or_insert_with(|| {
                bloom::deferred_pipeline(
                    device,
                    &self.deferred_pipeline_layout,
                    self.format,
                    self.bloom.enabled,
                )
            });
        }
        if self.interleaved != interleaved {
            self.key = None;
        }
        self.interleaved = interleaved;
        if !refine && !interleaved {
            self.completed = 0;
        }
        // Poll is nonblocking. WebGPU delivers map callbacks through its event loop.
        let _ = device.poll(wgpu::PollType::Poll);
        self.scheduler.retire(self.target_ms);
        if self.scheduler.gpu_fault().is_some() { return Work::Cached; }
        if self.scheduler.busy() { self.activity = Activity::Gpu; return Work::Cached; }
        // An encoder can be dropped before queue submission. Roll back its
        // unpublished cursor instead of waiting for a fence that cannot exist.
        if let Some(abandoned) = self.scheduler.abandon_encoded() {
            match abandoned {
                Work::Preview | Work::Relight { .. } => { self.key = None; self.preview_ready = false; },
                Work::Refine { first, .. } | Work::Interleave { first, .. } => self.completed = first,
                Work::Cached => {}
            }
        }
        if self.key.as_ref() != Some(&key) || self.refinement_region_changed {
            self.refinement_region_changed = false;
            let preview_override = self.preview_budget_override.take();
            if preview_override.is_none() {
                self.preview_step = 0;
            }
            self.scheduler.new_generation();
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
            // Empty background tiles can teach a very large throughput budget.
            // A different angle must not inherit that budget for its first preview.
            self.pixel_budget = preview_override
                .unwrap_or_else(|| self.scheduler.budget(Stage::Preview, self.initial_budget));
            let mut preview_size = if refine || interleaved {
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
                    targets.preview_size != preview_size
                        || targets.contact.is_some() != self.contact_pipeline.is_some()
                        || targets.deferred.is_some() != deferred
                        || targets.bloom.is_some() != self.bloom.enabled
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
                let bloom = self.bloom.enabled.then(|| {
                    self.bloom
                        .targets(device, &self.layout, &self.progress, key.size, preview_size)
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
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(
                                bloom
                                    .as_ref()
                                    .map_or(&self.bloom.empty, |targets| &targets.result),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: self.bloom.uniform.as_entire_binding(),
                        },
                    ],
                });
                self.targets = Some(Targets {
                    contact: self.contact_pipeline.as_ref().map(|_| ContactTarget::new(device, &self.contact_layout, key.size)),
                    preview,
                    refined,
                    preview_depth,
                    refined_depth,
                    composite,
                    deferred,
                    bloom,
                    preview_size,
                });
            }
            self.key = Some(key.clone());
            self.completed = 0;
            self.preview_ready = false;
            self.activity = Activity::Preview;
            // Keep a complete coarse image available while sparse batches
            // fill the native target, even before the first phase finishes.
            return Work::Preview;
        }
        if !self.preview_ready { self.activity = Activity::Preview; return Work::Preview; }
        if !interleaved
            && self
                .targets
                .as_ref()
                .is_some_and(|targets| targets.preview_size == key.size)
        {
            self.activity = Activity::Complete;
            return Work::Cached;
        }
        if (!refine && !interleaved) || self.interacting || self.refinement_paused {
            self.activity = if self.interacting { Activity::Interaction } else if self.refinement_paused { Activity::Optimization } else { Activity::Complete };
            return Work::Cached;
        }
        // Hybrid scenes with imported splats cannot use the sparse SDF path:
        // each splat must be composited over a complete depth image. Improve
        // the whole view in a couple of bounded preview steps before native
        // tiles start replacing it.
        if self.progressive_hybrid_preview
            && self.rendering_path == Some(RenderingPath::Hybrid)
            && !interleaved
            && self.preview_step < 2
        {
            let current = self.targets.as_ref().expect("prepared viewport").preview_size;
            let current_pixels = current[0] * current[1];
            let native_pixels = key.size[0] * key.size[1];
            let next_budget = current_pixels.saturating_mul(2).min(self.initial_budget.saturating_mul(4));
            if next_budget < native_pixels && preview_size(key.size, next_budget) != current {
                self.preview_step += 1;
                self.preview_budget_override = Some(next_budget);
                self.refinement_region_changed = true;
                return self.prepare(device, key, refine, deferred, interleaved);
            }
        }
        let plan = Plan::new(if interleaved { self.sparse_size(key.size) } else { key.size }, interleaved);
        let total = plan.total();
        if self.completed >= total {
            self.activity = Activity::Complete;
            return Work::Cached;
        }
        if self.refinement_resume_at.is_some_and(|deadline| web_time::Instant::now() < deadline) {
            self.activity = Activity::Pacing;
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
        self.activity = Activity::Ready;
        let stage = plan.stage(self.completed);
        let ceiling = match stage {
            Stage::Sparse => 1024,
            _ => self.initial_budget,
        };
        self.pixel_budget = self.scheduler.budget(stage, ceiling);
        plan.next(self.completed, self.pixel_budget).expect("incomplete plan has a ready batch")
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
        self.scheduler.gpu_fault().is_none() && self.preview_ready && !self.scheduler.busy() && self.key.as_ref().is_some_and(|key| {
            self.completed
                == if self.interleaved {
                    sparse_sample_count(self.sparse_size(key.size))
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

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn benchmark_bloom_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        let targets = self.targets.as_ref().and_then(|targets| targets.bloom.as_ref()).expect("prepared bloom targets");
        self.bloom.encode(encoder, targets, None);
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

fn sparse_phase_pixels(size: [u32; 2]) -> u32 {
    size[0].div_ceil(16) * size[1].div_ceil(16)
}

fn sparse_sample_count(size: [u32; 2]) -> u32 {
    Plan::new(size, true).total()
}

fn adjusted_budget(previous: u32, milliseconds: f64, target_ms: f64, minimum: u32) -> u32 {
    if !milliseconds.is_finite() || milliseconds <= 0.0 {
        return previous;
    }
    // React quickly to expensive views, grow cautiously, and cap the minimum
    // batch at one tile or one sparse phase. Timings come from the GPU.
    let ratio = (target_ms / milliseconds).max(0.25);
    ((previous as f64 * ratio) as u32).clamp(minimum.max(1).min(4 * 1024 * 1024), 4 * 1024 * 1024)
}

fn budget_after_sample(
    budget: u32,
    submitted_budget: u32,
    rendered_pixels: u32,
    milliseconds: f64,
    target_ms: f64,
    minimum: u32,
) -> u32 {
    // The final refinement batch can be just a few edge pixels. Its fixed GPU
    // overhead says nothing about the cost of a full batch, so retain the
    // learned budget for the next camera or selection change.
    if rendered_pixels < submitted_budget / 2 {
        return budget;
    }
    adjusted_budget(rendered_pixels, milliseconds, target_ms, minimum)
        .min(budget.saturating_mul(6).div_ceil(5).max(minimum))
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

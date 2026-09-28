//! Cache completed images and refine expensive scenes in bounded GPU batches.
//! UI rendering stays at native resolution throughout camera and object edits.
use std::sync::{Arc, Mutex};

const TILE: u32 = 32;
const INITIAL_PIXELS: u32 = 48 * 1024;
const TARGET_MS: f64 = 6.0;

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
            Work::Refine { .. } | Work::Relight { native: true } => {
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

// Screen-space effects resolve only against a complete geometry image.
fn resolve_deferred(work: Work, size: [u32; 2]) -> bool {
    match work {
        Work::Preview | Work::Relight { .. } => true,
        Work::Refine { end, .. } => end == tile_count(size),
        Work::Cached => false,
    }
}

fn displayed_tiles(completed: u32, size: [u32; 2], deferred: bool) -> u32 {
    if deferred && completed < tile_count(size) {
        0
    } else {
        completed
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Work {
    Preview,
    Refine { first: u32, end: u32 },
    Relight { native: bool },
    Cached,
}

pub struct HybridPass<'a> {
    pub depth_pipeline: &'a wgpu::RenderPipeline,
    pub splat_pipeline: &'a wgpu::RenderPipeline,
    pub splat_bind_group: &'a wgpu::BindGroup,
    pub splat_buffer: &'a wgpu::Buffer,
    pub splat_count: u32,
}

pub struct DeferredPass<'a> {
    pub geometry_pipeline: &'a wgpu::RenderPipeline,
}

pub struct Viewport {
    key: Option<ViewKey>,
    targets: Option<Targets>,
    completed: u32,
    pixel_budget: u32,
    initial_budget: u32,
    pending: bool,
    pending_frames: u32,
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
            pixel_budget: INITIAL_PIXELS,
            initial_budget: INITIAL_PIXELS,
            pending: false,
            pending_frames: 0,
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

    pub fn invalidate(&mut self) {
        self.key = None;
        self.completed = 0;
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        key: ViewKey,
        refine: bool,
        deferred: bool,
    ) -> Work {
        if !refine {
            self.completed = 0;
        }
        // Poll is nonblocking. WebGPU delivers map callbacks through its event loop.
        let _ = device.poll(wgpu::PollType::Poll);
        if let Some(milliseconds) = self.timing.lock().unwrap().take() {
            self.pending = false;
            if let Some(ms) = milliseconds.filter(|_| self.submitted_pixels > 0) {
                self.pixel_budget = budget_after_sample(
                    self.pixel_budget,
                    self.submitted_budget,
                    self.submitted_pixels,
                    ms,
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
            let mut preview_size = if refine {
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
            self.key = Some(key);
            self.completed = 0;
            return Work::Preview;
        }
        if self
            .targets
            .as_ref()
            .is_some_and(|targets| targets.preview_size == key.size)
        {
            return Work::Cached;
        }
        if !refine {
            return Work::Cached;
        }
        let total = tile_count(key.size);
        if self.completed >= total {
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
        let count = (self.pixel_budget.saturating_mul(2) / (TILE * TILE)).max(1);
        Work::Refine {
            first: self.completed,
            end: (self.completed + count).min(total),
        }
    }

    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        work: Work,
        scene: &wgpu::RenderPipeline,
        scene_bind_group: &wgpu::BindGroup,
        hybrid: Option<HybridPass<'_>>,
        deferred: Option<DeferredPass<'_>>,
    ) -> Option<wgpu::Buffer> {
        // A material pipeline change can invalidate the view while a previous
        // GPU timing query is still pending. prepare() then returns Cached
        // before installing a new key; there is no frame to encode yet.
        let Some(targets) = self.targets.as_ref() else {
            return None;
        };
        let Some(size) = self.key.as_ref().map(|key| key.size) else {
            return None;
        };
        if work != Work::Cached {
            let view = if matches!(work, Work::Preview | Work::Relight { native: false }) {
                &targets.preview
            } else {
                &targets.refined
            };
            let depth_view = if matches!(work, Work::Preview | Work::Relight { native: false }) {
                &targets.preview_depth
            } else {
                &targets.refined_depth
            };
            if let Some(deferred) = deferred
                .as_ref()
                .filter(|_| !matches!(work, Work::Relight { .. }))
            {
                let deferred_targets = targets
                    .deferred
                    .as_ref()
                    .expect("deferred targets")
                    .for_work(work);
                let clear = matches!(work, Work::Preview | Work::Refine { first: 0, .. });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("deferred SDF geometry"),
                    color_attachments: &deferred_targets
                        .buffers
                        .iter()
                        .map(|buffer| {
                            Some(wgpu::RenderPassColorAttachment {
                                view: buffer,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: if clear {
                                        wgpu::LoadOp::Clear(wgpu::Color {
                                            r: 0.0,
                                            g: 0.0,
                                            b: 0.0,
                                            a: -1.0,
                                        })
                                    } else {
                                        wgpu::LoadOp::Load
                                    },
                                    store: wgpu::StoreOp::Store,
                                },
                            })
                        })
                        .collect::<Vec<_>>(),
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: if clear {
                                wgpu::LoadOp::Clear(1.0)
                            } else {
                                wgpu::LoadOp::Load
                            },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: self.query_set.as_ref().map(|query_set| {
                        wgpu::RenderPassTimestampWrites {
                            query_set,
                            beginning_of_pass_write_index: Some(0),
                            end_of_pass_write_index: (!resolve_deferred(work, size)).then_some(1),
                        }
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(deferred.geometry_pipeline);
                pass.set_bind_group(0, scene_bind_group, &[]);
                draw_work(&mut pass, work, size);
            }
            if deferred.is_none() {
                if let Some(hybrid) = &hybrid {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("opaque SDF depth batch"),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: self.query_set.as_ref().map(|query_set| {
                            wgpu::RenderPassTimestampWrites {
                                query_set,
                                beginning_of_pass_write_index: Some(0),
                                end_of_pass_write_index: None,
                            }
                        }),
                        ..Default::default()
                    });
                    pass.set_pipeline(hybrid.depth_pipeline);
                    pass.set_bind_group(0, scene_bind_group, &[]);
                    draw_work(&mut pass, work, size);
                }
            }
            if deferred.is_none() || resolve_deferred(work, size) {
                let shading_work = if deferred.is_some() {
                    Work::Preview
                } else {
                    work
                };
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("budgeted scene batch"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        timestamp_writes: self
                            .query_set
                            .as_ref()
                            .filter(|_| hybrid.is_none() || matches!(work, Work::Relight { .. }))
                            .map(|query_set| wgpu::RenderPassTimestampWrites {
                                query_set,
                                beginning_of_pass_write_index: (deferred.is_none()
                                    || matches!(work, Work::Relight { .. }))
                                .then_some(0),
                                end_of_pass_write_index: hybrid.is_none().then_some(1),
                            }),
                        ..Default::default()
                    });
                    pass.set_pipeline(if deferred.is_some() {
                        &self.deferred_pipeline
                    } else {
                        scene
                    });
                    pass.set_bind_group(0, scene_bind_group, &[]);
                    if deferred.is_some() {
                        pass.set_bind_group(
                            1,
                            &targets
                                .deferred
                                .as_ref()
                                .expect("deferred targets")
                                .for_work(work)
                                .bind_group,
                            &[],
                        );
                    }
                    draw_work(&mut pass, shading_work, size);
                }
                if let Some(hybrid) = &hybrid {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Gaussian splat mesh batch"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: self.query_set.as_ref().map(|query_set| {
                            wgpu::RenderPassTimestampWrites {
                                query_set,
                                beginning_of_pass_write_index: None,
                                end_of_pass_write_index: Some(1),
                            }
                        }),
                        ..Default::default()
                    });
                    pass.set_pipeline(hybrid.splat_pipeline);
                    pass.set_bind_group(0, hybrid.splat_bind_group, &[]);
                    pass.set_vertex_buffer(0, hybrid.splat_buffer.slice(..));
                    draw_splat_work(&mut pass, shading_work, size, hybrid.splat_count);
                }
            }
            if let Work::Refine { end, .. } = work {
                self.completed = end;
            }
        }
        queue.write_buffer(
            &self.progress,
            0,
            bytemuck::cast_slice(&[
                size[0],
                size[1],
                TILE,
                displayed_tiles(self.completed, size, deferred.is_some()),
            ]),
        );
        if work == Work::Cached {
            return None;
        }
        self.submitted_pixels = match work {
            Work::Preview => targets.preview_size[0] * targets.preview_size[1],
            Work::Refine { first, end } => (first..end)
                .map(|tile| {
                    let rect = tile_rect(size, tile);
                    rect[2] * rect[3]
                })
                .sum(),
            Work::Relight { .. } | Work::Cached => 0,
        };
        self.submitted_budget = match work {
            Work::Refine { .. } => self.pixel_budget.saturating_mul(2),
            Work::Preview => self.pixel_budget,
            Work::Relight { .. } | Work::Cached => 0,
        };
        self.pending = true;
        if let (Some(query_set), Some(resolve)) = (&self.query_set, &self.query_resolve) {
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("viewport timestamp readback"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.resolve_query_set(query_set, 0..2, resolve, 0);
            encoder.copy_buffer_to_buffer(resolve, 0, &readback, 0, 16);
            Some(readback)
        } else {
            None
        }
    }

    pub fn submitted(&self, queue: &wgpu::Queue, work: Work, readback: Option<wgpu::Buffer>) {
        if work == Work::Cached {
            return;
        }
        let timing = self.timing.clone();
        if let Some(buffer) = readback {
            let period = queue.get_timestamp_period() as f64;
            let mapped = buffer.clone();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let milliseconds = result.ok().and_then(|_| {
                        let view = mapped.slice(..).get_mapped_range().ok()?;
                        let values: &[u64] = bytemuck::cast_slice(&view);
                        Some(values[1].saturating_sub(values[0]) as f64 * period / 1_000_000.0)
                    });
                    mapped.unmap();
                    *timing.lock().unwrap() = Some(milliseconds);
                });
        } else {
            queue.on_submitted_work_done(move || *timing.lock().unwrap() = Some(None));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn preview_pixels(&self) -> u32 {
        self.targets.as_ref().map_or(0, |targets| {
            targets.preview_size[0] * targets.preview_size[1]
        })
    }

    pub fn is_refined(&self) -> bool {
        self.key.as_ref().is_some_and(|key| {
            self.completed == tile_count(key.size)
                || self
                    .targets
                    .as_ref()
                    .is_some_and(|targets| targets.preview_size == key.size)
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

fn draw_work(pass: &mut wgpu::RenderPass<'_>, work: Work, size: [u32; 2]) {
    match work {
        Work::Preview => pass.draw(0..3, 0..1),
        Work::Refine { first, end } => {
            let columns = size[0].div_ceil(TILE);
            let mut tile = first;
            while tile < end {
                let row_end = ((tile / columns + 1) * columns).min(end);
                let [x, y, _, height] = tile_rect(size, tile);
                let last = tile_rect(size, row_end - 1);
                pass.set_scissor_rect(x, y, last[0] + last[2] - x, height);
                pass.draw(0..3, 0..1);
                tile = row_end;
            }
        }
        Work::Relight { .. } | Work::Cached => unreachable!(),
    }
}

fn draw_splat_work(pass: &mut wgpu::RenderPass<'_>, work: Work, size: [u32; 2], count: u32) {
    match work {
        Work::Preview => pass.draw(0..6, 0..count),
        Work::Refine { first, end } => {
            let columns = size[0].div_ceil(TILE);
            let mut tile = first;
            while tile < end {
                let row_end = ((tile / columns + 1) * columns).min(end);
                let [x, y, _, height] = tile_rect(size, tile);
                let last = tile_rect(size, row_end - 1);
                pass.set_scissor_rect(x, y, last[0] + last[2] - x, height);
                pass.draw(0..6, 0..count);
                tile = row_end;
            }
        }
        Work::Relight { .. } | Work::Cached => unreachable!(),
    }
}

fn adjusted_budget(previous: u32, milliseconds: f64) -> u32 {
    if !milliseconds.is_finite() || milliseconds <= 0.0 {
        return previous;
    }
    // React quickly to expensive views, grow cautiously, and cap the minimum
    // batch at one tile. Timings come from the GPU, not the vsync interval.
    let ratio = (TARGET_MS / milliseconds).max(0.25);
    ((previous as f64 * ratio) as u32).clamp(TILE * TILE, 4 * 1024 * 1024)
}

fn budget_after_sample(
    budget: u32,
    submitted_budget: u32,
    rendered_pixels: u32,
    milliseconds: f64,
) -> u32 {
    // The final refinement batch can be just a few edge pixels. Its fixed GPU
    // overhead says nothing about the cost of a full batch, so retain the
    // learned budget for the next camera or selection change.
    if rendered_pixels < submitted_budget / 2 {
        return budget;
    }
    adjusted_budget(rendered_pixels, milliseconds).min(budget.saturating_mul(6) / 5)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deferred_lighting_revision_reuses_geometry_but_edits_do_not() {
        let key = ViewKey {
            matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
            position: [0.0; 3],
            projection: 0,
            versions: [1, 1],
            size: [333, 217],
            refine: true,
        };
        let mut changed = key.clone();
        changed.versions[1] += 1;
        assert!(same_geometry(&key, &changed));
        changed.versions[0] += 1;
        assert!(!same_geometry(&key, &changed));
        for change in 0..5 {
            let mut changed = key.clone();
            match change {
                0 => changed.matrix[0][0] += 0.1,
                1 => changed.position[0] += 0.1,
                2 => changed.projection = 1,
                3 => changed.size[0] += 1,
                _ => changed.refine = false,
            }
            assert!(!same_geometry(&key, &changed));
        }
    }

    #[test]
    fn deferred_effects_wait_for_complete_native_geometry() {
        let size = [333, 217];
        let total = tile_count(size);
        assert!(resolve_deferred(Work::Preview, size));
        assert!(!resolve_deferred(Work::Cached, size));
        for end in 1..=total {
            let batch = Work::Refine {
                first: end - 1,
                end,
            };
            assert_eq!(resolve_deferred(batch, size), end == total);
            assert_eq!(
                displayed_tiles(end, size, true),
                if end == total { total } else { 0 }
            );
            // Exact rendering can publish each independent completed tile.
            assert_eq!(displayed_tiles(end, size, false), end);
        }
    }

    #[test]
    fn tiles_cover_odd_sized_viewports_exactly_once() {
        for size in [[1, 1], [63, 65], [1280, 720], [333, 217]] {
            let mut coverage = vec![0u8; (size[0] * size[1]) as usize];
            for tile in 0..tile_count(size) {
                let [x, y, w, h] = tile_rect(size, tile);
                for row in y..y + h {
                    for column in x..x + w {
                        coverage[(row * size[0] + column) as usize] += 1;
                    }
                }
            }
            assert!(coverage.iter().all(|&count| count == 1));
        }
    }
    #[test]
    fn preview_respects_budget_and_never_exceeds_native_size() {
        for size in [[1, 2000], [2000, 1], [1280, 720], [333, 217]] {
            for budget in [4096, INITIAL_PIXELS, 4 * 1024 * 1024] {
                let preview = preview_size(size, budget);
                assert!(preview[0] > 0 && preview[1] > 0);
                assert!(preview[0] <= size[0] && preview[1] <= size[1]);
                assert!(preview[0] * preview[1] <= budget);
            }
        }
    }
    #[test]
    fn gpu_budget_recovers_from_expensive_views_without_vsync_feedback() {
        assert!(adjusted_budget(INITIAL_PIXELS, 35.0) < INITIAL_PIXELS);
        assert!(adjusted_budget(INITIAL_PIXELS, 2.0) > INITIAL_PIXELS);
        assert_eq!(adjusted_budget(INITIAL_PIXELS, f64::NAN), INITIAL_PIXELS);
        assert_eq!(adjusted_budget(TILE * TILE, 100.0), TILE * TILE);
    }

    #[test]
    fn final_partial_tile_keeps_the_budget_for_the_next_view() {
        let budget = 16 * 1024;
        assert_eq!(budget_after_sample(budget, budget * 2, 288, 1.0), budget);
        assert!(budget_after_sample(budget, budget * 2, budget * 2, 20.0) < budget);
    }
}

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

#[cfg(test)]
#[test]
fn deferred_shader_modules_validate() {
    let source = deferred_shader_source();
    let module = wgpu::naga::front::wgsl::parse_str(&source)
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate deferred lighting and transmission");
}

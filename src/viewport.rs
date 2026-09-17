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
    pub versions: [i32; 2],
    pub size: [u32; 2],
}

struct Targets {
    preview: wgpu::TextureView,
    refined: wgpu::TextureView,
    composite: wgpu::BindGroup,
    preview_size: [u32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Work {
    Preview,
    Refine { first: u32, end: u32 },
    Cached,
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
    query_set: Option<wgpu::QuerySet>,
    query_resolve: Option<wgpu::Buffer>,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    progress: wgpu::Buffer,
    format: wgpu::TextureFormat,
}

impl Viewport {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, timestamps: bool) -> Self {
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
        Self {
            key: None,
            targets: None,
            completed: 0,
            pixel_budget: INITIAL_PIXELS,
            initial_budget: INITIAL_PIXELS,
            pending: false,
            pending_frames: 0,
            submitted_pixels: INITIAL_PIXELS,
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

    pub fn prepare(&mut self, device: &wgpu::Device, key: ViewKey) -> Work {
        // Poll is nonblocking. WebGPU delivers map callbacks through its event loop.
        let _ = device.poll(wgpu::PollType::Poll);
        if let Some(milliseconds) = self.timing.lock().unwrap().take() {
            self.pending = false;
            if let Some(ms) = milliseconds {
                self.pixel_budget = adjusted_budget(self.submitted_pixels, ms)
                    .min(self.pixel_budget.saturating_mul(6) / 5);
            }
        }
        if self.pending {
            self.pending_frames += 1;
            // Timestamp queries are optional in WebGPU. Without them, prevent
            // multi-frame GPU backlog by reducing the next batch conservatively.
            if self.query_set.is_none() && self.pending_frames == 2 {
                self.pixel_budget = (self.pixel_budget / 2).max(TILE * TILE);
            }
            return Work::Cached;
        }
        self.pending_frames = 0;
        if self.key.as_ref() != Some(&key) {
            // Object transforms invalidate the image, not the GPU's measured throughput.
            // Keep the learned budget while editing. More expensive scene classes
            // still lower it through set_initial_budget, and GPU timings adapt per batch.
            let mut preview_size = preview_size(key.size, self.pixel_budget);
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
                || self
                    .targets
                    .as_ref()
                    .is_none_or(|targets| targets.preview_size != preview_size);
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
                    composite,
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
        let total = tile_count(key.size);
        if self.completed >= total {
            return Work::Cached;
        }
        let count = (self.pixel_budget / (TILE * TILE)).max(1);
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
    ) -> Option<wgpu::Buffer> {
        let targets = self.targets.as_ref().expect("prepared viewport");
        let size = self.key.as_ref().unwrap().size;
        if work != Work::Cached {
            let view = if work == Work::Preview {
                &targets.preview
            } else {
                &targets.refined
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
                    timestamp_writes: self.query_set.as_ref().map(|query_set| {
                        wgpu::RenderPassTimestampWrites {
                            query_set,
                            beginning_of_pass_write_index: Some(0),
                            end_of_pass_write_index: Some(1),
                        }
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(scene);
                pass.set_bind_group(0, scene_bind_group, &[]);
                match work {
                    Work::Preview => pass.draw(0..3, 0..1),
                    Work::Refine { first, end } => {
                        // Adjacent tiles in a row share one draw, preserving
                        // the pixel budget while avoiding tiny GPU dispatches.
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
                        self.completed = end;
                    }
                    Work::Cached => unreachable!(),
                }
            }
        }
        queue.write_buffer(
            &self.progress,
            0,
            bytemuck::cast_slice(&[size[0], size[1], TILE, self.completed]),
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
            Work::Cached => 0,
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn is_refined(&self) -> bool {
        self.key.as_ref().is_some_and(|key| {
            self.completed == tile_count(key.size)
                || self
                    .targets
                    .as_ref()
                    .is_some_and(|targets| targets.preview_size == key.size)
        })
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

fn adjusted_budget(previous: u32, milliseconds: f64) -> u32 {
    if !milliseconds.is_finite() || milliseconds <= 0.0 {
        return previous;
    }
    // React quickly to expensive views, grow cautiously, and cap the minimum
    // batch at one tile. Timings come from the GPU, not the vsync interval.
    let ratio = (TARGET_MS / milliseconds).max(0.25);
    ((previous as f64 * ratio) as u32).clamp(TILE * TILE, 4 * 1024 * 1024)
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
}

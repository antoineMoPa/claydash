use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    camera::{Camera, ProjectionMode},
    model::{SdfObject, SdfParams},
};

#[cfg(not(target_arch = "wasm32"))]
mod benchmark;

const MAX_OBJECTS: usize = 1024;
const MAX_BVH_NODES: usize = MAX_OBJECTS * 2 - 1;
const BVH_LEAF: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuCamera {
    inverse_view_projection: [[f32; 4]; 4],
    position: [f32; 4],
    count: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuObject {
    meta: [i32; 4],
    color: [f32; 4],
    inverse_rows: [[f32; 4]; 3],
    params: [f32; 4],
    material: [f32; 4],
    repeat_spacing: [f32; 4],
    repeat_count: [i32; 4],
    component: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuBvhNode {
    center_radius: [f32; 4],
    // A leaf stores its object index in x. Internal nodes use BVH_LEAF.
    // y is the first node after this subtree, enabling stackless traversal.
    metadata: [u32; 4],
    aabb_min: [f32; 4],
    aabb_max: [f32; 4],
}

#[derive(Clone, Copy)]
struct ObjectBound {
    center: Vec3,
    radius: f32,
    object_index: u32,
    half_extent: Vec3,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    boolean_pipeline: Option<(u32, wgpu::RenderPipeline)>,
    shader_source: String,
    pipeline_layout: wgpu::PipelineLayout,
    use_bvh: bool,
    node_count: u32,
    has_booleans: bool,
    bind_group: wgpu::BindGroup,
    camera_buffer: wgpu::Buffer,
    objects_buffer: wgpu::Buffer,
    bvh_buffer: wgpu::Buffer,
    uploaded_scene_versions: [i32; 2],
    egui_renderer: egui_wgpu::Renderer,
    viewport: crate::viewport::Viewport,
    initial_pixel_budget: u32,
}

impl Renderer {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preview_pixels(&self) -> u32 {
        self.viewport.preview_pixels()
    }

    pub async fn new(window: Arc<Window>, use_bvh: bool, uncapped: bool) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("find GPU adapter");
        if uncapped {
            eprintln!("GPU: {:?}", adapter.get_info());
        }
        let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: if timestamps {
                    wgpu::Features::TIMESTAMP_QUERY
                } else {
                    wgpu::Features::empty()
                },
                ..Default::default()
            })
            .await
            .expect("create GPU device");
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(capabilities.formats[0]);
        let present_mode = if uncapped {
            capabilities
                .present_modes
                .iter()
                .copied()
                .find(|mode| *mode == wgpu::PresentMode::Immediate)
                .unwrap_or(wgpu::PresentMode::AutoNoVsync)
        } else {
            wgpu::PresentMode::Fifo
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<GpuCamera>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let objects_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("objects"),
            size: (MAX_OBJECTS * std::mem::size_of::<GpuObject>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bvh_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene bvh"),
            size: (MAX_BVH_NODES * std::mem::size_of::<GpuBvhNode>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene bind group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: objects_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: bvh_buffer.as_entire_binding(),
                },
            ],
        });
        let shader_source = include_str!("../assets/shaders/sdf.wgsl").to_owned();
        #[cfg(not(target_arch = "wasm32"))]
        let shader_source = if uncapped {
            std::env::args()
                .find_map(|arg| arg.strip_prefix("--benchmark-shader=").map(str::to_owned))
                .map(|path| std::fs::read_to_string(path).expect("read benchmark reference shader"))
                .unwrap_or(shader_source)
        } else {
            shader_source
        };
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sdf pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = create_scene_pipeline(
            &device,
            &shader_source,
            &pipeline_layout,
            format,
            use_bvh,
            1,
        );
        let egui_renderer =
            egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        let viewport = crate::viewport::Viewport::new(&device, format, timestamps);
        Self {
            viewport,
            initial_pixel_budget: 48 * 1024,
            surface,
            device,
            queue,
            config,
            pipeline,
            boolean_pipeline: None,
            shader_source,
            pipeline_layout,
            use_bvh,
            node_count: 0,
            has_booleans: false,
            bind_group,
            camera_buffer,
            objects_buffer,
            bvh_buffer,
            uploaded_scene_versions: [i32::MIN; 2],
            egui_renderer,
        }
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(self.config.width as f32, self.config.height as f32)
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn render(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
        egui: &egui::Context,
        output: &mut egui::FullOutput,
    ) {
        self.upload_scene(camera, objects, selected, scene_versions);
        let clipped = egui.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        for (id, image_deltas) in output.textures_delta.set.drain() {
            for image_delta in image_deltas {
                self.egui_renderer
                    .update_texture(&self.device, &self.queue, id, &image_delta);
            }
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let callback_commands = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &clipped,
            &screen,
        );
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                self.free_textures(output);
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                self.free_textures(output);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let work = self.viewport.prepare(
            &self.device,
            crate::viewport::ViewKey {
                matrix: (camera.projection() * camera.view()).to_cols_array_2d(),
                position: camera.position.to_array(),
                projection: u32::from(camera.projection_mode == ProjectionMode::Orthographic),
                versions: scene_versions,
                size: [
                    camera.viewport.x.max(1.0) as u32,
                    camera.viewport.y.max(1.0) as u32,
                ],
            },
        );
        let scene_pipeline = if self.has_booleans {
            &self.boolean_pipeline.as_ref().expect("boolean pipeline").1
        } else {
            &self.pipeline
        };
        let readback = self.viewport.encode(
            &self.device,
            &self.queue,
            &mut encoder,
            work,
            scene_pipeline,
            &self.bind_group,
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(
                camera.viewport_origin.x,
                camera.viewport_origin.y,
                camera.viewport.x,
                camera.viewport.y,
                0.0,
                1.0,
            );
            self.viewport.composite(&mut pass);
        }
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut pass, &clipped, &screen);
        }
        self.queue
            .submit(callback_commands.into_iter().chain([encoder.finish()]));
        self.viewport.submitted(&self.queue, work, readback);
        self.free_textures(output);
        self.queue.present(frame);
    }

    fn upload_scene(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
    ) {
        let scene_changed = self.uploaded_scene_versions != scene_versions;
        let mut gpu_camera = GpuCamera {
            inverse_view_projection: (camera.projection() * camera.view())
                .inverse()
                .to_cols_array_2d(),
            position: camera.position.extend(0.0).to_array(),
            count: [
                objects.len().min(MAX_OBJECTS) as u32,
                self.node_count,
                u32::from(
                    objects
                        .iter()
                        .any(|object| object.operation.gpu_code() != 0),
                ),
                u32::from(camera.projection_mode == ProjectionMode::Orthographic),
            ],
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&gpu_camera));
        if !scene_changed {
            return;
        }

        // Children precede parents, so the fragment shader can evaluate the
        // boolean tree in one forward pass while preserving sibling order.
        let ordered = boolean_postorder(&objects[..objects.len().min(MAX_OBJECTS)]);
        let objects = &ordered;
        let selected_ids: std::collections::HashSet<_> = selected.iter().copied().collect();
        let object_indices: std::collections::HashMap<_, _> = objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.uuid, index as i32))
            .collect();

        let mut bounds = Vec::with_capacity(objects.len().min(MAX_OBJECTS));
        let mut gpu_objects: Vec<GpuObject> = objects
            .iter()
            .take(MAX_OBJECTS)
            .enumerate()
            .map(|(index, object)| {
                let abs_scale = object.transform.scale.abs();
                let distance_scale = abs_scale.min_element().max(0.000_001);
                let (params, radius) = match object.params {
                    SdfParams::SphereParams(ref params) => (
                        [params.radius, 0.0, 0.0, distance_scale],
                        params.radius * abs_scale.max_element(),
                    ),
                    SdfParams::BoxParams(ref params) => (
                        [
                            params.box_q.x,
                            params.box_q.y,
                            params.box_q.z,
                            distance_scale,
                        ],
                        (params.box_q * abs_scale).length(),
                    ),
                    SdfParams::CylinderParams {
                        radius,
                        half_height,
                    } => (
                        [radius, half_height, 0.0, distance_scale],
                        Vec2::new(radius, half_height).length() * abs_scale.max_element(),
                    ),
                    SdfParams::TorusParams {
                        major_radius,
                        minor_radius,
                    } => (
                        [major_radius, minor_radius, 0.0, distance_scale],
                        (major_radius + minor_radius) * abs_scale.max_element(),
                    ),
                };
                let repeated_radius = if object.repetition.enabled {
                    let extent = Vec3::from_array([
                        if object.repetition.axes[0] {
                            (object.repetition.count[0].saturating_sub(1)) as f32
                                * object.repetition.spacing.x
                                * 0.5
                        } else {
                            0.0
                        },
                        if object.repetition.axes[1] {
                            (object.repetition.count[1].saturating_sub(1)) as f32
                                * object.repetition.spacing.y
                                * 0.5
                        } else {
                            0.0
                        },
                        if object.repetition.axes[2] {
                            (object.repetition.count[2].saturating_sub(1)) as f32
                                * object.repetition.spacing.z
                                * 0.5
                        } else {
                            0.0
                        },
                    ]);
                    radius + extent.length() * abs_scale.max_element()
                } else {
                    radius
                };
                let local_extent = match object.params {
                    SdfParams::SphereParams(ref p) => Vec3::splat(p.radius),
                    SdfParams::BoxParams(ref p) => p.box_q,
                    SdfParams::CylinderParams {
                        radius,
                        half_height,
                    } => Vec3::new(radius, half_height, radius),
                    SdfParams::TorusParams {
                        major_radius,
                        minor_radius,
                    } => Vec3::new(
                        major_radius + minor_radius,
                        minor_radius,
                        major_radius + minor_radius,
                    ),
                };
                let mut repeated_extent = local_extent;
                if object.repetition.enabled {
                    for axis in 0..3 {
                        if object.repetition.axes[axis] {
                            repeated_extent[axis] += object.repetition.count[axis].saturating_sub(1)
                                as f32
                                * object.repetition.spacing[axis].max(0.001)
                                * 0.5;
                        }
                    }
                }
                let matrix = object.transform.matrix();
                let half_extent = matrix.x_axis.truncate().abs() * repeated_extent.x
                    + matrix.y_axis.truncate().abs() * repeated_extent.y
                    + matrix.z_axis.truncate().abs() * repeated_extent.z;
                bounds.push(ObjectBound {
                    half_extent,
                    center: object.transform.translation,
                    radius: repeated_radius,
                    object_index: index as u32,
                });
                GpuObject {
                    // Spare component lanes carry blend width and material kind.
                    component: [
                        0,
                        0,
                        object.softness.to_bits(),
                        u32::from(object.material.kind == crate::model::MaterialKind::Wood),
                    ],
                    meta: [
                        i32::from(selected_ids.contains(&object.uuid)),
                        object.object_type,
                        object.operation.gpu_code(),
                        object
                            .boolean_parent
                            .and_then(|parent| object_indices.get(&parent).copied())
                            .unwrap_or(-1),
                    ],
                    color: object.color.to_array(),
                    inverse_rows: inverse_affine_rows(object.transform.matrix().inverse()),
                    params,
                    material: [
                        object.material.roughness,
                        object.material.metallic,
                        object.material.reflectivity,
                        object.material.opacity,
                    ],
                    repeat_spacing: object
                        .repetition
                        .spacing
                        .extend(object.material.refractive_index)
                        .to_array(),
                    repeat_count: if object.repetition.enabled {
                        [
                            if object.repetition.axes[0] {
                                object.repetition.count[0] as i32
                            } else {
                                1
                            },
                            if object.repetition.axes[1] {
                                object.repetition.count[1] as i32
                            } else {
                                1
                            },
                            if object.repetition.axes[2] {
                                object.repetition.count[2] as i32
                            } else {
                                1
                            },
                            1,
                        ]
                    } else {
                        [1, 1, 1, 0]
                    },
                }
            })
            .collect();
        expand_soft_bounds(&gpu_objects, &mut bounds);
        let component_bounds = boolean_component_bounds(&gpu_objects, &bounds);
        let mut group_bounds = Vec::new();
        let mut group_start = 0;
        let mut capacity = 1;
        let mut starts = vec![0u32; objects.len()];
        for (index, object) in gpu_objects.iter().enumerate() {
            if object.meta[3] < 0 {
                if let Some(bound) = component_bounds[index] {
                    group_bounds.push(bound);
                }
                starts[index] = group_start as u32;
                capacity = capacity.max((index - group_start + 1).next_power_of_two() as u32);
                group_start = index + 1;
            }
        }
        for bound in &group_bounds {
            let root = bound.object_index as usize;
            let start = starts[root] as usize;
            for object in &mut gpu_objects[start..=root] {
                object.component[0] = start as u32;
                object.component[1] = root as u32;
            }
        }
        let mut bvh = build_bvh(&mut group_bounds);
        for node in &mut bvh {
            if node.metadata[0] != BVH_LEAF {
                node.metadata[2] = starts[node.metadata[0] as usize];
            }
        }
        self.node_count = bvh.len() as u32;
        gpu_camera.count[1] = self.node_count;
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&gpu_camera));
        // Specialize scratch storage to the largest component, not scene size.
        self.has_booleans = capacity > 1;
        let transmitted_instances: u64 = gpu_objects
            .iter()
            .filter(|object| object.material[3] < 0.999 && object.material[1] < 0.999)
            .map(|object| {
                object.repeat_count[..3]
                    .iter()
                    .map(|&count| count.max(1) as u64)
                    .fold(1u64, u64::saturating_mul)
            })
            .fold(0u64, u64::saturating_add);
        self.initial_pixel_budget =
            if (capacity > 1 && gpu_objects.len() > 256) || transmitted_instances > 1024 {
                1024
            } else if (capacity > 1 && gpu_objects.len() > 64) || transmitted_instances > 256 {
                4096
            } else if transmitted_instances > 64 {
                16 * 1024
            } else {
                48 * 1024
            };
        self.viewport.set_initial_budget(self.initial_pixel_budget);
        if self.has_booleans
            && self
                .boolean_pipeline
                .as_ref()
                .is_none_or(|(size, _)| *size != capacity)
        {
            self.boolean_pipeline = Some((
                capacity,
                create_scene_pipeline(
                    &self.device,
                    &self.shader_source,
                    &self.pipeline_layout,
                    self.config.format,
                    self.use_bvh,
                    capacity,
                ),
            ));
        }
        if !gpu_objects.is_empty() {
            self.queue
                .write_buffer(&self.objects_buffer, 0, bytemuck::cast_slice(&gpu_objects));
            self.queue
                .write_buffer(&self.bvh_buffer, 0, bytemuck::cast_slice(&bvh));
        }
        self.uploaded_scene_versions = scene_versions;
    }

    fn free_textures(&mut self, output: &mut egui::FullOutput) {
        for id in output.textures_delta.free.drain() {
            self.egui_renderer.free_texture(&id);
        }
    }
}

// Smooth unions lower distance by at most k/4 per operand. Expand all primitive
// bounds in a component by that total, accounting for anisotropic distance scale.
fn expand_soft_bounds(objects: &[GpuObject], bounds: &mut [ObjectBound]) {
    let mut start = 0;
    for (root, object) in objects.iter().enumerate() {
        if object.meta[3] >= 0 {
            continue;
        }
        let group = &objects[start..=root];
        let blend: f32 = group
            .iter()
            .filter(|o| o.meta[3] >= 0 && o.meta[2] == 0)
            .map(|o| f32::from_bits(o.component[2]).max(0.0) * 0.25)
            .sum();
        if blend > 0.0 {
            let ratio = group
                .iter()
                .map(|o| {
                    let smallest_inverse_scale = o
                        .inverse_rows
                        .iter()
                        .map(|row| Vec3::new(row[0], row[1], row[2]).length())
                        .fold(f32::INFINITY, f32::min);
                    1.0 / (smallest_inverse_scale * o.params[3]).max(0.000001)
                })
                .fold(1.0_f32, f32::max);
            for bound in &mut bounds[start..=root] {
                bound.half_extent += Vec3::splat(blend * ratio);
                bound.radius = bound.half_extent.length();
            }
        }
        start = root + 1;
    }
}

fn boolean_component_bounds(
    objects: &[GpuObject],
    bounds: &[ObjectBound],
) -> Vec<Option<ObjectBound>> {
    let mut result: Vec<_> = bounds.iter().copied().map(Some).collect();
    for (index, object) in objects.iter().enumerate() {
        let parent = object.meta[3];
        if parent < 0 {
            continue;
        }
        let parent = parent as usize;
        let child = result[index];
        let target = result[parent];
        result[parent] = match object.meta[2] {
            0 => match (target, child) {
                (Some(a), Some(b)) => {
                    let (minimum, maximum) = enclosing_aabb(&[a, b]);
                    let half_extent = (maximum - minimum) * 0.5;
                    Some(ObjectBound {
                        center: (minimum + maximum) * 0.5,
                        half_extent,
                        radius: half_extent.length(),
                        object_index: parent as u32,
                    })
                }
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(ObjectBound {
                    object_index: parent as u32,
                    ..b
                }),
                (None, None) => None,
            },
            1 => target, // Subtraction cannot extend the target's occupied volume.
            _ => match (target, child) {
                (Some(a), Some(b)) => {
                    let minimum = (a.center - a.half_extent).max(b.center - b.half_extent);
                    let maximum = (a.center + a.half_extent).min(b.center + b.half_extent);
                    let half_extent = (maximum - minimum) * 0.5;
                    (minimum.cmple(maximum).all()).then_some(ObjectBound {
                        center: (minimum + maximum) * 0.5,
                        half_extent,
                        radius: half_extent.length(),
                        object_index: parent as u32,
                    })
                }
                _ => None,
            },
        };
    }
    result
}

fn specialized_shader_source(source: &str, capacity: u32) -> String {
    source.replace(
        "const CSG_SIZE: u32 = 256u;",
        &format!("const CSG_SIZE: u32 = {capacity}u;"),
    )
}

fn create_scene_pipeline(
    device: &wgpu::Device,
    shader_source: &str,
    pipeline_layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    use_bvh: bool,
    capacity: u32,
) -> wgpu::RenderPipeline {
    let source = specialized_shader_source(shader_source, capacity);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("specialized SDF shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sdf pipeline"),
        layout: Some(pipeline_layout),
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
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[
                    ("USE_BVH", f64::from(use_bvh)),
                    ("HAS_BOOLEANS", f64::from(capacity > 1)),
                ],
                ..Default::default()
            },
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn build_bvh(bounds: &mut [ObjectBound]) -> Vec<GpuBvhNode> {
    let mut nodes = Vec::with_capacity(bounds.len().saturating_mul(2).saturating_sub(1));
    if !bounds.is_empty() {
        build_bvh_subtree(bounds, &mut nodes);
    }
    nodes
}

fn boolean_postorder(objects: &[SdfObject]) -> Vec<&SdfObject> {
    let indices: std::collections::HashMap<_, _> = objects
        .iter()
        .enumerate()
        .map(|(index, object)| (object.uuid, index))
        .collect();
    let mut children = vec![Vec::new(); objects.len()];
    let mut roots = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        if let Some(parent) = object
            .boolean_parent
            .and_then(|id| indices.get(&id).copied())
        {
            children[parent].push(index);
        } else {
            roots.push(index);
        }
    }
    let mut visited = vec![false; objects.len()];
    let mut result = Vec::with_capacity(objects.len());
    let mut pending = Vec::new();
    for root in roots.into_iter().chain(0..objects.len()) {
        pending.push((root, false));
        while let Some((index, complete)) = pending.pop() {
            if complete {
                result.push(&objects[index]);
                continue;
            }
            if visited[index] {
                continue;
            }
            visited[index] = true;
            pending.push((index, true));
            pending.extend(children[index].iter().rev().map(|&child| (child, false)));
        }
    }
    result
}

fn inverse_affine_rows(inverse: glam::Mat4) -> [[f32; 4]; 3] {
    let columns = inverse.to_cols_array_2d();
    [
        [columns[0][0], columns[1][0], columns[2][0], columns[3][0]],
        [columns[0][1], columns[1][1], columns[2][1], columns[3][1]],
        [columns[0][2], columns[1][2], columns[2][2], columns[3][2]],
    ]
}

fn build_bvh_subtree(bounds: &mut [ObjectBound], nodes: &mut Vec<GpuBvhNode>) {
    let node_index = nodes.len();
    let (center, radius) = enclosing_bound(bounds);
    let (minimum, maximum) = enclosing_aabb(bounds);
    nodes.push(GpuBvhNode {
        center_radius: center.extend(radius).to_array(),
        metadata: [BVH_LEAF, 0, 0, 0],
        aabb_min: minimum.extend(0.0).to_array(),
        aabb_max: maximum.extend(0.0).to_array(),
    });

    if bounds.len() == 1 {
        nodes[node_index].metadata = [bounds[0].object_index, (node_index + 1) as u32, 0, 0];
        return;
    }

    let centroid_min = bounds
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |value, bound| {
            value.min(bound.center)
        });
    let centroid_max = bounds
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |value, bound| {
            value.max(bound.center)
        });
    let extent = centroid_max - centroid_min;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    bounds.sort_unstable_by(|left, right| left.center[axis].total_cmp(&right.center[axis]));
    let middle = bounds.len() / 2;
    let (left, right) = bounds.split_at_mut(middle);
    build_bvh_subtree(left, nodes);
    build_bvh_subtree(right, nodes);
    nodes[node_index].metadata[1] = nodes.len() as u32;
}

fn enclosing_aabb(bounds: &[ObjectBound]) -> (Vec3, Vec3) {
    bounds.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), bound| {
            (
                lo.min(bound.center - bound.half_extent),
                hi.max(bound.center + bound.half_extent),
            )
        },
    )
}

fn enclosing_bound(bounds: &[ObjectBound]) -> (Vec3, f32) {
    let minimum = bounds
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |value, bound| {
            value.min(bound.center - Vec3::splat(bound.radius))
        });
    let maximum = bounds
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |value, bound| {
            value.max(bound.center + Vec3::splat(bound.radius))
        });
    let center = (minimum + maximum) * 0.5;
    let radius = bounds
        .iter()
        .map(|bound| center.distance(bound.center) + bound.radius)
        .fold(0.0, f32::max);
    (center, radius)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    #[test]
    fn postorder_gpu_contract_matches_recursive_boolean_evaluation() {
        use crate::model::{scene_sample, ui_preview_scene, BooleanOperation, PrimitiveKind};
        let mut scene = ui_preview_scene();
        let mut nested = SdfObject::create_kind(PrimitiveKind::Sphere);
        nested.boolean_parent = Some(scene[7].uuid);
        nested.operation = BooleanOperation::Subtract;
        nested.transform.translation = scene[7].transform.translation;
        scene.insert(0, nested); // Deliberately not stored in traversal order.
        let ordered = boolean_postorder(&scene);
        let parents: Vec<_> = ordered
            .iter()
            .enumerate()
            .map(|(index, object)| {
                object.boolean_parent.map(|id| {
                    let parent = ordered
                        .iter()
                        .position(|candidate| candidate.uuid == id)
                        .unwrap();
                    assert!(parent > index, "each child must precede its parent");
                    parent
                })
            })
            .collect();
        for x in -12..=12 {
            for y in -8..=8 {
                for z in -5..=5 {
                    let point = Vec3::new(x as f32, y as f32, z as f32) * 0.2;
                    let mut values: Vec<_> = ordered
                        .iter()
                        .map(|object| object.distance(point))
                        .collect();
                    let mut closest = f32::INFINITY;
                    for (index, object) in ordered.iter().enumerate() {
                        if let Some(parent) = parents[index] {
                            values[parent] = crate::model::boolean_distance(
                                values[parent],
                                values[index],
                                object.operation,
                                object.softness,
                            );
                        } else {
                            closest = closest.min(values[index]);
                        }
                    }
                    let expected = scene_sample(point, &scene).unwrap().0;
                    assert!(
                        (closest - expected).abs() < 0.00001,
                        "boolean mismatch at {point:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn boolean_bounds_follow_volume_semantics_and_empty_subtrees() {
        let bound = |x: f32, index| ObjectBound {
            center: Vec3::new(x, 0.0, 0.0),
            radius: 1.0,
            half_extent: Vec3::ONE,
            object_index: index,
        };
        let object = |parent, operation| {
            let mut object = GpuObject::zeroed();
            object.meta[2] = operation;
            object.meta[3] = parent;
            object
        };
        let bounds = [bound(5.0, 0), bound(0.0, 1)];
        let subtraction = boolean_component_bounds(&[object(1, 1), object(-1, 0)], &bounds);
        assert_eq!(subtraction[1].unwrap().center, Vec3::ZERO);
        assert_eq!(subtraction[1].unwrap().half_extent, Vec3::ONE);
        let intersection = boolean_component_bounds(&[object(1, 2), object(-1, 0)], &bounds);
        assert!(intersection[1].is_none());
        let union = boolean_component_bounds(&[object(1, 0), object(-1, 0)], &bounds);
        let union = union[1].unwrap();
        assert_eq!(union.center - union.half_extent, Vec3::splat(-1.0));
        assert_eq!(union.center + union.half_extent, Vec3::new(6.0, 1.0, 1.0));
        // The distant cutter becomes empty before subtraction from the root.
        let nested = boolean_component_bounds(
            &[object(1, 2), object(2, 1), object(-1, 0)],
            &[bound(5.0, 0), bound(0.0, 1), bound(10.0, 2)],
        );
        assert!(nested[1].is_none());
        assert_eq!(nested[2].unwrap().center.x, 10.0);
        assert_eq!(nested[2].unwrap().half_extent, Vec3::ONE);
    }

    #[test]
    fn soft_union_bounds_account_for_nonuniform_scale_and_nested_blends() {
        let mut operand = GpuObject::zeroed();
        operand.meta = [0, 1, 0, 2];
        operand.params[3] = 0.5;
        operand.inverse_rows = [
            [0.1, 0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        operand.component[2] = 0.2_f32.to_bits();
        let mut root = operand;
        root.meta[3] = -1;
        let bound = ObjectBound {
            center: Vec3::ZERO,
            radius: 1.0,
            half_extent: Vec3::ONE,
            object_index: 0,
        };
        let mut bounds = [bound; 3];
        expand_soft_bounds(&[operand, operand, root], &mut bounds);
        // Two k/4 contributions, amplified by max_scale/min_scale = 20.
        for bound in bounds {
            assert_eq!(bound.half_extent, Vec3::splat(3.0));
        }
    }

    #[test]
    fn postorder_keeps_orphan_components_and_sibling_order() {
        let mut root = SdfObject::create(sdf_consts::TYPE_SPHERE);
        root.boolean_parent = Some(uuid::Uuid::new_v4());
        let mut first = SdfObject::create(sdf_consts::TYPE_BOX);
        first.boolean_parent = Some(root.uuid);
        let mut second = first.clone();
        second.uuid = uuid::Uuid::new_v4();
        let scene = [root.clone(), first.clone(), second.clone()];
        let ordered: Vec<_> = boolean_postorder(&scene)
            .iter()
            .map(|object| object.uuid)
            .collect();
        assert_eq!(ordered, [first.uuid, second.uuid, root.uuid]);
    }

    #[test]
    fn material_and_boolean_shader_validates() {
        let source = include_str!("../assets/shaders/sdf.wgsl");
        let mut sources: Vec<_> = [1, 2, 4, 8, 16, 256, 1024]
            .into_iter()
            .map(|capacity| specialized_shader_source(source, capacity))
            .collect();
        sources.push(include_str!("../assets/shaders/viewport.wgsl").into());
        for source in sources {
            let module = wgpu::naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .expect("shader must validate for portable WebGPU");
        }
    }

    fn stress_bounds() -> Vec<ObjectBound> {
        let mut bounds = Vec::with_capacity(MAX_OBJECTS);
        for z in 0..4 {
            for y in 0..8 {
                for x in 0..8 {
                    bounds.push(ObjectBound {
                        center: Vec3::new(
                            (x as f32 - 3.5) * 1.25,
                            (y as f32 - 3.5) * 1.25,
                            (z as f32 - 1.5) * 1.25,
                        ),
                        radius: 0.35,
                        object_index: bounds.len() as u32,
                        half_extent: Vec3::splat(0.35),
                    });
                }
            }
        }
        bounds
    }

    fn bvh_sphere_distance(
        point: Vec3,
        nodes: &[GpuBvhNode],
        bounds: &[ObjectBound],
    ) -> (f32, usize) {
        let mut closest = 100.0;
        let mut leaf_visits = 0;
        let mut node_index = 0;
        while node_index < nodes.len() {
            let node = nodes[node_index];
            let center = Vec3::from_array(node.center_radius[..3].try_into().unwrap());
            let lower_bound = point.distance(center) - node.center_radius[3];
            if lower_bound < closest {
                if node.metadata[0] != BVH_LEAF {
                    let bound = bounds[node.metadata[0] as usize];
                    closest = closest.min(point.distance(bound.center) - bound.radius);
                    leaf_visits += 1;
                }
                node_index += 1;
            } else {
                node_index = node.metadata[1] as usize;
            }
        }
        (closest, leaf_visits)
    }

    #[test]
    fn bvh_stress_scene_matches_brute_force_and_prunes_work() {
        let mut sorted_bounds = stress_bounds();
        let original_bounds = sorted_bounds.clone();
        let nodes = build_bvh(&mut sorted_bounds);
        assert_eq!(nodes.len(), sorted_bounds.len() * 2 - 1);

        let samples = [
            Vec3::new(-4.0, -4.0, -2.0),
            Vec3::new(0.1, 0.2, 0.3),
            Vec3::new(3.8, 4.1, 2.2),
            Vec3::new(9.0, -7.0, 3.0),
        ];
        for point in samples {
            let brute_force = original_bounds
                .iter()
                .map(|bound| point.distance(bound.center) - bound.radius)
                .fold(100.0, f32::min);
            let (accelerated, leaf_visits) = bvh_sphere_distance(point, &nodes, &original_bounds);
            assert!((accelerated - brute_force).abs() < 0.000_01);
            assert!(
                leaf_visits < original_bounds.len() / 4,
                "visited {leaf_visits} leaves"
            );
        }
    }

    #[test]
    fn parent_bounds_enclose_every_stress_object() {
        let mut bounds = stress_bounds();
        let original_bounds = bounds.clone();
        let nodes = build_bvh(&mut bounds);
        let root = nodes[0];
        let center = Vec3::from_array(root.center_radius[..3].try_into().unwrap());
        let radius = root.center_radius[3];
        for bound in original_bounds {
            assert!(center.distance(bound.center) + bound.radius <= radius + 0.000_01);
        }
    }

    #[test]
    fn packed_inverse_rows_match_matrix_transformation() {
        let inverse = glam::Mat4::from_scale_rotation_translation(
            Vec3::new(1.3, 0.7, 2.1),
            glam::Quat::from_rotation_y(0.63),
            Vec3::new(2.0, -1.0, 4.0),
        )
        .inverse();
        let rows = inverse_affine_rows(inverse);
        let point = Vec3::new(-0.2, 3.4, 1.1);
        let homogeneous = point.extend(1.0);
        let packed = Vec3::new(
            Vec4::from_array(rows[0]).dot(homogeneous),
            Vec4::from_array(rows[1]).dot(homogeneous),
            Vec4::from_array(rows[2]).dot(homogeneous),
        );
        let expected = (inverse * homogeneous).truncate();
        assert!(packed.abs_diff_eq(expected, 0.000_001));
    }
}

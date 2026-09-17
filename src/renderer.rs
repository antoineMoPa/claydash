use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    camera::Camera,
    model::{SdfObject, SdfParams},
};

const MAX_OBJECTS: usize = 256;
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
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuBvhNode {
    center_radius: [f32; 4],
    // A leaf stores its object index in x. Internal nodes use BVH_LEAF.
    // y is the first node after this subtree, enabling stackless traversal.
    metadata: [u32; 4],
}

#[derive(Clone, Copy)]
struct ObjectBound {
    center: Vec3,
    radius: f32,
    object_index: u32,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    camera_buffer: wgpu::Buffer,
    objects_buffer: wgpu::Buffer,
    bvh_buffer: wgpu::Buffer,
    uploaded_scene_versions: [i32; 2],
    egui_renderer: egui_wgpu::Renderer,
}

impl Renderer {
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
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sdf shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../assets/shaders/sdf.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sdf pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sdf pipeline"),
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
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("USE_BVH", f64::from(use_bvh))],
                    ..Default::default()
                },
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let egui_renderer =
            egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        Self {
            surface,
            device,
            queue,
            config,
            pipeline,
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
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
        self.free_textures(output);
        self.queue.present(frame);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn benchmark_scene(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
    ) {
        use std::time::{Duration, Instant};

        self.upload_scene(camera, objects, selected, scene_versions);
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("SDF stress benchmark target"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let draw_batch = |pass_count: usize| {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            for _ in 0..pass_count {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("SDF stress benchmark draw"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            let start = Instant::now();
            let submission = self.queue.submit([encoder.finish()]);
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(Duration::from_secs(15)),
                })
                .expect("SDF stress benchmark exceeded its 15 second GPU timeout");
            start.elapsed()
        };

        draw_batch(1);
        let passes = 3;
        let elapsed = draw_batch(passes);
        let milliseconds = elapsed.as_secs_f64() * 1000.0 / passes as f64;
        eprintln!(
            "SDF benchmark: {milliseconds:.3} ms/frame ({:.1} FPS), {} objects, {}x{}",
            1000.0 / milliseconds,
            objects.len().min(MAX_OBJECTS),
            self.config.width,
            self.config.height,
        );
    }

    fn upload_scene(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
    ) {
        let scene_changed = self.uploaded_scene_versions != scene_versions;
        let node_count = MAX_BVH_NODES.min(
            objects
                .len()
                .min(MAX_OBJECTS)
                .saturating_mul(2)
                .saturating_sub(1),
        );
        let gpu_camera = GpuCamera {
            inverse_view_projection: (camera.projection() * camera.view())
                .inverse()
                .to_cols_array_2d(),
            position: camera.position.extend(0.0).to_array(),
            count: [
                objects.len().min(MAX_OBJECTS) as u32,
                node_count as u32,
                0,
                0,
            ],
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&gpu_camera));
        if !scene_changed {
            return;
        }

        let mut bounds = Vec::with_capacity(objects.len().min(MAX_OBJECTS));
        let gpu_objects: Vec<GpuObject> = objects
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
                };
                bounds.push(ObjectBound {
                    center: object.transform.translation,
                    radius,
                    object_index: index as u32,
                });
                GpuObject {
                    meta: [
                        i32::from(selected.contains(&object.uuid)),
                        object.object_type,
                        0,
                        0,
                    ],
                    color: object.color.to_array(),
                    inverse_rows: inverse_affine_rows(object.transform.matrix().inverse()),
                    params,
                }
            })
            .collect();
        let bvh = build_bvh(&mut bounds);
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

fn build_bvh(bounds: &mut [ObjectBound]) -> Vec<GpuBvhNode> {
    let mut nodes = Vec::with_capacity(bounds.len().saturating_mul(2).saturating_sub(1));
    if !bounds.is_empty() {
        build_bvh_subtree(bounds, &mut nodes);
    }
    nodes
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
    nodes.push(GpuBvhNode {
        center_radius: center.extend(radius).to_array(),
        metadata: [BVH_LEAF, 0, 0, 0],
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
        assert_eq!(nodes.len(), MAX_BVH_NODES);

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
                leaf_visits < MAX_OBJECTS / 4,
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

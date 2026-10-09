use super::*;
use wgpu::util::DeviceExt;

pub(super) fn create_mesh_background_pipeline(device: &wgpu::Device,
    layout: &wgpu::PipelineLayout, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let source = [include_str!("../../assets/shaders/scene_abi.wgsl"),
        include_str!("../../assets/shaders/poisson_background.wgsl"),
        include_str!("../../assets/shaders/environment.wgsl")].join("\n");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mesh-only background"), source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mesh-only background"), layout: Some(layout),
        vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"),
            buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None,
                write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
        multiview_mask: None, cache: None,
    })
}

pub(super) fn create_splat_resources(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
    wgpu::Buffer,
    wgpu::BindGroup,
    wgpu::Buffer,
    wgpu::Buffer,
) {
    let camera = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("splat camera"),
        size: std::mem::size_of::<SplatCamera>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("splat camera layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("splat camera bind group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: camera.as_entire_binding(),
        }],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Gaussian splat quads"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../assets/shaders/splats.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("splat pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Gaussian splat mesh pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GpuSplat>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 32,
                        shader_location: 2,
                    },
                ],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("splat instances"),
        contents: bytemuck::bytes_of(&GpuSplat::zeroed()),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    });
    let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Poisson preview triangles"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../assets/shaders/poisson_mesh.wgsl").into()),
    });
    let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Poisson preview mesh pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &mesh_shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GpuMeshVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 1 },
                ],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &mesh_shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let mesh_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("empty Poisson preview mesh"),
        contents: bytemuck::bytes_of(&GpuMeshVertex::zeroed()),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    });
    (pipeline, mesh_pipeline, camera, bind_group, instances, mesh_buffer)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct GpuMeshVertex {
    pub position: [f32; 4],
    pub normal: [f32; 4],
    pub captured_color: [f32; 4],
}

impl Renderer {
    pub(super) fn upload_splat_camera(&mut self, camera: &Camera, world: World) {
        if !self.hybrid_enabled && self.mesh_vertex_count == 0 {
            return;
        }
        let view = camera.view().inverse();
        let sun = Vec3::from_array(world.sun_direction());
        let (direction, ambient, direct) = if world.background == crate::model::BackgroundMode::Sky
        {
            let daylight_position = ((sun.y + 0.18) / 0.34).clamp(0.0, 1.0);
            let daylight = daylight_position * daylight_position * (3.0 - 2.0 * daylight_position);
            (
                sun,
                0.035 + 0.165 * daylight,
                world.sun_intensity * daylight_position,
            )
        } else if world.background == crate::model::BackgroundMode::NightSky {
            (Vec3::new(2.0, 3.0, 2.0).normalize(), 0.08, 0.12)
        } else {
            (Vec3::new(2.0, 3.0, 2.0).normalize(), 0.13, 0.75)
        };
        let mut right = view.x_axis.to_array();
        let mut up = view.y_axis.to_array();
        right[3] = ambient * world.ambient_light;
        up[3] = 1.0;
        let data = SplatCamera {
            view_projection: (camera.projection() * camera.view()).to_cols_array_2d(),
            right,
            up,
            light: direction.extend(direct).to_array(),
        };
        self.queue
            .write_buffer(&self.splat_camera_buffer, 0, bytemuck::bytes_of(&data));
        // Back to front alpha composition preserves overlapping layers.
        let forward = camera.target - camera.position;
        self.splat_instances.sort_by(|a, b| {
            let a_depth = (Vec3::from_array(a.center_radius[..3].try_into().unwrap())
                - camera.position)
                .dot(forward);
            let b_depth = (Vec3::from_array(b.center_radius[..3].try_into().unwrap())
                - camera.position)
                .dot(forward);
            b_depth.total_cmp(&a_depth)
        });
        let bytes = bytemuck::cast_slice(&self.splat_instances);
        if self.splat_instances.len() > self.splat_buffer_capacity {
            self.splat_buffer_capacity = self.splat_instances.len();
            self.splat_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("splat instances"),
                size: bytes.len().max(std::mem::size_of::<GpuSplat>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&self.splat_buffer, 0, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn poisson_mesh_draws_triangles_and_respects_depth() {
        pollster::block_on(async {
            let adapter = wgpu::Instance::default()
                .request_adapter(&Default::default()).await.expect("GPU adapter");
            let (device, queue) = adapter.request_device(&Default::default()).await.expect("GPU device");
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let (_, pipeline, camera, bind_group, _, _) = create_splat_resources(&device, format);
            let data = SplatCamera {
                view_projection: glam::Mat4::IDENTITY.to_cols_array_2d(),
                right: [1.0, 0.0, 0.0, 0.25],
                up: [0.0, 1.0, 0.0, 1.0],
                light: [0.0, 0.0, 1.0, 0.75],
            };
            queue.write_buffer(&camera, 0, bytemuck::bytes_of(&data));
            let vertices = [
                GpuMeshVertex { position: [-0.8, -0.8, 0.5, 1.0], normal: [0.0, 0.0, 1.0, 0.0], captured_color: [0.0; 4] },
                GpuMeshVertex { position: [0.8, -0.8, 0.5, 1.0], normal: [0.0, 0.0, 1.0, 0.0], captured_color: [0.0; 4] },
                GpuMeshVertex { position: [0.0, 0.8, 0.5, 1.0], normal: [0.0, 0.0, 1.0, 0.0], captured_color: [0.0; 4] },
            ];
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh depth test triangle"), contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let dim_vertices = vertices.map(|mut vertex| {
                vertex.normal = [0.0, -1.0, 0.0, 0.0];
                vertex
            });
            let dim_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh depth test dim triangle"), contents: bytemuck::cast_slice(&dim_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let color = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mesh depth test color"), size: wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let depth = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mesh depth test depth"), size: wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh depth test readback"), size: 256 * 8,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let color_view = color.create_view(&Default::default());
            let depth_view = depth.create_view(&Default::default());
            let mut bright = 0;
            for (clear_depth, visible, dim, ambient) in [
                (1.0, true, false, 0.25),
                (1.0, true, true, 0.25),
                (0.1, false, false, 0.25),
                (1.0, false, true, 0.0),
            ] {
                let mut lighting = data;
                lighting.right[3] = ambient;
                queue.write_buffer(&camera, 0, bytemuck::bytes_of(&lighting));
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("mesh depth test"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &color_view, resolve_target: None,
                            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &depth_view,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(clear_depth), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                    });
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bind_group, &[]);
                    pass.set_vertex_buffer(0, if dim { dim_vertex_buffer.slice(..) } else { vertex_buffer.slice(..) });
                    pass.draw(0..3, 0..1);
                }
                encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo { texture: &color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(8) } },
                    wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
                );
                queue.submit([encoder.finish()]);
                let (sender, receiver) = mpsc::channel();
                readback.slice(..).map_async(wgpu::MapMode::Read, move |result| sender.send(result).unwrap());
                device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
                receiver.recv().unwrap().unwrap();
                let data = readback.slice(..).get_mapped_range().unwrap();
                let center = &data[4 * 256 + 4 * 4..4 * 256 + 4 * 4 + 3];
                assert_eq!(center[0] > 20, visible, "triangle depth result: {center:?}");
                if !dim && visible { bright = center[0]; }
                if ambient == 0.0 {
                    assert_eq!(
                        center,
                        &[0, 0, 0],
                        "zero ambient must disable all mesh fill"
                    );
                }
                if dim && ambient > 0.0 { assert!(center[0] + 25 < bright, "surface normals must visibly affect shading: bright={bright}, dim={center:?}"); }
                drop(data);
                readback.unmap();
            }
        });
    }

    #[test]
    fn gaussian_quad_shader_validates() {
        let source = include_str!("../../assets/shaders/splats.wgsl");
        let module = wgpu::naga::front::wgsl::parse_str(source).expect("parse splat shader");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validate splat shader");
    }

    #[test]
    fn poisson_mesh_shader_validates() {
        let source = include_str!("../../assets/shaders/poisson_mesh.wgsl");
        let module = wgpu::naga::front::wgsl::parse_str(source).expect("parse mesh shader");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validate mesh shader");
    }

    #[test]
    fn poisson_background_shader_validates() {
        let source = [include_str!("../../assets/shaders/scene_abi.wgsl"),
            include_str!("../../assets/shaders/poisson_background.wgsl"),
            include_str!("../../assets/shaders/environment.wgsl")].join("\n");
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        ).validate(&module).expect("validate background shader");
    }
}

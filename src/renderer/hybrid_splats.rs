use super::*;
use wgpu::util::DeviceExt;

pub(super) fn create_splat_resources(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (
    wgpu::RenderPipeline,
    wgpu::Buffer,
    wgpu::BindGroup,
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
    (pipeline, camera, bind_group, instances)
}

impl Renderer {
    pub(super) fn upload_splat_camera(&mut self, camera: &Camera, world: World) {
        if !self.hybrid_enabled {
            return;
        }
        let view = camera.view().inverse();
        let sun = Vec3::from_array(world.sun_direction());
        let (direction, ambient, direct) = if world.background == crate::model::BackgroundMode::Sky
        {
            let daylight = ((sun.y + 0.18) / 0.34).clamp(0.0, 1.0);
            (
                sun,
                0.035 + 0.165 * daylight,
                world.sun_intensity * daylight,
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
}

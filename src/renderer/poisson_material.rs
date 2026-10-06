use super::*;


pub(super) fn source(scene: &str, capacity: u32) -> String {
    fn section<'a>(source: &'a str, begin: &str, end: &str) -> &'a str {
        source.split_once(begin).expect("mesh shader section").1
            .split_once(end).expect("mesh shader section end").0
    }
    let common = section(scene, "// POISSON_COMMON_BEGIN", "// POISSON_COMMON_END");
    let custom = section(scene, "// POISSON_CUSTOM_BEGIN", "// POISSON_CUSTOM_END");
    let stencil = section(include_str!("../../assets/shaders/sdf.wgsl"),
        "fn stencil_color(", "const CSG_SIZE");
    let stencil = format!("fn stencil_color({stencil}");
    let metal = include_str!("../../assets/shaders/material_metal_light.wgsl")
        .split_once("// One traced scene reflection").unwrap().0;
    [include_str!("../../assets/shaders/scene_abi.wgsl"),
        "@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> objects: array<Object>;
struct BvhNode { center_radius: vec4<f32>, metadata: vec4<u32>, aabb_min: vec4<f32>, aabb_max: vec4<f32> }
@group(0) @binding(2) var<storage, read> bvh: array<BvhNode>;
@group(0) @binding(11) var image_atlas: texture_2d_array<f32>;
@group(0) @binding(12) var image_sampler: sampler;
struct MaterialCapture { color: vec3<f32>, index: u32 }
fn material_capture(point: vec3<f32>, view: vec3<f32>, object: Object) -> MaterialCapture { return MaterialCapture(object.color.rgb, object.component.w); }
fn ambient_occlusion(point: vec3<f32>, normal: vec3<f32>) -> f32 { return 1.0; }",
        &stencil,
        include_str!("../../assets/shaders/environment.wgsl"),
        include_str!("../../assets/shaders/modifier_common.wgsl"),
        include_str!("../../assets/shaders/modifier_lattice.wgsl"),
        common,
        include_str!("../../assets/shaders/material_wood.wgsl"),
        include_str!("../../assets/shaders/material_fabric.wgsl"),
        include_str!("../../assets/shaders/material_metal.wgsl"),
        metal,
        include_str!("../../assets/shaders/material_brick.wgsl"),
        include_str!("../../assets/shaders/material_diagnostic.wgsl"),
        custom,
        include_str!("../../assets/shaders/poisson_owner.wgsl"),
        include_str!("../../assets/shaders/poisson_material.wgsl"),
    ].join("\n").replace("const POISSON_CSG_SIZE: u32 = 256u;",
        &format!("const POISSON_CSG_SIZE: u32 = {}u;", capacity.max(1)))
}

pub(super) fn create_pipeline(device: &wgpu::Device, layout: Option<&wgpu::PipelineLayout>,
    scene: &str, format: wgpu::TextureFormat, capacity: u32,
    features: SceneShaderFeatures) -> wgpu::RenderPipeline {
    let source = source(scene, capacity);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Poisson source materials"), source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let constants = [
        ("HAS_WOOD_MATERIAL", f64::from(features.materials.wood)),
        ("HAS_BRICK_MATERIAL", f64::from(features.materials.brick)),
        ("HAS_FABRIC_MATERIAL", f64::from(features.materials.fabric)),
        ("HAS_METAL_MATERIAL", f64::from(features.materials.metal)),
        ("HAS_DIAGNOSTIC_MATERIAL", f64::from(features.materials.diagnostic)),
        ("HAS_POLYGON_PRISMS", f64::from(features.primitives.polygon_prisms)),
        ("HAS_BEZIER_CURVES", f64::from(features.primitives.bezier_curves)),
        ("HAS_LOFTS", f64::from(features.primitives.lofts)),
        ("HAS_TEXT", f64::from(features.primitives.text)),
        ("HAS_LATTICE_MODIFIERS", f64::from(features.spatial.lattice)),
        ("HAS_MIRRORS", f64::from(features.spatial.mirror)),
        ("HAS_REPETITION", f64::from(features.spatial.repetition)),
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Poisson source material raster"), layout,
        vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_poisson"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<hybrid_splats::GpuMeshVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
            })], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_poisson"),
            targets: &[Some(wgpu::ColorTargetState { format,
                blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: wgpu::PipelineCompilationOptions { constants: &constants,
                ..Default::default() } }),
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(), bias: Default::default() }),
        multisample: Default::default(), multiview_mask: None, cache: None,
    })
}

#[cfg(test)]
mod tests {
    use bytemuck::Zeroable;
    use wgpu::util::DeviceExt;

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn one_triangle_resolves_straight_union_and_subtraction_material_bands() {
        pollster::block_on(async {
            let adapter = wgpu::Instance::default().request_adapter(&Default::default())
                .await.expect("GPU adapter");
            let (device, queue) = adapter.request_device(&Default::default()).await.expect("GPU device");
            let mut shader = super::source(&super::super::material_gpu::shader_source(), 2);
            shader.push_str("\n@group(0) @binding(13) var<storage, read_write> owners_out: array<u32>;
@compute @workgroup_size(2) fn check_owner(@builtin(global_invocation_id) id: vec3<u32>) {
    let point = select(vec3(-0.5, 0.0, 1.0), vec3(0.0, 0.0, 1.0), id.x == 0u);
    owners_out[id.x] = poisson_component_owner(point, 1u);
}");
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("mesh per-point owner test"), source: wgpu::ShaderSource::Wgsl(shader.into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("mesh per-point owner test"), layout: None, module: &module,
                entry_point: Some("check_owner"), compilation_options: Default::default(), cache: None,
            });
            for (operation, flat_root) in [(0, super::super::FLAT_UNION_ROOT),
                (1, super::super::FLAT_COMPONENT_ROOT)] {
                let mut child = super::super::GpuObject::zeroed();
                child.meta = [0, 2, operation, 1];
                child.inverse_rows = super::super::inverse_affine_rows(
                    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.0, 0.05)).inverse());
                child.group_inverse_rows = child.inverse_rows;
                child.params = [0.08, 2.0, 1.0, 1.0];
                child.scale = [1.0, 1.0, 1.0, 0.0];
                child.repeat_count = [1, 1, 1, 0];
                child.component = [0, 1, 0, 0];
                let mut root = super::super::GpuObject::zeroed();
                root.meta = [0, 2, 0, flat_root];
                root.inverse_rows = super::super::inverse_affine_rows(glam::Mat4::IDENTITY);
                root.group_inverse_rows = root.inverse_rows;
                root.params = [2.0, 2.0, 1.0, 1.0];
                root.scale = [1.0, 1.0, 1.0, 0.0];
                root.repeat_count = [1, 1, 1, 0];
                root.component = [0, 1, 0, 0];
                let objects = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None, contents: bytemuck::cast_slice(&[child, root]),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let result = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None, size: 8, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None, size: 8, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let layout = pipeline.get_bind_group_layout(0);
                let dummy_uniform = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None, size: 1024, usage: wgpu::BufferUsages::UNIFORM,
                    mapped_at_creation: false,
                });
                let dummy_storage = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None, size: 1024, usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                });
                let texture = |dimension| device.create_texture(&wgpu::TextureDescriptor {
                    label: None, size: wgpu::Extent3d { width: 1, height: 1,
                        depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
                    dimension, format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
                });
                let lattice = texture(wgpu::TextureDimension::D3);
                let lattice_view = lattice.create_view(&Default::default());
                let sampler = device.create_sampler(&Default::default());
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None,
                    layout: &layout, entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: dummy_uniform.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: objects.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: dummy_storage.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: dummy_storage.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: dummy_storage.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 5, resource: dummy_storage.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 6, resource: dummy_storage.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 8, resource: dummy_storage.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&lattice_view) },
                        wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::Sampler(&sampler) },
                        wgpu::BindGroupEntry { binding: 13, resource: result.as_entire_binding() },
                    ],
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                encoder.copy_buffer_to_buffer(&result, 0, &readback, 0, 8);
                queue.submit([encoder.finish()]);
                let (send, recv) = std::sync::mpsc::channel();
                readback.slice(..).map_async(wgpu::MapMode::Read, move |status| {
                    send.send(status).unwrap();
                });
                device.poll(wgpu::PollType::Wait { submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(30)) }).unwrap();
                recv.recv().unwrap().unwrap();
                let mapped = readback.slice(..).get_mapped_range().unwrap();
                let resolved: &[u32] = bytemuck::cast_slice(&mapped);
                let expected: &[u32] = if operation == 0 { &[0, 1] } else { &[1, 1] };
                assert_eq!(resolved, expected, "Boolean operation {operation}");
            }
        });
    }
    #[test]
    #[ignore = "requires a GPU adapter"]
    fn compact_source_material_pipeline_compiles_on_gpu() {
        pollster::block_on(async {
            let adapter = wgpu::Instance::default().request_adapter(&Default::default())
                .await.expect("GPU adapter");
            let (device, _) = adapter.request_device(&Default::default()).await.expect("GPU device");
            let scene = super::super::material_gpu::shader_source();
            let mut features = super::super::SceneShaderFeatures::for_scene(&[], &[]);
            for (label, capacity) in [("simple", 1), ("Astra-like polygon/loft", 2)] {
                let start = std::time::Instant::now();
                let _pipeline = super::create_pipeline(&device, None, &scene,
                    wgpu::TextureFormat::Rgba8Unorm, capacity, features);
                eprintln!("Poisson material Metal pipeline ({label}): {:?}", start.elapsed());
                features.primitives.polygon_prisms = true;
                features.primitives.lofts = true;
                features.materials.metal = true;
                features.spatial.mirror = true;
                features.spatial.repetition = true;
            }
        });
    }

    #[test]
    fn source_material_mesh_and_bake_shaders_validate() {
        let mut custom = crate::model::MaterialAsset::custom("Test".into());
        custom.wgsl = Some("var result = base; result.color = vec3(0.8, 0.2, 0.1); return result;".into());
        for scene in [super::super::material_gpu::shader_source(),
            super::super::material_gpu::shader_source_for_assets(&[custom])] {
            for suffix in ["", include_str!("../../assets/shaders/poisson_bake.wgsl")] {
                let source = format!("{}\n{suffix}", super::source(&scene, 8));
                assert!(!source.contains("fn scene_distance"));
                assert!(!source.contains("fn scene_normal"));
                let module = wgpu::naga::front::wgsl::parse_str(&source)
                    .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
                wgpu::naga::valid::Validator::new(
                    wgpu::naga::valid::ValidationFlags::all(),
                    wgpu::naga::valid::Capabilities::all(),
                ).validate(&module).unwrap();
            }
        }
    }
}

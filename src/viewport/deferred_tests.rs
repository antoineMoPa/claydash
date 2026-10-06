use super::*;
use wgpu::util::DeviceExt;

const SIZE: [u32; 2] = [128, 64];

fn composite_pixels(device: &wgpu::Device, viewport: &Viewport,
    encoder: &mut wgpu::CommandEncoder) -> (wgpu::Buffer, wgpu::Texture) {
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("deferred progress test output"),
        size: wgpu::Extent3d { width: SIZE[0], height: SIZE[1], depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let output_view = output.create_view(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("deferred progress test composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output_view, depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store },
            })], ..Default::default()
        });
        viewport.composite(&mut pass);
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("deferred progress test readback"),
        size: u64::from(SIZE[0] * SIZE[1] * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(output.as_image_copy(), wgpu::TexelCopyBufferInfo {
        buffer: &readback,
        layout: wgpu::TexelCopyBufferLayout { offset: 0,
            bytes_per_row: Some(SIZE[0] * 4), rows_per_image: Some(SIZE[1]) },
    }, wgpu::Extent3d { width: SIZE[0], height: SIZE[1], depth_or_array_layers: 1 });
    (readback, output)
}

fn pixel(device: &wgpu::Device, readback: &wgpu::Buffer, point: [u32; 2]) -> [u8; 4] {
    let (sender, receiver) = std::sync::mpsc::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).unwrap();
    });
    device.poll(wgpu::PollType::Wait { submission_index: None,
        timeout: Some(std::time::Duration::from_secs(30)) }).unwrap();
    receiver.recv().unwrap().unwrap();
    let mapped = readback.slice(..).get_mapped_range().unwrap();
    let offset = ((point[1] * SIZE[0] + point[0]) * 4) as usize;
    let value = mapped[offset..offset + 4].try_into().unwrap();
    drop(mapped);
    readback.unmap();
    value
}

#[test]
#[ignore = "requires a GPU adapter"]
fn deferred_refinement_composites_completed_tiles_before_full_frame() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default().request_adapter(&Default::default())
            .await.expect("GPU adapter");
        let (device, queue) = adapter.request_device(&Default::default()).await.expect("GPU device");
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deferred progress test scene layout"),
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let mut camera = [0u8; 320];
        camera[64..80].copy_from_slice(bytemuck::cast_slice(&[0.0f32, 0.0, 5.0, 1.0]));
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("deferred progress test camera"), contents: &camera,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let scene_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None, layout: &scene_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0,
                resource: camera_buffer.as_entire_binding() }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("synthetic blue G-buffer"),
            source: wgpu::ShaderSource::Wgsl(r#"
struct VertexOut { @builtin(position) position: vec4<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let corners = array<vec2<f32>, 3>(vec2(-1.0,-3.0), vec2(3.0,1.0), vec2(-1.0,1.0));
    return VertexOut(vec4(corners[index], 0.0, 1.0));
}
struct GeometryOut {
    @location(0) position: vec4<f32>, @location(1) normal: vec4<f32>,
    @location(2) color: vec4<f32>, @location(3) optics: vec4<f32>,
}
@fragment fn fs_main() -> GeometryOut {
    return GeometryOut(vec4(0.0,0.0,0.0,0.0), vec4(0.0,0.0,1.0,1.0),
        vec4(0.0,0.0,1.0,0.0), vec4(1.0,1.0,0.0,0.0));
}
"#.into()),
        });
        let geometry = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("synthetic blue G-buffer"), layout: None,
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"),
                buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"),
                targets: &[0, 1, 2, 3].map(|_| Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float, blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })), compilation_options: Default::default() }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false), depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(), bias: Default::default(),
            }),
            multisample: Default::default(), multiview_mask: None, cache: None,
        });
        let mut viewport = Viewport::new(&device, wgpu::TextureFormat::Rgba8Unorm,
            false, &scene_layout);
        viewport.set_initial_budget(TILE * TILE);
        let key = ViewKey { matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
            position: [0.0, 0.0, 5.0], projection: 0, versions: [1, 1],
            size: SIZE, refine: true };
        assert_eq!(viewport.prepare(&device, key.clone(), true, true, false), Work::Preview);
        // The coarse preview stands in for a visibly red first frame.
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let target = &viewport.targets.as_ref().unwrap().preview;
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("red preview"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target, depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                        store: wgpu::StoreOp::Store },
                })], ..Default::default()
            });
        }
        queue.submit([encoder.finish()]);
        let first = viewport.prepare(&device, key, true, true, false);
        assert_eq!(first, Work::Refine { first: 0, end: 1 });
        let mut encoder = device.create_command_encoder(&Default::default());
        viewport.encode(&device, &queue, &mut encoder, first, &geometry,
            &scene_bind, None, Some(DeferredPass { geometry_pipeline: &geometry }));
        let (readback, output) = composite_pixels(&device, &viewport, &mut encoder);
        queue.submit([encoder.finish()]);
        let completed = pixel(&device, &readback, [16, 16]);
        let unfinished = pixel(&device, &readback, [48, 16]);
        assert!(completed[2] > completed[0] + 30,
            "completed deferred tile should be blue: {completed:?}");
        assert!(unfinished[0] > unfinished[2] + 100,
            "unfinished deferred tile should retain red preview: {unfinished:?}");
        drop(output);

        let total = tile_count(SIZE);
        let mut encoder = device.create_command_encoder(&Default::default());
        viewport.encode(&device, &queue, &mut encoder,
            Work::Refine { first: 1, end: total }, &geometry,
            &scene_bind, None, Some(DeferredPass { geometry_pipeline: &geometry }));
        let (readback, _output) = composite_pixels(&device, &viewport, &mut encoder);
        queue.submit([encoder.finish()]);
        let final_tile = pixel(&device, &readback, [48, 16]);
        assert!(final_tile[2] > final_tile[0] + 30,
            "final deferred resolve should shade whole frame blue: {final_tile:?}");
    });
}

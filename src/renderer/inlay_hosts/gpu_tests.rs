use super::*;
use bytemuck::Zeroable;
use wgpu::util::DeviceExt;

fn shader_source() -> String {
    let sdf = include_str!("../../../assets/shaders/sdf.wgsl");
    let combine = sdf.split_once("fn combine_operand(").unwrap().1
        .split_once("// Inlays evaluate").unwrap().0;
    let inlay = sdf.split_once("fn inlay_host_distance(").unwrap().1
        .split_once("fn object_distance(").unwrap().0;
    format!("{}\n\
@group(0) @binding(1) var<storage, read> objects: array<Object>;\n\
@group(0) @binding(5) var<storage, read> polygon_points: array<vec2<f32>>;\n\
@group(0) @binding(14) var<storage, read_write> result: array<f32>;\n\
const CSG_SIZE: u32 = 4u;\n\
fn base_object_distance_at(point: vec3<f32>, object: Object) -> f32 {{\n\
    return length(point - object.params.yzw) - object.params.x;\n\
}}\n\
fn combine_operand({combine}\n\
fn inlay_host_distance({inlay}\n\
@compute @workgroup_size(1) fn check() {{\n\
    let point = vec3(1.3, 0.0, 0.0);\n\
    let inlay_object = objects[3];\n\
    result[0] = inlay_host_distance(point, 2u, polygon_points[1]);\n\
    result[1] = object_distance_at(point, inlay_object);\n\
}}", include_str!("../../../assets/shaders/scene_abi.wgsl"))
}

fn fixture(grouped: bool) -> (Vec<GpuObject>, Vec<GpuPolygonPoint>) {
    let mut host = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    let mut child = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    child.boolean_parent = Some(host.uuid);
    let mut cut = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    cut.boolean_parent = Some(child.uuid);
    cut.operation = crate::model::BooleanOperation::Subtract;
    let car = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    if grouped { host.boolean_parent = Some(car.uuid); }
    let mut patch = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    if grouped { patch.boolean_parent = Some(car.uuid); }
    patch.surface_inlay = Some(crate::model::SurfaceInlay {
        host: host.uuid, offset: 0.0, thickness: 0.01,
    });
    let mut order = vec![&cut, &child, &host, &patch];
    if grouped { order.push(&car); }
    let mut points = vec![GpuPolygonPoint { position: [0.0, 0.01] },
        GpuPolygonPoint { position: [0.0, 0.0] }];
    let program = pack(&order, &mut points);
    points[1].position = [f32::from_bits(program.starts[2]),
        f32::from_bits(program.parents_offset)];
    assert_eq!(program.starts[2], 0);
    assert_eq!(program.capacity, 4);
    let primitive = |radius, center: [f32; 3], parent: i32| {
        let mut object = GpuObject::zeroed();
        object.meta = [0, 1, 0, parent];
        object.params = [radius, center[0], center[1], center[2]];
        object.component = [0, if grouped { 4 } else { 2 }, 0, 0];
        object
    };
    let mut cut_gpu = primitive(0.4, [1.0, 0.0, 0.0], 1);
    cut_gpu.meta[2] = 1;
    let mut gpu = vec![cut_gpu,
        primitive(0.6, [1.0, 0.0, 0.0], 2),
        primitive(1.0, [0.0, 0.0, 0.0], if grouped { 4 } else { -1 }),
        primitive(100.0, [0.0, 0.0, 0.0], if grouped { 4 } else { -1 })];
    gpu[3].mirror_axes[3] = 3; // host index + 1
    gpu[3].repeat_spacing[3] = f32::from_bits(0); // inlay record offset
    if grouped {
        // The large sibling would dominate a whole-car host query. Render-only
        // acceleration may also rewrite GPU parent links, so the packed host
        // program must be the source of truth for nested reductions.
        gpu[0].meta[3] = 4;
        gpu[1].meta[3] = 4;
        gpu.push(primitive(10.0, [0.0, 0.0, 0.0], -1));
    }
    (gpu, points)
}

#[test]
#[ignore = "requires a GPU adapter"]
fn inlay_host_gpu_query_ignores_enclosing_union_and_flattened_parents() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default().request_adapter(&Default::default())
            .await.expect("GPU adapter");
        let (device, queue) = adapter.request_device(&Default::default()).await.expect("GPU device");
        let source = shader_source();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("inlay host GPU regression"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("inlay host GPU regression"), layout: None, module: &module,
            entry_point: Some("check"), compilation_options: Default::default(), cache: None,
        });
        let mut readings = Vec::new();
        for grouped in [false, true] {
            let (objects, points) = fixture(grouped);
            let objects = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None, contents: bytemuck::cast_slice(&objects),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let points = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None, contents: bytemuck::cast_slice(&points),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let result = device.create_buffer(&wgpu::BufferDescriptor {
                label: None, size: 8,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None, size: 8,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None, layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 1, resource: objects.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: points.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 14, resource: result.as_entire_binding() },
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
            let (sender, receiver) = std::sync::mpsc::channel();
            readback.slice(..).map_async(wgpu::MapMode::Read, move |status| {
                sender.send(status).unwrap();
            });
            device.poll(wgpu::PollType::Wait { submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)) }).unwrap();
            receiver.recv().unwrap().unwrap();
            let mapped = readback.slice(..).get_mapped_range().unwrap();
            let values: &[f32] = bytemuck::cast_slice(&mapped);
            readings.push([values[0], values[1]]);
        }
        for values in &readings {
            assert!((values[0] - 0.1).abs() < 1e-4, "nested subtraction host: {values:?}");
            assert!((values[1] - 0.09).abs() < 1e-4, "inlay clips to host: {values:?}");
        }
        assert!((readings[0][0] - readings[1][0]).abs() < 1e-5);
        assert!((readings[0][1] - readings[1][1]).abs() < 1e-5);
    });
}

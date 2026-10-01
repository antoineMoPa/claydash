use super::*;
use wgpu::util::DeviceExt;

// Exercise the production ray traversal with an independent sphere SDF fixture.
// Analytic hits are disabled so every candidate takes the component marcher.
#[test]
#[cfg(not(target_arch = "wasm32"))]
#[ignore = "requires a GPU adapter"]
fn depth_accelerator_texture_approach_refines_source_hits() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let source = include_str!("../../assets/shaders/sdf.wgsl").replace(
            "// SCENE_ABI_MODULE",
            include_str!("../../assets/shaders/scene_abi.wgsl"),
        );
        let structs = &source[..source.find("@group").unwrap()];
        let start = source.find("fn empty_splat_exclusions(").unwrap();
        let end = source[start..].find("// A membership query").unwrap() + start;
        let traversal = source[start..end]
            .replace("objects[", "objects[variant * 3u + ")
            .replace("var node_index = 0u;", "var node_index = ray_node_start;");
        let sphere_start = source.find("fn sphere_depth_uv(").unwrap();
        let sphere_end = source.find("fn sphere_depth_surface_sample(").unwrap();
        let sphere = &source[sphere_start..sphere_end];
        let box_start = source.find("fn box_depth_face_uv(").unwrap();
        let box_end = source.find("fn box_depth_view_face(").unwrap();
        let box_capture = &source[box_start..box_end];
        let combine_start = source.find("fn combine_operand(").unwrap();
        let combine_end =
            source[combine_start..].find("// Inlays evaluate").unwrap() + combine_start;
        let combine = &source[combine_start..combine_end];
        let refine_start = source.find("fn refine_neural_crossing(").unwrap();
        let refine_end = source.find("fn box_depth_face_uv(").unwrap();
        let refine =
            source[refine_start..refine_end].replace("objects[", "objects[variant * 3u + ");
        let shader = format!(
            r#"{structs}
@group(0) @binding(0) var<storage, read> objects: array<Object>;
@group(0) @binding(1) var<storage, read> bvh: array<BvhNode>;
@group(0) @binding(2) var<storage, read_write> hits: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> box_depth_texels: array<vec4<f32>>;
struct BoxDepthFaceSample {{ capture: vec4<f32>, material_index: u32 }}
const CSG_SIZE: u32 = 4u;
const HAS_REPETITION: bool = false;
var<private> camera: Camera;
var<private> variant: u32;
var<private> queries: u32;
var<private> component_queries: u32;
var<private> source_queries: vec3<u32>;
var<private> ray_node_start: u32;
var<private> disable_analytic: bool;
const HAS_BOOLEANS: bool = false;
const HAS_NEURAL_SDF: bool = false;
fn analytic_subtraction(a: u32, b: u32) -> bool {{ return false; }}
fn hard_subtraction(a: u32, b: u32) -> bool {{ return false; }}
fn component_has_neural(owner: f32) -> bool {{ return false; }}
fn has_analytic_interval(object: Object) -> bool {{ return !disable_analytic && object.operand_tree.z != 0u && (object.state.y == 1 || object.state.y == 2); }}
fn gaussian_splat_ray_entry(o: vec3<f32>, d: vec3<f32>, object: Object, a: f32, b: f32) -> vec2<f32> {{ return vec2(100.0, -1.0); }}
fn subtraction_entry(o: vec3<f32>, d: vec3<f32>, a: u32, b: u32) -> vec3<f32> {{ return vec3(100.0, -1.0, 0.0); }}
fn primitive_interval(origin: vec3<f32>, direction: vec3<f32>, object: Object) -> vec2<f32> {{
    let p = vec4(origin, 1.0);
    let v = vec4(direction, 0.0);
    let o = vec3(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p));
    let d = vec3(dot(object.inverse_rows[0], v), dot(object.inverse_rows[1], v), dot(object.inverse_rows[2], v));
    if object.state.y == 1 {{
        let a = dot(d, d);
        let b = dot(o, d);
        let c = dot(o, o) - object.params.x * object.params.x;
        let discriminant = b * b - a * c;
        if discriminant < 0.0 {{ return vec2(1.0, -1.0); }}
        let chord = sqrt(discriminant);
        return vec2(-b - chord, -b + chord) / a;
    }}
    let safe_d = select(vec3(-1.0), vec3(1.0), d >= vec3(0.0)) * max(abs(d), vec3(1e-20));
    let first = (-object.params.xyz - o) / safe_d;
    let second = (object.params.xyz - o) / safe_d;
    let entry = min(first, second);
    let exit = max(first, second);
    return vec2(max(entry.x, max(entry.y, entry.z)), min(exit.x, min(exit.y, exit.z)));
}}
fn mirror_point(point: vec3<f32>, object: Object) -> vec3<f32> {{ return point; }}
fn group_repeat_point(point: vec3<f32>, object: Object) -> vec3<f32> {{ return point; }}
fn modifier_point(point: vec3<f32>, object: Object) -> vec3<f32> {{ return point; }}
fn neural_shape(local: vec3<f32>, object: Object) -> f32 {{
    if variant == 8u {{ return length(local - vec3(0.0, 0.0, 1.2)) - 0.65; }}
    return 100.0;
}}
fn test_source_distance(point: vec3<f32>, object: Object) -> f32 {{
    let p = vec4(point, 1.0);
    let local = vec3(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p));
    if object.state.y == 2 {{
        let q = abs(local) - object.params.xyz;
        return length(max(q, vec3(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
    }}
    return length(local) - object.params.x;
}}
fn object_distance_at(point: vec3<f32>, object: Object) -> f32 {{
    queries += 1u;
    source_queries[u32(object.state.x)] += 1u;
    return test_source_distance(point, object);
}}
{sphere}
{box_capture}
{combine}
{refine}
fn surface_hit_tolerance(owner: f32, epsilon: f32) -> f32 {{ return epsilon; }}
fn component_distance(point: vec3<f32>, start: u32, root: u32) -> vec2<f32> {{
    queries += 1u;
    component_queries += 1u;
    let object = objects[variant * 3u + root];
    return vec2(test_source_distance(point, object), f32(root));
}}
fn smooth_subtraction_distance(point: vec3<f32>, start: u32, root: u32) -> vec2<f32> {{ return component_distance(point, start, root); }}
{traversal}
@compute @workgroup_size(1)
fn test_rays(@builtin(global_invocation_id) id: vec3<u32>) {{
    variant = id.x / 8u;
    disable_analytic = id.x == 49u || id.x == 57u;
    let grouped_start = arrayLength(&bvh) - 4u;
    camera.count.y = grouped_start;
    if variant >= 5u {{
        ray_node_start = grouped_start + variant - 5u;
        camera.count.y = ray_node_start + 1u;
    }}
    let origins = array<vec3<f32>, 8>(vec3(0.0, 0.0, 3.0), vec3(0.2, 0.0, 3.0),
        vec3(0.5, 0.0, 3.0), vec3(0.75, 0.0, 3.0), vec3(1.1, 0.0, 3.0),
        vec3(0.0, 0.0, 0.8), vec3(0.0, 0.0, -3.0), vec3(0.399, 0.0, 3.0));
    let origin = origins[id.x % 8u];
    let direction = select(vec3(0.0, 0.0, -1.0), vec3(0.0, 0.0, 1.0), id.x % 8u == 6u);
    var hit = trace_objects(origin, direction, 0.0015, empty_splat_exclusions(), false);
    if variant == 4u {{
        // Unrefined depth-texture rendering provides an independent before/after.
        var travel = 0.0;
        hit = vec3(100.0, -1.0, -1.0);
        for (var step = 0u; step < 192u; step++) {{
            var sample = vec2(100.0, -1.0);
            for (var i = 0u; i < 3u; i++) {{
                let distance = depth_accelerator_distance(origin + direction * travel, direction, objects[variant * 3u + i]);
                if distance < sample.x {{ sample = vec2(distance, f32(i)); }}
            }}
            if sample.x < 0.0015 {{ hit = vec3(travel, sample.y, -1.0); break; }}
            travel += sample.x * 0.35;
            if travel > 100.0 {{ break; }}
        }}
    }}
    hits[id.x] = vec4(hit, f32(queries));
    hits[72u + id.x] = vec4(vec3<f32>(source_queries), f32(component_queries));
}}
"#
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sphere accelerator ray parity"),
            source: wgpu::ShaderSource::Wgsl(shader.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("test_rays"),
            compilation_options: Default::default(),
            cache: None,
        });
        let spheres = [
            (Vec3::ZERO, 0.4),
            (Vec3::new(0.0, 0.0, 1.2), 0.15),
            (Vec3::new(0.0, 0.0, -1.0), 0.4),
        ];
        let mut objects = Vec::new();
        let mut texels = Vec::new();
        for clearance in [0.0_f32, 0.025, 0.1, 1.0, 0.1] {
            for (index, (center, radius)) in spheres.into_iter().enumerate() {
                let mut source = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
                source.transform.translation = center;
                source.params = SdfParams::SphereParams(crate::model::SphereParams { radius });
                let atlas = bake_sphere_depth_atlas(
                    &[source.clone()],
                    source.uuid,
                    SPHERE_DEPTH_WIDTH,
                    SPHERE_DEPTH_HEIGHT,
                )
                .unwrap();
                let transform = texels.len() as u32;
                texels.extend_from_slice(
                    &super::atlas_upload::depth_accelerator_capture_records(
                        &[source.clone()],
                        source.uuid,
                    )
                    .unwrap(),
                );
                let offset = texels.len() as u32;
                for texel in &atlas.texels {
                    // Simulate finite capture error: refinement must find the
                    // true surface instead of stopping at this smaller shell.
                    texels.push([texel[0] + 0.04, texel[1], texel[2], texel[3]]);
                    texels.push([0.0, (index + 1) as f32, 0.0, 0.0]);
                }
                let mut object = GpuObject::zeroed();
                object.inverse_rows = inverse_affine_rows(glam::Mat4::from_translation(-center));
                object.params[0] = radius;
                object.meta[0] = index as i32;
                object.meta[1] = sdf_consts::TYPE_SPHERE;
                object.modifier[1] = 0.8_f32.to_bits();
                object.operand_tree[3] = clearance.to_bits();
                object.operand_tree[2] = if clearance > 0.0 { index as u32 + 1 } else { 0 };
                object.component = [index as u32, index as u32, 0, 0];
                object.box_depth_meta = [offset, atlas.width, atlas.height, transform + 1];
                object.box_depth_max = [atlas.radius, atlas.radius, atlas.radius, 0.0];
                objects.push(object);
            }
        }
        // A captured union whose nearest map owner is a child, not its root.
        // Count source queries individually to catch accidental group marching.
        let mut group: Vec<_> = spheres
            .into_iter()
            .map(|(center, radius)| {
                let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
                object.transform.translation = center;
                object.params = SdfParams::SphereParams(crate::model::SphereParams { radius });
                object
            })
            .collect();
        let root = group[2].uuid;
        group[0].boolean_parent = Some(root);
        group[1].boolean_parent = Some(root);
        let atlas =
            bake_sphere_depth_atlas(&group, root, SPHERE_DEPTH_WIDTH, SPHERE_DEPTH_HEIGHT).unwrap();
        let transform = texels.len() as u32;
        texels.extend_from_slice(
            &super::atlas_upload::depth_accelerator_capture_records(&group, root).unwrap(),
        );
        let offset = texels.len() as u32;
        for (texel, owner) in atlas.texels.iter().zip(&atlas.owners) {
            let owner_index = owner
                .and_then(|id| group.iter().position(|object| object.uuid == id))
                .map_or(0, |index| index + 1);
            texels.push([texel[0] + 0.04, texel[1], texel[2], texel[3]]);
            texels.push([0.0, owner_index as f32, 0.0, 0.0]);
        }
        for (index, (center, radius)) in spheres.into_iter().enumerate() {
            let mut object = GpuObject::zeroed();
            object.inverse_rows = inverse_affine_rows(glam::Mat4::from_translation(-center));
            object.params[0] = radius;
            object.meta = [
                index as i32,
                sdf_consts::TYPE_SPHERE,
                0,
                if index == 2 { -1 } else { 2 },
            ];
            object.modifier[1] = 0.8_f32.to_bits();
            object.component = [0, 2, 0, 0];
            object.operand_tree[2] = 3;
            if index == 2 {
                object.operand_tree[3] = 0.1_f32.to_bits();
                object.box_depth_meta = [offset, atlas.width, atlas.height, transform + 1];
                object.box_depth_max = [atlas.radius, atlas.radius, atlas.radius, 0.0];
            }
            objects.push(object);
        }
        let mut box_group: Vec<_> = spheres
            .into_iter()
            .map(|(center, half)| {
                let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
                object.transform.translation = center;
                object.params = SdfParams::BoxParams(crate::model::BoxParams {
                    box_q: Vec3::splat(half),
                    corner_radius: 0.0,
                });
                object
            })
            .collect();
        let box_root = box_group[2].uuid;
        box_group[0].boolean_parent = Some(box_root);
        box_group[1].boolean_parent = Some(box_root);
        let box_atlas = bake_box_depth_atlas(
            &box_group,
            box_root,
            BOX_DEPTH_RESOLUTION,
            BoxCaptureStart::AtBounds,
        )
        .unwrap();
        let box_transform = texels.len() as u32;
        let mut records =
            super::atlas_upload::depth_accelerator_capture_records(&box_group, box_root).unwrap();
        records[3][1] = 8.0;
        texels.extend_from_slice(&records);
        let box_offset = texels.len() as u32;
        for texel in &box_atlas.texels {
            // A stale nearby thin-feature owner leaves the box depth intact.
            let owner_index = if texel[0] >= 0.0 { 2 } else { 0 };
            texels.push([
                if texel[0] >= 0.0 {
                    texel[0] + 0.04
                } else {
                    texel[0]
                },
                texel[1],
                texel[2],
                texel[3],
            ]);
            texels.push([0.0, owner_index as f32, 0.0, 0.0]);
        }
        for (index, (center, half)) in spheres.into_iter().enumerate() {
            let mut object = GpuObject::zeroed();
            object.inverse_rows = inverse_affine_rows(glam::Mat4::from_translation(-center));
            object.params = [half, half, half, 1.0];
            object.meta = [
                index as i32,
                sdf_consts::TYPE_BOX,
                0,
                if index == 2 { -1 } else { 2 },
            ];
            object.modifier[1] = 0.8_f32.to_bits();
            object.component = [0, 2, 0, 0];
            object.operand_tree[2] = 3;
            if index == 2 {
                object.operand_tree[3] = 0.1_f32.to_bits();
                object.box_depth_meta = [
                    box_offset,
                    box_atlas.resolution,
                    box_atlas.resolution,
                    box_transform + 1,
                ];
                object.box_depth_min = box_atlas.local_min.extend(0.0).to_array();
                object.box_depth_max = box_atlas.local_max.extend(0.0).to_array();
            }
            objects.push(object);
        }
        // Reuse the sphere depths but swap the front child and body owners.
        // This covers both an owner that misses the ray and one behind the
        // true first hit; neither should force a full-component march.
        let wrong_transform = texels.len() as u32;
        let captured = texels[transform as usize..box_transform as usize].to_vec();
        texels.extend_from_slice(&captured);
        let wrong_offset = wrong_transform + 4;
        for index in 0..atlas.texels.len() {
            if texels[wrong_offset as usize + index * 2][0] >= 0.0 {
                texels[wrong_offset as usize + index * 2 + 1][1] =
                    if atlas.owners[index] == Some(group[1].uuid) {
                        1.0
                    } else {
                        2.0
                    };
            }
        }
        let recovered_objects = objects[15..18].to_vec();
        for mut object in recovered_objects {
            if object.meta[0] == 2 {
                object.box_depth_meta[0] = wrong_offset;
                object.box_depth_meta[3] = wrong_transform + 1;
            }
            objects.push(object);
        }
        // A fitted field expanded by the 0.3 training offset hands off to
        // one original source object before reaching its true surface.
        let neural_transform = texels.len() as u32;
        texels.extend_from_slice(&inverse_affine_rows(glam::Mat4::IDENTITY));
        texels.push([1.0, 12.0, 0.0, 0.0]);
        let neural_offset = texels.len() as u32;
        texels.push([0.0, 0.0, 0.0, 1.0]);
        for _ in 0..32 * 32 * 32 {
            texels.push([0.0; 4]);
            texels.push([2.0, 0.0, 0.0, 0.0]);
        }
        for mut object in objects[15..18].to_vec() {
            if object.meta[0] == 2 {
                object.operand_tree[3] = 0.3_f32.to_bits();
                object.box_depth_meta = [neural_offset, 32, 32, neural_transform + 1];
                object.box_depth_max = [2.0, 2.0, 2.0, 0.0];
            }
            objects.push(object);
        }
        // The loose sphere and box admit rays that miss the source. The second
        // sphere sits nearer the camera and must win despite traversal order.
        let mut bounds: Vec<_> = spheres
            .into_iter()
            .enumerate()
            .map(|(index, (center, _))| ObjectBound {
                center,
                radius: 0.8,
                half_extent: Vec3::splat(0.8),
                object_index: index as u32,
            })
            .collect();
        let mut nodes = build_bvh(&mut bounds);
        for node in &mut nodes {
            if node.metadata[0] != BVH_LEAF {
                node.metadata[2] = node.metadata[0];
            }
        }
        let end = nodes.len() as u32 + 1;
        nodes.push(GpuBvhNode {
            center_radius: [0.0, 0.0, 0.0, 2.0],
            metadata: [2, end, 0, 0],
            aabb_min: [-0.4, -0.4, -1.4, 0.0],
            aabb_max: [0.4, 0.4, 1.35, 0.0],
        });
        let box_end = nodes.len() as u32 + 1;
        nodes.push(GpuBvhNode {
            center_radius: [0.0, 0.0, 0.0, 2.0],
            metadata: [2, box_end, 0, 0],
            aabb_min: [-0.4, -0.4, -1.4, 0.0],
            aabb_max: [0.4, 0.4, 1.35, 0.0],
        });
        let recovery_end = nodes.len() as u32 + 1;
        nodes.push(GpuBvhNode {
            center_radius: [0.0, 0.0, 0.0, 2.0],
            metadata: [2, recovery_end, 0, 0],
            aabb_min: [-0.4, -0.4, -1.4, 0.0],
            aabb_max: [0.4, 0.4, 1.35, 0.0],
        });
        let neural_end = nodes.len() as u32 + 1;
        nodes.push(GpuBvhNode {
            center_radius: [0.0, 0.0, 0.0, 2.0],
            metadata: [2, neural_end, 0, 0],
            aabb_min: [-0.8, -0.8, -1.4, 0.0],
            aabb_max: [0.8, 0.8, 1.35, 0.0],
        });
        let storage = |label, contents: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let object_buffer = storage("objects", bytemuck::cast_slice(&objects));
        let node_buffer = storage("nodes", bytemuck::cast_slice(&nodes));
        let texture_buffer = storage("baked sphere depth", bytemuck::cast_slice(&texels));
        let result = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 144 * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: result.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: object_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: node_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: result.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: texture_buffer.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(72, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&result, 0, &readback, 0, result.size());
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        receive.recv().unwrap().unwrap();
        let data = readback.slice(..).get_mapped_range().unwrap();
        let hits: &[[f32; 4]] = bytemuck::cast_slice(&data);
        for variant in 0..4 {
            for ray in 0..8 {
                let hit = hits[variant * 8 + ray];
                if variant == 1 && ray == 7 {
                    // Capture error 0.04 exceeds this narrow 0.025 band at a
                    // grazing silhouette; the default 0.1 band recovers it.
                    assert_eq!(hit[1], -1.0);
                    continue;
                }
                assert_eq!(hit[1], hits[ray][1], "owner variant {variant}, ray {ray}");
                assert!(
                    (hit[0] - hits[ray][0]).abs() < 0.015,
                    "depth variant {variant}, ray {ray}: {hit:?} vs {:?}",
                    hits[ray]
                );
            }
        }
        assert_eq!(hits[0][1], 1.0);
        assert!((hits[0][0] - 1.65).abs() < 0.002);
        assert_eq!(hits[2][1], -1.0);
        assert_eq!(hits[5][1], 0.0);
        assert_eq!(hits[6][1], 2.0);
        assert!(
            (hits[32][0] - 1.65).abs() > 0.02,
            "the raw depth texture should retain the injected capture error"
        );
        assert!(
            (hits[16][0] - 1.65).abs() < 0.002,
            "exact refinement must clean up the capture error"
        );
        assert!(
            hits[16 + 3][3] < hits[3][3],
            "texture approach must avoid exact queries on a missed ray"
        );
        assert_eq!(
            hits[19][3], 0.0,
            "a missed texture ray should not enter source refinement"
        );
        assert!(
            hits[16][3] < hits[0][3],
            "texture approach should reduce source queries on the hit ray"
        );
        assert_eq!(
            hits[40][1], 1.0,
            "the sphere map must select the front child"
        );
        assert!((hits[40][0] - 1.65).abs() < 0.002);
        let counts = hits[72 + 40];
        assert_eq!(counts[0], 0.0, "unselected sibling must not be marched");
        assert!(counts[1] > 0.0, "selected map owner must be refined");
        assert_eq!(counts[2], 0.0, "unselected group root must not be marched");
        assert_eq!(
            counts[3], 0.0,
            "accelerated refinement must never query the component"
        );
        assert_eq!(hits[48][1], 1.0, "box map must select the front child");
        assert!((hits[48][0] - 1.65).abs() < 0.002);
        let box_counts = hits[72 + 48];
        assert_eq!(box_counts[0], 0.0);
        assert!(box_counts[1] > 0.0);
        assert_eq!(box_counts[2], 0.0);
        assert_eq!(
            box_counts[3], 0.0,
            "box refinement must never query the component"
        );
        assert_eq!(
            hits[48 + 3][1],
            -1.0,
            "box rays outside the group should miss"
        );
        assert_eq!(hits[49][1], 1.0, "box atlas fallback must remain visible");
        assert_eq!(hits[49][2], -2.0);
        assert!((hits[49][0] - 2.6).abs() < 0.12);
        let box_fallback_counts = hits[72 + 49];
        assert_eq!(box_fallback_counts[0], 0.0);
        assert!(box_fallback_counts[1] > 0.0);
        assert_eq!(box_fallback_counts[2], 0.0);
        assert_eq!(box_fallback_counts[3], 0.0);
        assert_eq!(
            hits[56 + 1][1],
            1.0,
            "atlas fallback must preserve an occupied texel"
        );
        assert_eq!(hits[56 + 1][2], -2.0, "fallback must mark an atlas normal");
        assert!((hits[56 + 1][0] - 2.65).abs() < 0.12);
        let recovery_counts = hits[72 + 56 + 1];
        assert_eq!(recovery_counts[0], 0.0, "fallback must not march a sibling");
        assert!(recovery_counts[1] > 0.0, "only the mapped owner is tried");
        assert_eq!(
            recovery_counts[2], 0.0,
            "far group root must not be marched"
        );
        assert_eq!(
            recovery_counts[3], 0.0,
            "recovery must not march the component"
        );
        assert_eq!(hits[56][1], 1.0, "nearer front child must beat mapped body");
        assert!((hits[56][0] - 1.65).abs() < 0.002);
        let nearer_counts = hits[72 + 56];
        assert_eq!(
            nearer_counts[0], 0.0,
            "farther mapped body must not be marched"
        );
        assert!(nearer_counts[1] > 0.0);
        assert_eq!(nearer_counts[2], 0.0);
        assert_eq!(nearer_counts[3], 0.0);
        assert_eq!(hits[64][1], 1.0, "neural handoff must keep the source owner");
        assert!((hits[64][0] - 1.65).abs() < 0.01);
        let neural_counts = hits[72 + 64];
        assert_eq!(neural_counts[0], 0.0);
        assert!(neural_counts[1] > 0.0);
        assert_eq!(neural_counts[2], 0.0);
        assert_eq!(neural_counts[3], 0.0);
        assert_eq!(
            hits[64 + 2][1],
            -1.0,
            "a neural surface with no contacted source must not become a hit"
        );
        let neural_miss_counts = hits[72 + 64 + 2];
        assert!(neural_miss_counts[1] > 0.0, "the selected source was tested");
        assert_eq!(neural_miss_counts[3], 0.0, "the subtree was not marched");
        eprintln!("grouped sphere source query counts: {counts:?}; box: {box_counts:?}");
        eprintln!("raw texture depth {}, refined depth {}, exact reference {}; missed-ray exact queries {} -> {}", hits[32][0], hits[16][0], hits[0][0], hits[3][3], hits[19][3]);
    });
}

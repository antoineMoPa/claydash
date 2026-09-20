use super::*;
use glam::Vec4;

#[test]
fn material_and_boolean_shader_validates() {
    let source = include_str!("../../assets/shaders/sdf.wgsl");
    let mut sources: Vec<_> = [1, 2, 4, 8, 16, 256, 1024]
        .into_iter()
        .map(|capacity| specialized_shader_source(source, capacity))
        .collect();
    sources.push(include_str!("../../assets/shaders/viewport.wgsl").into());
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

fn bvh_sphere_distance(point: Vec3, nodes: &[GpuBvhNode], bounds: &[ObjectBound]) -> (f32, usize) {
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

use super::*;

#[test]
fn top_level_union_splits_nested_subtraction_into_independent_bvh_leaves() {
    let mut objects = vec![GpuObject::zeroed(); 11];
    objects[0].meta[3] = 2;
    objects[1].meta[3] = 2;
    objects[1].meta[2] = 1; // Subtraction stays inside its child component.
    objects[2].meta[3] = 10;
    for object in &mut objects[3..10] { object.meta[3] = 10; }
    objects[10].meta[3] = -1;
    let original_bounds: Vec<_> = (0..11).map(|index| ObjectBound {
        center: Vec3::new(index as f32, 0.0, 0.0),
        half_extent: Vec3::ONE, radius: Vec3::ONE.length(), object_index: index as u32,
    }).collect();
    let mut component_bounds = boolean_component_bounds(&objects, &original_bounds);
    assert!(component_bounds[10].unwrap().half_extent.x > 1.0);
    let eligible = [10].into_iter().collect();
    assert_eq!(split_top_level_hard_unions(&mut objects, &eligible,
        &original_bounds, &mut component_bounds), 1);
    assert_eq!(objects[0].meta[3], 2);
    assert_eq!(objects[1].meta[3], 2);
    assert_eq!(objects[1].meta[2], 1);
    assert!(objects[2..=10].iter().all(|object| object.meta[3] == -1));
    assert_eq!(component_bounds[10].unwrap().half_extent, Vec3::ONE);
    assert_eq!(mark_component_evaluation(&mut objects, 0, 2), 2);
    assert_eq!(mark_component_evaluation(&mut objects, 10, 10), 1);
}

#[test]
fn top_level_split_rejects_blend_modifiers_and_capture_subtrees() {
    let base = {
        let mut objects = vec![GpuObject::zeroed(); 9];
        for child in &mut objects[..8] { child.meta[3] = 8; }
        objects[8].meta[3] = -1;
        objects
    };
    let bounds = vec![ObjectBound { center: Vec3::ZERO, half_extent: Vec3::ONE,
        radius: Vec3::ONE.length(), object_index: 0 }; 9];
    for obstruction in ["blend", "subtract", "repeat", "mirror", "capture"] {
        let mut objects = base.clone();
        match obstruction {
            "blend" => objects[8].component[2] = 0.2_f32.to_bits(),
            "subtract" => objects[3].meta[2] = 1,
            "repeat" => objects[8].repeat_count[3] = 1,
            "mirror" => objects[8].mirror_axes[0] = 1,
            "capture" => objects[2].operand_tree[3] = 0.2_f32.to_bits(),
            _ => unreachable!(),
        }
        let mut component_bounds = boolean_component_bounds(&objects, &bounds);
        assert_eq!(split_top_level_hard_unions(&mut objects, &[8].into_iter().collect(),
            &bounds, &mut component_bounds), 0, "{obstruction}");
        assert_eq!(objects[3].meta[3], 8, "{obstruction}");
    }
}

#[test]
fn grouped_fusca_keeps_mirrored_child_components_bounded_on_both_sides() {
    let scene = crate::document::deserialize_scene(include_bytes!("../../examples/fusca.claydash"))
        .expect("Fusca scene");
    let source = match scene.get_path("sdf_objects") {
        crate::model::ClaydashValue::VecSDFObject(objects) => objects,
        _ => panic!("Fusca objects missing"),
    };
    let ordered = boolean_postorder(&source);
    let indices: std::collections::HashMap<_, _> = ordered.iter().enumerate()
        .map(|(index, object)| (object.uuid, index)).collect();
    let mut gpu = vec![GpuObject::zeroed(); ordered.len()];
    for (index, object) in ordered.iter().enumerate() {
        gpu[index].meta[3] = object.boolean_parent
            .map_or(-1, |parent| indices[&parent] as i32);
        gpu[index].meta[2] = object.operation.gpu_code();
        gpu[index].component[2] = object.softness.to_bits();
        if let Some(mirror) = object.mirror {
            gpu[index].mirror_axes[..3].copy_from_slice(&mirror.axes.map(u32::from));
        }
    }
    let root = gpu.len() - 1;
    assert_eq!(ordered[root].boolean_parent, None);
    let mirror_index = ordered.iter().position(|object| {
        object.boolean_parent == Some(ordered[root].uuid)
            && object.mirror.is_some_and(|mirror| mirror.axes[2])
    }).expect("direct mirrored car part");
    let group = crate::model::group_world_matrix(&source, ordered[mirror_index].uuid);
    let mirror_center = group.transform_point3(Vec3::new(0.0, 0.0, 3.0));
    let primitive_bounds: Vec<_> = (0..gpu.len()).map(|index| ObjectBound {
        center: if index == mirror_index { mirror_center } else { Vec3::ZERO },
        half_extent: Vec3::splat(0.2), radius: 0.4, object_index: index as u32,
    }).collect();
    let mut components = boolean_component_bounds(&gpu, &primitive_bounds);
    for mode in [crate::model::GroupRenderRepresentation::ExactSdf,
        crate::model::GroupRenderRepresentation::PoissonMesh] {
        let mut fallback_gpu = gpu.clone();
        let mut fallback_bounds = components.clone();
        let eligible = if union_split_supports_source_mode(mode) {
            [root].into_iter().collect()
        } else { std::collections::HashSet::new() };
        assert_eq!(split_top_level_hard_unions(&mut fallback_gpu, &eligible,
            &primitive_bounds, &mut fallback_bounds), 1, "{mode:?} fallback lost BVH splitting");
        assert_eq!(fallback_gpu.iter().filter(|object| object.meta[3] < 0).count(), 67);
    }
    assert_eq!(split_top_level_hard_unions(&mut gpu, &[root].into_iter().collect(),
        &primitive_bounds, &mut components), 1);
    assert_eq!(gpu.iter().filter(|object| object.meta[3] < 0).count(), 67);
    let mirror = ordered[mirror_index].mirror.unwrap();
    let expanded = mirrored_bound(components[mirror_index].unwrap(), group, mirror);
    let inverse = group.inverse();
    let local_min = inverse.transform_point3(expanded.center - expanded.half_extent);
    let local_max = inverse.transform_point3(expanded.center + expanded.half_extent);
    assert!(local_min.z <= -2.8 && local_max.z >= 2.8,
        "detached mirrored component must cover both copies");
}

#[test]
fn operand_bvh_indexes_safe_flat_union_children() {
    let mut objects = vec![GpuObject::zeroed(); 9];
    for (index, object) in objects.iter_mut().take(8).enumerate() {
        object.distance_bound = [index as f32 * 10.0, 0.0, 0.0, 1.0];
    }
    objects[8].meta[3] = FLAT_UNION_ROOT;
    objects[8].operand_tree[3] = 0.1_f32.to_bits();
    let mut nodes = Vec::new();
    append_operand_bvhs(&mut nodes, &mut objects, &[0; 9]);
    let [start, end, _, _] = objects[8].operand_tree;
    assert_eq!(start, 0);
    assert_eq!(end, 15);
    assert_eq!(f32::from_bits(objects[8].operand_tree[3]), 0.1);
    let mut leaves: Vec<_> = nodes
        .iter()
        .filter(|node| node.metadata[0] != BVH_LEAF)
        .map(|node| node.metadata[0])
        .collect();
    leaves.sort_unstable();
    assert_eq!(leaves, [0, 1, 2, 3, 4, 5, 6, 7]);
    for (index, node) in nodes.iter().enumerate() {
        assert!(node.metadata[1] > index as u32);
        assert!(node.metadata[1] <= end);
    }

    objects[1].distance_bound[3] = -1.0;
    objects[8].operand_tree = [0; 4];
    nodes.clear();
    append_operand_bvhs(&mut nodes, &mut objects, &[0; 9]);
    assert!(nodes.is_empty());
    assert_eq!(objects[8].operand_tree, [0; 4]);
}
use glam::Vec4;

#[test]
fn operand_bvh_keeps_acceleration_with_shared_cage_and_exceptional_operands() {
    let mut objects = vec![GpuObject::zeroed(); 13];
    for (index, object) in objects.iter_mut().enumerate() {
        object.distance_bound = [index as f32, 0.0, 0.0, 0.5];
        object.modifier[0] = 7;
    }
    objects[12].meta[3] = FLAT_UNION_ROOT;
    objects[3].distance_bound[3] = -1.0; // e.g. a loft
    objects[8].modifier[0] = 9; // separate deformation frame
    let mut nodes = Vec::new();
    append_operand_bvhs(&mut nodes, &mut objects, &[0; 13]);
    let mut leaves: Vec<_> = nodes
        .iter()
        .filter(|node| node.metadata[0] != BVH_LEAF)
        .map(|node| node.metadata[0])
        .collect();
    leaves.sort_unstable();
    assert_eq!(leaves, [0, 1, 2, 4, 5, 6, 7, 9, 10, 11]);
    assert_eq!(objects[12].operand_tree[1], nodes.len() as u32);
}

#[test]
fn material_and_boolean_shader_validates() {
    let source = material_gpu::shader_source();
    let mut sources: Vec<_> = [1, 2, 4, 8, 16, 256, 1024]
        .into_iter()
        .map(|capacity| specialized_shader_source(&source, capacity))
        .collect();
    sources.extend(
        [4, 33, 65, 128, 256, 1024].map(|width| specialized_neural_shader_source(&source, width)),
    );
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

#[test]
fn sphere_accelerator_keeps_nested_source_ranges_and_enough_shader_scratch() {
    let mut objects = vec![GpuObject::zeroed(); 5];
    objects[0].meta[3] = 2;
    objects[1].meta[3] = 2;
    objects[2].meta[3] = 4;
    objects[3].meta[3] = 4;
    objects[4].meta[3] = -1;
    objects[2].operand_tree[3] = 0.1_f32.to_bits();
    mark_depth_accelerator_subtrees(&mut objects);
    assert_eq!(
        objects
            .iter()
            .map(|o| o.operand_tree[2])
            .collect::<Vec<_>>(),
        [3, 3, 3, 0, 0]
    );
    flatten_nested_hard_unions(&mut objects);
    assert_eq!(objects[0].meta[3], 2, "capture hierarchy must not flatten");
    assert_eq!(mark_component_evaluation(&mut objects, 0, 4), 8);
    assert_eq!(f32::from_bits(objects[4].operand_tree[3]), 0.1);
    assert_eq!(objects[4].meta[3], -1);
}

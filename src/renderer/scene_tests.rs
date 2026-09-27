use super::*;

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
                            ordered[parent].softness,
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
fn nested_hard_unions_use_the_flat_gpu_path_without_changing_the_source_tree() {
    let mut objects = vec![GpuObject::zeroed(); 5];
    // Two operands union into an inner group, then that group and another
    // operand union into the outer root.
    objects[0].meta[3] = 2;
    objects[1].meta[3] = 2;
    objects[2].meta[3] = 4;
    objects[3].meta[3] = 4;
    objects[4].meta[3] = -1;
    objects[0].component[2] = 0.05_f32.to_bits(); // Leaf softness is unused.
    flatten_nested_hard_unions(&mut objects);
    assert!(objects[..4].iter().all(|object| object.meta[3] == 4));
    assert_eq!(objects[4].meta[3], -1);
}

#[test]
fn nested_union_flattening_keeps_nonunion_and_modified_components_intact() {
    for obstruction in ["subtract", "smooth", "repeat", "mirror"] {
        let mut objects = vec![GpuObject::zeroed(); 3];
        objects[0].meta[3] = 1;
        objects[1].meta[3] = 2;
        objects[2].meta[3] = -1;
        match obstruction {
            "subtract" => objects[1].meta[2] = 1,
            "smooth" => objects[1].component[2] = 0.2_f32.to_bits(),
            "repeat" => objects[1].repeat_count[3] = 1,
            "mirror" => objects[1].mirror_axes[0] = 1,
            _ => unreachable!(),
        }
        flatten_nested_hard_unions(&mut objects);
        assert_eq!(objects[0].meta[3], 1, "{obstruction}");
        assert_eq!(objects[1].meta[3], 2, "{obstruction}");
    }
}

#[test]
fn box_depth_capture_keeps_hit_owner_for_custom_materials() {
    use crate::model::{BooleanOperation, GroupRenderRepresentation, MaterialKind, PrimitiveKind};
    let mut root = SdfObject::create_kind(PrimitiveKind::Box);
    root.render_representation = GroupRenderRepresentation::BoxDepthAtlas;
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(root.uuid);
    child.operation = BooleanOperation::Union;
    child.transform.translation = Vec3::new(0.75, 0.0, 0.0);
    child.material.kind = MaterialKind::Custom;
    let child_id = child.uuid;
    let source = [root, child];
    let atlas = bake_box_depth_atlas(&source, source[0].uuid, 8).unwrap();
    assert!(atlas.owners.contains(&Some(child_id)));
    assert!(atlas.texels.iter().any(|sample| sample[0] >= 0.0));
}

#[test]
fn sphere_depth_capture_keeps_hit_owner_and_empty_directions() {
    use crate::model::{BooleanOperation, GroupRenderRepresentation, MaterialKind, PrimitiveKind};
    let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
    root.render_representation = GroupRenderRepresentation::SphereDepthAtlas;
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(root.uuid);
    child.operation = BooleanOperation::Union;
    child.transform.translation = Vec3::new(0.75, 0.0, 0.0);
    child.material.kind = MaterialKind::Custom;
    let child_id = child.uuid;
    let source = [root, child];
    let atlas = bake_sphere_depth_atlas(&source, source[0].uuid, 32, 16).unwrap();
    assert!(atlas.owners.contains(&Some(child_id)));
    assert!(atlas.texels.iter().any(|sample| sample[0] >= 0.0));
    assert!(atlas.texels.iter().any(|sample| sample[0] < 0.0));
    let prepared = scene_upload::prepare_group_scene(&source);
    assert_eq!(prepared.objects.len(), 1);
    assert!(prepared.sphere_depth_atlases.contains_key(&source[0].uuid));
}

#[test]
fn default_duck_sphere_depth_capture_contains_surface_hits() {
    let document: serde_json::Value = serde_json::from_str(crate::duck::DEFAULT_DUCK).unwrap();
    let mut source: Vec<SdfObject> =
        serde_json::from_value(document["subtree"]["sdf_objects"]["value"]["VecSDFObject"].clone())
            .unwrap();
    let root = source[0].uuid;
    source[0].render_representation = crate::model::GroupRenderRepresentation::SphereDepthAtlas;
    let atlas =
        bake_sphere_depth_atlas(&source, root, SPHERE_DEPTH_WIDTH, SPHERE_DEPTH_HEIGHT).unwrap();
    assert!(atlas.texels.iter().any(|sample| sample[0] >= 0.0));
    assert!(atlas.owners.iter().flatten().any(|owner| *owner != root));
}

#[test]
fn default_duck_box_depth_capture_contains_surface_hits() {
    let document: serde_json::Value = serde_json::from_str(crate::duck::DEFAULT_DUCK).unwrap();
    let mut source: Vec<SdfObject> =
        serde_json::from_value(document["subtree"]["sdf_objects"]["value"]["VecSDFObject"].clone())
            .unwrap();
    let root = source[0].uuid;
    source[0].render_representation = crate::model::GroupRenderRepresentation::BoxDepthAtlas;
    let atlas = bake_box_depth_atlas(&source, root, BOX_DEPTH_RESOLUTION).unwrap();
    let side = &atlas.texels[..(atlas.resolution * atlas.resolution) as usize];
    assert!(side.iter().any(|sample| sample[0] >= 0.0));
    assert!(side.iter().any(|sample| sample[0] < 0.0));
    assert!(side
        .iter()
        .any(|sample| sample[0] < 0.0 && sample[0] > -0.1));
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
fn mirror_bounds_cover_reflected_side_in_rotated_group() {
    let bound = ObjectBound {
        center: Vec3::new(0.0, 2.0, 0.0),
        half_extent: Vec3::splat(0.25),
        radius: 0.5,
        object_index: 0,
    };
    let group = glam::Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let mirrored = mirrored_bound(bound, group, crate::model::Mirror::default());
    assert!(mirrored.center.y - mirrored.half_extent.y < -2.2);
    assert!(mirrored.center.y + mirrored.half_extent.y > 2.2);
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
    let mut root = operand;
    root.meta[3] = -1;
    root.component[2] = 0.2_f32.to_bits();
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

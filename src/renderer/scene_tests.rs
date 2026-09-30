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
fn large_direct_boolean_components_keep_fragment_scratch_small() {
    let mut objects = vec![GpuObject::zeroed(); 600];
    for child in &mut objects[..599] {
        child.meta[3] = 599;
    }
    objects[599].meta[3] = -1;
    assert_eq!(mark_component_evaluation(&mut objects, 0, 599), 2);
    assert_eq!(objects[599].meta[3], FLAT_UNION_ROOT);

    objects[0].meta[2] = 1;
    objects[599].meta[3] = -1;
    assert_eq!(mark_component_evaluation(&mut objects, 0, 599), 2);
    assert_eq!(objects[599].meta[3], FLAT_COMPONENT_ROOT);

    objects[0].meta[3] = 1;
    objects[599].meta[3] = -1;
    assert_eq!(mark_component_evaluation(&mut objects, 0, 599), 1024);
    assert_eq!(objects[599].meta[3], -1);
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
    let atlas =
        bake_box_depth_atlas(&source, source[0].uuid, 8, BoxCaptureStart::AtBounds).unwrap();
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
    let prepared = scene_upload::prepare_group_scene(&source, &mut Default::default());
    assert_eq!(prepared.objects.len(), 1);
    assert!(prepared.sphere_depth_atlases.contains_key(&source[0].uuid));
}

#[test]
fn gaussian_splat_mode_replaces_group_and_preserves_capture_owners() {
    use crate::model::{BooleanOperation, GroupRenderRepresentation, PrimitiveKind};
    let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
    root.render_representation = GroupRenderRepresentation::GaussianSplats;
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(root.uuid);
    child.operation = BooleanOperation::Union;
    child.transform.translation = Vec3::new(0.75, 0.0, 0.0);
    let child_id = child.uuid;
    let source = [root, child];
    let prepared = scene_upload::prepare_group_scene(&source, &mut Default::default());
    assert_eq!(prepared.objects.len(), 1);
    assert!(prepared.gaussian_splats.contains(&source[0].uuid));
    let atlas = &prepared.box_depth_atlases[&source[0].uuid];
    assert!(atlas.owners.contains(&Some(child_id)));
    let SdfParams::BoxParams(proxy) = &prepared.objects[0].params else {
        panic!("Gaussian splat proxy should have box bounds");
    };
    assert!(proxy.box_q.x > atlas.local_max.x);
}

#[test]
fn default_duck_and_ui_style_box_union_prepare_as_independent_groups() {
    use crate::model::{BooleanOperation, GroupRenderRepresentation};
    let document: serde_json::Value = serde_json::from_str(crate::duck::DEFAULT_DUCK).unwrap();
    let source: Vec<SdfObject> =
        serde_json::from_value(document["subtree"]["sdf_objects"]["value"]["VecSDFObject"].clone())
            .unwrap();
    let duck_id = source[0].uuid;
    let box_id = source[7].uuid;
    let operands = [source[8].uuid, source[9].uuid];
    let mut tree = crate::model::DataTree::default();
    crate::model::set_objects(&mut tree, source);
    assert!(crate::ui::scene_actions::attach(
        &mut tree,
        box_id,
        &operands,
        BooleanOperation::Union,
    ));
    let mut source = crate::model::objects(&tree);
    source[0].render_representation = GroupRenderRepresentation::GaussianSplats;
    assert!(source[8..10]
        .iter()
        .all(|object| object.boolean_parent == Some(box_id)));

    for mode in [
        GroupRenderRepresentation::ExactSdf,
        GroupRenderRepresentation::SphereAccelerator,
        GroupRenderRepresentation::BoxAccelerator,
        GroupRenderRepresentation::GaussianSplats,
        GroupRenderRepresentation::BoxDepthAtlas,
        GroupRenderRepresentation::SphereDepthAtlas,
    ] {
        source[7].render_representation = mode;
        let prepared = scene_upload::prepare_group_scene(&source, &mut Default::default());
        assert!(prepared.gaussian_splats.contains(&duck_id));
        assert!(prepared.box_depth_atlases.contains_key(&duck_id));
        assert!(prepared.objects.iter().any(|object| object.uuid == duck_id));
        assert!(prepared.objects.iter().any(|object| object.uuid == box_id));
        if matches!(
            mode,
            GroupRenderRepresentation::ExactSdf | GroupRenderRepresentation::SphereAccelerator | GroupRenderRepresentation::BoxAccelerator
        ) {
            assert_eq!(prepared.objects.len(), 4);
            assert!(prepared
                .objects
                .iter()
                .any(|object| object.uuid == source[8].uuid));
            assert!(prepared
                .objects
                .iter()
                .any(|object| object.uuid == source[9].uuid));
        } else {
            assert_eq!(prepared.objects.len(), 2);
            match mode {
                GroupRenderRepresentation::GaussianSplats => {
                    assert!(prepared.gaussian_splats.contains(&box_id));
                    assert!(prepared.box_depth_atlases.contains_key(&box_id));
                }
                GroupRenderRepresentation::BoxDepthAtlas => {
                    assert!(prepared.box_depth_atlases.contains_key(&box_id));
                }
                GroupRenderRepresentation::SphereDepthAtlas => {
                    assert!(prepared.sphere_depth_atlases.contains_key(&box_id));
                }
                GroupRenderRepresentation::ExactSdf
                | GroupRenderRepresentation::NeuralSdf
                | GroupRenderRepresentation::SphereAccelerator
                | GroupRenderRepresentation::BoxAccelerator => {
                    unreachable!()
                }
            }
        }
    }
}

#[test]
fn gaussian_capture_starts_outside_planar_bounds_without_moving_the_surface() {
    let object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    let source = [object];
    let ordinary =
        bake_box_depth_atlas(&source, source[0].uuid, 32, BoxCaptureStart::AtBounds).unwrap();
    let splats =
        bake_box_depth_atlas(&source, source[0].uuid, 32, BoxCaptureStart::OutsideBounds).unwrap();
    assert!((splats.local_max.x - ordinary.local_max.x - 0.5).abs() < 0.0001);
    let center_pixel = 16 * 32 + 16;
    let before = ordinary.texels[center_pixel][0];
    let after = splats.texels[center_pixel][0];
    assert!(after > before);
    assert!(((ordinary.local_max.x - before) - (splats.local_max.x - after)).abs() < 0.02);
}

#[test]
fn gaussian_box_capture_sees_exposed_faces_of_three_cube_union() {
    use crate::model::{BooleanOperation, PrimitiveKind};
    let root = SdfObject::create_kind(PrimitiveKind::Box);
    let mut right = SdfObject::create_kind(PrimitiveKind::Box);
    right.boolean_parent = Some(root.uuid);
    right.operation = BooleanOperation::Union;
    right.transform.translation = Vec3::new(0.8, 0.0, 0.0);
    let mut back = SdfObject::create_kind(PrimitiveKind::Box);
    back.boolean_parent = Some(root.uuid);
    back.operation = BooleanOperation::Union;
    back.transform.translation = Vec3::new(0.0, 0.0, 0.8);
    let ids = [root.uuid, right.uuid, back.uuid];
    let source = [root, right, back];
    let atlas = bake_box_depth_atlas(&source, ids[0], 64, BoxCaptureStart::OutsideBounds).unwrap();
    assert_eq!(atlas.layers, 8);
    let face_size = 64 * 64;
    for id in ids {
        let visible_faces = atlas
            .owners
            .chunks(face_size)
            .filter(|face| face.contains(&Some(id)))
            .count();
        assert!(
            visible_faces >= 3,
            "cube {id} appears in only {visible_faces} captures"
        );
    }
    assert!(atlas
        .normals
        .iter()
        .filter(|normal| normal.length_squared() > 0.0)
        .all(|normal| (normal.length() - 1.0).abs() < 0.001));
    let rear_positive_z = &atlas.owners[(2 * 6 + 4) * face_size..(2 * 6 + 5) * face_size];
    assert!(rear_positive_z.contains(&Some(ids[0])));
}

#[test]
fn splat_hierarchy_keeps_each_spatially_separate_capture_reachable() {
    let mut texels = vec![[0.0; 4]; 10];
    let mut bounds = [-2.0, 0.0, 2.0].map(|x| splat_bvh::SplatBound {
        center: Vec3::new(x, 0.0, 0.0),
        radius: 0.25,
        texel_offset: ((x + 2.0) as u32) * 2,
    });
    let (start, end) = splat_bvh::append_splat_bvh(&mut texels, &mut bounds).unwrap();
    assert_eq!(end - start, 10); // Three leaves and two internal nodes.
    for (x, expected) in [(-2.0, 0.0), (0.0, 4.0), (2.0, 8.0)] {
        let mut node = start;
        let mut reached = Vec::new();
        while node < end {
            let minimum = texels[node as usize];
            let maximum = texels[node as usize + 1];
            if x < minimum[0] || x > maximum[0] || 0.0 < minimum[1] || 0.0 > maximum[1] {
                node = maximum[3] as u32;
            } else {
                if minimum[3] >= 0.0 {
                    reached.push(minimum[3]);
                }
                node += 2;
            }
        }
        assert_eq!(reached, [expected]);
    }
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
    let atlas = bake_box_depth_atlas(
        &source,
        root,
        BOX_DEPTH_RESOLUTION,
        BoxCaptureStart::AtBounds,
    )
    .unwrap();
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

#[test]
fn render_text_resolves_optimized_path_from_source_scene() {
    use crate::model::{BezierCurveParams, BoxParams, PrimitiveKind, TextParams};
    let mut path = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    path.params = SdfParams::BezierCurveParams(BezierCurveParams {
        points: vec![Vec3::ZERO, Vec3::X, Vec3::X * 2.0, Vec3::X * 3.0],
        closed: false,
    });
    let mut text = SdfObject::create_kind(PrimitiveKind::Text);
    text.params = SdfParams::TextParams(TextParams {
        text: "AB".into(),
        path: Some(path.uuid),
        ..TextParams::default()
    });
    let source = [path.clone(), text.clone()];
    let mut proxy_path = path;
    proxy_path.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::ONE,
        corner_radius: 0.0,
    });
    assert!(crate::model::prepared_scene_text(&[proxy_path, text.clone()], &text).is_err());
    let geometry = scene_upload::prepared_render_text(&source, &text).unwrap();
    assert_eq!(geometry.glyphs.len(), 2);
}

#[test]
fn group_capture_reuses_rigid_pose_and_invalidates_surface_changes() {
    use crate::model::{GroupRenderRepresentation, PrimitiveKind};
    let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
    root.render_representation = GroupRenderRepresentation::BoxDepthAtlas;
    let mut child = SdfObject::create_kind(PrimitiveKind::Box);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation.x = 0.8;
    let root_id = root.uuid;
    let mut source = vec![root, child];
    let mut cache = Default::default();
    let initial = scene_upload::prepare_group_scene(&source, &mut cache);
    let initial_atlas = initial.box_depth_atlases[&root_id].clone();
    source[0].group_transform.translation = Vec3::new(3.0, -2.0, 1.0);
    source[0].group_transform.rotation = glam::Quat::from_rotation_y(0.7);
    let moved = scene_upload::prepare_group_scene(&source, &mut cache);
    assert!(Arc::ptr_eq(
        &initial_atlas,
        &moved.box_depth_atlases[&root_id]
    ));
    assert_eq!(
        moved.objects[0].group_transform.translation,
        source[0].group_transform.translation
    );

    source[1].transform.translation.x += 0.3;
    let changed = scene_upload::prepare_group_scene(&source, &mut cache);
    assert!(!Arc::ptr_eq(
        &initial_atlas,
        &changed.box_depth_atlases[&root_id]
    ));
    let previous = changed.box_depth_atlases[&root_id].clone();
    source[1].material.color.x = 0.9;
    let changed = scene_upload::prepare_group_scene(&source, &mut cache);
    assert!(!Arc::ptr_eq(
        &previous,
        &changed.box_depth_atlases[&root_id]
    ));
    let previous = changed.box_depth_atlases[&root_id].clone();
    let mut lattice = crate::model::Lattice::new(-Vec3::ONE, Vec3::ONE, 2);
    lattice.offsets.fill(Vec3::new(0.2, 0.0, 0.0));
    source[0].lattice = Some(lattice);
    let changed = scene_upload::prepare_group_scene(&source, &mut cache);
    assert!(!Arc::ptr_eq(
        &previous,
        &changed.box_depth_atlases[&root_id]
    ));
    assert!(
        changed.objects[0].lattice.is_none(),
        "baked cage must not apply twice"
    );

    // A referenced world-space path can move relative to the capture. Even
    // an unrelated reference conservatively disables pose-only reuse.
    let mut text = SdfObject::create_kind(PrimitiveKind::Text);
    text.params = SdfParams::TextParams(crate::model::TextParams {
        path: Some(uuid::Uuid::new_v4()),
        ..Default::default()
    });
    source.push(text);
    let with_reference = scene_upload::prepare_group_scene(&source, &mut cache);
    source[0].group_transform.translation.x += 1.0;
    let moved_reference = scene_upload::prepare_group_scene(&source, &mut cache);
    assert!(!Arc::ptr_eq(
        &with_reference.box_depth_atlases[&root_id],
        &moved_reference.box_depth_atlases[&root_id],
    ));
}

#[test]
fn prepared_capture_sampler_matches_tree_and_cage_contracts() {
    use crate::model::{
        scene_subtree_sample, BooleanOperation, PreparedSubtreeSampler, PrimitiveKind,
    };
    let mut root = SdfObject::create_kind(PrimitiveKind::Box);
    root.transform.scale = Vec3::new(0.6, 1.4, 0.9);
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation = Vec3::new(0.4, 0.2, -0.1);
    let root_id = root.uuid;
    for operation in [
        BooleanOperation::Union,
        BooleanOperation::Subtract,
        BooleanOperation::Intersect,
    ] {
        child.operation = operation;
        let source = [root.clone(), child.clone()];
        let mut sampler = PreparedSubtreeSampler::new(&source, root_id).unwrap();
        for x in -8..=8 {
            for y in -5..=5 {
                let point = Vec3::new(x as f32 * 0.19, y as f32 * 0.17, 0.23);
                let expected = scene_subtree_sample(point, &source, root_id).unwrap();
                let actual = sampler.sample(point);
                assert!((actual.0 - expected.0).abs() < 0.00001);
                assert_eq!(actual.1, expected.1);
            }
        }
    }
    // A constant cage displacement has an exact inverse and moves both root
    // and inherited child surfaces once, with no numerical solver ambiguity.
    child.operation = BooleanOperation::Union;
    let rest = [root.clone(), child.clone()];
    let mut cage = crate::model::Lattice::new(-Vec3::splat(2.0), Vec3::splat(2.0), 7);
    let shift = Vec3::new(0.45, -0.2, 0.15);
    cage.offsets.fill(shift);
    root.lattice = Some(cage);
    let deformed = [root, child];
    let mut sampler = PreparedSubtreeSampler::new(&deformed, root_id).unwrap();
    for x in -8..=8 {
        let point = Vec3::new(x as f32 * 0.23, 0.12, 0.24);
        let expected = scene_subtree_sample(point - shift, &rest, root_id).unwrap();
        let actual = sampler.sample(point);
        assert!((actual.0 - expected.0).abs() < 0.00001);
        assert_eq!(actual.1, expected.1);
    }
}

#[test]
fn capture_cache_retains_only_admitted_atlases() {
    use crate::model::{GroupRenderRepresentation, PrimitiveKind};
    let mut source: Vec<_> = (0..4)
        .map(|index| {
            let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
            root.transform.translation.x = index as f32 * 2.0;
            root.render_representation = GroupRenderRepresentation::GaussianSplats;
            root
        })
        .collect();
    let mut cache = Default::default();
    let prepared = scene_upload::prepare_group_scene(&source, &mut cache);
    assert!(
        prepared.box_depth_atlases.len() < source.len(),
        "fixture must exceed atlas budget"
    );
    assert_eq!(cache.len(), prepared.box_depth_atlases.len());
    assert!(cache
        .keys()
        .all(|id| prepared.box_depth_atlases.contains_key(id)));

    for root in &mut source {
        root.render_representation = GroupRenderRepresentation::ExactSdf;
    }
    let restored = scene_upload::prepare_group_scene(&source, &mut cache);
    assert!(restored.box_depth_atlases.is_empty());
    assert!(
        cache.is_empty(),
        "dormant Exact captures must release CPU memory"
    );
}

#[test]
fn duck_sphere_capture_can_name_a_surface_beside_the_view_ray() {
    use crate::model::{object_world_matrix, PreparedSubtreeSampler};
    let document: serde_json::Value = serde_json::from_str(crate::duck::DEFAULT_DUCK).unwrap();
    let source: Vec<SdfObject> =
        serde_json::from_value(document["subtree"]["sdf_objects"]["value"]["VecSDFObject"].clone())
            .unwrap();
    let root = source[0].uuid;
    let atlas =
        bake_sphere_depth_atlas(&source, root, SPHERE_DEPTH_WIDTH, SPHERE_DEPTH_HEIGHT).unwrap();
    // A front-facing ray at this position hits the body. The radial map
    // labels its surface point as Head, which lies farther along this ray.
    let point = Vec3::new(-0.32, 0.1, 0.19);
    let (distance, true_owner) = PreparedSubtreeSampler::new(&source, root)
        .unwrap()
        .sample(point);
    assert!(distance.abs() < 0.02);
    assert_eq!(true_owner, root);
    let (minimum, maximum) = crate::model::lattice_bounds(&source, root).unwrap();
    let center = (minimum + maximum) * 0.5;
    let local = crate::model::lattice_world_matrix(&source, root)
        .inverse()
        .transform_point3(point)
        - center;
    let uv = Vec2::new(
        (local.z.atan2(local.x) / std::f32::consts::TAU + 0.5).fract(),
        (local.y / local.length()).clamp(-1.0, 1.0).acos() / std::f32::consts::PI,
    );
    let x = (uv.x * atlas.width as f32).floor() as usize;
    let y = (uv.y * atlas.height as f32)
        .floor()
        .min((atlas.height - 1) as f32) as usize;
    let map_owner = atlas.owners[y * atlas.width as usize + x].unwrap();
    assert_eq!(
        source
            .iter()
            .find(|object| object.uuid == map_owner)
            .unwrap()
            .name,
        "Head"
    );
    let head = source
        .iter()
        .find(|object| object.uuid == map_owner)
        .unwrap();
    let head_world = object_world_matrix(&source, head.uuid);
    let body_world = object_world_matrix(&source, root);
    let mut head_entry = None;
    let mut body_entry = None;
    for sample in 0..300 {
        let along_ray = Vec3::new(point.x, point.y, 1.5 - sample as f32 * 0.01);
        if head_entry.is_none()
            && head.distance_with_matrix_with_geometry(along_ray, head_world, true, None, None)
                < 0.001
        {
            head_entry = Some(sample);
        }
        if body_entry.is_none()
            && source[0].distance_with_matrix_with_geometry(along_ray, body_world, true, None, None)
                < 0.001
        {
            body_entry = Some(sample);
        }
    }
    assert!(body_entry.unwrap() < head_entry.unwrap());
}

#[test]
fn concrete_tower_box_accelerator_reuses_occupied_box_atlas() {
    use crate::model::GroupRenderRepresentation;
    let document: serde_json::Value =
        serde_json::from_str(include_str!("../../examples/concrete_tower.claydash")).unwrap();
    let mut source: Vec<SdfObject> =
        serde_json::from_value(document["subtree"]["sdf_objects"]["value"]["VecSDFObject"].clone())
            .unwrap();
    let root = source[0].uuid;
    let atlas = bake_box_depth_atlas(
        &source,
        root,
        BOX_DEPTH_RESOLUTION,
        BoxCaptureStart::AtBounds,
    )
    .unwrap();
    let repeated: std::collections::HashSet<_> = source
        .iter()
        .filter(|object| object.repetition.enabled)
        .map(|object| object.uuid)
        .collect();
    assert!(atlas
        .owners
        .iter()
        .any(|owner| owner.is_some_and(|id| repeated.contains(&id))));
    source[0].render_representation = GroupRenderRepresentation::BoxAccelerator;
    let mut cache = std::collections::HashMap::new();
    let prepared = super::group_capture::prepare_group_scene_with_neural(
        &source,
        &mut cache,
        &std::collections::HashMap::new(),
        &std::collections::HashSet::from([root]),
    );
    assert_eq!(prepared.objects.len(), source.len());
    assert_eq!(prepared.box_depth_atlases[&root].texels, atlas.texels);
    assert_eq!(prepared.box_depth_atlases[&root].owners, atlas.owners);
}

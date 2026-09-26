use super::*;
use glam::{Quat, Vec2, Vec3};
use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

#[test]
fn rounded_box_keeps_half_extents_and_loads_old_boxes() {
    let mut object = SdfObject::create_kind(PrimitiveKind::Box);
    let SdfParams::BoxParams(params) = &mut object.params else {
        unreachable!()
    };
    params.box_q = Vec3::splat(1.0);
    params.corner_radius = 0.25;
    assert!((object.distance(Vec3::new(1.0, 1.0, 1.0)) - 0.1830127).abs() < 0.0001);
    assert!(object.distance(Vec3::new(1.0, 0.0, 0.0)).abs() < 0.0001);
    let mut saved = serde_json::to_value(&object).unwrap();
    saved["params"]["BoxParams"]
        .as_object_mut()
        .unwrap()
        .remove("corner_radius");
    let old: SdfObject = serde_json::from_value(saved).unwrap();
    let SdfParams::BoxParams(old_params) = old.params else {
        unreachable!()
    };
    assert_eq!(old_params.corner_radius, 0.0);
}

#[test]
fn loft_sections_change_the_sdf_without_a_mesh() {
    let object = SdfObject::create_kind(PrimitiveKind::Loft);
    assert!(object.distance(Vec3::new(0.0, 0.0, 0.5)) < 0.0);
    assert!(object.distance(Vec3::new(0.0, 0.0, 0.6)) > 0.0);
    assert!(object.distance(Vec3::new(-1.0, 0.0, 0.3)) > 0.0);
    assert!(object.distance(Vec3::new(1.2, 0.0, 0.0)) > 0.0);
    let saved = serde_json::to_string(&object).unwrap();
    let loaded: SdfObject = serde_json::from_str(&saved).unwrap();
    assert!(loaded.distance(Vec3::new(0.0, 0.0, 0.5)) < 0.0);
}

#[test]
fn custom_loft_profiles_define_closed_cross_sections_and_interpolate() {
    let mut object = SdfObject::create_kind(PrimitiveKind::Loft);
    let SdfParams::LoftParams(loft) = &mut object.params else {
        unreachable!()
    };
    let rectangle = vec![
        Vec2::new(1.0, 1.0),
        Vec2::new(-1.0, 1.0),
        Vec2::new(-1.0, -1.0),
        Vec2::new(1.0, -1.0),
    ];
    for section in &mut loft.sections {
        section.profile = Some(rectangle.clone());
    }
    assert!(object.distance(Vec3::new(0.0, 0.37, 0.5)) < 0.0);
    assert!(object.distance(Vec3::new(0.0, 0.4, 0.5)) > 0.0);
    assert!(object.distance(Vec3::new(1.2, 0.0, 0.0)) > 0.0);
    if let SdfParams::LoftParams(loft) = &mut object.params {
        for point in loft.sections[2].profile.as_mut().unwrap() {
            if point.x > 0.0 {
                point.x = 0.5;
            }
        }
    }
    assert!(object.distance(Vec3::new(0.0, 0.27, 0.0)) < 0.0);
    assert!(object.distance(Vec3::new(0.0, 0.32, 0.0)) > 0.0);
    let loaded: SdfObject = serde_json::from_str(&serde_json::to_string(&object).unwrap()).unwrap();
    assert!(loaded.distance(Vec3::new(0.0, 0.27, 0.0)) < 0.0);
}

#[test]
fn surface_inlay_follows_a_host_sdf() {
    let mut host = SdfObject::create_kind(PrimitiveKind::Sphere);
    host.params = SdfParams::SphereParams(SphereParams { radius: 1.0 });
    let mut patch = SdfObject::create_kind(PrimitiveKind::Box);
    patch.transform.translation = Vec3::X;
    patch.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(0.4),
        corner_radius: 0.0,
    });
    patch.surface_inlay = Some(SurfaceInlay {
        host: host.uuid,
        offset: 0.025,
        thickness: 0.015,
    });
    let scene = [host, patch.clone()];
    let (distance, owner) = scene_sample(Vec3::new(1.025, 0.0, 0.0), &scene).unwrap();
    assert_eq!(owner, patch.uuid);
    assert!(distance < 0.0);
    assert!(scene_sample(Vec3::new(1.025, 0.6, 0.0), &scene).unwrap().0 > 0.0);
    let loaded: SdfObject = serde_json::from_str(&serde_json::to_string(&patch).unwrap()).unwrap();
    assert_eq!(loaded.surface_inlay, patch.surface_inlay);
}

#[test]
fn surface_inlay_follows_a_smooth_boolean_host() {
    let mut host = SdfObject::create_kind(PrimitiveKind::Sphere);
    host.params = SdfParams::SphereParams(SphereParams { radius: 1.0 });
    host.softness = 0.1;
    let mut fender = SdfObject::create_kind(PrimitiveKind::Sphere);
    fender.params = SdfParams::SphereParams(SphereParams { radius: 0.45 });
    fender.transform.translation = Vec3::X;
    fender.boolean_parent = Some(host.uuid);
    let mut patch = SdfObject::create_kind(PrimitiveKind::Box);
    patch.transform.translation = Vec3::new(1.45, 0.0, 0.0);
    patch.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(0.3),
        corner_radius: 0.0,
    });
    patch.surface_inlay = Some(SurfaceInlay {
        host: host.uuid,
        offset: 0.025,
        thickness: 0.015,
    });
    let scene = [host, fender, patch.clone()];
    let (distance, owner) = scene_sample(Vec3::new(1.475, 0.0, 0.0), &scene).unwrap();
    assert_eq!(owner, patch.uuid);
    assert!(distance < 0.0);
}

#[test]
fn selecting_the_same_objects_keeps_the_render_selection_version() {
    let mut tree = DataTree::default();
    let empty_version = tree.path_version("scene.selected_uuids");
    set_selected(&mut tree, Vec::new());
    assert_eq!(tree.path_version("scene.selected_uuids"), empty_version);
    let id = uuid::Uuid::new_v4();
    set_selected(&mut tree, vec![id]);
    let version = tree.path_version("scene.selected_uuids");
    set_selected(&mut tree, vec![id]);
    set_selected_exact(&mut tree, vec![id]);
    assert_eq!(tree.path_version("scene.selected_uuids"), version);
}

#[test]
fn lattice_interpolates_hidden_points_and_preserves_shape_when_resized() {
    let mut lattice = Lattice::new(Vec3::splat(-1.0), Vec3::ONE, 2);
    let corner = lattice.index(1, 1, 1);
    lattice.offsets[corner] = Vec3::new(0.8, 0.0, 0.0);
    assert!((lattice.displacement(Vec3::ZERO).x - 0.1).abs() < 0.0001);
    lattice.resize(5);
    assert_eq!(lattice.offsets.len(), 125);
    assert!((lattice.displacement(Vec3::ZERO).x - 0.1).abs() < 0.0001);
    assert!((lattice.offsets[lattice.index(4, 4, 4)].x - 0.8).abs() < 0.0001);
    let mut cage = Lattice::new(Vec3::ZERO, Vec3::ONE, 3);
    let face = cage.index(0, 1, 1);
    cage.offsets[face] = Vec3::new(0.6, 0.0, 0.0);
    assert!((cage.effective_offsets()[cage.index(1, 1, 1)].x - 0.1).abs() < 0.0001);
}

#[test]
fn lattice_bounds_cover_complete_boolean_group_and_save_with_scene() {
    let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
    let mut child = SdfObject::create_kind(PrimitiveKind::Box);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation = Vec3::new(2.0, 0.0, 0.0);
    let (min, max) = lattice_bounds(&[root.clone(), child], root.uuid).unwrap();
    assert!(min.x < -0.25 && max.x > 2.3);
    root.lattice = Some(Lattice::new(min, max, 3));
    let encoded = serde_json::to_string(&root).unwrap();
    let restored: SdfObject = serde_json::from_str(&encoded).unwrap();
    assert_eq!(restored.lattice, root.lattice);
}

#[test]
fn single_object_lattice_moves_with_its_object_transform() {
    let mut sphere = SdfObject::create_kind(PrimitiveKind::Sphere);
    sphere.transform.translation = Vec3::new(4.0, 2.0, -1.0);
    let scene = [sphere.clone()];
    let (min, max) = lattice_bounds(&scene, sphere.uuid).unwrap();
    assert!(min.x < 0.0 && max.x > 0.0);
    assert!(
        (lattice_world_matrix(&scene, sphere.uuid).transform_point3(Vec3::ZERO)
            - sphere.transform.translation)
            .length()
            < 0.0001
    );
}

#[test]
fn primitive_kind_maps_every_gpu_type_explicitly() {
    for kind in PrimitiveKind::ALL {
        assert_eq!(PrimitiveKind::from_object_type(kind.object_type()), kind);
    }
}

#[test]
fn bezier_extrusion_follows_edited_3d_controls_and_survives_save() {
    let mut object = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    let SdfParams::BezierCurveParams(curve) = &mut object.params else {
        unreachable!()
    };
    curve.points = vec![
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-0.5, 0.0, 1.0),
        Vec3::new(0.5, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 0.0),
    ];
    assert!((curve.point(0, 0.5).z - 0.75).abs() < 0.0001);
    assert!(object.distance(Vec3::new(0.0, 0.0, 0.75)) > 0.0);
    object.path_extrusion = Some(PathExtrusion {
        radius: 0.2,
        ..Default::default()
    });
    assert!(object.distance(Vec3::new(0.0, 0.0, 0.75)) < 0.0);
    assert!(object.distance(Vec3::new(0.0, 0.5, 0.75)) > 0.0);
    let restored: SdfObject =
        serde_json::from_str(&serde_json::to_string(&object).unwrap()).unwrap();
    assert!(restored.distance(Vec3::new(0.0, 0.0, 0.75)) < 0.0);
    let SdfParams::BezierCurveParams(curve) = restored.params else {
        unreachable!()
    };
    assert_eq!(curve.points[1].z, 1.0);
}

#[test]
fn bezier_square_profile_is_distinct_from_round_profile() {
    let mut object = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    let SdfParams::BezierCurveParams(curve) = &mut object.params else {
        unreachable!()
    };
    curve.points = vec![
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-0.3, 0.0, 0.0),
        Vec3::new(0.3, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    ];
    object.path_extrusion = Some(PathExtrusion {
        radius: 0.2,
        ..Default::default()
    });
    let corner = Vec3::new(0.0, 0.16, 0.16);
    assert!(object.distance(corner) > 0.0);
    object.path_extrusion.as_mut().unwrap().profile = BezierProfile::Square;
    assert!(object.distance(corner) < 0.0);
}

#[test]
fn extending_bezier_curve_preserves_join_tangent() {
    let mut object = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    let SdfParams::BezierCurveParams(curve) = &mut object.params else {
        unreachable!()
    };
    let join = curve.points[3];
    let incoming = join - curve.points[2];
    assert_eq!(curve.extend_from_end(), Some(6));
    assert_eq!(curve.points[3], join);
    assert_eq!(curve.points[4] - join, incoming);
    assert_eq!(curve.segment_count(), 2);
    assert_eq!(curve.point(0, 1.0), curve.point(1, 0.0));
    let next_midpoint = curve.point(1, 0.5);
    object.path_extrusion = Some(PathExtrusion::default());
    assert!(object.distance(next_midpoint) < 0.0);
}

#[test]
fn deleting_an_interior_curve_anchor_joins_its_neighbors() {
    let mut object = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    let SdfParams::BezierCurveParams(curve) = &mut object.params else {
        unreachable!()
    };
    curve.extend_from_end();
    curve.extend_from_end();
    let before = curve.points.clone();
    assert_eq!(curve.delete_anchor(3), Some(0));
    assert_eq!(curve.segment_count(), 2);
    assert_eq!(curve.points[0], before[0]);
    assert_eq!(curve.points[1], before[1]);
    assert_eq!(curve.points[2], before[5]);
    assert_eq!(curve.points[3], before[6]);
    assert_eq!(curve.points[6], before[9]);
}

#[test]
fn closing_a_curve_joins_end_to_start_with_a_smooth_seam() {
    let mut object = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    let SdfParams::BezierCurveParams(curve) = &mut object.params else {
        unreachable!()
    };
    assert!(curve.close_from_end());
    assert!(curve.closed);
    assert_eq!(curve.points[0], *curve.points.last().unwrap());
    let last = curve.points.len() - 1;
    assert_eq!(
        curve.points[1] - curve.points[0],
        curve.points[last] - curve.points[last - 1]
    );
    assert_eq!(curve.segment_count(), 2);
    assert_eq!(curve.extend_from_end(), None);
}

#[test]
fn another_curve_can_define_the_path_extrusions_cross_section() {
    let mut path = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    let SdfParams::BezierCurveParams(path_curve) = &mut path.params else {
        unreachable!()
    };
    path_curve.points = vec![
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-0.3, 0.0, 0.0),
        Vec3::new(0.3, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    ];
    let mut profile = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    let SdfParams::BezierCurveParams(profile_curve) = &mut profile.params else {
        unreachable!()
    };
    profile_curve.points = vec![
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(-1.0, 1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
    ];
    path.path_extrusion = Some(PathExtrusion {
        radius: 0.3,
        profile_curve: Some(profile.uuid),
        ..Default::default()
    });
    let scene = [path, profile];
    assert!(scene_sample(Vec3::ZERO, &scene).unwrap().0 < 0.0);
    assert!(scene_sample(Vec3::new(0.0, 0.0, 0.7), &scene).unwrap().0 > 0.0);
    assert_eq!(scene[1].distance(Vec3::ZERO), 100.0);
}

#[test]
fn polygon_prism_distance_covers_sides_caps_and_concavity() {
    let mut prism = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    prism.params = SdfParams::PolygonPrismParams(PolygonPrismParams {
        vertices: vec![
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ],
        half_depth: 0.25,
    });
    assert!(prism.distance(Vec3::new(-0.5, 0.0, 0.0)) < 0.0);
    assert!(prism.distance(Vec3::new(0.8, 0.0, 0.0)) > 0.0);
    assert!(prism.distance(Vec3::new(-0.5, 0.0, 0.5)) > 0.0);
}

#[test]
fn box_faces_remain_selectable_after_boolean_cuts() {
    let source = SdfObject::create_kind(PrimitiveKind::Box);
    let mut cutter = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    cutter.boolean_parent = Some(source.uuid);
    cutter.operation = BooleanOperation::Subtract;
    let source_id = source.uuid;
    let scene = [source, cutter];

    let face = box_face_at_world_position(&scene, source_id, Vec3::new(0.0, 0.0, 0.3));
    assert_eq!(
        face,
        Some(BoxFaceSelection {
            object: source_id,
            axis: VectorAxis::Z,
            positive: true,
        })
    );
    assert_eq!(
        box_face_at_world_position(&scene, source_id, Vec3::new(0.0, 0.0, 0.1)),
        None,
        "an interior cut wall must not masquerade as an original box face"
    );
}

#[test]
fn polygon_prism_caps_and_sides_are_explicitly_selectable() {
    let prism = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    let id = prism.uuid;
    let scene = [prism];

    assert_eq!(
        modeling_face_at_world_position(&scene, id, Vec3::new(0.0, -0.1, 0.1)),
        Some(ModelingFaceSelection::PolygonPrism(
            PolygonPrismFaceSelection {
                object: id,
                face: PolygonPrismFace::Cap { positive: true },
            }
        ))
    );
    assert_eq!(
        modeling_face_at_world_position(&scene, id, Vec3::new(0.0, -0.2, 0.0)),
        Some(ModelingFaceSelection::PolygonPrism(
            PolygonPrismFaceSelection {
                object: id,
                face: PolygonPrismFace::Side { edge: 0 },
            }
        ))
    );
    assert_eq!(
        modeling_face_at_world_position(&scene, id, Vec3::ZERO),
        None
    );
}

#[test]
fn finite_domain_repetition_creates_pickable_copies() {
    let mut sphere = SdfObject::create_kind(PrimitiveKind::Sphere);
    sphere.repetition.enabled = true;
    sphere.repetition.count = [3, 1, 1];
    sphere.repetition.spacing = Vec3::splat(1.0);

    assert!(sphere.distance(Vec3::X) < 0.0);
    assert!(sphere.distance(Vec3::X * 2.0) > 0.5);
}

#[test]
fn legacy_repeat_axis_flags_become_counts_on_load() {
    let repetition: Repetition = serde_json::from_value(serde_json::json!({
        "enabled": true,
        "axes": [false, true, false],
        "count": [3, 4, 5],
        "spacing": [1.0, 2.0, 3.0]
    }))
    .unwrap();
    assert_eq!(repetition.count, [1, 4, 1]);
    let saved = serde_json::to_value(repetition).unwrap();
    assert!(saved.get("axes").is_none());
    assert_eq!(saved["count"], serde_json::json!([1, 4, 1]));
}

#[test]
fn repeating_a_boolean_group_copies_its_assembled_shape() {
    let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
    root.group_transform.translation = Vec3::new(0.4, -0.2, 0.0);
    root.transform.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation = Vec3::X;
    let child_id = child.uuid;
    let base = [root.clone(), child.clone()];
    let child_center = object_world_matrix(&base, child_id).transform_point3(Vec3::ZERO);
    root.repetition.enabled = true;
    root.repetition.spacing = Vec3::splat(3.0);
    let repeated = [root, child];
    let copy = child_center + Vec3::X * 3.0;

    assert!(scene_sample(child_center, &base).unwrap().0 < 0.0);
    assert!(scene_sample(copy, &base).unwrap().0 > 0.5);
    let sample = scene_sample(copy, &repeated).unwrap();
    assert!(sample.0 < 0.0);
    assert_eq!(sample.1, child_id);
}

#[test]
fn mirror_reflects_complete_boolean_group_in_group_axes() {
    let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
    root.group_transform.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    root.transform.translation = Vec3::new(1.0, 0.0, 0.0);
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation = Vec3::new(2.0, 0.0, 0.0);
    root.mirror = Some(Mirror::default());
    let scene = [root.clone(), child.clone()];
    let group = group_world_matrix(&scene, root.uuid);
    let original = group.transform_point3(Vec3::new(2.0, 0.0, 0.0));
    let reflected = group.transform_point3(Vec3::new(-2.0, 0.0, 0.0));
    assert!(scene_sample(original, &scene).unwrap().0 < 0.0);
    let sample = scene_sample(reflected, &scene).unwrap();
    assert!(sample.0 < 0.0);
    assert_eq!(sample.1, child.uuid);
    let restored: SdfObject = serde_json::from_str(&serde_json::to_string(&root).unwrap()).unwrap();
    assert_eq!(restored.mirror, root.mirror);
}

#[test]
fn material_presets_expose_distinct_surface_properties() {
    let transparent = Material::preset(MaterialKind::Transparent);
    let metallic = Material::preset(MaterialKind::Metallic);
    let solid = Material::preset(MaterialKind::Solid);

    assert!(transparent.opacity < solid.opacity);
    assert!(metallic.metallic > solid.metallic);
    assert!(transparent.refractive_index > 1.0);
}

#[test]
fn brick_settings_round_trip_and_legacy_materials_load() {
    let mut brick = Material::preset(MaterialKind::Brick);
    brick.brick.width = 0.64;
    brick.brick.mortar_width = 0.026;
    let serialized = serde_json::to_value(brick).unwrap();
    assert_eq!(
        serde_json::from_value::<Material>(serialized.clone()).unwrap(),
        brick
    );
    let mut legacy = serialized.clone();
    legacy.as_object_mut().unwrap().remove("brick");
    assert_eq!(
        serde_json::from_value::<Material>(legacy).unwrap().brick,
        BrickSettings::default()
    );
    let mut early_brick = serialized;
    let settings = early_brick
        .get_mut("brick")
        .unwrap()
        .as_object_mut()
        .unwrap();
    settings.remove("relief");
    settings.remove("bevel");
    settings.remove("mortar_color");
    let restored: Material = serde_json::from_value(early_brick).unwrap();
    assert_eq!(restored.brick.width, 0.64);
    assert_eq!(restored.brick.relief, BrickSettings::default().relief);
    assert_eq!(
        restored.brick.mortar_color,
        BrickSettings::default().mortar_color
    );
}

#[test]
fn wood_species_are_distinct_and_old_materials_load_with_default_grain() {
    let oak = Material::wood_preset(WoodSpecies::Oak);
    let walnut = Material::wood_preset(WoodSpecies::Walnut);
    let pine = Material::wood_preset(WoodSpecies::Pine);
    assert_ne!(oak.color, walnut.color);
    assert_ne!(oak.wood.ring_spacing, pine.wood.ring_spacing);
    assert_eq!(MaterialAsset::new(oak).name, "Oak");

    let mut legacy = serde_json::to_value(oak).unwrap();
    legacy.as_object_mut().unwrap().remove("wood");
    let restored: Material = serde_json::from_value(legacy).unwrap();
    assert_eq!(restored.wood, WoodSettings::default());

    let mut finished = walnut;
    finished.wood.cut_angle = 0.7;
    finished.wood.fiber_pigment = 0.9;
    finished.wood.stain_color = WoodStain::Smoked;
    finished.wood.stain_load = 0.6;
    finished.wood.coat = 0.8;
    assert_eq!(
        serde_json::from_str::<Material>(&serde_json::to_string(&finished).unwrap()).unwrap(),
        finished
    );
}

#[test]
fn renamed_material_asset_keeps_its_name_when_edited() {
    let mut tree = DataTree::default();
    let mut material = Material::preset(MaterialKind::Metallic);
    let id = ensure_material_asset(&mut tree, material);

    assert!(rename_material_asset(&mut tree, id, "Brushed steel".into()));
    material.roughness = 0.4;
    update_material_asset(&mut tree, id, material);

    let asset = material_assets(&tree)
        .into_iter()
        .find(|asset| asset.uuid == id)
        .unwrap();
    assert_eq!(asset.name, "Brushed steel");
    assert_eq!(asset.material.roughness, 0.4);
}

#[test]
fn unlinking_material_creates_an_independently_named_copy() {
    let mut tree = DataTree::default();
    let material = Material::preset(MaterialKind::Wood);
    let source_id = ensure_material_asset(&mut tree, material);
    rename_material_asset(&mut tree, source_id, "Walnut".into());

    let copy_id = create_unlinked_material_asset(&mut tree, Some(source_id), material);
    assert_ne!(copy_id, source_id);
    let assets = material_assets(&tree);
    assert_eq!(assets.len(), 2);
    assert_eq!(
        assets
            .iter()
            .find(|asset| asset.uuid == copy_id)
            .unwrap()
            .name,
        "Walnut copy"
    );
}

#[test]
fn softness_blends_all_operations_and_zero_preserves_hard_edges() {
    assert_eq!(
        boolean_distance(0.0, 0.0, BooleanOperation::Union, 0.2),
        -0.05
    );
    for operation in [BooleanOperation::Subtract, BooleanOperation::Intersect] {
        assert_eq!(boolean_distance(0.0, 0.0, operation, 0.2), 0.05);
    }
    for operation in [
        BooleanOperation::Union,
        BooleanOperation::Subtract,
        BooleanOperation::Intersect,
    ] {
        let b = if operation == BooleanOperation::Subtract {
            -0.7
        } else {
            0.7
        };
        let hard = if operation == BooleanOperation::Union {
            (-0.1_f32).min(b)
        } else {
            (-0.1_f32).max(b)
        };
        assert_eq!(boolean_distance(-0.1, 0.7, operation, 0.0), hard);
        assert_eq!(boolean_distance(-0.1, 0.7, operation, 0.05), hard);
    }
}

#[test]
fn boolean_group_owns_the_softness_used_for_its_operands() {
    let mut group = SdfObject::create(TYPE_SPHERE);
    group.softness = 0.2;
    let mut operand = SdfObject::create(TYPE_SPHERE);
    operand.boolean_parent = Some(group.uuid);
    operand.transform.translation.x = 0.5;
    operand.softness = 0.0;
    let join = Vec3::X * 0.25;

    let soft = scene_sample(join, &[group.clone(), operand.clone()])
        .unwrap()
        .0;
    assert!((soft + 0.05).abs() < 0.0001);

    group.softness = 0.0;
    operand.softness = 0.2;
    let hard = scene_sample(join, &[group, operand]).unwrap().0;
    assert!(hard.abs() < 0.0001);
}

#[test]
fn new_objects_are_soft_but_old_documents_keep_their_geometry() {
    let object = SdfObject::create_kind(PrimitiveKind::Sphere);
    assert_eq!(object.softness, 0.05);
    let mut value = serde_json::to_value(&object).unwrap();
    value.as_object_mut().unwrap().remove("softness");
    let old: SdfObject = serde_json::from_value(value).unwrap();
    assert_eq!(old.softness, 0.0);
}

#[test]
fn boolean_cut_does_not_remove_an_unrelated_root() {
    let mut target = SdfObject::create(TYPE_BOX);
    target.transform.scale = Vec3::splat(4.0);
    let mut cutter = SdfObject::create(TYPE_SPHERE);
    cutter.operation = BooleanOperation::Subtract;
    cutter.boolean_parent = Some(target.uuid);
    let island = SdfObject::create(TYPE_SPHERE);
    assert!(
        scene_sample(Vec3::ZERO, &[target.clone(), cutter.clone()])
            .unwrap()
            .0
            > 0.0
    );
    assert!(
        scene_sample(Vec3::ZERO, &[island, cutter, target])
            .unwrap()
            .0
            < 0.0
    );
}

#[test]
fn nested_cutter_group_is_evaluated_before_subtraction() {
    let mut target = SdfObject::create(TYPE_BOX);
    target.transform.scale = Vec3::splat(4.0);
    let mut cutter = SdfObject::create(TYPE_SPHERE);
    cutter.boolean_parent = Some(target.uuid);
    cutter.operation = BooleanOperation::Subtract;
    let mut cutter_hole = SdfObject::create(TYPE_SPHERE);
    cutter_hole.transform.scale = Vec3::splat(0.4);
    cutter_hole.boolean_parent = Some(cutter.uuid);
    cutter_hole.operation = BooleanOperation::Subtract;
    let scene = [cutter_hole, target, cutter];
    assert!(scene_sample(Vec3::ZERO, &scene).unwrap().0 < 0.0);
    assert!(scene_sample(Vec3::X * 0.2, &scene).unwrap().0 > 0.0);
}

#[test]
fn nested_group_transforms_compose_without_changing_primitive_transforms() {
    let mut root = SdfObject::create(TYPE_BOX);
    root.group_transform.translation = Vec3::new(2.0, 0.0, 0.0);
    let mut child = SdfObject::create(TYPE_SPHERE);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation = Vec3::new(0.5, 0.0, 0.0);
    child.group_transform.translation = Vec3::new(0.0, 3.0, 0.0);
    let mut grandchild = SdfObject::create(TYPE_SPHERE);
    grandchild.boolean_parent = Some(child.uuid);
    grandchild.transform.translation = Vec3::new(0.0, 0.0, 4.0);
    let scene = vec![root.clone(), child.clone(), grandchild.clone()];

    assert_eq!(
        object_world_matrix(&scene, root.uuid).transform_point3(Vec3::ZERO),
        Vec3::new(2.0, 0.0, 0.0)
    );
    assert_eq!(
        object_world_matrix(&scene, child.uuid).transform_point3(Vec3::ZERO),
        Vec3::new(2.5, 3.0, 0.0)
    );
    assert_eq!(
        object_world_matrix(&scene, grandchild.uuid).transform_point3(Vec3::ZERO),
        Vec3::new(2.0, 3.0, 4.0)
    );
    assert_eq!(child.transform.translation, Vec3::new(0.5, 0.0, 0.0));
    assert_eq!(grandchild.transform.translation, Vec3::new(0.0, 0.0, 4.0));
}

#[test]
fn group_rotation_and_scale_affect_every_primitive_in_the_subtree() {
    let mut root = SdfObject::create(TYPE_BOX);
    root.transform.translation = Vec3::X;
    root.group_transform.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    root.group_transform.scale = Vec3::splat(2.0);
    let mut child = SdfObject::create(TYPE_SPHERE);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation = Vec3::X * 2.0;
    let scene = vec![root.clone(), child.clone()];

    let root_position = object_world_matrix(&scene, root.uuid).transform_point3(Vec3::ZERO);
    let child_position = object_world_matrix(&scene, child.uuid).transform_point3(Vec3::ZERO);
    assert!((root_position - Vec3::Y * 2.0).length() < 0.0001);
    assert!((child_position - Vec3::Y * 4.0).length() < 0.0001);
    assert_eq!(root.transform.translation, Vec3::X);
    assert_eq!(child.transform.translation, Vec3::X * 2.0);
}

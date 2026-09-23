use super::*;
use glam::{Quat, Vec3};
use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

#[test]
fn primitive_kind_maps_every_gpu_type_explicitly() {
    for kind in PrimitiveKind::ALL {
        assert_eq!(PrimitiveKind::from_object_type(kind.object_type()), kind);
    }
}

#[test]
fn finite_domain_repetition_creates_pickable_copies() {
    let mut sphere = SdfObject::create_kind(PrimitiveKind::Sphere);
    sphere.repetition.enabled = true;
    sphere.repetition.axes = [true, false, false];
    sphere.repetition.count = [3, 1, 1];
    sphere.repetition.spacing = Vec3::splat(1.0);

    assert!(sphere.distance(Vec3::X) < 0.0);
    assert!(sphere.distance(Vec3::X * 2.0) > 0.5);
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

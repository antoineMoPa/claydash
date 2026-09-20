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

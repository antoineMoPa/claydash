use claydash_engine::model::{
    raymarch_hit, scene_sample, BooleanOperation, PrimitiveKind, SdfObject,
};
use glam::Vec3;

#[test]
fn serialized_scene_preserves_distances_and_visible_ray_owner() {
    let outer = SdfObject::create_kind(PrimitiveKind::Sphere);
    let mut cutter = SdfObject::create_kind(PrimitiveKind::Sphere);
    cutter.boolean_parent = Some(outer.uuid);
    cutter.operation = BooleanOperation::Subtract;
    cutter.transform.scale = Vec3::splat(0.5);
    let bytes = serde_json::to_vec(&vec![outer.clone(), cutter]).unwrap();
    let scene: Vec<SdfObject> = serde_json::from_slice(&bytes).unwrap();
    assert!(scene_sample(Vec3::ZERO, &scene).unwrap().0 > 0.0);
    let hit = raymarch_hit(Vec3::new(0.0, 0.0, 5.0), -Vec3::Z, &scene).unwrap();
    assert_eq!(hit.object, outer.uuid);
    assert!(scene_sample(hit.position, &scene).unwrap().0.abs() < 0.01);
    assert!(raymarch_hit(Vec3::new(0.0, 0.0, 5.0), Vec3::Z, &scene).is_none());
}

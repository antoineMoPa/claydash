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
                            object.softness,
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
fn soft_union_bounds_account_for_nonuniform_scale_and_nested_blends() {
    let mut operand = GpuObject::zeroed();
    operand.meta = [0, 1, 0, 2];
    operand.params[3] = 0.5;
    operand.inverse_rows = [
        [0.1, 0.0, 0.0, 0.0],
        [0.0, 2.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    operand.component[2] = 0.2_f32.to_bits();
    let mut root = operand;
    root.meta[3] = -1;
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

use super::*;
use crate::model::BooleanOperation;

#[test]
fn all_new_primitives_inherit_the_complete_picked_material() {
    let mut tree = DataTree::default();
    let mut material = crate::model::Material::preset(crate::model::MaterialKind::Wood);
    material.roughness = 0.37;
    let material_id = crate::model::ensure_material_asset(&mut tree, material);
    tree.set_path("editor.material", ClaydashValue::Material(material));
    tree.set_path("editor.material_id", ClaydashValue::Uuid(material_id));
    for kind in [TYPE_BOX, TYPE_SPHERE, TYPE_CYLINDER, TYPE_TORUS] {
        spawn(&mut tree, kind);
        let scene = objects(&tree);
        let object = scene.last().unwrap();
        assert_eq!(object.material.kind, material.kind);
        assert_eq!(object.material.roughness, material.roughness);
        assert_eq!(object.color, material.color);
        assert_eq!(object.material_id, Some(material_id));
    }
}

fn group_tree() -> DataTree {
    let mut tree = DataTree::default();
    let target = SdfObject::create(TYPE_BOX);
    let mut cutter = SdfObject::create(TYPE_SPHERE);
    cutter.boolean_parent = Some(target.uuid);
    cutter.operation = BooleanOperation::Subtract;
    set_selected(&mut tree, vec![target.uuid]);
    set_objects(&mut tree, vec![target, cutter]);
    tree
}

#[test]
fn invert_selection_selects_only_previously_unselected_objects() {
    let mut tree = DataTree::default();
    let scene = vec![
        SdfObject::create(TYPE_BOX),
        SdfObject::create(TYPE_SPHERE),
        SdfObject::create(TYPE_CYLINDER),
    ];
    set_selected(&mut tree, vec![scene[1].uuid]);
    set_objects(&mut tree, scene.clone());

    invert_selection(&mut tree);

    assert_eq!(selected(&tree), vec![scene[0].uuid, scene[2].uuid]);
}

#[test]
fn deleting_a_group_removes_its_operands() {
    let mut tree = group_tree();
    delete(&mut tree);
    assert!(objects(&tree).is_empty());
    assert!(selected(&tree).is_empty());
}

#[test]
fn deleting_an_exactly_selected_split_prism_removes_its_void() {
    let mut tree = DataTree::default();
    let source = SdfObject::create(TYPE_BOX);
    let mut prism = SdfObject::create(sdf_consts::TYPE_POLYGON_PRISM);
    prism.boolean_parent = Some(source.uuid);
    let mut void = SdfObject::create(sdf_consts::TYPE_POLYGON_PRISM);
    void.boolean_parent = Some(prism.uuid);
    void.operation = BooleanOperation::Subtract;
    crate::model::set_selected_exact(&mut tree, vec![prism.uuid]);
    set_objects(&mut tree, vec![source.clone(), prism, void]);

    delete(&mut tree);

    let scene = objects(&tree);
    assert_eq!(scene.len(), 1);
    assert_eq!(scene[0].uuid, source.uuid);
}

#[test]
fn duplicating_a_group_remaps_operand_parents() {
    let mut tree = group_tree();
    duplicate(&mut tree);
    let scene = objects(&tree);
    assert_eq!(scene.len(), 4);
    assert_eq!(scene[3].boolean_parent, Some(scene[2].uuid));
    assert_eq!(scene[3].operation, BooleanOperation::Subtract);
    assert_ne!(scene[2].uuid, scene[0].uuid);
    assert_eq!(selected(&tree), vec![scene[2].uuid, scene[3].uuid]);
}

#[test]
fn duplicating_a_cutter_keeps_its_target_and_operation() {
    let mut tree = group_tree();
    let original = objects(&tree);
    duplicate_object(&mut tree, original[1].uuid);
    let scene = objects(&tree);
    assert_eq!(scene.len(), 3);
    assert_eq!(scene[2].boolean_parent, Some(original[0].uuid));
    assert_eq!(scene[2].operation, BooleanOperation::Subtract);
    assert_ne!(scene[2].uuid, original[1].uuid);
    assert_eq!(selected(&tree), vec![scene[2].uuid]);
}

#[test]
fn camera_objects_are_regular_transform_targets() {
    let mut tree = DataTree::default();
    let view = crate::camera::Camera::new();
    let camera = crate::camera::SceneCamera::from_view("Camera", &view);
    let id = camera.uuid;
    crate::model::set_scene_cameras(&mut tree, vec![camera]);
    set_selected(&mut tree, vec![id]);

    let target = transform_targets(&tree)[0];
    assert_eq!(target.kind, TransformTargetKind::Camera);
    let mut scene = objects(&tree);
    let mut cameras = crate::model::scene_cameras(&tree);
    let mut transform = target.transform;
    transform.translation.x += 2.0;
    transform.rotation = glam::Quat::from_rotation_y(0.5);
    transform.scale = glam::Vec3::splat(1.5);
    set_transform_target(
        &mut scene,
        &mut cameras,
        TransformTargetKind::Camera,
        id,
        transform,
    );

    assert_eq!(cameras[0].transform, transform);
}

#[test]
fn extruding_a_box_face_creates_an_adjacent_selected_box() {
    let mut tree = DataTree::default();
    let mut source = SdfObject::create(TYPE_BOX);
    source.transform.translation = glam::Vec3::new(1.0, 2.0, 3.0);
    source.transform.rotation = glam::Quat::from_rotation_z(0.4);
    source.transform.scale = glam::Vec3::new(1.5, 0.8, 1.2);
    let source_id = source.uuid;
    let expected_offset = source
        .transform
        .matrix()
        .transform_vector3(glam::Vec3::X * (0.3 + EXTRUSION_INITIAL_HALF_EXTENT));
    set_objects(&mut tree, vec![source.clone()]);
    crate::model::set_selected_exact(&mut tree, vec![source_id]);
    crate::model::set_selected_box_face(
        &mut tree,
        Some(crate::model::BoxFaceSelection {
            object: source_id,
            axis: crate::model::VectorAxis::X,
            positive: true,
        }),
    );

    extrude_selected_face(&mut tree);

    let scene = objects(&tree);
    assert_eq!(scene.len(), 2);
    let (
        crate::model::SdfParams::BoxParams(source_params),
        crate::model::SdfParams::BoxParams(extrusion_params),
    ) = (&source.params, &scene[1].params)
    else {
        unreachable!()
    };
    assert_eq!(extrusion_params.box_q.y, source_params.box_q.y);
    assert_eq!(extrusion_params.box_q.z, source_params.box_q.z);
    assert_eq!(extrusion_params.box_q.x, EXTRUSION_INITIAL_HALF_EXTENT);
    assert!(
        scene[1]
            .transform
            .translation
            .distance(source.transform.translation + expected_offset)
            < 0.0001
    );
    assert_eq!(selected(&tree), vec![scene[1].uuid]);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Extruding)
    ));
    assert_eq!(
        crate::model::selected_box_face(&tree),
        Some(crate::model::BoxFaceSelection {
            object: scene[1].uuid,
            axis: crate::model::VectorAxis::X,
            positive: true,
        })
    );
}

#[test]
fn extruding_the_bottom_cylinder_cap_creates_a_matching_adjacent_cylinder() {
    let mut tree = DataTree::default();
    let source = SdfObject::create(TYPE_CYLINDER);
    let source_id = source.uuid;
    set_objects(&mut tree, vec![source]);
    set_selected(&mut tree, vec![source_id]);
    crate::model::set_selected_modeling_face(
        &mut tree,
        Some(crate::model::ModelingFaceSelection::CylinderCap(
            crate::model::CylinderCapSelection {
                object: source_id,
                positive: false,
            },
        )),
    );
    extrude_selected_face(&mut tree);
    let scene = objects(&tree);
    assert_eq!(scene.len(), 2);
    let crate::model::SdfParams::CylinderParams {
        radius,
        half_height,
    } = scene[1].params
    else {
        unreachable!()
    };
    assert_eq!(radius, 0.25);
    assert_eq!(half_height, EXTRUSION_INITIAL_HALF_EXTENT);
    assert!((scene[1].transform.translation.y + 0.35 + half_height).abs() < 0.0001);
    assert_eq!(selected(&tree), vec![scene[1].uuid]);
}

#[test]
fn extrude_command_extends_the_selected_curve_from_its_endpoint() {
    let mut tree = DataTree::default();
    let curve = SdfObject::create_kind(crate::model::PrimitiveKind::BezierCurve);
    let id = curve.uuid;
    set_objects(&mut tree, vec![curve]);
    set_selected(&mut tree, vec![id]);
    crate::model::set_selected_curve_point(
        &mut tree,
        Some(crate::model::CurvePointSelection {
            object: id,
            index: 3,
        }),
    );
    extrude_selected_face(&mut tree);
    let scene = objects(&tree);
    let crate::model::SdfParams::BezierCurveParams(curve) = &scene[0].params else {
        unreachable!()
    };
    assert_eq!(curve.segment_count(), 2);
    assert_eq!(crate::model::selected_curve_point(&tree).unwrap().index, 6);
    assert!(matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::ExtendingCurve)
    ));
    finish(&mut tree);
    extrude_selected_face(&mut tree);
    let scene = objects(&tree);
    let crate::model::SdfParams::BezierCurveParams(curve) = &scene[0].params else {
        unreachable!()
    };
    assert_eq!(curve.segment_count(), 3);
}

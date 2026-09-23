use super::frame::{source_and_frame, FaceDomain};
use super::guides::*;
use super::shape::*;
use super::*;

#[test]
fn polygon_validation_accepts_concave_shapes_and_rejects_crossings() {
    let concave = [
        Vec2::new(-1.0, -1.0),
        Vec2::new(1.0, -1.0),
        Vec2::ZERO,
        Vec2::new(1.0, 1.0),
        Vec2::new(-1.0, 1.0),
    ];
    let crossed = [
        Vec2::new(-1.0, -1.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(-1.0, 1.0),
        Vec2::new(1.0, -1.0),
    ];
    assert!(polygon_is_valid(&concave));
    assert!(!polygon_is_valid(&crossed));
}

#[test]
fn face_shape_depth_maps_explicitly_to_union_or_subtraction() {
    let source = SdfObject::create_kind(PrimitiveKind::Box);
    let face = crate::model::BoxFaceSelection {
        object: source.uuid,
        axis: VectorAxis::Z,
        positive: true,
    };
    let modeling_face = crate::model::ModelingFaceSelection::Box(face);
    let vertices = [
        Vec2::new(-0.2, -0.2),
        Vec2::new(0.2, -0.2),
        Vec2::new(0.0, 0.2),
    ];
    let scene = [source];
    let raised =
        create_face_shape(&scene, modeling_face, &vertices, 0.2, uuid::Uuid::new_v4()).unwrap();
    let cut =
        create_face_shape(&scene, modeling_face, &vertices, -0.2, uuid::Uuid::new_v4()).unwrap();
    assert_eq!(raised.operation, BooleanOperation::Union);
    assert_eq!(cut.operation, BooleanOperation::Subtract);
    assert_eq!(raised.boolean_parent, Some(face.object));
    assert_eq!(cut.boolean_parent, Some(face.object));
}

#[test]
fn polygon_caps_and_sides_can_spawn_nested_face_shapes() {
    let source = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    let scene = [source.clone()];
    let outline = [
        Vec2::new(-0.05, -0.05),
        Vec2::new(0.05, -0.05),
        Vec2::new(0.0, 0.05),
    ];
    for face in [
        crate::model::PolygonPrismFace::Cap { positive: true },
        crate::model::PolygonPrismFace::Side { edge: 0 },
    ] {
        let selection = crate::model::ModelingFaceSelection::PolygonPrism(
            crate::model::PolygonPrismFaceSelection {
                object: source.uuid,
                face,
            },
        );
        let child =
            create_face_shape(&scene, selection, &outline, -0.05, uuid::Uuid::new_v4()).unwrap();
        assert_eq!(child.boolean_parent, Some(source.uuid));
        assert_eq!(child.operation, BooleanOperation::Subtract);
    }
}

#[test]
fn cylinder_caps_accept_face_cuts_only_inside_their_disks() {
    let source = SdfObject::create_kind(PrimitiveKind::Cylinder);
    let scene = [source.clone()];
    let outline = [
        Vec2::new(-0.05, -0.05),
        Vec2::new(0.05, -0.05),
        Vec2::new(0.0, 0.05),
    ];
    for positive in [true, false] {
        let face =
            crate::model::ModelingFaceSelection::CylinderCap(crate::model::CylinderCapSelection {
                object: source.uuid,
                positive,
            });
        let (_, frame) = source_and_frame(&scene, face).unwrap();
        assert!(face_point_inside(&frame.domain, Vec2::new(0.1, 0.1)));
        assert!(!face_point_inside(&frame.domain, Vec2::new(0.3, 0.0)));
        let shape = create_face_shape(&scene, face, &outline, 0.1, uuid::Uuid::new_v4()).unwrap();
        assert_eq!(shape.boolean_parent, Some(source.uuid));
        assert_eq!(shape.operation, BooleanOperation::Union);
        assert_eq!(shape.transform.translation.y.is_sign_positive(), positive);
    }
}

#[test]
fn face_edge_directions_cover_rectangle_and_polygon_edges() {
    assert_eq!(
        face_edge_directions(&FaceDomain::Rectangle {
            extent_u: 1.0,
            extent_v: 1.0,
        }),
        vec![Vec2::X, Vec2::Y]
    );
    let polygon = FaceDomain::Polygon(vec![Vec2::ZERO, Vec2::X, Vec2::new(1.0, 1.0)]);
    let directions = face_edge_directions(&polygon);
    assert_eq!(directions.len(), 3);
    assert_eq!(directions[0], Vec2::X);
    assert_eq!(directions[1], Vec2::Y);
}

#[test]
fn face_point_snaps_parallel_to_a_box_edge() {
    let source = SdfObject::create_kind(PrimitiveKind::Box);
    let face = crate::model::ModelingFaceSelection::Box(crate::model::BoxFaceSelection {
        object: source.uuid,
        axis: VectorAxis::Z,
        positive: true,
    });
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let (point, guide) = guided_face_point(
        &camera,
        1.0,
        &[source.clone()],
        face,
        Vec2::ZERO,
        &[],
        Vec2::new(0.2, 0.003),
    );
    assert!(!guide.is_empty());
    assert!(point.y.abs() < 0.0001);
    let (diagonal, guide) = guided_face_point(
        &camera,
        1.0,
        &[source],
        face,
        Vec2::ZERO,
        &[],
        Vec2::new(0.2, 0.203),
    );
    assert!(!guide.is_empty());
    assert!((diagonal.x - diagonal.y).abs() < 0.0001);
}

#[test]
fn fourth_corner_aligns_to_first_and_third_for_a_rectangle() {
    let source = SdfObject::create_kind(PrimitiveKind::Box);
    let face = crate::model::ModelingFaceSelection::Box(crate::model::BoxFaceSelection {
        object: source.uuid,
        axis: VectorAxis::Z,
        positive: true,
    });
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let first = Vec2::new(-0.3, -0.2);
    let second = Vec2::new(0.3, -0.2);
    let third = Vec2::new(0.3, 0.2);
    let (fourth, guides) = guided_face_point(
        &camera,
        1.0,
        &[source],
        face,
        third,
        &[first, second],
        Vec2::new(-0.29, 0.21),
    );
    assert_eq!(guides.len(), 2);
    assert!(fourth.distance(Vec2::new(-0.3, 0.2)) < 0.0001);
}

#[test]
fn third_corner_can_match_the_first_side_for_a_square() {
    let source = SdfObject::create_kind(PrimitiveKind::Box);
    let face = crate::model::ModelingFaceSelection::Box(crate::model::BoxFaceSelection {
        object: source.uuid,
        axis: VectorAxis::Z,
        positive: true,
    });
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let first = Vec2::new(-0.2, -0.2);
    let second = Vec2::new(0.2, -0.2);
    let (third, guides) = guided_face_point(
        &camera,
        1.0,
        &[source],
        face,
        second,
        &[first],
        Vec2::new(0.204, 0.19),
    );
    assert_eq!(guides[0].label, "Equal length");
    assert!(third.distance(Vec2::new(0.2, 0.2)) < 0.0001);
}

#[test]
fn first_face_point_snaps_to_corners_and_edge_fractions() {
    let source = SdfObject::create_kind(PrimitiveKind::Box);
    let face = crate::model::ModelingFaceSelection::Box(crate::model::BoxFaceSelection {
        object: source.uuid,
        axis: VectorAxis::Z,
        positive: true,
    });
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let scene = [source];
    let (corner, corner_guide) =
        guided_face_anchor_point(&camera, 1.0, &scene, face, Vec2::new(-0.294, -0.295));
    assert!(corner.distance(Vec2::splat(-0.3)) < 0.0001);
    assert_eq!(corner_guide.unwrap().label, "Corner");
    let (edge, edge_guide) =
        guided_face_anchor_point(&camera, 1.0, &scene, face, Vec2::new(0.296, 0.004));
    assert!(edge.distance(Vec2::new(0.3, 0.0)) < 0.0001);
    assert!(edge_guide.unwrap().label.starts_with("Edge"));
}

#[test]
fn fraction_grid_gets_finer_as_the_face_grows_on_screen() {
    assert_eq!(fraction_divisions(100.0), 4);
    assert_eq!(fraction_divisions(400.0), 16);
    assert_eq!(fraction_divisions(1600.0), 64);
    assert_eq!(fraction_label(2, 4), "1/2");
    assert_eq!(fraction_label(1, 4), "1/4");
}

#[test]
fn later_points_can_snap_to_face_fractions_without_losing_angle_guides() {
    let source = SdfObject::create_kind(PrimitiveKind::Box);
    let face = crate::model::ModelingFaceSelection::Box(crate::model::BoxFaceSelection {
        object: source.uuid,
        axis: VectorAxis::Z,
        positive: true,
    });
    let mut camera = Camera::new();
    camera.viewport = Vec2::new(800.0, 600.0);
    let scene = [source];
    let vertices = [Vec2::new(-0.2, -0.15)];
    let (edge, guides, anchor) = guided_outline_point(
        &camera,
        1.0,
        &scene,
        face,
        &vertices,
        Vec2::new(0.296, 0.004),
    );
    assert!(edge.distance(Vec2::new(0.3, 0.0)) < 0.0001);
    assert!(guides.is_empty());
    assert!(anchor.is_some());

    let (aligned, guides, anchor) = guided_outline_point(
        &camera,
        1.0,
        &scene,
        face,
        &vertices,
        Vec2::new(0.1, -0.149),
    );
    assert!((aligned.y + 0.15).abs() < 0.0001);
    assert!(!guides.is_empty());
    assert!(anchor.is_none());
}

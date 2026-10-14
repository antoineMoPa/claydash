use super::*;
use crate::model::{BoxParams, Lattice, Mirror, PrimitiveKind, SdfParams, SurfaceInlay};

#[test]
fn cell_discontinuities_follow_sampled_dependencies_only() {
    let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
    let mut other = SdfObject::create_kind(PrimitiveKind::Box);
    other.repetition.enabled = true;
    other.repetition.count = [2, 1, 1];
    let mut scene = vec![root.clone(), other.clone()];
    assert!(!PreparedDirectField::new(&scene, root.uuid)
        .unwrap()
        .has_cell_discontinuities());
    scene[0].surface_inlay = Some(SurfaceInlay {
        host: other.uuid,
        offset: 0.0,
        thickness: 0.1,
    });
    assert!(PreparedDirectField::new(&scene, root.uuid)
        .unwrap()
        .has_cell_discontinuities());
    scene[0].surface_inlay = None;
    scene[0].boolean_parent = Some(other.uuid);
    assert!(
        !PreparedDirectField::new(&scene, root.uuid)
            .unwrap()
            .has_cell_discontinuities(),
        "selected child does not sample ancestor repetition"
    );
    root.repetition.enabled = true;
    root.repetition.count = [1, 1, 1];
    assert!(!PreparedDirectField::new(&[root.clone()], root.uuid)
        .unwrap()
        .has_cell_discontinuities());
    root.repetition.count = [2, 1, 1];
    assert!(PreparedDirectField::new(&[root.clone()], root.uuid)
        .unwrap()
        .has_cell_discontinuities());
    root.repetition.mode = crate::model::RepetitionMode::Radial {
        axis: crate::model::RepetitionAxis::Z,
        count: 4,
        pivot: Vec3::ZERO,
    };
    assert!(!PreparedDirectField::new(&[root.clone()], root.uuid)
        .unwrap()
        .has_cell_discontinuities());
}

#[test]
fn invalid_text_paths_fail_instead_of_falling_back_to_plain_text() {
    let mut text = SdfObject::create_kind(PrimitiveKind::Text);
    let SdfParams::TextParams(params) = &mut text.params else {
        unreachable!()
    };
    params.path = Some(Uuid::new_v4());
    let error = PreparedDirectField::new(&[text.clone()], text.uuid)
        .err()
        .unwrap();
    assert!(error.contains("text path is missing"), "{error}");
    let wrong = SdfObject::create_kind(PrimitiveKind::Sphere);
    let SdfParams::TextParams(params) = &mut text.params else {
        unreachable!()
    };
    params.path = Some(wrong.uuid);
    let error = PreparedDirectField::new(&[text.clone(), wrong], text.uuid)
        .err()
        .unwrap();
    assert!(error.contains("Bézier curve"), "{error}");
}

#[test]
fn invalid_custom_profiles_fail_instead_of_falling_back_to_round_extrusion() {
    let mut curve = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    curve.path_extrusion = Some(crate::model::PathExtrusion {
        profile_curve: Some(Uuid::new_v4()),
        ..Default::default()
    });
    let error = PreparedDirectField::new(&[curve.clone()], curve.uuid)
        .err()
        .unwrap();
    assert!(error.contains("Missing custom profile"), "{error}");
    let wrong = SdfObject::create_kind(PrimitiveKind::Box);
    curve.path_extrusion.as_mut().unwrap().profile_curve = Some(wrong.uuid);
    let error = PreparedDirectField::new(&[curve.clone(), wrong], curve.uuid)
        .err()
        .unwrap();
    assert!(error.contains("Bézier curve"), "{error}");
}

#[test]
fn unrelated_scene_cage_does_not_inflate_bounds() {
    let root = SdfObject::create_kind(PrimitiveKind::Sphere);
    let mut unrelated = SdfObject::create_kind(PrimitiveKind::Box);
    let mut cage = Lattice::new(Vec3::splat(-2.0), Vec3::splat(2.0), 2);
    cage.offsets.fill(Vec3::splat(1000.0));
    unrelated.lattice = Some(cage);
    let source = vec![root.clone(), unrelated];
    let field = PreparedDirectField::new(&source, root.uuid).unwrap();
    assert_eq!(field.deformation_extent(Mat4::IDENTITY), Vec3::ZERO);
}

#[test]
fn selected_child_inherits_nearest_ancestor_cage() {
    let mut parent = SdfObject::create_kind(PrimitiveKind::Box);
    let mut cage = Lattice::new(Vec3::splat(-2.0), Vec3::splat(2.0), 2);
    cage.offsets.fill(Vec3::new(0.3, -0.2, 0.1));
    parent.lattice = Some(cage);
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(parent.uuid);
    let root = child.uuid;
    let mut source = vec![parent, child];
    let point = Vec3::new(0.3, -0.2, 0.1);
    let deformed = PreparedDirectField::new(&source, root)
        .unwrap()
        .sample(point)
        .0;
    source[0].lattice = None;
    let rest = PreparedDirectField::new(&source, root)
        .unwrap()
        .sample(Vec3::ZERO)
        .0;
    assert!(
        (deformed - rest).abs() < 1e-6,
        "ancestor cage was omitted: {deformed} versus {rest}"
    );
    source[1].lattice = Some(Lattice::new(Vec3::splat(-2.0), Vec3::splat(2.0), 2));
    source[0].lattice = Some({
        let mut cage = Lattice::new(Vec3::splat(-2.0), Vec3::splat(2.0), 2);
        cage.offsets.fill(point);
        cage
    });
    let overridden = PreparedDirectField::new(&source, root)
        .unwrap()
        .sample(Vec3::ZERO)
        .0;
    assert!(
        (overridden - rest).abs() < 1e-6,
        "zero cage must override ancestor deformation"
    );
}

#[test]
fn inlay_shell_uses_deformed_external_host_field() {
    let mut host = SdfObject::create_kind(PrimitiveKind::Sphere);
    let mut cage = Lattice::new(Vec3::splat(-2.0), Vec3::splat(2.0), 2);
    cage.offsets.fill(Vec3::X * 0.2);
    host.lattice = Some(cage);
    let mut inlay = SdfObject::create_kind(PrimitiveKind::Box);
    inlay.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(2.0),
        corner_radius: 0.0,
    });
    inlay.surface_inlay = Some(SurfaceInlay {
        host: host.uuid,
        offset: 0.03,
        thickness: 0.02,
    });
    let root = inlay.uuid;
    let source = vec![host.clone(), inlay];
    let mut field = PreparedDirectField::new(&source, root).unwrap();
    let mut host_field = PreparedDirectField::new(&source, host.uuid).unwrap();
    for x in [-0.8, -0.3, 0.2, 0.7, 1.2] {
        let point = Vec3::new(x, 0.1, 0.0);
        let expected = (host_field.sample(point).0 - 0.03).abs() - 0.02;
        let (actual, owner) = field.sample(point);
        assert!((actual - expected).abs() < 1e-6);
        assert_eq!(owner, root);
    }
}

#[test]
fn cycles_and_missing_hosts_fail_before_sampling() {
    let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
    object.surface_inlay = Some(SurfaceInlay {
        host: object.uuid,
        offset: 0.0,
        thickness: 0.1,
    });
    assert!(PreparedDirectField::new(&[object.clone()], object.uuid).is_err());
    object.surface_inlay.as_mut().unwrap().host = Uuid::new_v4();
    assert!(PreparedDirectField::new(&[object.clone()], object.uuid).is_err());
    object.surface_inlay = None;
    object.boolean_parent = Some(object.uuid);
    assert!(PreparedDirectField::new(&[object.clone()], object.uuid).is_err());
}

#[test]
fn image_alpha_remains_material_and_spatial_fields_match_source() {
    let mut plane = SdfObject::create_image_plane();
    plane.mirror = Some(Mirror {
        axes: [true, false, false],
    });
    plane.repetition.enabled = true;
    plane.repetition.count = [2, 1, 1];
    let root = plane.uuid;
    let mut source = vec![plane];
    let probes = [
        Vec3::ZERO,
        Vec3::new(0.8, 0.2, 0.0),
        Vec3::new(-0.8, 0.2, 0.05),
    ];
    let with_image: Vec<_> = probes
        .iter()
        .map(|&point| {
            PreparedDirectField::new(&source, root)
                .unwrap()
                .sample(point)
        })
        .collect();
    source[0].image_stencil = None;
    for (point, expected) in probes.into_iter().zip(with_image) {
        assert_eq!(
            PreparedDirectField::new(&source, root)
                .unwrap()
                .sample(point),
            expected
        );
    }
}

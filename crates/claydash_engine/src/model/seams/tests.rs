use super::*;
use crate::model::{PrimitiveKind, SheetBend, SheetPanelParams};
fn seam(points: Vec<Vec3>) -> Seam {
    Seam {
        id: Uuid::new_v4(),
        name: "Join".into(),
        points: points.into_iter().map(SeamPoint::World).collect(),
        closed: false,
    }
}
fn cage_object() -> SdfObject {
    let mut object = SdfObject::create_kind(PrimitiveKind::Box);
    object.lattice = Some(Lattice::new(Vec3::splat(-1.), Vec3::ONE, 3));
    object
}
fn binding(object: Uuid, seam: Uuid, target: SeamTarget) -> SeamBinding {
    SeamBinding {
        object,
        seam,
        target,
        span: None,
        reverse: false,
        tolerance: 0.001,
    }
}
#[test]
fn lattice_edge_connection_moves_only_bound_controls_and_reacts_to_seam_edits() {
    let object = cage_object();
    let id = object.uuid;
    let edge = LatticeEdge {
        axis: CageAxis::Y,
        sides: [true, false],
    };
    let mut guide = seam(vec![
        Vec3::new(1., -1., -1.),
        Vec3::new(1.2, 0., -0.9),
        Vec3::new(1., 1., -1.),
    ]);
    let mut scene = vec![object.clone()];
    let mut constraints = SceneSeams {
        seams: vec![guide.clone()],
        bindings: vec![binding(id, guide.id, SeamTarget::LatticeEdge(edge))],
    };
    resolve_seam_bindings(&constraints, &mut scene).unwrap();
    assert_eq!(scene.len(), 1);
    let cage = scene[0].lattice.as_ref().unwrap();
    let i = edge.control(cage, 1);
    assert!(cage.offsets[i].distance(Vec3::new(0.2, 0., 0.1)) < 1e-6);
    for (index, offset) in cage.offsets.iter().enumerate() {
        if !(0..3).any(|s| edge.control(cage, s) == index) {
            assert_eq!(*offset, Vec3::ZERO);
        }
    }
    guide.points[1] = SeamPoint::World(Vec3::new(1.3, 0., -0.8));
    constraints.seams[0] = guide;
    resolve_seam_bindings(&constraints, &mut scene).unwrap();
    assert!(scene[0].lattice.as_ref().unwrap().offsets[i].distance(Vec3::new(0.3, 0., 0.2)) < 1e-6);
    assert_eq!(
        serde_json::to_value(&scene[0].params).unwrap(),
        serde_json::to_value(&object.params).unwrap()
    );
}
#[test]
fn adjacent_native_sheets_follow_one_seam_through_existing_cages() {
    let mut left = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    left.transform.translation.x = -1.;
    let mut right = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    right.transform.translation.x = 1.;
    for o in [&mut left, &mut right] {
        o.lattice = Some(Lattice::new(
            Vec3::new(-1., -0.5, -0.1),
            Vec3::new(1., 0.5, 0.1),
            3,
        ));
    }
    let guide = seam(vec![
        Vec3::new(0., -0.5, 0.),
        Vec3::new(0., 0., 0.04),
        Vec3::new(0., 0.5, 0.),
    ]);
    let data = SceneSeams {
        bindings: vec![
            binding(
                left.uuid,
                guide.id,
                SeamTarget::SheetBoundary(SheetEdge::MaxX),
            ),
            binding(
                right.uuid,
                guide.id,
                SeamTarget::SheetBoundary(SheetEdge::MinX),
            ),
        ],
        seams: vec![guide.clone()],
    };
    let mut scene = vec![left, right];
    resolve_seam_bindings(&data, &mut scene).unwrap();
    let points = guide.positions(&scene).unwrap();
    for (o, edge) in scene.iter().zip([SheetEdge::MaxX, SheetEdge::MinX]) {
        let SdfParams::SheetPanelParams(panel) = &o.params else {
            panic!()
        };
        let frame = lattice_world_matrix(&scene, o.uuid);
        for step in 0..=200 {
            let t = step as f32 / 200.;
            let world = seam_sample(&points, t);
            let target = frame.inverse().transform_point3(world);
            let rest = panel.material_point(sheet_edge_material_point(edge, panel.size, t));
            assert!(native_rest(o.lattice.as_ref().unwrap(), target).distance(rest) < 0.001);
        }
    }
}
#[test]
fn native_bends_keep_stationary_boundaries_and_reject_detached_boundaries_atomically() {
    let mut sheet = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    let mut panel = SheetPanelParams::default();
    panel.bends.push(SheetBend {
        start: 0.,
        direction: 0.,
        radius: 0.3,
        angle: 0.6,
    });
    sheet.params = SdfParams::SheetPanelParams(panel.clone());
    let guide = seam(vec![Vec3::new(-1., -0.5, 0.), Vec3::new(-1., 0.5, 0.)]);
    let data = SceneSeams {
        bindings: vec![binding(
            sheet.uuid,
            guide.id,
            SeamTarget::SheetBoundary(SheetEdge::MinX),
        )],
        seams: vec![guide],
    };
    let mut scene = vec![sheet];
    resolve_seam_bindings(&data, &mut scene).unwrap();
    panel.bends[0].angle = 0.9;
    scene[0].params = SdfParams::SheetPanelParams(panel);
    resolve_seam_bindings(&data, &mut scene).unwrap();
    assert!(scene[0].lattice.is_none());
    scene[0].transform.translation.z = 0.1;
    let before = serde_json::to_value(&scene).unwrap();
    assert!(resolve_seam_bindings(&data, &mut scene)
        .unwrap_err()
        .contains("sheet boundary"));
    assert_eq!(serde_json::to_value(&scene).unwrap(), before);
}
#[test]
fn source_attachments_follow_mesh_transforms_and_rays_choose_nearest_triangle() {
    use crate::renderer::poisson_mesh::geometry::Mesh;
    let mut mesh = SdfObject::create_kind(PrimitiveKind::Box);
    mesh.imported_mesh = Some(std::sync::Arc::new(Mesh::from_parts(
        vec![
            Vec3::new(-1., -1., 0.),
            Vec3::new(1., -1., 0.),
            Vec3::new(0., 1., 0.),
        ],
        vec![[0, 1, 2]],
        vec![mesh.uuid],
    )));
    let id = mesh.uuid;
    let mut scene = vec![mesh];
    let anchor = seam_surface_hit(&scene, id, Vec3::new(0., 0., 2.), -Vec3::Z).unwrap();
    assert!(seam_point_position(&anchor, &scene).unwrap().length() < 1e-6);
    scene[0].transform.translation = Vec3::new(3., 2., 1.);
    assert!(
        seam_point_position(&anchor, &scene)
            .unwrap()
            .distance(Vec3::new(3., 2., 1.))
            < 1e-6
    );
}
#[test]
fn closed_seam_spans_and_conflicting_corner_bindings_have_explicit_semantics() {
    let object = cage_object();
    let mut guide = seam(vec![
        Vec3::new(-1., -1., -1.),
        Vec3::new(1., -1., -1.),
        Vec3::new(1., 1., -1.),
    ]);
    guide.closed = true;
    let mut link = binding(
        object.uuid,
        guide.id,
        SeamTarget::LatticeEdge(LatticeEdge {
            axis: CageAxis::X,
            sides: [false, false],
        }),
    );
    let mut scene = vec![object];
    let mut data = SceneSeams {
        seams: vec![guide],
        bindings: vec![link.clone()],
    };
    assert!(resolve_seam_bindings(&data, &mut scene).is_err());
    link.span = Some([0, 1]);
    data.bindings[0] = link;
    resolve_seam_bindings(&data, &mut scene).unwrap();
    let second = seam(vec![Vec3::new(8., -1., -1.), Vec3::new(8., 1., -1.)]);
    data.bindings.push(binding(
        scene[0].uuid,
        second.id,
        SeamTarget::LatticeEdge(LatticeEdge {
            axis: CageAxis::Y,
            sides: [false, false],
        }),
    ));
    data.seams.push(second);
    let before = serde_json::to_value(&scene).unwrap();
    assert!(resolve_seam_bindings(&data, &mut scene).is_err());
    assert_eq!(serde_json::to_value(&scene).unwrap(), before);
}

#[test]
fn binding_weights_match_existing_cage_interpolation_including_derived_interior_controls() {
    for n in [2, 3, 5, 9] {
        let mut cage = Lattice::new(Vec3::new(-1., -0.5, -0.2), Vec3::new(1., 0.5, 0.2), n);
        let mut controls = Vec::new();
        let n = n as usize;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    let index = cage.index(x, y, z);
                    cage.offsets[index] = Vec3::new(
                        index as f32 * 0.001,
                        (index % 7) as f32 * 0.01,
                        (index % 11) as f32 * -0.01,
                    );
                    if cage.is_surface_point(x, y, z) {
                        controls.push(index);
                    }
                }
            }
        }
        for point in [
            Vec3::ZERO,
            Vec3::new(0.31, 0.22, -0.08),
            Vec3::new(1.5, 0.1, 0.7),
            cage.min,
            cage.max,
        ] {
            let sum = controls
                .iter()
                .zip(weights(&cage, &controls, point))
                .fold(Vec3::ZERO, |p, (i, w)| p + cage.offsets[*i] * w);
            assert!(
                sum.distance(cage.displacement(point)) < 1e-6,
                "density {n} at {point:?}"
            );
        }
    }
}
#[test]
fn native_sheet_connections_allow_unrelated_cuts_and_reject_trimmed_edges() {
    use crate::model::SheetCut;
    let mut sheet = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    let mut panel = SheetPanelParams::default();
    panel.cuts.push(SheetCut::Circle {
        center: Vec2::ZERO,
        radius: 0.1,
    });
    sheet.params = SdfParams::SheetPanelParams(panel.clone());
    let guide = seam(vec![Vec3::new(-1., -0.5, 0.), Vec3::new(-1., 0.5, 0.)]);
    let data = SceneSeams {
        bindings: vec![binding(
            sheet.uuid,
            guide.id,
            SeamTarget::SheetBoundary(SheetEdge::MinX),
        )],
        seams: vec![guide],
    };
    let mut scene = vec![sheet];
    resolve_seam_bindings(&data, &mut scene).unwrap();
    panel.cuts[0] = SheetCut::Circle {
        center: Vec2::new(-1., 0.),
        radius: 0.1,
    };
    scene[0].params = SdfParams::SheetPanelParams(panel);
    assert!(resolve_seam_bindings(&data, &mut scene)
        .unwrap_err()
        .contains("surviving"));
}

#[test]
fn a_cut_cannot_keep_only_a_zero_area_edge_for_a_sheet_connection() {
    use crate::model::SheetCut;
    let mut panel = SheetPanelParams::default();
    panel.cuts.push(SheetCut::Straight {
        from: Vec2::new(-1., -1.),
        to: Vec2::new(-1., 1.),
    });
    assert!(!sheet_edge_survives(&panel, SheetEdge::MinX));
    panel.cuts[0] = SheetCut::Straight {
        from: Vec2::new(-1., 1.),
        to: Vec2::new(-1., -1.),
    };
    assert!(sheet_edge_survives(&panel, SheetEdge::MinX));
}

#[test]
fn satisfied_sheet_bindings_preserve_control_values_across_repeated_solves() {
    let mut sheet = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    sheet.lattice = Some(Lattice::new(
        Vec3::new(-1., -0.5, -0.1),
        Vec3::new(1., 0.5, 0.1),
        3,
    ));
    let guide = seam(vec![
        Vec3::new(-1., -0.5, 0.),
        Vec3::new(-1., 0., 0.04),
        Vec3::new(-1., 0.5, 0.),
    ]);
    let data = SceneSeams {
        bindings: vec![binding(
            sheet.uuid,
            guide.id,
            SeamTarget::SheetBoundary(SheetEdge::MinX),
        )],
        seams: vec![guide],
    };
    let mut objects = vec![sheet];
    resolve_seam_bindings(&data, &mut objects).unwrap();
    let solved = serde_json::to_value(&objects).unwrap();
    for _ in 0..10 {
        resolve_seam_bindings(&data, &mut objects).unwrap();
        assert_eq!(serde_json::to_value(&objects).unwrap(), solved);
    }
    let mut edited = data.clone();
    edited.seams[0].points[1] = SeamPoint::World(Vec3::new(-1., 0., 0.07));
    resolve_seam_bindings(&edited, &mut objects).unwrap();
    assert_ne!(serde_json::to_value(&objects).unwrap(), solved);
}

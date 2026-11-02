use super::*;
use crate::model::{
    CageAxis, Lattice, LatticeEdge, SceneSeams, Seam, SeamBinding, SeamPoint, SeamTarget,
};
fn guide() -> Seam {
    Seam {
        id: uuid::Uuid::new_v4(),
        name: "Shared edge".into(),
        points: vec![
            SeamPoint::World(Vec3::new(1., -1., -1.)),
            SeamPoint::World(Vec3::new(1.1, 0., -1.)),
            SeamPoint::World(Vec3::new(1., 1., -1.)),
        ],
        closed: false,
    }
}
#[test]
fn independent_seams_and_edge_bindings_apply_undo_save_and_delete_without_geometry_creation() {
    let mut app = App::new();
    app.new_document();
    let mut object = SdfObject::create_kind(PrimitiveKind::Box);
    let id = object.uuid;
    object.lattice = Some(Lattice::new(Vec3::splat(-1.), Vec3::ONE, 3));
    model::set_objects(&mut app.tree, vec![object]);
    app.tree.make_undo_redo_snapshot();
    let seam = guide();
    let sid = seam.id;
    let target = SeamTarget::LatticeEdge(LatticeEdge {
        axis: CageAxis::Y,
        sides: [true, false],
    });
    let binding = SeamBinding {
        object: id,
        seam: sid,
        target,
        span: None,
        reverse: false,
        tolerance: 0.001,
    };
    app.agent_apply(ApplyArgs {
        expected_revision: None,
        actions: vec![
            Action::PutSeam { seam: seam.clone() },
            Action::SetSeamBinding {
                binding: binding.clone(),
            },
        ],
    })
    .unwrap();
    assert_eq!(model::objects_ref(&app.tree).len(), 1);
    assert_eq!(
        app.agent_state().unwrap()["seams"]["seams"][0]["id"],
        json!(sid)
    );
    let cage = model::objects_ref(&app.tree)[0].lattice.as_ref().unwrap();
    assert!((cage.offsets[cage.index(2, 1, 0)].x - 0.1).abs() < 1e-6);
    let bytes = document::serialize_scene(&app.tree).unwrap();
    let restored = document::deserialize_scene(&bytes).unwrap();
    let model::ClaydashValue::Seams(data) = restored.get_path("seams") else {
        panic!()
    };
    assert_eq!(data.bindings, vec![binding]);
    app.tree.undo();
    assert!(model::scene_seams(&app.tree).seams.is_empty());
    app.tree.redo();
    assert_eq!(model::scene_seams(&app.tree).seams.len(), 1);
    let pose = serde_json::to_value(model::objects_ref(&app.tree)).unwrap();
    app.agent_apply(ApplyArgs {
        expected_revision: None,
        actions: vec![Action::DeleteSeam { id: sid }],
    })
    .unwrap();
    assert!(model::scene_seams(&app.tree).bindings.is_empty());
    assert_eq!(
        serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),
        pose
    );
}
#[test]
fn conflicting_bindings_fail_atomically_and_target_deletion_removes_only_its_bindings() {
    let mut app = App::new();
    app.new_document();
    let mut object = SdfObject::create_kind(PrimitiveKind::Box);
    let id = object.uuid;
    object.lattice = Some(Lattice::new(Vec3::splat(-1.), Vec3::ONE, 3));
    model::set_objects(&mut app.tree, vec![object]);
    let seam = guide();
    let sid = seam.id;
    app.agent_apply(ApplyArgs {
        expected_revision: None,
        actions: vec![
            Action::PutSeam { seam },
            Action::SetSeamBinding {
                binding: SeamBinding {
                    object: id,
                    seam: sid,
                    target: SeamTarget::LatticeEdge(LatticeEdge {
                        axis: CageAxis::Y,
                        sides: [true, false],
                    }),
                    span: None,
                    reverse: false,
                    tolerance: 0.001,
                },
            },
        ],
    })
    .unwrap();
    let before = document::serialize_scene(&app.tree).unwrap();
    assert!(app
        .agent_apply(ApplyArgs {
            expected_revision: None,
            actions: vec![Action::SetSeamBinding {
                binding: SeamBinding {
                    object: id,
                    seam: sid,
                    target: SeamTarget::LatticeEdge(LatticeEdge {
                        axis: CageAxis::X,
                        sides: [false, false]
                    }),
                    span: None,
                    reverse: false,
                    tolerance: 0.001
                }
            }]
        })
        .is_err());
    assert_eq!(document::serialize_scene(&app.tree).unwrap(), before);
    app.agent_apply(ApplyArgs {
        expected_revision: None,
        actions: vec![Action::DeleteObject { id }],
    })
    .unwrap();
    assert!(model::scene_seams(&app.tree).bindings.is_empty());
    assert_eq!(model::scene_seams(&app.tree).seams.len(), 1);
}
#[test]
fn seam_point_snapping_uses_source_triangles_and_retains_attachment_on_roundtrip() {
    let mut app = App::new();
    app.new_document();
    let result = app
        .agent_apply(ApplyArgs {
            expected_revision: None,
            actions: vec![Action::CreateMesh {
                name: Some("Source".into()),
                vertices: vec![[-1., -1., 0.], [1., -1., 0.], [0., 1., 0.]],
                faces: vec![[0, 1, 2]],
                shading: None,
            }],
        })
        .unwrap();
    let source = serde_json::from_value(result["created_ids"][0].clone()).unwrap();
    let mut seam = guide();
    seam.points.clear();
    let sid = seam.id;
    app.agent_apply(ApplyArgs {
        expected_revision: None,
        actions: vec![
            Action::PutSeam { seam },
            Action::SnapSeamPoint {
                seam: sid,
                point: 0,
                source,
                position: [0., 0., 3.],
            },
            Action::SnapSeamPoint {
                seam: sid,
                point: 1,
                source,
                position: [0.1, 0.2, 2.],
            },
        ],
    })
    .unwrap();
    let seams = model::scene_seams(&app.tree);
    assert!(seams.seams[0]
        .points
        .iter()
        .all(|p| matches!(p, SeamPoint::Surface { .. })));
    assert!(
        seams.seams[0]
            .positions(model::objects_ref(&app.tree))
            .unwrap()[0]
            .length()
            < 1e-6
    );
    let before = document::serialize_scene(&app.tree).unwrap();
    assert!(app
        .agent_apply(ApplyArgs {
            expected_revision: None,
            actions: vec![Action::DeleteObject { id: source }]
        })
        .is_err());
    assert_eq!(document::serialize_scene(&app.tree).unwrap(), before);
}
#[test]
fn red_line_capture_is_opt_in_and_cli_flag_is_typed() {
    let request =
        serde_json::from_value::<Request>(request_payload("CaptureViewport", json!({}))).unwrap();
    let Request::CaptureViewport(args) = request else {
        panic!()
    };
    assert!(!args.show_seams);
    let (request, _) = parse_agent_arguments("CaptureViewport", &["--show-seams".into()]).unwrap();
    assert_eq!(request["args"]["show_seams"], true);
    assert!(parse_agent_arguments("GetState", &["--show-seams".into()]).is_err());
    assert!(
        parse_agent_arguments("CaptureViewport", &["[]".into(), "--show-seams".into()]).is_err()
    );
    use crate::renderer::CapturedFrame;
    let mut camera = Camera::new();
    camera.position = Vec3::new(0., 0., 3.);
    camera.target = Vec3::ZERO;
    camera.viewport = Vec2::splat(128.);
    camera.viewport_origin = Vec2::ZERO;
    let seam = Seam {
        id: uuid::Uuid::new_v4(),
        name: "Line".into(),
        points: vec![
            SeamPoint::World(Vec3::new(-0.5, 0., 0.)),
            SeamPoint::World(Vec3::new(0.5, 0., 0.)),
        ],
        closed: false,
    };
    let mut frame = CapturedFrame {
        width: 128,
        height: 128,
        rgba: vec![0; 128 * 128 * 4],
    };
    assert_eq!(
        frame.rgba.chunks_exact(4).filter(|p| p[0] == 255).count(),
        0
    );
    claydash_engine::seam_guides::annotate(
        &mut frame,
        &SceneSeams {
            seams: vec![seam],
            bindings: vec![],
        },
        &[],
        &camera,
    );
    assert!(
        frame
            .rgba
            .chunks_exact(4)
            .filter(|p| p == &&[255, 0, 0, 255][..])
            .count()
            > 50
    );
}
#[test]
fn seam_example_uses_ordinary_bent_sheets_and_existing_lattices() {
    let mut app = App::new();
    app.new_document();
    let mut scene = Vec::new();
    let seam = Seam {
        id: uuid::Uuid::new_v4(),
        name: "Shared sheet boundary".into(),
        points: vec![
            SeamPoint::World(Vec3::new(0., -0.5, 0.)),
            SeamPoint::World(Vec3::new(0., 0., 0.04)),
            SeamPoint::World(Vec3::new(0., 0.5, 0.)),
        ],
        closed: false,
    };
    let mut bindings = Vec::new();
    for (x, direction, edge, color) in [
        (
            -1.,
            std::f32::consts::PI,
            model::SheetEdge::MaxX,
            [0.1, 0.6, 0.9, 1.],
        ),
        (1., 0., model::SheetEdge::MinX, [0.95, 0.5, 0.1, 1.]),
    ] {
        let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
        object.transform.translation.x = x;
        object.name = if x < 0. { "Left sheet" } else { "Right sheet" }.into();
        object.color = glam::Vec4::from_array(color);
        object.material.color = object.color;
        object.params = SdfParams::SheetPanelParams(model::SheetPanelParams {
            bends: vec![model::SheetBend {
                start: 0.,
                direction,
                radius: 0.3,
                angle: 0.6,
            }],
            ..Default::default()
        });
        object.lattice = Some(Lattice::new(
            Vec3::new(-1., -0.5, -0.1),
            Vec3::new(1., 0.5, 0.8),
            3,
        ));
        bindings.push(SeamBinding {
            object: object.uuid,
            seam: seam.id,
            target: SeamTarget::SheetBoundary(edge),
            span: None,
            reverse: false,
            tolerance: 0.001,
        });
        scene.push(object);
    }
    model::set_objects(&mut app.tree, scene);
    model::try_set_seams(
        &mut app.tree,
        SceneSeams {
            seams: vec![seam],
            bindings,
        },
    )
    .unwrap();
    assert_eq!(model::objects_ref(&app.tree).len(), 2);
    assert!(model::objects_ref(&app.tree)
        .iter()
        .all(|o| o.imported_mesh.is_none() && o.lattice.is_some()));
    if let Some(path) = std::env::var_os("CLAYDASH_SEAM_EXAMPLE") {
        std::fs::write(path, document::serialize_scene(&app.tree).unwrap()).unwrap();
    }
}

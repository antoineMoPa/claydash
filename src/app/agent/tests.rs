use super::capture::orthographic_sheet;
use super::mcp::mcp_tools;
use super::*;

#[test]
fn capture_modes_are_typed_and_override_legacy_refinement() {
    for op in ["CaptureViewport", "CaptureOrthographic"] {
        for (mode, expected) in [
            ("outline", CaptureMode::Outline),
            ("simple_shading", CaptureMode::SimpleShading),
            ("full_material", CaptureMode::FullMaterial),
        ] {
            let request = serde_json::from_value::<Request>(request_payload(
                op,
                json!({"mode": mode, "refine": true}),
            ))
            .unwrap();
            let actual = match request {
                Request::CaptureViewport(args) => CaptureMode::from_options(args.mode, args.refine),
                Request::CaptureOrthographic(args) => {
                    CaptureMode::from_options(args.mode, args.refine)
                }
                _ => panic!("unexpected request"),
            };
            assert_eq!(actual, expected);
        }
        assert!(
            serde_json::from_value::<Request>(request_payload(op, json!({"mode": "unknown"})))
                .is_err()
        );
    }
    assert_eq!(
        CaptureMode::from_options(None, None),
        CaptureMode::SimpleShading
    );
    assert_eq!(
        CaptureMode::from_options(None, Some(false)),
        CaptureMode::SimpleShading
    );
    assert_eq!(
        CaptureMode::from_options(None, Some(true)),
        CaptureMode::FullMaterial
    );
    for name in ["capture_viewport", "capture_orthographic"] {
        let tool = mcp_tools()
            .into_iter()
            .find(|tool| tool["name"] == name)
            .unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["mode"]["enum"],
            json!(["simple_shading", "full_material", "outline"])
        );
    }
}

#[test]
fn wire_requests_accept_mcp_empty_arguments_and_typed_actions() {
    let read = serde_json::from_value::<Request>(request_payload("GetState", json!({})));
    assert!(read.is_ok(), "{}", read.err().unwrap());
    let apply = serde_json::from_value::<Request>(json!({"op": "Apply", "args": {
        "actions": [{"type": "CreateObject", "kind": "Box", "position": [1, 2, 3]}]
    }}));
    assert!(apply.is_ok(), "{}", apply.err().unwrap());
    let save = serde_json::from_value::<Request>(request_payload("Save", Value::Null));
    assert!(save.is_ok(), "{}", save.err().unwrap());
    let capture =
        serde_json::from_value::<Request>(request_payload("CaptureViewport", Value::Null));
    assert!(matches!(
        capture,
        Ok(Request::CaptureViewport(CaptureViewportArgs {
            object_ids: None,
            ..
        }))
    ));
    let capture = serde_json::from_value::<Request>(request_payload(
        "CaptureViewport",
        json!({"object_ids": [uuid::Uuid::nil()]}),
    ));
    assert!(matches!(
        capture,
        Ok(Request::CaptureViewport(CaptureViewportArgs {
            object_ids: Some(_),
            ..
        }))
    ));
    let orthographic = serde_json::from_value::<Request>(request_payload(
        "CaptureOrthographic",
        json!({"panel_size": 256, "refine": true}),
    ));
    assert!(matches!(
        orthographic,
        Ok(Request::CaptureOrthographic(CaptureOrthographicArgs {
            panel_size: Some(256),
            refine: Some(true),
            ..
        }))
    ));
}

#[test]
fn orthographic_capture_uses_three_axes_and_combines_panels() {
    let (reply, _) = mpsc::channel();
    let mut capture = AgentCapture {
        reply,
        objects: None,
        mode: CaptureMode::SimpleShading,
        view: CaptureView::Orthographic {
            panel_size: 96,
            distance: 8.0,
            frames: Vec::new(),
        },
    };
    for (index, direction) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
        let camera = capture.camera(&Camera::new());
        assert_eq!(camera.position, direction * 8.0);
        assert_eq!(camera.target, Vec3::ZERO);
        assert_eq!(camera.projection_mode, ProjectionMode::Orthographic);
        let frame = crate::renderer::CapturedFrame {
            width: 96,
            height: 96,
            rgba: [index as u8 * 60, 0, 0, 255].repeat(96 * 96),
        };
        if let CaptureView::Orthographic { frames, .. } = &mut capture.view {
            frames.push(frame);
        }
    }
    if let CaptureView::Orthographic { frames, .. } = &capture.view {
        let sheet = orthographic_sheet(frames).unwrap();
        assert_eq!((sheet.width, sheet.height), (288, 120));
        for (index, red) in [0, 60, 120].into_iter().enumerate() {
            let pixel = ((30 * sheet.width + index as u32 * 96 + 48) * 4) as usize;
            assert_eq!(sheet.rgba[pixel], red);
        }
    }
}

#[test]
fn isolated_capture_keeps_boolean_groups_and_hosted_inlays() {
    let root = SdfObject::create_kind(PrimitiveKind::Box);
    let mut cutter = SdfObject::create_kind(PrimitiveKind::Sphere);
    cutter.boolean_parent = Some(root.uuid);
    cutter.operation = BooleanOperation::Subtract;
    let mut inlay = SdfObject::create_kind(PrimitiveKind::Box);
    inlay.surface_inlay = Some(model::SurfaceInlay {
        host: root.uuid,
        offset: 0.01,
        thickness: 0.02,
    });
    let mut other_inlay = SdfObject::create_kind(PrimitiveKind::Box);
    other_inlay.surface_inlay = inlay.surface_inlay;
    let unrelated = SdfObject::create_kind(PrimitiveKind::Box);
    let scene = vec![
        inlay.clone(),
        other_inlay.clone(),
        unrelated.clone(),
        cutter.clone(),
        root.clone(),
    ];
    for selected in [root.uuid, cutter.uuid] {
        let filtered = capture_objects(&scene, &[selected]).unwrap();
        let ids: HashSet<_> = filtered.iter().map(|object| object.uuid).collect();
        assert_eq!(
            ids,
            HashSet::from([root.uuid, cutter.uuid, inlay.uuid, other_inlay.uuid])
        );
        assert_eq!(scene.len(), 5, "capture must not edit the live scene");
    }
    let filtered = capture_objects(&scene, &[inlay.uuid]).unwrap();
    let ids: HashSet<_> = filtered.iter().map(|object| object.uuid).collect();
    assert_eq!(ids, HashSet::from([root.uuid, cutter.uuid, inlay.uuid]));
    assert!(capture_objects(&scene, &[]).is_err());
    assert!(capture_objects(&scene, &[uuid::Uuid::nil()]).is_err());
}

#[test]
fn agent_edits_are_undoable_and_reject_stale_revisions() {
    let mut app = App::new();
    let original = model::objects_ref(&app.tree).len();
    let revision = app.scene_revision();
    let result = app
        .agent_apply(ApplyArgs {
            expected_revision: Some(revision),
            actions: vec![Action::CreateObject {
                kind: PrimitiveKind::Box,
                name: Some("Agent box".into()),
                transform: None,
                position: None,
                params: None,
                render_representation: None,
            }],
        })
        .unwrap();
    assert_eq!(model::objects_ref(&app.tree).len(), original + 1);
    assert_eq!(result["created_ids"].as_array().unwrap().len(), 1);
    assert!(app
        .agent_apply(ApplyArgs {
            expected_revision: Some(revision),
            actions: vec![Action::CreateObject {
                kind: PrimitiveKind::Sphere,
                name: None,
                transform: None,
                position: None,
                params: None,
                render_representation: None,
            }],
        })
        .is_err());
    app.tree.undo();
    assert_eq!(model::objects_ref(&app.tree).len(), original);
}

#[test]
fn post_processing_actions_are_atomic_ordered_and_undoable() {
    let mut app = App::new();
    let original = model::post_processing(&app.tree);
    let result = app
        .agent_apply(ApplyArgs {
            expected_revision: Some(app.scene_revision()),
            actions: vec![
                Action::CreatePostProcessPass {
                    name: "First".into(),
                    wgsl: "return color;".into(),
                    enabled: None,
                },
                Action::CreatePostProcessPass {
                    name: "Second".into(),
                    wgsl: "return vec4<f32>(1.0 - color.rgb, color.a);".into(),
                    enabled: Some(false),
                },
            ],
        })
        .unwrap();
    let ids: Vec<uuid::Uuid> =
        serde_json::from_value(result["created_post_process_pass_ids"].clone()).unwrap();
    assert_eq!(model::post_processing_ref(&app.tree).len(), 2);
    assert!(!model::post_processing_ref(&app.tree)[1].enabled);
    let revision = app.scene_revision();
    let error = app
        .agent_apply(ApplyArgs {
            expected_revision: Some(revision),
            actions: vec![
                Action::MovePostProcessPass {
                    id: ids[1],
                    index: 0,
                },
                Action::UpdatePostProcessPass {
                    id: ids[0],
                    name: None,
                    wgsl: Some("return bad_symbol;".into()),
                    enabled: None,
                },
            ],
        })
        .unwrap_err();
    assert!(error.contains("First"));
    assert_eq!(app.scene_revision(), revision);
    assert_eq!(model::post_processing_ref(&app.tree)[0].uuid, ids[0]);
    app.agent_apply(ApplyArgs {
        expected_revision: Some(revision),
        actions: vec![Action::MovePostProcessPass {
            id: ids[1],
            index: 0,
        }],
    })
    .unwrap();
    assert_eq!(model::post_processing_ref(&app.tree)[0].uuid, ids[1]);
    app.tree.undo();
    assert_eq!(model::post_processing_ref(&app.tree)[0].uuid, ids[0]);
    app.tree.undo();
    assert_eq!(model::post_processing_ref(&app.tree), original.as_slice());
}

#[test]
fn agent_creates_assigns_and_validates_custom_materials_atomically() {
    let mut app = App::new();
    let object_id = model::objects_ref(&app.tree)[0].uuid;
    let source = model::DEFAULT_CUSTOM_WGSL.to_owned();
    let result = app
        .agent_apply(ApplyArgs {
            expected_revision: Some(app.scene_revision()),
            actions: vec![Action::CreateCustomMaterial {
                name: "Bands".into(),
                wgsl: source.clone(),
            }],
        })
        .unwrap();
    let id: uuid::Uuid = serde_json::from_value(result["created_material_ids"][0].clone()).unwrap();
    app.agent_apply(ApplyArgs {
        expected_revision: Some(app.scene_revision()),
        actions: vec![Action::AssignMaterial {
            id,
            object_ids: vec![object_id],
        }],
    })
    .unwrap();
    let object = model::objects_ref(&app.tree)
        .iter()
        .find(|object| object.uuid == object_id)
        .unwrap();
    assert_eq!(object.material_id, Some(id));
    assert_eq!(object.material.kind, model::MaterialKind::Custom);
    let invalid = app.agent_apply(ApplyArgs {
        expected_revision: Some(app.scene_revision()),
        actions: vec![Action::UpdateCustomMaterial {
            id,
            name: None,
            wgsl: Some("return ;".into()),
        }],
    });
    assert!(invalid.is_err());
    assert_eq!(
        model::material_assets(&app.tree)[0].wgsl.as_deref(),
        Some(source.as_str())
    );
    app.tree.undo();
    assert_ne!(model::objects_ref(&app.tree)[0].material_id, Some(id));
}

#[test]
fn invalid_boolean_edit_leaves_live_scene_untouched() {
    let mut app = App::new();
    let mut object = model::objects_ref(&app.tree)[0].clone();
    object.boolean_parent = Some(object.uuid);
    let result = app.agent_apply(ApplyArgs {
        expected_revision: Some(app.scene_revision()),
        actions: vec![Action::PutObject { object }],
    });
    assert!(result.unwrap_err().contains("boolean cycle"));
    assert_ne!(
        model::objects_ref(&app.tree)[0].boolean_parent,
        Some(model::objects_ref(&app.tree)[0].uuid)
    );
}

#[test]
fn invalid_text_edit_is_atomic_and_valid_text_is_undoable() {
    let mut app = App::new();
    let before = model::objects_ref(&app.tree).len();
    let invalid = Action::CreateObject {
        kind: PrimitiveKind::Text,
        name: Some("Bad text".into()),
        transform: None,
        position: None,
        params: Some(SdfParams::TextParams(model::TextParams {
            text: "B8".into(),
            path: Some(uuid::Uuid::new_v4()),
            ..model::TextParams::default()
        })),
        render_representation: None,
    };
    let result = app.agent_apply(ApplyArgs {
        expected_revision: Some(app.scene_revision()),
        actions: vec![invalid],
    });
    assert!(result.unwrap_err().contains("text path is missing"));
    assert_eq!(model::objects_ref(&app.tree).len(), before);
    let valid = Action::CreateObject {
        kind: PrimitiveKind::Text,
        name: Some("Good text".into()),
        transform: None,
        position: None,
        params: Some(SdfParams::TextParams(model::TextParams {
            text: "B8".into(),
            ..model::TextParams::default()
        })),
        render_representation: None,
    };
    app.agent_apply(ApplyArgs {
        expected_revision: Some(app.scene_revision()),
        actions: vec![valid],
    })
    .unwrap();
    assert_eq!(model::objects_ref(&app.tree).len(), before + 1);
    let serialized = serde_json::to_value(model::objects_ref(&app.tree)).unwrap();
    let restored: Vec<SdfObject> = serde_json::from_value(serialized).unwrap();
    assert!(matches!(
        restored.last().unwrap().params,
        SdfParams::TextParams(_)
    ));
    app.tree.undo();
    assert_eq!(model::objects_ref(&app.tree).len(), before);
}

#[test]
fn available_render_representations_are_authorable_by_agents() {
    let declared: Vec<Value> = schema()["render_representations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["value"].clone())
        .collect();
    let advertised = &mcp_tools()
        .into_iter()
        .find(|tool| tool["name"] == "apply")
        .unwrap()["inputSchema"]["properties"]["actions"]["items"]["oneOf"];
    let set_action = advertised
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["properties"]["type"]["const"] == "SetRenderRepresentation")
        .unwrap();
    let create_action = advertised
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["properties"]["type"]["const"] == "CreateObject")
        .unwrap();
    let set_values = set_action["properties"]["render_representation"]["enum"]
        .as_array()
        .unwrap();
    let create_values = create_action["properties"]["render_representation"]["enum"]
        .as_array()
        .unwrap();
    let available: Vec<Value> = GroupRenderRepresentation::ALL
        .into_iter()
        .map(|mode| serde_json::to_value(mode).unwrap())
        .collect();
    assert_eq!(set_values, &available);
    assert_eq!(create_values, &available);
    assert_eq!(declared.len(), GroupRenderRepresentation::ALL.len());

    let mut app = App::new();
    let root_id = model::objects_ref(&app.tree)[0].uuid;
    let child = SdfObject::create_kind(PrimitiveKind::Box);
    let child_id = child.uuid;
    let mut scene = model::objects(&app.tree);
    scene.push(child);
    model::set_objects(&mut app.tree, scene);
    app.agent_apply(ApplyArgs {
        expected_revision: Some(app.scene_revision()),
        actions: vec![Action::SetBoolean {
            id: child_id,
            parent: Some(root_id),
            operation: BooleanOperation::Subtract,
            softness: None,
        }],
    })
    .unwrap();

    for mode in GroupRenderRepresentation::ALL {
        let value = serde_json::to_value(mode).unwrap();
        let request: Request = serde_json::from_value(json!({
            "op": "Apply", "args": {"actions": [{
                "type": "SetRenderRepresentation", "id": root_id,
                "render_representation": value
            }]}
        }))
        .unwrap();
        let Request::Apply(args) = request else {
            unreachable!()
        };
        app.agent_apply(args).unwrap();
        let root = model::objects_ref(&app.tree)
            .iter()
            .find(|object| object.uuid == root_id)
            .unwrap();
        assert_eq!(root.render_representation, mode);
        let restored: SdfObject =
            serde_json::from_value(serde_json::to_value(root).unwrap()).unwrap();
        assert_eq!(restored.render_representation, mode);
        let state = app.agent_state().unwrap();
        let state_root: SdfObject = serde_json::from_value(
            state["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|object| object["uuid"] == json!(root_id))
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(state_root.render_representation, mode);

        let create: Request = serde_json::from_value(json!({
            "op": "Apply", "args": {"actions": [{
                "type": "CreateObject", "kind": "Box",
                "render_representation": value
            }]}
        }))
        .unwrap();
        let Request::Apply(args) = create else {
            unreachable!()
        };
        let created = app.agent_apply(args).unwrap();
        let created_id: uuid::Uuid =
            serde_json::from_value(created["created_ids"][0].clone()).unwrap();
        let object = model::objects_ref(&app.tree)
            .iter()
            .find(|object| object.uuid == created_id)
            .unwrap();
        assert_eq!(object.render_representation, mode);
    }
}

#[test]
fn agent_can_compose_nested_union_groups_with_separate_render_choices() {
    let mut app = App::new();
    let outer_id = model::objects_ref(&app.tree)[0].uuid;
    let create: Request = serde_json::from_value(json!({
        "op": "Apply", "args": {"actions": [
            {"type": "CreateObject", "kind": "Box", "name": "Group A"},
            {"type": "CreateObject", "kind": "Sphere", "name": "A child"},
            {"type": "CreateObject", "kind": "Box", "name": "Group B"},
            {"type": "CreateObject", "kind": "Sphere", "name": "B child"}
        ]}
    }))
    .unwrap();
    let Request::Apply(args) = create else {
        unreachable!()
    };
    let created = app.agent_apply(args).unwrap();
    let ids: Vec<uuid::Uuid> = serde_json::from_value(created["created_ids"].clone()).unwrap();
    let compose: Request = serde_json::from_value(json!({
            "op": "Apply", "args": {"actions": [
                {"type": "SetBoolean", "id": outer_id, "parent": null, "operation": "Union", "softness": 0.0},
                {"type": "SetBoolean", "id": ids[1], "parent": ids[0], "operation": "Union"},
                {"type": "SetBoolean", "id": ids[3], "parent": ids[2], "operation": "Union"},
                {"type": "SetBoolean", "id": ids[0], "parent": outer_id, "operation": "Union", "softness": 0.0},
                {"type": "SetBoolean", "id": ids[2], "parent": outer_id, "operation": "Union", "softness": 0.0},
                {"type": "SetRenderRepresentation", "id": ids[0], "render_representation": "box_depth_atlas"},
                {"type": "SetRenderRepresentation", "id": ids[2], "render_representation": "exact_sdf"}
            ]}
        }))
        .unwrap();
    let Request::Apply(args) = compose else {
        unreachable!()
    };
    app.agent_apply(args).unwrap();
    let scene = model::objects_ref(&app.tree);
    assert_eq!(
        scene
            .iter()
            .find(|object| object.uuid == ids[1])
            .unwrap()
            .boolean_parent,
        Some(ids[0])
    );
    assert_eq!(
        scene
            .iter()
            .find(|object| object.uuid == ids[3])
            .unwrap()
            .boolean_parent,
        Some(ids[2])
    );
    assert_eq!(
        scene
            .iter()
            .find(|object| object.uuid == ids[0])
            .unwrap()
            .render_representation,
        GroupRenderRepresentation::BoxDepthAtlas
    );
    assert_eq!(
        scene
            .iter()
            .find(|object| object.uuid == ids[2])
            .unwrap()
            .render_representation,
        GroupRenderRepresentation::ExactSdf
    );
    let captured = capture_objects(scene, &[outer_id]).unwrap();
    assert!(ids
        .iter()
        .all(|id| captured.iter().any(|object| object.uuid == *id)));
}

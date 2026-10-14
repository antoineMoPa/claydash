use super::capture::orthographic_sheet;
use super::mcp::mcp_tools;
use super::*;

#[test]
fn sheet_surface_path_and_persistent_seams_apply_and_reload_atomically() {
    let mut app=App::new(); app.new_document();
    let mut source:model::SheetPanelParams=serde_json::from_value(mcp::schema()["sheet_surface_example"]["SheetPanelParams"].clone()).unwrap();
    let make=|params:model::SheetPanelParams,position| Action::CreateObject {
        kind:PrimitiveKind::SheetPanel,name:None,position:Some(position),transform:None,
        params:Some(SdfParams::SheetPanelParams(params)),render_representation:None,
    };
    let result=app.agent_apply(ApplyArgs {expected_revision:None,actions:vec![make(source.clone(),[0.0,0.0,0.0])]}).unwrap();
    let source_id:uuid::Uuid=serde_json::from_value(result["created_ids"][0].clone()).unwrap();
    let target=model::SheetPanelParams {
        size:source.size,thickness:source.thickness,
        seam:Some(model::SheetSeam {source:source_id,source_edge:model::SheetEdge::MaxX,edge:model::SheetEdge::MinX,gap:0.005}),
        ..Default::default()
    };
    let result=app.agent_apply(ApplyArgs {expected_revision:None,actions:vec![make(target,[0.905,0.0,0.0])]}).unwrap();
    let target_id:uuid::Uuid=serde_json::from_value(result["created_ids"][0].clone()).unwrap();
    let check=|app:&App,source:&model::SheetPanelParams| {
        let object=model::objects_ref(&app.tree).iter().find(|o|o.uuid==target_id).unwrap();
        let SdfParams::SheetPanelParams(p)=&object.params else {panic!()};
        for row in 0..4 {
            let heights=&p.surface.as_ref().unwrap().heights;
            let original=&source.surface.as_ref().unwrap().heights;
            assert!((heights[row*4]-original[row*4+3]).abs()<1e-6);
            assert!((heights[row*4+1]-heights[row*4]-(original[row*4+3]-original[row*4+2])).abs()<1e-6);
        }
    };
    check(&app,&source);
    // A compatible pair move is valid even when the target action comes first.
    let source_transform=model::objects_ref(&app.tree).iter().find(|o|o.uuid==source_id).unwrap().transform;
    let target_transform=model::objects_ref(&app.tree).iter().find(|o|o.uuid==target_id).unwrap().transform;
    let mut moved_source=source_transform; moved_source.translation.x+=0.1;
    let mut moved_target=target_transform; moved_target.translation.x+=0.1;
    app.agent_apply(ApplyArgs {expected_revision:None,actions:vec![
        Action::SetObjectTransform {id:target_id,transform:moved_target},
        Action::SetObjectTransform {id:source_id,transform:moved_source},
    ]}).unwrap();
    app.agent_apply(ApplyArgs {expected_revision:None,actions:vec![
        Action::SetObjectTransform {id:source_id,transform:source_transform},
        Action::SetObjectTransform {id:target_id,transform:target_transform},
    ]}).unwrap();
    source.surface.as_mut().unwrap().heights[7]+=0.025;
    app.agent_apply(ApplyArgs {expected_revision:None,actions:vec![Action::SetObjectParams {id:source_id,params:SdfParams::SheetPanelParams(source.clone())}]}).unwrap();
    check(&app,&source);
    let before=serde_json::to_value(model::objects_ref(&app.tree)).unwrap(); let revision=app.scene_revision();
    let mut invalid=source.clone(); invalid.path_embosses[0].stations[0].width=0.0;
    let mut transform=model::objects_ref(&app.tree).iter().find(|o|o.uuid==target_id).unwrap().transform;
    transform.translation.x+=0.01;
    for actions in [
        vec![Action::SetObjectParams {id:source_id,params:SdfParams::SheetPanelParams(invalid)}],
        vec![Action::SetObjectTransform {id:target_id,transform}],
        vec![Action::DeleteObject {id:source_id}],
    ] {
        assert!(app.agent_apply(ApplyArgs {expected_revision:None,actions}).is_err());
        assert_eq!(serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),before); assert_eq!(app.scene_revision(),revision);
    }
    for stage in [model::SheetStage::Blank,model::SheetStage::Cut,model::SheetStage::Formed] {
        let snapshot=capture::capture_scene(model::objects_ref(&app.tree),Some(&[source_id,target_id]),Some(stage)).unwrap().unwrap();
        for object in snapshot {let SdfParams::SheetPanelParams(p)=object.params else {panic!()};assert_eq!(p.stage,stage);assert!(p.surface.is_some());}
    }
    assert_eq!(serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),before);
    let saved=crate::document::serialize_scene(&app.tree).unwrap();
    let mut restored=model::DataTree::default(); restored.set_tree("scene",crate::document::deserialize_scene(&saved).unwrap());
    assert_eq!(serde_json::to_value(model::objects_ref(&restored)).unwrap(),before);
    // Ordinary editor changes re-solve dependants and retain the last valid
    // object state if a transform breaks the declared seam.
    let mut objects=model::objects(&app.tree); let old=objects.clone();
    objects.iter_mut().find(|o|o.uuid==target_id).unwrap().transform.translation.x+=0.1;
    model::set_objects(&mut app.tree,objects); assert_eq!(serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),serde_json::to_value(old).unwrap());
    let variable_id=uuid::Uuid::new_v4();
    let variables=model::SceneVariables {
        vectors:vec![model::VectorVariable {id:variable_id,name:"Seam source position".into(),value:Vec3::ZERO,space:model::VariableSpace::Local}],
        bindings:vec![model::VectorBinding {object:source_id,target:model::VectorBindingTarget::Position,variable:variable_id,offset:Vec3::ZERO}],
        ..Default::default()
    };
    model::try_set_scene_variables(&mut app.tree,variables.clone()).unwrap();
    let bound_before=serde_json::to_value(model::objects_ref(&app.tree)).unwrap();
    let objects=model::objects(&app.tree).into_iter().filter(|o|o.uuid!=source_id).collect();
    model::set_objects(&mut app.tree,objects);
    assert_eq!(model::scene_variables(&app.tree),variables,"rejected deletion must retain bindings");
    assert_eq!(serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),bound_before);
    let mut invalid_variables=variables.clone(); invalid_variables.vectors[0].value.x=0.1;
    assert!(model::try_set_scene_variables(&mut app.tree,invalid_variables.clone()).is_err());
    model::set_scene_variables(&mut app.tree,invalid_variables);
    assert_eq!(model::scene_variables(&app.tree),variables);
    assert_eq!(serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),bound_before);
    assert!(app.agent_apply(ApplyArgs {expected_revision:None,actions:vec![Action::UpdateVectorVariable {
        id:variable_id,name:None,value:Some(Vec3::X*0.1),space:None,
    }]}).is_err());
    assert_eq!(model::scene_variables(&app.tree),variables);
    assert_eq!(serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),bound_before);
}

#[test]
fn sheet_mcp_edits_validate_atomically_and_capture_stages_do_not_mutate_scene() {
    let mut app = App::new();
    app.new_document();
    let mut panel = model::SheetPanelParams::default();
    panel.cuts.push(model::SheetCut::Circle { center: Vec2::ZERO, radius: 0.2 });
    panel.embosses.push(model::SheetEmboss::Circle { center: Vec2::ZERO, radius: 0.2, depth: 0.025, transition: 0.1 });
    panel.bends.push(model::SheetBend { direction: 0.0, start: -0.5, radius: 0.3, angle: 1.0 });
    panel.bends.push(model::SheetBend { direction: std::f32::consts::FRAC_PI_2, start: -0.1, radius: 0.3, angle: -1.0 });
    let result = app.agent_apply(ApplyArgs { expected_revision: None, actions: vec![Action::CreateObject {
        kind: PrimitiveKind::SheetPanel, name: Some("Fender".into()),
        position: None, transform: None, params: Some(SdfParams::SheetPanelParams(panel.clone())),
        render_representation: None,
    }] }).unwrap();
    let id: uuid::Uuid = serde_json::from_value(result["created_ids"][0].clone()).unwrap();
    let before = serde_json::to_value(model::objects_ref(&app.tree)).unwrap();
    for stage in [model::SheetStage::Blank, model::SheetStage::Cut, model::SheetStage::Formed] {
        let snapshot = capture::capture_scene(model::objects_ref(&app.tree), Some(&[id]), Some(stage)).unwrap().unwrap();
        let SdfParams::SheetPanelParams(recipe) = &snapshot[0].params else { panic!("sheet recipe") };
        assert_eq!(recipe.stage, stage);
        assert_eq!(recipe.cuts.len(), 1);
        assert_eq!(recipe.embosses.len(), 1);
        assert_eq!(recipe.bends.len(), 2);
        assert_eq!(recipe.bends[1].direction, std::f32::consts::FRAC_PI_2);
    }
    assert_eq!(before, serde_json::to_value(model::objects_ref(&app.tree)).unwrap());
    let mut invalid_emboss = panel.clone();
    let model::SheetEmboss::Circle { transition, .. } = &mut invalid_emboss.embosses[0];
    *transition = 0.0;
    assert!(app.agent_apply(ApplyArgs { expected_revision: None, actions: vec![Action::SetObjectParams { id, params: SdfParams::SheetPanelParams(invalid_emboss) }] }).is_err());
    assert_eq!(before, serde_json::to_value(model::objects_ref(&app.tree)).unwrap());
    let saved = crate::document::serialize_scene(&app.tree).unwrap();
    let mut restored = model::DataTree::default();
    restored.set_tree("scene", crate::document::deserialize_scene(&saved).unwrap());
    assert_eq!(before, serde_json::to_value(model::objects_ref(&restored)).unwrap());
    let schema = mcp::schema();
    assert!(schema["sheet_emboss_example"]["SheetPanelParams"]["embosses"].is_array());
    let mut singular = panel.clone();
    singular.bends[1].angle = 1.0;
    singular.bends[1].radius = 0.03;
    let error = app.agent_apply(ApplyArgs { expected_revision: None, actions: vec![Action::SetObjectParams { id, params: SdfParams::SheetPanelParams(singular) }] }).unwrap_err();
    assert!(error.contains("radial clearance"));
    assert_eq!(before, serde_json::to_value(model::objects_ref(&app.tree)).unwrap());
    let mut excessive = panel.clone();
    excessive.bends.resize(5, panel.bends[0].clone());
    assert!(app.agent_apply(ApplyArgs { expected_revision: None, actions: vec![Action::SetObjectParams { id, params: SdfParams::SheetPanelParams(excessive) }] }).is_err());
    assert_eq!(before, serde_json::to_value(model::objects_ref(&app.tree)).unwrap());
    panel.bends[0].radius = 0.001;
    assert!(app.agent_apply(ApplyArgs { expected_revision: None, actions: vec![Action::SetObjectParams { id, params: SdfParams::SheetPanelParams(panel) }] }).is_err());
    assert_eq!(before, serde_json::to_value(model::objects_ref(&app.tree)).unwrap());
}

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

fn apply_variables_wire(app: &mut App, actions: Value) -> AgentResult {
    let Request::Apply(args) = serde_json::from_value(request_payload(
        "Apply",
        json!({
            "expected_revision": app.scene_revision(), "actions": actions,
        }),
    ))
    .unwrap() else {
        panic!()
    };
    app.agent_apply(args)
}

#[test]
fn vector_variables_mcp_transaction_lifecycle() {
    let mut app = App::new();
    let curve = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    let object = curve.uuid;
    model::set_objects(&mut app.tree, vec![curve]);
    let id = uuid::Uuid::new_v4();
    let result = apply_variables_wire(&mut app, json!([
        {"type":"CreateVectorVariable", "id":id, "name":"Anchor", "value":[1,2,3]},
        {"type":"SetVectorBinding", "object":object, "target":"Position", "variable":id},
        {"type":"SetVectorBinding", "object":object, "target":{"BezierPoint":0}, "variable":id, "offset":[0,1,0]}
    ])).unwrap();
    assert_eq!(result["created_variable_ids"], json!([id]));
    assert_eq!(
        app.agent_state().unwrap()["variables"]["vectors"][0]["space"],
        "World"
    );
    let saved = document::serialize_scene(&app.tree).unwrap();
    let mut restored = model::DataTree::default();
    restored.set_tree("scene", document::deserialize_scene(&saved).unwrap());
    assert_eq!(
        model::scene_variables(&restored),
        model::scene_variables(&app.tree)
    );
    apply_variables_wire(&mut app, json!([
        {"type":"UpdateVectorVariable", "id":id, "value":[4,5,6]},
        {"type":"SetVectorBinding", "object":object, "target":"Position", "variable":id, "offset":[1,0,0]}
    ])).unwrap();
    assert_eq!(
        model::objects_ref(&app.tree)[0].transform.translation,
        Vec3::new(5., 5., 6.)
    );
    app.tree.undo();
    assert_eq!(
        model::scene_variables(&app.tree).vectors[0].value,
        Vec3::new(1., 2., 3.)
    );
    app.tree.redo();
    apply_variables_wire(
        &mut app,
        json!([
            {"type":"RemoveVectorBinding", "object":object, "target":"Position"},
            {"type":"DeleteVectorVariable", "id":id}
        ]),
    )
    .unwrap();
    assert_eq!(
        model::objects_ref(&app.tree)[0].transform.translation,
        Vec3::new(5., 5., 6.)
    );
    assert!(model::scene_variables(&app.tree).bindings.is_empty());
    assert!(model::scene_variables(&app.tree).vectors.is_empty());
}

#[test]
fn vector_variables_mcp_reject_invalid_batches_atomically() {
    let mut app = App::new();
    let object = SdfObject::create_kind(PrimitiveKind::Sphere);
    let object_id = object.uuid;
    model::set_objects(&mut app.tree, vec![object]);
    let id = uuid::Uuid::new_v4();
    for invalid in [
        json!({"type":"CreateVectorVariable", "id":id, "name":"duplicate", "value":[0,0,0]}),
        json!({"type":"SetVectorBinding", "object":object_id, "target":{"BezierPoint":0}, "variable":id}),
        json!({"type":"SetVectorBinding", "object":uuid::Uuid::new_v4(), "target":"Position", "variable":id}),
        json!({"type":"SetVectorBinding", "object":object_id, "target":"Position", "variable":uuid::Uuid::new_v4()}),
    ] {
        let before = app.agent_state().unwrap();
        assert!(apply_variables_wire(
            &mut app,
            json!([
                {"type":"CreateVectorVariable", "id":id, "name":"Anchor", "value":[1,2,3]}, invalid
            ])
        )
        .is_err());
        assert_eq!(before, app.agent_state().unwrap());
    }
    let revision = app.scene_revision();
    apply_variables_wire(
        &mut app,
        json!([
            {"type":"CreateVectorVariable", "id":id, "name":"Anchor", "value":[1,2,3]},
            {"type":"SetVectorBinding", "object":object_id, "target":"Position", "variable":id}
        ]),
    )
    .unwrap();
    let Request::Apply(stale) = serde_json::from_value(request_payload(
        "Apply",
        json!({
            "expected_revision":revision, "actions":[{"type":"DeleteVectorVariable", "id":id}]
        }),
    ))
    .unwrap() else {
        panic!()
    };
    assert!(app
        .agent_apply(stale)
        .unwrap_err()
        .contains("scene changed"));
    apply_variables_wire(&mut app, json!([{"type":"DeleteObject", "id":object_id}])).unwrap();
    assert!(model::scene_variables(&app.tree).bindings.is_empty());
}

#[test]
fn vector_variables_schema_matches_wire_targets_and_defaults() {
    let tool = mcp_tools()
        .into_iter()
        .find(|tool| tool["name"] == "apply")
        .unwrap();
    let actions = tool["inputSchema"]["properties"]["actions"]["items"]["oneOf"]
        .as_array()
        .unwrap();
    for name in [
        "CreateVectorVariable",
        "UpdateVectorVariable",
        "DeleteVectorVariable",
        "SetVectorBinding",
        "RemoveVectorBinding",
        "SetVariables",
    ] {
        assert!(actions
            .iter()
            .any(|action| action["properties"]["type"]["const"] == name));
        assert!(schema()["actions"]
            .as_array()
            .unwrap()
            .contains(&json!(name)));
    }
    assert_eq!(
        schema()["vector_binding_targets"][2],
        json!({"BezierPoint":0})
    );
}

#[test]
fn vector_variables_validate_frames_indices_and_re_evaluate_raw_scene() {
    let mut app = App::new();
    let curve = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    let object = curve.uuid;
    let id = uuid::Uuid::new_v4();
    model::set_objects(&mut app.tree, vec![curve]);
    apply_variables_wire(
        &mut app,
        json!([
            {"type":"CreateVectorVariable","id":id,"name":"Anchor","value":[1,2,3]},
            {"type":"SetVectorBinding","object":object,"target":{"BezierPoint":0},"variable":id}
        ]),
    )
    .unwrap();
    let before = app.agent_state().unwrap();
    let mut singular = model::objects_ref(&app.tree)[0].clone();
    singular.transform.scale = Vec3::ZERO;
    assert!(
        apply_variables_wire(&mut app, json!([{"type":"PutObject","object":singular}])).is_err()
    );
    assert!(apply_variables_wire(
        &mut app,
        json!([
            {"type":"SetVectorBinding","object":object,"target":{"BezierPoint":999},"variable":id}
        ])
    )
    .is_err());
    assert_eq!(before, app.agent_state().unwrap());
    let mut variables = model::scene_variables(&app.tree);
    variables.vectors[0].value = Vec3::splat(7.);
    // Deliberately author variable metadata without evaluating the objects.
    app.tree.set_path(
        "scene.variables",
        model::ClaydashValue::SceneVariables(variables),
    );
    let raw = app.agent_state().unwrap()["document"].clone();
    apply_variables_wire(&mut app, json!([{"type":"ReplaceScene","scene":raw}])).unwrap();
    let SdfParams::BezierCurveParams(params) = &model::objects_ref(&app.tree)[0].params else {
        panic!()
    };
    assert_eq!(params.points[0], Vec3::splat(7.));
    let mut variables = model::scene_variables(&app.tree);
    variables.bindings.push(variables.bindings[0].clone());
    assert!(app
        .agent_apply(ApplyArgs {
            expected_revision: None,
            actions: vec![Action::SetVariables { variables }]
        })
        .is_err());
    let mut variables = model::scene_variables(&app.tree);
    variables.vectors[0].value = Vec3::splat(f32::INFINITY);
    assert!(app
        .agent_apply(ApplyArgs {
            expected_revision: None,
            actions: vec![Action::SetVariables { variables }]
        })
        .is_err());
    apply_variables_wire(&mut app, json!([{"type":"DeleteObject","id":object}])).unwrap();
    apply_variables_wire(&mut app, json!([{"type":"SetSelection","ids":[]}])).unwrap();
}

#[test]
fn voxel_settings_are_authorable_persisted_and_undoable() {
    let mut app = App::new();
    let id = model::objects_ref(&app.tree)[0].uuid;
    let before = model::objects_ref(&app.tree)[0].render_representation;
    let request: Request = serde_json::from_value(json!({
        "op": "Apply", "args": {"actions": [{
            "type": "SetRenderRepresentation", "id": id,
            "render_representation": "voxels", "voxels": {"resolution": 48}
        }]}
    }))
    .unwrap();
    let Request::Apply(args) = request else {
        unreachable!()
    };
    app.agent_apply(args).unwrap();
    let object = &model::objects_ref(&app.tree)[0];
    assert_eq!(
        object.render_representation,
        GroupRenderRepresentation::Voxels
    );
    assert_eq!(object.voxels.resolution, 48);
    let restored: SdfObject =
        serde_json::from_value(serde_json::to_value(object).unwrap()).unwrap();
    assert_eq!(restored.voxels.resolution, 48);
    assert_eq!(
        app.agent_state().unwrap()["objects"][0]["voxels"]["resolution"],
        48
    );
    app.tree.undo();
    assert_eq!(
        model::objects_ref(&app.tree)[0].render_representation,
        before
    );
    assert_eq!(
        model::objects_ref(&app.tree)[0].voxels,
        model::VoxelSettings::default()
    );
    for resolution in [0, 129] {
        assert!(serde_json::from_value::<Request>(json!({
            "op": "Apply", "args": {"actions": [{
                "type": "SetRenderRepresentation", "id": id,
                "render_representation": "voxels", "voxels": {"resolution": resolution}
            }]}
        }))
        .is_err());
    }
}

#[test]
fn poisson_cache_requests_queue_without_editing_the_scene_and_report_completion() {
    use crate::renderer::poisson_mesh::{PoissonMeshAction, PoissonMeshStatus};
    let mut app = App::new();
    let mut root = SdfObject::create_kind(PrimitiveKind::Box);
    root.render_representation = GroupRenderRepresentation::PoissonMesh;
    let id = root.uuid;
    model::set_objects(&mut app.tree, vec![root]);
    let revision = app.scene_revision();
    assert_eq!(
        app.agent_poisson_mesh_status(id).unwrap()["status"],
        "not_built"
    );
    for op in ["BuildPoissonMesh", "GetPoissonMeshStatus"] {
        assert!(serde_json::from_value::<Request>(request_payload(op, json!({"id": id}))).is_ok());
        assert!(
            serde_json::from_value::<Request>(request_payload(op, json!({"id": "invalid"})))
                .is_err()
        );
    }
    let (reply, receiver) = mpsc::channel();
    app.process_agent_request(Inbound {
        request: Request::BuildPoissonMesh { id },
        reply,
    });
    assert_eq!(receiver.recv().unwrap().unwrap()["status"], "queued");
    assert!(app.agent_redraw_pending);
    app.agent_build_poisson_mesh(id).unwrap();
    assert_eq!(
        app.agent_poisson_mesh_status(id).unwrap()["status"],
        "queued"
    );
    assert_eq!(app.scene_revision(), revision);
    app.egui.data_mut(|data| {
        let actions = data
            .remove_temp::<Vec<PoissonMeshAction>>(egui::Id::new("poisson-mesh-actions"))
            .unwrap();
        assert_eq!(
            actions.len(),
            1,
            "duplicate requests must preserve the existing queue"
        );
        assert!(matches!(actions[0], PoissonMeshAction::Build(root) if root == id));
        data.insert_temp(
            egui::Id::new("poisson-mesh-status"),
            HashMap::from([(
                id,
                PoissonMeshStatus::Ready {
                    vertices: 24,
                    triangles: 12,
                    showing: true,
                    available: true,
                    sample_resolution: 32,
                    mesh_resolution: 64,
                },
            )]),
        );
    });
    let ready = app.agent_poisson_mesh_status(id).unwrap();
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["vertices"], 24);
    assert_eq!(ready["triangles"], 12);
    assert_eq!(ready["showing"], true);
    assert_eq!(ready["available"], true);
    assert_eq!(ready["mesh_resolution"], 64);
    app.agent_build_poisson_mesh(id).unwrap();
    assert_eq!(
        app.agent_poisson_mesh_status(id).unwrap()["status"],
        "queued",
        "recompute must not report an old ready cache"
    );
    for name in ["build_poisson_mesh", "get_poisson_mesh_status"] {
        let tool = mcp_tools()
            .into_iter()
            .find(|tool| tool["name"] == name)
            .unwrap();
        assert_eq!(tool["inputSchema"]["required"], json!(["id"]));
    }
}

#[test]
fn poisson_cache_requests_reject_missing_roots_and_ineligible_objects() {
    let mut app = App::new();
    let root = SdfObject::create_kind(PrimitiveKind::Box);
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(root.uuid);
    child.render_representation = GroupRenderRepresentation::PoissonMesh;
    for id in [root.uuid, child.uuid, uuid::Uuid::nil()] {
        model::set_objects(&mut app.tree, vec![root.clone(), child.clone()]);
        assert!(app.agent_build_poisson_mesh(id).is_err());
        assert!(app.agent_poisson_mesh_status(id).is_err());
    }
    assert!(app
        .egui
        .data(
            |data| data.get_temp::<Vec<crate::renderer::poisson_mesh::PoissonMeshAction>>(
                egui::Id::new("poisson-mesh-actions")
            )
        )
        .is_none());
}

#[test]
fn rigid_binding_mcp_is_atomic_and_validates_frames_conflicts_and_references() {
    let mut app = App::new();
    let object = SdfObject::create_kind(PrimitiveKind::Sphere);
    let id = object.uuid;
    model::set_objects(&mut app.tree, vec![object.clone()]);
    let origin = uuid::Uuid::new_v4();
    let aim = uuid::Uuid::new_v4();
    let binding = json!({"object":id,"target":"Object","origin":origin,"aim":aim,"up":{"WorldDirection":[0,0,1]},"local_origin":[0,0,0],"local_aim":[1,0,0],"local_up":[0,0,1]});
    apply_variables_wire(
        &mut app,
        json!([
            {"type":"SetRigidBinding","binding":binding},
            {"type":"CreateVectorVariable","id":origin,"name":"Origin","value":[1,2,0]},
            {"type":"CreateVectorVariable","id":aim,"name":"Aim","value":[1,3,0]}
        ]),
    )
    .unwrap();
    assert_eq!(
        model::objects_ref(&app.tree)[0].transform.translation,
        Vec3::new(1., 2., 0.)
    );
    let mut missing_binding = binding.clone();
    missing_binding["aim"] = json!(uuid::Uuid::new_v4());
    for bad in [
        json!({"type":"SetVectorBinding","object":id,"target":"Position","variable":origin}),
        json!({"type":"UpdateVectorVariable","id":aim,"space":"Local"}),
        json!({"type":"UpdateVectorVariable","id":aim,"value":[1,2,0]}),
        json!({"type":"SetRigidBinding","binding":missing_binding}),
    ] {
        let before = crate::document::serialize_scene(&app.tree).unwrap();
        assert!(apply_variables_wire(&mut app, json!([bad])).is_err());
        assert_eq!(crate::document::serialize_scene(&app.tree).unwrap(), before);
    }
    let mut nonuniform = object;
    nonuniform.group_transform.scale = Vec3::new(2., 1., 1.);
    assert!(
        apply_variables_wire(&mut app, json!([{"type":"PutObject","object":nonuniform}])).is_err()
    );
    let pose = model::objects_ref(&app.tree)[0].transform;
    apply_variables_wire(
        &mut app,
        json!([{"type":"RemoveRigidBinding","object":id,"target":"Object"}]),
    )
    .unwrap();
    assert_eq!(model::objects_ref(&app.tree)[0].transform, pose);
    app.tree.undo();
    assert_eq!(model::scene_variables(&app.tree).rigid_bindings.len(), 1);
}

#[test]
fn four_bar_mcp_materializes_clamped_outputs_and_rejects_invalid_edits_atomically() {
    let mut app = App::new();
    let ids: [uuid::Uuid; 5] = std::array::from_fn(|_| uuid::Uuid::new_v4());
    let constraint = json!({"driver":ids[0],"lower_pivot":ids[1],"upper_pivot":ids[2],"lower_joint":ids[3],"upper_joint":ids[4],"lower_length":2,"upper_length":2,"upright_length":1,"driver_y_offset":0,"travel_min":-0.5,"travel_max":0.5,"branch":"Positive"});
    let vectors = ids
        .iter()
        .zip([
            [9., 0., 7.],
            [0., 0., 0.],
            [0., 1., 0.],
            [0., 0., 0.],
            [0., 0., 0.],
        ])
        .map(|(id, value)| json!({"id":id,"name":"Point","value":value,"space":"World"}))
        .collect::<Vec<_>>();
    apply_variables_wire(&mut app,json!([{"type":"SetVariables","variables":{"vectors":vectors,"bindings":[],"four_bar_constraints":[constraint]}}])).unwrap();
    apply_variables_wire(
        &mut app,
        json!([{"type":"UpdateVectorVariable","id":ids[0],"value":[9,100,7]}]),
    )
    .unwrap();
    let state = model::scene_variables(&app.tree);
    assert_eq!(state.vectors[0].value.y, 0.5);
    assert!((state.vectors[3].value.distance(state.vectors[1].value) - 2.).abs() < 1e-5);
    for bad in [
        json!({"type":"UpdateVectorVariable","id":ids[3],"value":[1,2,3]}),
        json!({"type":"UpdateVectorVariable","id":ids[2],"value":[0,100,0]}),
        json!({"type":"UpdateVectorVariable","id":ids[1],"space":"Local"}),
    ] {
        let before = model::scene_variables(&app.tree);
        assert!(apply_variables_wire(&mut app, json!([bad])).is_err());
        assert_eq!(model::scene_variables(&app.tree), before);
    }
    for field in ["lower_joint", "driver"] {
        let mut invalid = state.clone();
        if field == "lower_joint" {
            invalid
                .four_bar_constraints
                .push(invalid.four_bar_constraints[0].clone());
        } else {
            invalid.four_bar_constraints[0].driver = ids[3];
        }
        assert!(apply_variables_wire(
            &mut app,
            json!([{"type":"SetVariables","variables":invalid}])
        )
        .is_err());
    }
    let mut invalid = state.clone();
    invalid.four_bar_constraints[0].upper_pivot = uuid::Uuid::new_v4();
    assert!(apply_variables_wire(
        &mut app,
        json!([{"type":"SetVariables","variables":invalid}])
    )
    .is_err());
    let saved = crate::document::serialize_scene(&app.tree).unwrap();
    let mut restored = model::DataTree::default();
    restored.set_tree("scene", crate::document::deserialize_scene(&saved).unwrap());
    assert_eq!(model::scene_variables(&restored), state);
    let outputs = [state.vectors[3].value, state.vectors[4].value];
    apply_variables_wire(
        &mut app,
        json!([{"type":"DeleteVectorVariable","id":ids[0]}]),
    )
    .unwrap();
    let after = model::scene_variables(&app.tree);
    assert!(after.four_bar_constraints.is_empty());
    assert_eq!([after.vectors[2].value, after.vectors[3].value], outputs);
    app.tree.undo();
    assert_eq!(model::scene_variables(&app.tree), state);
}

fn glb_request(app: &mut App, op: &str, args: Value) -> AgentResult {
    let request = serde_json::from_value(request_payload(op, args)).map_err(|error| error.to_string())?;
    let (reply, receiver) = mpsc::channel();
    app.process_agent_request(Inbound { request, reply });
    receiver.recv_timeout(Duration::from_secs(1)).unwrap()
}

#[test]
fn glb_export_mcp_validates_arguments_and_cancellation_survives_document_changes() {
    let mut app = App::new();
    let path = std::env::temp_dir().join(format!("claydash-mcp-export-{}.glb", uuid::Uuid::new_v4()));
    for patch in [json!({"path":"relative.glb"}), json!({"object_ids":[]}),
        json!({"object_ids":[uuid::Uuid::new_v4()]}), json!({"geometry":"unknown"}),
        json!({"geometry":"voxels","voxel_resolution":7}), json!({"geometry":"voxels","voxel_resolution":129}),
        json!({"geometry":"smooth","voxel_resolution":32}),
        json!({"geometry":"direct","direct_resolution":7}), json!({"geometry":"direct","direct_resolution":257}),
        json!({"geometry":"smooth","direct_resolution":32}), json!({"typo":true})] {
        let mut args = json!({"path":path});
        args.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        assert!(glb_request(&mut app, "ExportGlb", args).is_err());
        assert!(app.mesh_export.is_none());
    }
    std::fs::write(&path, b"existing file").unwrap();
    assert!(glb_request(&mut app, "ExportGlb", json!({"path":path})).unwrap_err().contains("overwrite"));
    let before = app.scene_revision();
    let started = glb_request(&mut app, "ExportGlb", json!({"path":path,"geometry":"voxels","voxel_resolution":8,"overwrite":true})).unwrap();
    let id = started["id"].clone();
    assert_eq!(app.scene_revision(), before);
    assert!(glb_request(&mut app, "ExportGlb", json!({"path":path,"overwrite":true})).unwrap_err().contains("in progress"));
    assert!(glb_request(&mut app, "CancelGlbExport", json!({"id":uuid::Uuid::new_v4()})).is_err());
    assert!(app.mesh_export.is_some());
    app.replace_scene(crate::model::DataTree::default());
    assert_eq!(glb_request(&mut app, "GetGlbExportStatus", json!({"id":id})).unwrap()["status"], "cancelled");
    assert_eq!(glb_request(&mut app, "CancelGlbExport", json!({"id":id})).unwrap()["status"], "cancelled");
    assert_eq!(std::fs::read(&path).unwrap(), b"existing file");
    std::fs::remove_file(path).unwrap();
    for name in ["export_glb", "get_glb_export_status", "cancel_glb_export"] {
        assert!(mcp_tools().iter().any(|tool| tool["name"] == name));
    }
}

#[test]
fn glb_export_mcp_uses_selected_snapshot_and_retains_completion() {
    let mut app = App::new();
    let mut sphere = SdfObject::create_kind(PrimitiveKind::Sphere);
    sphere.name = "Exported snapshot".into();
    let other = SdfObject::create_kind(PrimitiveKind::Box);
    model::set_objects(&mut app.tree, vec![sphere.clone(), other.clone()]);
    model::set_selected(&mut app.tree, vec![other.uuid]);
    let path = std::env::temp_dir().join(format!("claydash-mcp-snapshot-{}.glb", uuid::Uuid::new_v4()));
    let started = glb_request(&mut app, "ExportGlb", json!({"path":path,"geometry":"voxels","voxel_resolution":8,"object_ids":[sphere.uuid]})).unwrap();
    let id = started["id"].clone();
    sphere.name = "Changed after export started".into();
    model::set_objects(&mut app.tree, vec![sphere, other]);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        app.poll_mesh_export();
        let status = glb_request(&mut app, "GetGlbExportStatus", json!({"id":id})).unwrap();
        if status["status"] != "running" { break status; }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(status["status"], "completed", "{status}");
    assert_eq!(glb_request(&mut app, "CancelGlbExport", json!({"id":id})).unwrap()["status"], "completed");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document: Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
    assert_eq!(document["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(document["nodes"][0]["name"], "Exported snapshot");
    assert!(document["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"].is_number());
}

#[test]
fn glb_export_mcp_reports_write_failures_and_explicit_cancellation() {
    let mut app = App::new();
    let object = SdfObject::create_kind(PrimitiveKind::Sphere);
    model::set_objects(&mut app.tree, vec![object]);
    model::set_selected(&mut app.tree, vec![]);
    let directory = std::env::temp_dir().join(format!("claydash-mcp-failure-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("output.glb");
    let start = |app: &mut App| glb_request(app, "ExportGlb", json!({"path":path,"geometry":"voxels","voxel_resolution":8})).unwrap();
    let cancelled = start(&mut app);
    assert_eq!(glb_request(&mut app, "CancelGlbExport", json!({"id":cancelled["id"]})).unwrap()["status"], "cancelled");
    let failed = start(&mut app);
    std::fs::remove_dir(&directory).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_mesh_export();
        let status = glb_request(&mut app, "GetGlbExportStatus", json!({"id":failed["id"]})).unwrap();
        if status["status"] != "running" {
            assert_eq!(status["status"], "failed", "{status}");
            assert!(!status["error"].as_str().unwrap().is_empty());
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!path.exists());
    assert_eq!(glb_request(&mut app, "GetGlbExportStatus", json!({"id":cancelled["id"]})).unwrap()["status"], "cancelled");
}

#[test]
#[ignore = "requires a GPU adapter and native material readback"]
fn direct_sheet_glb_export_bakes_materials_and_preserves_editable_source() {
    let mut app = App::new();
    let mut sheet = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    sheet.name = "Thin cut direct sheet".into();
    let panel = model::SheetPanelParams {
        size: Vec2::new(1.2, 0.8), thickness: 0.002,
        cuts: vec![model::SheetCut::Circle { center: Vec2::ZERO, radius: 0.15 }],
        ..Default::default()
    };
    sheet.params = SdfParams::SheetPanelParams(panel);
    sheet.color = glam::Vec4::new(0.8, 0.1, 0.25, 1.0);
    model::set_objects(&mut app.tree, vec![sheet.clone()]);
    model::set_selected(&mut app.tree, vec![]);
    let source = crate::document::serialize_scene(&app.tree).unwrap();
    app.renderer = Some(pollster::block_on(crate::renderer::Renderer::new_headless(
        crate::renderer::HeadlessOptions { width: 128, height: 128, ..Default::default() }
    )).expect("headless direct export renderer"));
    let path = std::env::temp_dir().join(format!("claydash-direct-sheet-export-{}.glb", uuid::Uuid::new_v4()));
    let started = glb_request(&mut app, "ExportGlb", json!({
        "path":path, "geometry":"direct", "direct_resolution":16, "object_ids":[sheet.uuid]
    })).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        app.advance_background_computations();
        let status = glb_request(&mut app, "GetGlbExportStatus", json!({"id":started["id"]})).unwrap();
        if status["status"] != "running" {
            assert_eq!(status["status"], "completed", "{status}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "direct GLB export timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(crate::document::serialize_scene(&app.tree).unwrap(), source);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(&bytes[..4], b"glTF");
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document: Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
    assert_eq!(document["nodes"][0]["name"], sheet.name);
    assert!(!document["images"].as_array().unwrap().is_empty(), "direct meshes retain material texture baking");
    for image in document["images"].as_array().unwrap() {
        assert_eq!(image["mimeType"], "image/png");
        assert!(image["bufferView"].is_number(), "material images are embedded in the GLB");
    }
    for mesh in document["meshes"].as_array().unwrap() {
        for primitive in mesh["primitives"].as_array().unwrap() {
            for attribute in ["POSITION", "NORMAL", "TEXCOORD_0"] {
                let accessor = primitive["attributes"][attribute].as_u64().unwrap() as usize;
                assert!(document["accessors"][accessor]["count"].as_u64().unwrap() > 0);
            }
            assert!(primitive["material"].is_number());
            assert!(primitive["indices"].is_number());
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter and native material readback"]
fn direct_smooth_modified_glb_export_retains_materials_and_source() {
    let mut app = App::new();
    let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
    root.name = "Smooth repeated mirrored cage".into();
    root.softness = 0.12;
    root.mirror = Some(model::Mirror { axes: [true, false, false] });
    root.repetition.enabled = true;
    root.repetition.count = [2, 1, 1];
    root.repetition.spacing = Vec3::new(1.1, 1.0, 1.0);
    let mut cage = model::Lattice::new(Vec3::splat(-2.0), Vec3::splat(2.0), 2);
    cage.offsets.fill(Vec3::new(0.04, 0.08, -0.02));
    root.lattice = Some(cage);
    let mut child = SdfObject::create_kind(PrimitiveKind::Box);
    child.boolean_parent = Some(root.uuid);
    child.transform.translation = Vec3::new(0.3, 0.1, 0.0);
    child.color = glam::Vec4::new(0.1, 0.6, 0.8, 1.0);
    model::set_objects(&mut app.tree, vec![root.clone(), child]);
    model::set_selected(&mut app.tree, vec![]);
    let source = crate::document::serialize_scene(&app.tree).unwrap();
    app.renderer = Some(pollster::block_on(crate::renderer::Renderer::new_headless(
        crate::renderer::HeadlessOptions { width: 128, height: 128, ..Default::default() }
    )).expect("headless direct modifier export renderer"));
    let path = std::env::temp_dir().join(format!("claydash-direct-modifier-export-{}.glb", uuid::Uuid::new_v4()));
    let started = glb_request(&mut app, "ExportGlb", json!({
        "path":path, "geometry":"direct", "direct_resolution":16, "object_ids":[root.uuid]
    })).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        app.advance_background_computations();
        let status = glb_request(&mut app, "GetGlbExportStatus", json!({"id":started["id"]})).unwrap();
        if status["status"] != "running" {
            assert_eq!(status["status"], "completed", "{status}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "direct modifier GLB export timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(crate::document::serialize_scene(&app.tree).unwrap(), source);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(&bytes[..4], b"glTF");
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document: Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
    assert_eq!(document["nodes"][0]["name"], root.name);
    assert!(!document["images"].as_array().unwrap().is_empty());
    for mesh in document["meshes"].as_array().unwrap() {
        for primitive in mesh["primitives"].as_array().unwrap() {
            assert!(primitive["material"].is_number());
            assert!(primitive["attributes"]["TEXCOORD_0"].is_number());
            assert!(primitive["indices"].is_number());
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter and native material readback"]
fn direct_image_plane_glb_export_preserves_baked_alpha() {
    let mut app = App::new();
    let mut sheet = SdfObject::create_image_plane();
    sheet.name = "Direct RGBA image plane".into();
    let image = image::RgbaImage::from_raw(2, 2, vec![
        255, 0, 0, 0, 0, 255, 0, 255,
        0, 0, 255, 0, 255, 255, 0, 255,
    ]).unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image).write_to(&mut png, image::ImageFormat::Png).unwrap();
    sheet.image_stencil = Some(model::ImageStencil::new("RGBA test".into(), png.into_inner(), true));
    model::set_objects(&mut app.tree, vec![sheet.clone()]);
    model::set_selected(&mut app.tree, vec![]);
    let source = crate::document::serialize_scene(&app.tree).unwrap();
    app.renderer = Some(pollster::block_on(crate::renderer::Renderer::new_headless(
        crate::renderer::HeadlessOptions { width: 128, height: 128, ..Default::default() }
    )).expect("headless direct export renderer"));
    let path = std::env::temp_dir().join(format!("claydash-direct-image-export-{}.glb", uuid::Uuid::new_v4()));
    let started = glb_request(&mut app, "ExportGlb", json!({
        "path":path, "geometry":"direct", "direct_resolution":16, "object_ids":[sheet.uuid]
    })).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        app.advance_background_computations();
        let status = glb_request(&mut app, "GetGlbExportStatus", json!({"id":started["id"]})).unwrap();
        if status["status"] != "running" {
            assert_eq!(status["status"], "completed", "{status}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "direct GLB export timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(crate::document::serialize_scene(&app.tree).unwrap(), source);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(&bytes[..4], b"glTF");
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document: Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
    assert_eq!(document["nodes"][0]["name"], sheet.name);
    assert!(!document["images"].as_array().unwrap().is_empty(), "direct meshes retain material texture baking");
    assert!(document["materials"].as_array().unwrap().iter().any(|material| material["alphaMode"] == "BLEND"),
        "image-plane alpha must select glTF blending");
    let binary_offset = 20 + length + 8;
    let mut found_transparency = false;
    for image in document["images"].as_array().unwrap() {
        let view = &document["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
        let offset = binary_offset + view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let size = view["byteLength"].as_u64().unwrap() as usize;
        let baked = image::load_from_memory(&bytes[offset..offset + size]).unwrap().to_rgba8();
        found_transparency |= baked.pixels().any(|pixel| pixel[3] < 255);
    }
    assert!(found_transparency, "embedded PNG must retain alpha");
    for image in document["images"].as_array().unwrap() {
        assert_eq!(image["mimeType"], "image/png");
        assert!(image["bufferView"].is_number(), "material images are embedded in the GLB");
    }
    for mesh in document["meshes"].as_array().unwrap() {
        for primitive in mesh["primitives"].as_array().unwrap() {
            for attribute in ["POSITION", "NORMAL", "TEXCOORD_0"] {
                let accessor = primitive["attributes"][attribute].as_u64().unwrap() as usize;
                assert!(document["accessors"][accessor]["count"].as_u64().unwrap() > 0);
            }
            assert!(primitive["material"].is_number());
            assert!(primitive["indices"].is_number());
        }
    }
}

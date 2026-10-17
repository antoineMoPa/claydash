//! Scene editing, sheet recipes, capture isolation and transaction regression tests.
use super::capture::orthographic_sheet;
use super::*;

mod rendering;
mod variables;

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
        let snapshot=capture::capture_scene(model::objects_ref(&app.tree),Some(&[source_id,target_id]),Some(stage),CaptureIsolation::Groups).unwrap().unwrap();
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
        let snapshot = capture::capture_scene(model::objects_ref(&app.tree), Some(&[id]), Some(stage), CaptureIsolation::Groups).unwrap().unwrap();
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
fn part_capture_detaches_operands_without_moving_or_editing_them() {
    let mut root = SdfObject::create_kind(PrimitiveKind::Box);
    root.group_transform.translation = Vec3::new(2.0, 0.0, 0.0);
    root.group_transform.rotation = glam::Quat::from_rotation_z(0.4);
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(root.uuid);
    child.operation = BooleanOperation::Subtract;
    child.group_transform.translation = Vec3::new(0.0, 1.0, 0.0);
    child.transform.translation = Vec3::new(0.0, 0.0, 0.5);
    child.render_representation = GroupRenderRepresentation::BoxDepthAtlas;
    let mut unrelated = SdfObject::create_kind(PrimitiveKind::Box);
    unrelated.transform.translation = Vec3::splat(9.0);
    let scene = vec![root, child.clone(), unrelated];
    let original = serde_json::to_value(&scene).unwrap();
    let captured = capture::capture_scene(
        &scene, Some(&[child.uuid, child.uuid]), None, CaptureIsolation::Parts,
    ).unwrap().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].uuid, child.uuid);
    assert_eq!(captured[0].boolean_parent, None);
    assert_eq!(captured[0].operation, BooleanOperation::Union);
    assert_eq!(captured[0].render_representation, GroupRenderRepresentation::ExactSdf);
    assert!(model::object_world_matrix(&captured, child.uuid)
        .abs_diff_eq(model::object_world_matrix(&scene, child.uuid), 1e-4));
    assert_eq!(serde_json::to_value(&scene).unwrap(), original);
    assert!(capture::capture_scene(&scene, None, None, CaptureIsolation::Parts).is_err());
    assert!(capture::capture_scene(&scene, Some(&[]), None, CaptureIsolation::Parts).is_err());
    assert!(capture::capture_scene(&scene, Some(&[uuid::Uuid::nil()]), None, CaptureIsolation::Parts).is_err());
}

#[test]
fn part_capture_rejects_inlays_that_require_their_host() {
    let host = SdfObject::create_kind(PrimitiveKind::Box);
    let mut inlay = SdfObject::create_kind(PrimitiveKind::Box);
    inlay.surface_inlay = Some(model::SurfaceInlay { host: host.uuid, offset: 0.01, thickness: 0.02 });
    let error = capture::capture_parts(&[host, inlay.clone()], &[inlay.uuid]).err().unwrap();
    assert!(error.contains("requires its host"), "{error}");
}

#[test]
fn part_capture_rejects_sheared_parent_frames() {
    let mut root = SdfObject::create_kind(PrimitiveKind::Box);
    root.group_transform.scale = Vec3::new(2.0, 1.0, 1.0);
    let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
    child.boolean_parent = Some(root.uuid);
    child.group_transform.rotation = glam::Quat::from_rotation_z(0.5);
    let error = capture::capture_parts(&[root, child.clone()], &[child.uuid]).err().unwrap();
    assert!(error.contains("sheared group transform"), "{error}");
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
fn imported_gaussian_ply_is_embedded_undoable_and_failure_is_atomic() {
    let mut app = App::new();
    app.new_document();
    let names = ["x", "y", "z", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2", "rot_3", "f_dc_0", "f_dc_1", "f_dc_2", "opacity"];
    let ply = format!("ply\nformat ascii 1.0\nelement vertex 1\n{}end_header\n0 0 0 -2 -3 -4 1 0 0 0 1 0 0 3\n", names.map(|n| format!("property float {n}\n")).join(""));
    let path = std::env::temp_dir().join(format!("claydash-{}.ply", uuid::Uuid::new_v4()));
    std::fs::write(&path, ply).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    app.process_agent_request(Inbound { request: Request::ImportGaussianPly { path: path.clone() }, reply: tx });
    let response = rx.recv().unwrap().unwrap();
    std::fs::remove_file(&path).unwrap();
    let id: uuid::Uuid = serde_json::from_value(response["id"].clone()).unwrap();
    assert_eq!(model::selected(&app.tree), vec![id]);
    let saved = document::serialize_scene(&app.tree).unwrap();
    assert!(app.import_gaussian_ply("bad", b"bad").is_err());
    assert_eq!(saved, document::serialize_scene(&app.tree).unwrap());
    app.tree.undo();
    assert!(!model::objects(&app.tree).iter().any(|o| o.uuid == id));
    app.tree.redo();
    assert!(model::objects(&app.tree).iter().any(|o| o.uuid == id));
    let scene = document::deserialize_scene(&saved).unwrap();
    app.replace_scene(scene);
    let object = model::objects(&app.tree).into_iter().find(|o| o.uuid == id).unwrap();
    assert_eq!(object.imported_splats.unwrap().splats.len(), 1);
}

#[test]
fn compact_gaussian_queries_preserve_revision_and_transform_positions() {
    let mut app = App::new();
    app.new_document();
    let names = ["x", "y", "z", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2", "rot_3", "f_dc_0", "f_dc_1", "f_dc_2", "opacity"];
    let ply = format!("ply\nformat ascii 1.0\nelement vertex 3\n{}end_header\n-1 0 0 -2 -2 -2 1 0 0 0 1 0 0 3\n0 1 0 -2 -2 -2 1 0 0 0 0 1 0 3\n1 2 0 -2 -2 -2 1 0 0 0 0 0 1 3\n", names.map(|name| format!("property float {name}\n")).join(""));
    let id = app.import_gaussian_ply("sample", ply.as_bytes()).unwrap();
    let mut transform = model::objects_ref(&app.tree).iter().find(|object| object.uuid == id).unwrap().transform;
    transform.translation = Vec3::new(10.0, 0.0, 0.0);
    app.agent_apply(ApplyArgs { expected_revision: None, actions: vec![Action::SetObjectTransform { id, transform }] }).unwrap();
    let summary = app.agent_scene_summary().unwrap();
    assert_eq!(summary["revision"], app.scene_revision());
    let entry = summary["objects"].as_array().unwrap().iter().find(|entry| entry["id"] == json!(id)).unwrap();
    assert_eq!(entry["imported_gaussian"]["count"], 3);
    assert_eq!(entry["imported_gaussian"]["world_bounds"]["min"], json!([9.0, 0.0, 0.0]));
    assert!(!summary.to_string().contains("gaussian-f32-le-v1"));
    let local = app.agent_gaussian_cloud(id, Some(CloudSpace::Object), Some(2)).unwrap();
    let world = app.agent_gaussian_cloud(id, Some(CloudSpace::World), Some(2)).unwrap();
    assert_eq!(local["percentiles"]["p50"], json!([0.0, 1.0, 0.0]));
    assert_eq!(world["percentiles"]["p50"], json!([10.0, 1.0, 0.0]));
    assert_eq!(world["samples"].as_array().unwrap().len(), 2);
    assert!(app.agent_gaussian_cloud(id, None, Some(257)).is_err());
}

#[test]
fn targeted_action_schema_shows_tag_and_sheet_bends() {
    let result = mcp::action_schema("CreateObject", Some(PrimitiveKind::SheetPanel)).unwrap();
    assert_eq!(result["action"]["properties"]["type"]["const"], "CreateObject");
    assert_eq!(result["sheet_bend"]["example"]["direction"], 0.0);
    assert!(result["params_example"]["SheetPanelParams"].is_object());
    assert!(serde_json::from_value::<Request>(request_payload("Apply", result["apply_example"].clone())).is_ok());
    assert!(mcp::action_schema("MissingAction", None).is_err());
    let orthographic = mcp::mcp_tools().into_iter().find(|tool| tool["name"] == "capture_orthographic").unwrap();
    assert_eq!(orthographic["inputSchema"]["properties"]["panel_size"]["minimum"], 96);
    assert_eq!(orthographic["inputSchema"]["properties"]["panel_size"]["maximum"], 512);
}

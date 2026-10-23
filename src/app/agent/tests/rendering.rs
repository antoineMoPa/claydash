//! Capture modes, render representations, mesh caches and GLB export tests.
use super::super::mcp::mcp_tools;
use super::super::*;

#[test]
fn agent_view_is_independent_and_user_capture_is_explicit() {
    let mut app = App::new();
    app.new_document();
    app.camera.position = Vec3::new(0.0, 0.0, 12.0);
    let user_position = app.camera.position;
    app.agent_set_view(ViewArgs {
        position: [3.0, 4.0, 5.0], target: [0.0, 0.0, 0.0],
        up: None, projection_mode: Some(ProjectionMode::Orthographic),
    }).unwrap();
    assert_eq!(app.camera.position, user_position);
    assert_eq!(app.agent_view_camera().position, Vec3::new(3.0, 4.0, 5.0));
    assert_eq!(app.agent_state().unwrap()["view"]["position"], json!([3.0, 4.0, 5.0]));
    let default: Request = serde_json::from_value(request_payload("CaptureViewport", json!({}))).unwrap();
    let user: Request = serde_json::from_value(request_payload("CaptureViewport", json!({"view":"user"}))).unwrap();
    assert!(matches!(default, Request::CaptureViewport(CaptureViewportArgs { view: CaptureCameraSource::Agent, .. })));
    assert!(matches!(user, Request::CaptureViewport(CaptureViewportArgs { view: CaptureCameraSource::User, .. })));
    assert!(mcp_tools().iter().any(|tool| tool["name"] == "get_user_view"));
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
fn capture_isolation_is_typed_and_advertised_for_both_views() {
    for (op, tool_name) in [
        ("CaptureViewport", "capture_viewport"),
        ("CaptureOrthographic", "capture_orthographic"),
    ] {
        for (option, expected) in [
            ("groups", CaptureIsolation::Groups),
            ("parts", CaptureIsolation::Parts),
        ] {
            let request = serde_json::from_value::<Request>(request_payload(
                op, json!({"isolation": option, "object_ids": [uuid::Uuid::new_v4()]}),
            )).unwrap();
            let actual = match request {
                Request::CaptureViewport(args) => args.isolation,
                Request::CaptureOrthographic(args) => args.isolation,
                _ => panic!("unexpected request"),
            };
            assert_eq!(actual, expected);
        }
        assert!(serde_json::from_value::<Request>(request_payload(op, json!({"isolation": "unknown"}))).is_err());
        let tool = mcp_tools().into_iter().find(|tool| tool["name"] == tool_name).unwrap();
        assert_eq!(tool["inputSchema"]["properties"]["isolation"]["enum"], json!(["groups", "parts"]));
    }
    assert_eq!(CaptureIsolation::default(), CaptureIsolation::Groups);
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
    app.new_document();
    let mut root = SdfObject::create_kind(PrimitiveKind::Box);
    root.softness = 0.;
    let root_id = root.uuid;
    model::set_objects(&mut app.tree, vec![root]);
    let mut child = SdfObject::create_kind(PrimitiveKind::Box);
    child.softness = 0.;
    let child_id = child.uuid;
    let mut scene = model::objects(&app.tree);
    scene.push(child);
    model::set_objects(&mut app.tree, scene);
    app.agent_enable_boolean_operations().unwrap();
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
    app.agent_enable_boolean_operations().unwrap();
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
        json!({"geometry":"smooth","direct_resolution":32}), json!({"geometry":"direct","direct_error":0}),
        json!({"geometry":"direct","direct_error":-0.001}),json!({"geometry":"smooth","direct_error":0.001}),json!({"typo":true})] {
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
fn direct_sheet_glb_export_uses_indexed_solid_materials_and_preserves_editable_source() {
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
    assert!(document["images"].as_array().unwrap().is_empty(), "solid sheets do not bake textures");
    for image in document["images"].as_array().unwrap() {
        assert_eq!(image["mimeType"], "image/png");
        assert!(image["bufferView"].is_number(), "material images are embedded in the GLB");
    }
    for mesh in document["meshes"].as_array().unwrap() {
        for primitive in mesh["primitives"].as_array().unwrap() {
            for attribute in ["POSITION", "NORMAL"] {
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

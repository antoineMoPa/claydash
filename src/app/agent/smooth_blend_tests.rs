use super::*;

fn request(app: &mut App, op: &str, args: Value) -> AgentResult {
    let request = serde_json::from_value(request_payload(op, args)).unwrap();
    let (reply, receiver) = mpsc::channel();
    app.process_agent_request(Inbound { request, reply });
    receiver.recv_timeout(Duration::from_secs(1)).unwrap()
}

fn apply(app: &mut App, actions: Value) -> AgentResult {
    request(app, "Apply", json!({"actions": actions}))
}

fn empty_app() -> App {
    let mut app = App::new();
    app.new_document();
    app
}

fn enabled(app: &mut App) -> bool {
    request(app, "GetState", json!({})).unwrap()["modeling_policy"]["slow_smooth_blends_enabled"]
        .as_bool()
        .unwrap()
}

fn objects(app: &App) -> Value {
    serde_json::to_value(model::objects_ref(&app.tree)).unwrap()
}

fn create_pair(app: &mut App) -> [uuid::Uuid; 2] {
    let result = apply(
        app,
        json!([
            {"type":"CreateObject","kind":"Box"},
            {"type":"CreateObject","kind":"Sphere"}
        ]),
    )
    .unwrap();
    [0, 1].map(|index| serde_json::from_value(result["created_ids"][index].clone()).unwrap())
}

fn assert_policy_rejection(app: &mut App, actions: Value) {
    let before = objects(app);
    let revision = app.scene_revision();
    let scene = document::serialize_scene(&app.tree).unwrap();
    let error = apply(app, actions).unwrap_err();
    assert!(error.contains("enable_slow_smooth_blends"), "{error}");
    assert_eq!(objects(app), before);
    assert_eq!(document::serialize_scene(&app.tree).unwrap(), scene);
    assert_eq!(app.scene_revision(), revision);
}

#[test]
fn mcp_defaults_every_primitive_to_hard_boolean_without_gating_local_rounding() {
    let mut app = empty_app();
    assert!(!enabled(&mut app));
    for kind in PrimitiveKind::ALL {
        apply(&mut app, json!([{"type":"CreateObject","kind":kind}])).unwrap();
        assert_eq!(model::objects_ref(&app.tree).last().unwrap().softness, 0.0);
    }
    let box_id = model::objects_ref(&app.tree)
        .iter()
        .find(|object| matches!(object.params, SdfParams::BoxParams(_)))
        .unwrap()
        .uuid;
    let mut params = model::objects_ref(&app.tree)
        .iter()
        .find(|object| object.uuid == box_id)
        .unwrap()
        .params
        .clone();
    let SdfParams::BoxParams(ref mut box_params) = params else {
        unreachable!()
    };
    box_params.corner_radius = 0.1;
    apply(
        &mut app,
        json!([{"type":"SetObjectParams","id":box_id,"params":params}]),
    )
    .unwrap();
    assert!(!enabled(&mut app));
    // Native editor/model callers retain their primitive defaults and are not gated.
    let native = SdfObject::create_kind(PrimitiveKind::Sphere);
    let native_softness = native.softness;
    model::set_objects(&mut app.tree, vec![native]);
    assert_eq!(model::objects_ref(&app.tree)[0].softness, native_softness);
}

#[test]
fn positive_boolean_and_put_object_bypasses_reject_atomically() {
    let mut app = empty_app();
    let [root, child] = create_pair(&mut app);
    assert_policy_rejection(
        &mut app,
        json!([
            {"type":"SetObjectName","id":root,"name":"must roll back"},
            {"type":"SetBoolean","id":root,"parent":null,"operation":"Union","softness":0.2}
        ]),
    );
    // Rejection must not insert an undo snapshot for the first successful action.
    request(&mut app, "Undo", json!({})).unwrap();
    assert!(model::objects_ref(&app.tree).is_empty());
    request(&mut app, "Redo", json!({})).unwrap();
    assert_eq!(model::objects_ref(&app.tree).len(), 2);
    let mut changed = model::objects_ref(&app.tree)[0].clone();
    changed.softness = 0.2;
    assert_policy_rejection(&mut app, json!([{"type":"PutObject","object":changed}]));
    assert_policy_rejection(
        &mut app,
        json!([
            {"type":"SetBoolean","id":child,"parent":root,"operation":"Subtract","softness":0.1}
        ]),
    );
}

#[test]
fn legacy_blends_remain_editable_but_cannot_be_activated_or_enlarged() {
    let mut app = empty_app();
    let [root, child] = create_pair(&mut app);
    let mut legacy = model::objects(&app.tree);
    legacy[0].softness = 0.3;
    model::set_objects(&mut app.tree, legacy);
    app.tree.make_undo_redo_snapshot();
    assert_policy_rejection(
        &mut app,
        json!([
            {"type":"SetBoolean","id":child,"parent":root,"operation":"Union","softness":0.0}
        ]),
    );
    // Import an already active legacy blend through native scene loading.
    let mut legacy = model::objects(&app.tree);
    legacy[1].boolean_parent = Some(root);
    model::set_objects(&mut app.tree, legacy);
    app.tree.make_undo_redo_snapshot();
    apply(
        &mut app,
        json!([{"type":"SetObjectName","id":root,"name":"Imported group"}]),
    )
    .unwrap();
    assert_eq!(model::objects_ref(&app.tree)[0].softness, 0.3);
    let mut lowered = model::objects_ref(&app.tree)[0].clone();
    lowered.softness = 0.15;
    apply(&mut app, json!([{"type":"PutObject","object":lowered}])).unwrap();
    assert_policy_rejection(
        &mut app,
        json!([
            {"type":"SetBoolean","id":root,"parent":null,"operation":"Union","softness":0.2}
        ]),
    );
    assert!(!enabled(&mut app));
}

#[test]
fn explicit_enable_warns_preserves_geometry_and_is_one_undoable_change() {
    let mut app = empty_app();
    let [root, child] = create_pair(&mut app);
    let before = objects(&app);
    let response = request(&mut app, "EnableSlowSmoothBlends", json!({})).unwrap();
    assert_eq!(response["enabled"], true);
    let warning = response["warning"].as_str().unwrap();
    assert!(warning.contains("dense"), "{warning}");
    assert!(warning.contains("export"), "{warning}");
    assert!(enabled(&mut app));
    assert_eq!(objects(&app), before);
    let revision = app.scene_revision();
    request(&mut app, "EnableSlowSmoothBlends", json!({})).unwrap();
    assert_eq!(app.scene_revision(), revision);
    request(&mut app, "Undo", json!({})).unwrap();
    assert!(!enabled(&mut app));
    assert_eq!(objects(&app), before);
    request(&mut app, "Redo", json!({})).unwrap();
    assert!(enabled(&mut app));
    apply(
        &mut app,
        json!([
            {"type":"SetBoolean","id":root,"parent":null,"operation":"Union","softness":0.2},
            {"type":"SetBoolean","id":child,"parent":root,"operation":"Union","softness":0.2}
        ]),
    )
    .unwrap();
    assert_eq!(model::objects_ref(&app.tree)[0].softness, 0.2);
    request(&mut app, "Undo", json!({})).unwrap();
    assert!(enabled(&mut app));
    assert_eq!(objects(&app), before);
}

#[test]
fn saved_opt_in_survives_reload_new_resets_and_replace_cannot_enable_it() {
    let mut app = empty_app();
    create_pair(&mut app);
    request(&mut app, "EnableSlowSmoothBlends", json!({})).unwrap();
    let saved = document::serialize_scene(&app.tree).unwrap();
    let saved_objects = objects(&app);
    app.new_document();
    assert!(!enabled(&mut app));
    app.replace_scene(document::deserialize_scene(&saved).unwrap());
    assert!(enabled(&mut app));
    assert_eq!(objects(&app), saved_objects);
    let raw = request(&mut app, "GetState", json!({})).unwrap()["document"].clone();
    app.new_document();
    apply(&mut app, json!([{"type":"ReplaceScene","scene":raw}])).unwrap();
    assert!(
        !enabled(&mut app),
        "ReplaceScene must ignore an injected opt-in"
    );
    request(&mut app, "EnableSlowSmoothBlends", json!({})).unwrap();
    let mut disabled = empty_app();
    let raw = request(&mut disabled, "GetState", json!({})).unwrap()["document"].clone();
    apply(&mut app, json!([{"type":"ReplaceScene","scene":raw}])).unwrap();
    assert!(
        enabled(&mut app),
        "ReplaceScene must retain current explicit opt-in"
    );
}

#[test]
fn execute_command_uses_hard_spawn_defaults_and_cannot_duplicate_legacy_blends() {
    let mut app = empty_app();
    for name in [
        "spawn-box",
        "spawn-sphere",
        "spawn-cylinder",
        "spawn-torus",
        "spawn-polygon-prism",
        "spawn-loft",
        "spawn-text",
    ] {
        request(&mut app, "ExecuteCommand", json!({"name":name})).unwrap();
        assert_eq!(model::objects_ref(&app.tree).last().unwrap().softness, 0.0);
    }
    let mut scene = model::objects(&app.tree);
    scene[0].softness = 0.3;
    scene[1].boolean_parent = Some(scene[0].uuid);
    let root = scene[0].uuid;
    let child = scene[1].uuid;
    model::set_objects(&mut app.tree, scene);
    apply(
        &mut app,
        json!([{"type":"SetSelection","ids":[root,child]}]),
    )
    .unwrap();
    let before = objects(&app);
    let revision = app.scene_revision();
    let error = request(&mut app, "ExecuteCommand", json!({"name":"duplicate"})).unwrap_err();
    assert!(error.contains("enable_slow_smooth_blends"), "{error}");
    assert_eq!(objects(&app), before);
    assert_eq!(app.scene_revision(), revision);
}

#[test]
fn mcp_advertises_explicit_opt_in_and_efficient_modeling_guidance() {
    let tools = mcp::mcp_tools();
    for name in ["enable_slow_smooth_blends", "apply"] {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        let description = tool["description"].as_str().unwrap();
        assert!(description.contains("prefer arrangements of sheet bends"));
    }
    let schema = mcp::schema();
    assert!(schema["operations"]
        .as_array()
        .unwrap()
        .contains(&json!("EnableSlowSmoothBlends")));
    assert!(schema["notes"]
        .as_str()
        .unwrap()
        .contains("prefer arrangements of sheet bends"));
    for primitive in schema["primitive_kinds"].as_array().unwrap() {
        assert_eq!(primitive["example"]["softness"], 0.0);
    }
    let actions = &tools.iter().find(|tool| tool["name"] == "apply").unwrap()["inputSchema"]
        ["properties"]["actions"]["items"]["oneOf"];
    let boolean = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["properties"]["type"]["const"] == "SetBoolean")
        .unwrap();
    assert_eq!(boolean["properties"]["softness"]["default"], 0);
}

#[test]
fn animation_softness_keys_and_positive_bezier_controls_require_opt_in() {
    let mut app = empty_app();
    let [root, _] = create_pair(&mut app);
    let mut animation = AnimationData::default();
    animation.tracks.push(model::AnimationTrack {
        binding: model::AnimationBinding {
            object: root,
            property: model::AnimatableProperty::OperandSoftness,
        },
        keyframes: vec![model::Keyframe {
            frame: 0,
            value: 0.2,
            interpolation: model::KeyframeInterpolation::Bezier,
            incoming_handle: None,
            outgoing_handle: None,
        }],
    });
    assert_policy_rejection(
        &mut app,
        json!([{"type":"SetAnimation","animation":animation}]),
    );
    animation.tracks[0].keyframes[0].value = 0.0;
    animation.tracks[0].keyframes[0].outgoing_handle = Some(model::BezierHandle {
        frame_offset: 1.0,
        value_offset: 0.2,
    });
    assert_policy_rejection(
        &mut app,
        json!([{"type":"SetAnimation","animation":animation}]),
    );
    request(&mut app, "EnableSlowSmoothBlends", json!({})).unwrap();
    apply(
        &mut app,
        json!([{"type":"SetAnimation","animation":animation}]),
    )
    .unwrap();
}

#[test]
fn command_spawn_undo_redo_never_restores_positive_softness_intermediate_snapshots() {
    for name in [
        "spawn-box",
        "spawn-sphere",
        "spawn-cylinder",
        "spawn-torus",
        "spawn-polygon-prism",
        "spawn-loft",
        "spawn-text",
        "add-image-plane",
    ] {
        let mut app = empty_app();
        request(&mut app, "ExecuteCommand", json!({"name":name})).unwrap();
        let expected = objects(&app);
        assert_eq!(model::objects_ref(&app.tree).len(), 1, "{name}");
        let assert_hard = |app: &App| {
            for object in model::objects_ref(&app.tree) {
                assert_eq!(
                    object.softness, 0.0,
                    "{name}: history restored a positive intermediate object"
                );
            }
        };
        assert_hard(&app);
        // A command may create an interaction snapshot as well as the final one.
        // Visit every reachable intermediate state and then replay the history.
        for _ in 0..4 {
            request(&mut app, "Undo", json!({})).unwrap();
            assert_hard(&app);
        }
        assert!(model::objects_ref(&app.tree).is_empty(), "{name}");
        for _ in 0..4 {
            request(&mut app, "Redo", json!({})).unwrap();
            assert_hard(&app);
        }
        assert_eq!(objects(&app), expected, "{name}");
    }
}

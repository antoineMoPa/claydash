use super::*;

fn request(app: &mut App, op: &str, args: Value) -> AgentResult {
    let request = serde_json::from_value(request_payload(op, args)).unwrap();
    let (reply, receiver) = mpsc::channel();
    app.process_agent_request(Inbound { request, reply });
    receiver.recv_timeout(Duration::from_secs(1)).unwrap()
}

fn apply(app: &mut App, actions: Value) -> AgentResult {
    request(app, "Apply", json!({"actions":actions}))
}

fn empty_app() -> App {
    let mut app = App::new();
    app.new_document();
    app
}

fn pair(app: &mut App) -> [uuid::Uuid; 2] {
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

fn rejected(app: &mut App, actions: Value) {
    let before = document::serialize_scene(&app.tree).unwrap();
    let revision = app.scene_revision();
    let error = apply(app, actions).unwrap_err();
    assert!(error.contains("enable_boolean_operations"), "{error}");
    assert_eq!(document::serialize_scene(&app.tree).unwrap(), before);
    assert_eq!(app.scene_revision(), revision);
}

#[test]
fn new_boolean_edges_and_roles_reject_atomically_for_every_operation() {
    for operation in ["Union", "Subtract", "Intersect"] {
        let mut app = empty_app();
        let [root, child] = pair(&mut app);
        assert!(!boolean_policy::enabled(&app.tree));
        rejected(
            &mut app,
            json!([
                {"type":"SetObjectName","id":root,"name":"roll back"},
                {"type":"SetBoolean","id":child,"parent":root,"operation":operation,"softness":0}
            ]),
        );
        if operation != "Union" {
            rejected(
                &mut app,
                json!([
                    {"type":"SetBoolean","id":child,"parent":null,"operation":operation}
                ]),
            );
        }
        request(&mut app, "Undo", json!({})).unwrap();
        assert!(
            model::objects_ref(&app.tree).is_empty(),
            "rejection must not create an undo step"
        );
    }
}

#[test]
fn put_object_and_replace_scene_cannot_bypass_opt_in() {
    let mut app = empty_app();
    let [root, _] = pair(&mut app);
    let mut object = model::objects_ref(&app.tree)[1].clone();
    object.boolean_parent = Some(root);
    rejected(&mut app, json!([{"type":"PutObject","object":object}]));
    let mut raw = request(&mut app, "GetState", json!({})).unwrap()["document"].clone();
    raw["subtree"]["sdf_objects"]["value"]["VecSDFObject"][1]["boolean_parent"] = json!(root);
    raw["subtree"]["boolean_operations_enabled"] = json!({"subtree":{},"value":{"Bool":true}});
    rejected(&mut app, json!([{"type":"ReplaceScene","scene":raw}]));
    assert!(!boolean_policy::enabled(&app.tree));
}

#[test]
fn loaded_booleans_remain_editable_but_new_links_and_operation_changes_are_gated() {
    let mut app = empty_app();
    let [root, child] = pair(&mut app);
    let mut objects = model::objects(&app.tree);
    objects[1].boolean_parent = Some(root);
    model::set_objects(&mut app.tree, objects);
    apply(
        &mut app,
        json!([
            {"type":"SetObjectName","id":root,"name":"legacy assembly"},
            {"type":"SetBoolean","id":child,"parent":root,"operation":"Union","softness":0}
        ]),
    )
    .unwrap();
    rejected(
        &mut app,
        json!([
            {"type":"SetBoolean","id":child,"parent":root,"operation":"Subtract","softness":0}
        ]),
    );
    apply(
        &mut app,
        json!([
            {"type":"SetBoolean","id":child,"parent":null,"operation":"Union","softness":0}
        ]),
    )
    .unwrap();
    assert!(model::objects_ref(&app.tree)[1].boolean_parent.is_none());
    rejected(
        &mut app,
        json!([
            {"type":"SetBoolean","id":child,"parent":root,"operation":"Union","softness":0}
        ]),
    );
}

#[test]
fn commands_cannot_arm_a_union_or_duplicate_an_imported_boolean_tree() {
    let mut app = empty_app();
    let [root, child] = pair(&mut app);
    let before = document::serialize_scene(&app.tree).unwrap();
    let error = request(&mut app, "ExecuteCommand", json!({"name":"union"})).unwrap_err();
    assert!(error.contains("enable_boolean_operations"));
    assert_eq!(document::serialize_scene(&app.tree).unwrap(), before);
    let mut objects = model::objects(&app.tree);
    objects[1].boolean_parent = Some(root);
    model::set_objects(&mut app.tree, objects);
    apply(
        &mut app,
        json!([{"type":"SetSelection","ids":[root,child]}]),
    )
    .unwrap();
    let before = document::serialize_scene(&app.tree).unwrap();
    let revision = app.scene_revision();
    let error = request(&mut app, "ExecuteCommand", json!({"name":"duplicate"})).unwrap_err();
    assert!(error.contains("enable_boolean_operations"), "{error}");
    assert_eq!(document::serialize_scene(&app.tree).unwrap(), before);
    assert_eq!(app.scene_revision(), revision);
    apply(&mut app, json!([{"type":"DeleteObject","id":root}])).unwrap();
    assert!(model::objects_ref(&app.tree)[0].boolean_parent.is_none());
}

#[test]
fn boolean_and_smooth_blend_opt_ins_are_independent() {
    let mut app = empty_app();
    let [root, child] = pair(&mut app);
    request(&mut app, "EnableSlowSmoothBlends", json!({})).unwrap();
    rejected(
        &mut app,
        json!([
            {"type":"SetBoolean","id":child,"parent":root,"operation":"Union","softness":0}
        ]),
    );
    app.new_document();
    let [root, child] = pair(&mut app);
    let result = request(&mut app, "EnableBooleanOperations", json!({})).unwrap();
    assert!(result["warning"].as_str().unwrap().contains("rendering"));
    assert!(!smooth_blend_policy::enabled(&app.tree));
    for operation in ["Union", "Subtract", "Intersect"] {
        apply(
            &mut app,
            json!([
                {"type":"SetBoolean","id":child,"parent":root,"operation":operation,"softness":0}
            ]),
        )
        .unwrap();
    }
    let error = apply(
        &mut app,
        json!([
            {"type":"SetBoolean","id":root,"parent":null,"operation":"Union","softness":0.1}
        ]),
    )
    .unwrap_err();
    assert!(error.contains("enable_slow_smooth_blends"));
}

#[test]
fn boolean_opt_in_is_saved_undoable_and_reset_by_new_but_not_injectable() {
    let mut app = empty_app();
    pair(&mut app);
    request(&mut app, "EnableBooleanOperations", json!({})).unwrap();
    let revision = app.scene_revision();
    request(&mut app, "EnableBooleanOperations", json!({})).unwrap();
    assert_eq!(app.scene_revision(), revision);
    request(&mut app, "Undo", json!({})).unwrap();
    assert!(!boolean_policy::enabled(&app.tree));
    request(&mut app, "Redo", json!({})).unwrap();
    assert!(boolean_policy::enabled(&app.tree));
    let saved = document::serialize_scene(&app.tree).unwrap();
    let raw = request(&mut app, "GetState", json!({})).unwrap()["document"].clone();
    app.new_document();
    assert!(!boolean_policy::enabled(&app.tree));
    apply(&mut app, json!([{"type":"ReplaceScene","scene":raw}])).unwrap();
    assert!(!boolean_policy::enabled(&app.tree));
    app.replace_scene(document::deserialize_scene(&saved).unwrap());
    assert!(boolean_policy::enabled(&app.tree));
    let mut other = empty_app();
    let raw = request(&mut other, "GetState", json!({})).unwrap()["document"].clone();
    apply(&mut app, json!([{"type":"ReplaceScene","scene":raw}])).unwrap();
    assert!(boolean_policy::enabled(&app.tree));
}

#[test]
fn independent_parts_move_together_with_a_shared_anchor_without_booleans() {
    let mut app = empty_app();
    let [a, b] = pair(&mut app);
    let anchor = uuid::Uuid::new_v4();
    apply(&mut app, json!([
        {"type":"CreateVectorVariable","id":anchor,"name":"Assembly origin","value":[0,0,0],"space":"World"},
        {"type":"SetVectorBinding","object":a,"target":"Position","variable":anchor,"offset":[-1,0,0]},
        {"type":"SetVectorBinding","object":b,"target":"Position","variable":anchor,"offset":[1,0,0]},
        {"type":"UpdateVectorVariable","id":anchor,"value":[3,2,1]}
    ])).unwrap();
    let objects = model::objects_ref(&app.tree);
    assert_eq!(objects[0].transform.translation, Vec3::new(2.0, 2.0, 1.0));
    assert_eq!(objects[1].transform.translation, Vec3::new(4.0, 2.0, 1.0));
    assert!(objects.iter().all(|object| object.boolean_parent.is_none()));
    assert!(!boolean_policy::enabled(&app.tree));
}

#[test]
fn mcp_schema_and_tool_help_default_to_independent_parts() {
    let mut app = empty_app();
    let state = request(&mut app, "GetState", json!({})).unwrap();
    assert_eq!(
        state["modeling_policy"]["boolean_operations_enabled"],
        false
    );
    let schema = mcp::schema();
    assert!(schema["operations"]
        .as_array()
        .unwrap()
        .contains(&json!("EnableBooleanOperations")));
    let notes = schema["notes"].as_str().unwrap();
    assert!(notes.contains("Do not create unions just to organize parts"));
    assert!(!notes.contains("Try grouping related objects into named Boolean unions"));
    let tools = mcp::mcp_tools();
    let tool = tools
        .iter()
        .find(|tool| tool["name"] == "enable_boolean_operations")
        .unwrap();
    assert!(tool["description"]
        .as_str()
        .unwrap()
        .contains("only when the user explicitly requests"));
    let apply = tools.iter().find(|tool| tool["name"] == "apply").unwrap();
    assert!(apply["description"]
        .as_str()
        .unwrap()
        .contains("enable_boolean_operations"));
}

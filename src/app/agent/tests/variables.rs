//! Request decoding and vector, rigid-frame and linkage binding tests.
use super::super::mcp::mcp_tools;
use super::super::*;

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

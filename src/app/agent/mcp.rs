use super::*;

const MODEL_ORGANIZATION: &str = concat!(
    "Try grouping related objects into named Boolean unions for better organization and to move, rotate, or scale the whole model at once. ",
    "For example, keep a sword's blade, guard, grip, pommel, and decorations in one union named Sword. ",
    "Use an existing component as the root, rename it with SetObjectName, and attach the other components with SetBoolean (parent: root UUID, operation: Union). ",
    "Set softness to 0 for a hard union that preserves the parts' sharp edges. ",
    "To transform the complete union through MCP, update the root's group_transform with PutObject. ",
    "Keep each component's material and shape editable; nested unions can organize larger assemblies. "
);

pub(super) fn schema() -> Value {
    json!({
        "version": 14,
        "operations": ["GetState", "GetSchema", "ListCommands", "Apply", "ExecuteCommand", "SetView", "BuildPoissonMesh", "GetPoissonMeshStatus", "ExportGlb", "GetGlbExportStatus", "CancelGlbExport", "CaptureViewport", "CaptureOrthographic", "Undo", "Redo", "Save", "Open"],
        "actions": ["CreateObject", "PutObject", "SetObjectName", "SetObjectTransform", "SetObjectParams", "SetRenderRepresentation", "SetBoolean", "DeleteObject", "SetWorld", "CreateVectorVariable", "UpdateVectorVariable", "DeleteVectorVariable", "SetVectorBinding", "RemoveVectorBinding", "SetRigidBinding", "RemoveRigidBinding", "SetVariables", "CreatePostProcessPass", "UpdatePostProcessPass", "MovePostProcessPass", "DeletePostProcessPass", "SetMaterials", "CreateCustomMaterial", "UpdateCustomMaterial", "AssignMaterial", "SetCameras", "SetAnimation", "SetSelection", "SetActiveCamera", "ReplaceScene"],
        "variable_spaces": ["Local", "World"],
        "vector_binding_targets": ["Position", "GroupPosition", {"BezierPoint": 0}],
        "variables_example": {"vectors": [{"id": "00000000-0000-0000-0000-000000000001", "name": "Anchor", "value": [0, 1, 0], "space": "World"}], "bindings": []},
        "primitive_kinds": PrimitiveKind::ALL.iter().map(|kind| json!({"kind": kind, "example": SdfObject::create_kind(*kind)})).collect::<Vec<_>>(),
        "render_representations": GroupRenderRepresentation::ALL.iter().map(|mode| json!({"value": mode, "label": mode.label(), "description": mode.description(), "limitation": mode.limitation()})).collect::<Vec<_>>(),
        "notes": format!("{}{}", MODEL_ORGANIZATION, concat!(
            "GetState returns complete typed objects, variables (vectors and bindings), and the raw .claydash scene document. ",
            "CreateVectorVariable accepts name, value [x,y,z], optional id, and space (World by default). It returns created_variable_ids; supply an id to bind within the same Apply. ",
            "UpdateVectorVariable edits name/value/space. SetVectorBinding takes object, target (Position, GroupPosition, or {BezierPoint:index}), variable, and optional offset [x,y,z] defaulting to zero, replacing any link on that target. ",
            "SetRigidBinding takes binding {object,target:Object|Group,origin,aim,up:{WorldDirection:[x,y,z]}|{Point:uuid},local_origin,local_aim,local_up}; anchors are local points and referenced points require World space. It preserves geometry and scale, solving translation/rotation from a rigid frame. RemoveRigidBinding takes object and target and retains the current pose. Positive uniform parent scale is required. ",
            "SetVariables optionally includes rigid_bindings and four_bar_constraints. Four-bar constraints solve a fixed-length World XY linkage from driver Y, with typed driver/pivot/output point UUIDs, lower_length,upper_length,upright_length,driver_y_offset,travel_min,travel_max,branch:Positive|Negative. Output points are derived and driver Y is clamped to the feasible authored travel range. ",
            "Offsets use the variable space. RemoveVectorBinding and DeleteVectorVariable preserve the last evaluated coordinates. SetVariables replaces the complete typed vectors/bindings state. ",
            "CreateCustomMaterial takes name and wgsl, returning its UUID in created_material_ids. ",
            "The WGSL is a function body returning Surface, with point, normal, view, and base inputs. ",
            "UpdateCustomMaterial edits name and/or wgsl; AssignMaterial links it to object_ids. ",
            "Post-processing WGSL is the body of effect(uv: vec2<f32>, color: vec4<f32>, resolution: vec2<f32>, time: f32) -> vec4<f32>. ",
            "UV and resolution are local to the scene viewport; sample_scene(uv) samples the previous pass and clamps to that viewport. ",
            "Passes run in array order before editor UI and exports, with source validation before atomic Apply. ",
            "CreatePostProcessPass returns its UUID in created_post_process_pass_ids. ",
            "CreateObject accepts an optional position [x,y,z], full transform, shape params, and render_representation. ",
            "SetRenderRepresentation accepts an object or Boolean group root id and optional voxels settings {resolution:8..128}, default 32. Voxels automatically captures six box faces and renders surface samples as cubes; Recompute in Group optimizations refreshes the capture. ",
            "BuildPoissonMesh queues a cache build/recompute for an existing standalone object or Boolean root in PoissonMesh mode. GetPoissonMeshStatus reads queued/not_built/sampling/reconstructing/ready/failed; ready includes vertex and triangle counts, showing, available, and built resolutions. Caches are runtime-only and may be invalidated by source edits. ",
            "ExportGlb starts an asynchronous GLB file export. path is absolute; geometry is current_representation (default), smooth, or voxels. Forced voxels accepts voxel_resolution 8..128, default 32. Current representation retains voxel roots and saved resolutions; other roots become smooth meshes. Optional object_ids selects objects/Boolean subtrees; otherwise current selection or whole scene is exported. The scene is snapshotted at start. Existing files require overwrite:true. Poll GetGlbExportStatus with the returned id for running/completed/failed/cancelled, or CancelGlbExport to cancel. The most recent 32 terminal results are retained until this process exits. Opening or replacing the document cancels pending exports. ",
            "ExactSdf uses the source; BoxDepthAtlas captures from six box faces; SphereDepthAtlas captures with radial rays; GaussianSplats uses layered box-face captures rasterized as hybrid splats. ",
            "Captures retain depth, base color, and the hit material. ",
            "Older documents with removed choices load as ExactSdf. ",
            "BoxParams includes corner_radius; LoftParams contains ordered sections, each with an optional closed profile of 3–32 [Y,Z] points in unit ellipse coordinates. ",
            "Custom profiles in one loft must have matching point counts. ",
            "A PutObject can set surface_inlay to a host object id, offset, and thickness. ",
            "CaptureViewport and CaptureOrthographic accept optional object_ids to render Boolean groups and attached inlays. ",
            "Their mode is simple_shading (default), full_material, or outline. Outline captures show all primitive wires, including hidden Boolean operands, without editor UI. ",
            "Legacy refine:true requests full_material; an explicit mode takes precedence over refine. Capture mode does not change the live editor view. ",
            "Apply actions run as one undoable edit. ",
            "Send expected_revision from GetState to reject stale edits. ",
            "ReplaceScene accepts the raw document value and must be the sole action.",
        ))
    })
}

pub(super) fn run_mcp(mut call: impl FnMut(Value) -> AgentResult) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };
        let Some(id) = request.get("id") else {
            continue;
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "initialize" => Ok(
                json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "claydash", "version": env!("CARGO_PKG_VERSION")}, "instructions": MODEL_ORGANIZATION}),
            ),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": mcp_tools()})),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or(Value::Null);
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                let op = match name {
                    "get_state" => "GetState",
                    "get_schema" => "GetSchema",
                    "list_commands" => "ListCommands",
                    "apply" => "Apply",
                    "execute_command" => "ExecuteCommand",
                    "set_view" => "SetView",
                    "build_poisson_mesh" => "BuildPoissonMesh",
                    "get_poisson_mesh_status" => "GetPoissonMeshStatus",
                    "export_glb" => "ExportGlb",
                    "get_glb_export_status" => "GetGlbExportStatus",
                    "cancel_glb_export" => "CancelGlbExport",
                    "capture_viewport" => "CaptureViewport",
                    "capture_orthographic" => "CaptureOrthographic",
                    "undo" => "Undo",
                    "redo" => "Redo",
                    "save" => "Save",
                    "open" => "Open",
                    _ => "",
                };
                if op.is_empty() {
                    Err(format!("unknown tool: {name}"))
                } else {
                    match call(request_payload(op, arguments)) {
                        Ok(value)
                            if name == "capture_viewport" || name == "capture_orthographic" =>
                        {
                            Ok(
                                json!({"content": [{"type": "image", "data": value["data"], "mimeType": "image/png"}]}),
                            )
                        }
                        Ok(value) => {
                            Ok(json!({"content": [{"type": "text", "text": value.to_string()}]}))
                        }
                        Err(error) => Ok(
                            json!({"isError": true, "content": [{"type": "text", "text": error}]}),
                        ),
                    }
                }
            }
            _ => Err(format!("unsupported MCP method: {method}")),
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": error}})
            }
        };
        if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

pub(super) fn mcp_tools() -> Vec<Value> {
    let object = json!({"type": "object", "additionalProperties": true});
    let uuid = json!({"type": "string", "format": "uuid"});
    let representations = json!({
        "type": "string",
        "enum": GroupRenderRepresentation::ALL
    });
    let vector =
        json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3});
    let space = json!({"enum": ["Local", "World"]});
    let target = json!({"oneOf": [{"enum": ["Position", "GroupPosition"]}, {"type": "object", "properties": {"BezierPoint": {"type": "integer", "minimum": 0}}, "required": ["BezierPoint"], "additionalProperties": false}]});
    let variable = json!({"type": "object", "properties": {"id": uuid, "name": {"type": "string"}, "value": vector, "space": space}, "required": ["id", "name", "value"]});
    let binding = json!({"type": "object", "properties": {"object": uuid, "target": target, "variable": uuid, "offset": vector}, "required": ["object", "target", "variable", "offset"]});
    let rigid_target = json!({"enum":["Object","Group"]});
    let rigid_up = json!({"oneOf":[{"type":"object","properties":{"Point":uuid},"required":["Point"],"additionalProperties":false},{"type":"object","properties":{"WorldDirection":vector},"required":["WorldDirection"],"additionalProperties":false}]});
    let rigid_binding = json!({"type":"object","properties":{"object":uuid,"target":rigid_target,"origin":uuid,"aim":uuid,"up":rigid_up,"local_origin":vector,"local_aim":vector,"local_up":vector},"required":["object","target","origin","aim","up","local_origin","local_aim","local_up"]});
    let four_bar = json!({"type":"object","properties":{"driver":uuid,"lower_pivot":uuid,"upper_pivot":uuid,"lower_joint":uuid,"upper_joint":uuid,"lower_length":{"type":"number","exclusiveMinimum":0},"upper_length":{"type":"number","exclusiveMinimum":0},"upright_length":{"type":"number","exclusiveMinimum":0},"driver_y_offset":{"type":"number"},"travel_min":{"type":"number"},"travel_max":{"type":"number"},"branch":{"enum":["Positive","Negative"]}},"required":["driver","lower_pivot","upper_pivot","lower_joint","upper_joint","lower_length","upper_length","upright_length","driver_y_offset","travel_min","travel_max","branch"]});
    let variables = json!({"type": "object", "properties": {"vectors": {"type": "array", "items": variable}, "bindings": {"type": "array", "items": binding}, "rigid_bindings":{"type":"array","items":rigid_binding}, "four_bar_constraints":{"type":"array","items":four_bar}}, "required": ["vectors", "bindings"]});
    let action = json!({"oneOf": [
        {"type": "object", "properties": {"type": {"const": "CreateObject"}, "kind": {"enum": ["Sphere", "Box", "Cylinder", "Torus", "PolygonPrism", "BezierCurve", "Loft", "Text"]}, "name": {"type": "string"}, "position": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "transform": object, "params": object, "render_representation": representations}, "required": ["type", "kind"]},
        {"type": "object", "properties": {"type": {"const": "PutObject"}, "object": object}, "required": ["type", "object"]},
        {"type": "object", "properties": {"type": {"const": "SetObjectName"}, "id": uuid, "name": {"type": "string"}}, "required": ["type", "id", "name"]},
        {"type": "object", "properties": {"type": {"const": "SetObjectTransform"}, "id": uuid, "transform": object}, "required": ["type", "id", "transform"]},
        {"type": "object", "properties": {"type": {"const": "SetObjectParams"}, "id": uuid, "params": object}, "required": ["type", "id", "params"]},
        {"type": "object", "properties": {"type": {"const": "SetRenderRepresentation"}, "id": uuid, "render_representation": representations, "voxels": {"type": "object", "properties": {"resolution": {"type": "integer", "minimum": 8, "maximum": 128}}, "additionalProperties": false}}, "required": ["type", "id", "render_representation"]},
        {"type": "object", "properties": {"type": {"const": "SetBoolean"}, "id": uuid, "parent": {"type": ["string", "null"]}, "operation": {"enum": ["Union", "Subtract", "Intersect"]}, "softness": {"type": "number"}}, "required": ["type", "id", "parent", "operation"]},
        {"type": "object", "properties": {"type": {"const": "DeleteObject"}, "id": uuid}, "required": ["type", "id"]},
        {"type": "object", "properties": {"type": {"const": "CreateVectorVariable"}, "id": uuid, "name": {"type": "string"}, "value": vector, "space": space}, "required": ["type", "name", "value"]},
        {"type": "object", "properties": {"type": {"const": "UpdateVectorVariable"}, "id": uuid, "name": {"type": "string"}, "value": vector, "space": space}, "required": ["type", "id"]},
        {"type": "object", "properties": {"type": {"const": "DeleteVectorVariable"}, "id": uuid}, "required": ["type", "id"]},
        {"type": "object", "properties": {"type": {"const": "SetVectorBinding"}, "object": uuid, "target": target, "variable": uuid, "offset": vector}, "required": ["type", "object", "target", "variable"]},
        {"type": "object", "properties": {"type": {"const": "RemoveVectorBinding"}, "object": uuid, "target": target}, "required": ["type", "object", "target"]},
        {"type":"object","properties":{"type":{"const":"SetRigidBinding"},"binding":rigid_binding},"required":["type","binding"]},
        {"type":"object","properties":{"type":{"const":"RemoveRigidBinding"},"object":uuid,"target":rigid_target},"required":["type","object","target"]},
        {"type": "object", "properties": {"type": {"const": "SetVariables"}, "variables": variables}, "required": ["type", "variables"]},
        {"type": "object", "properties": {"type": {"const": "SetWorld"}, "world": object}, "required": ["type", "world"]},
        {"type": "object", "properties": {"type": {"const": "CreatePostProcessPass"}, "name": {"type": "string"}, "wgsl": {"type": "string"}, "enabled": {"type": "boolean"}}, "required": ["type", "name", "wgsl"]},
        {"type": "object", "properties": {"type": {"const": "UpdatePostProcessPass"}, "id": uuid, "name": {"type": "string"}, "wgsl": {"type": "string"}, "enabled": {"type": "boolean"}}, "required": ["type", "id"]},
        {"type": "object", "properties": {"type": {"const": "MovePostProcessPass"}, "id": uuid, "index": {"type": "integer", "minimum": 0}}, "required": ["type", "id", "index"]},
        {"type": "object", "properties": {"type": {"const": "DeletePostProcessPass"}, "id": uuid}, "required": ["type", "id"]},
        {"type": "object", "properties": {"type": {"const": "SetMaterials"}, "materials": {"type": "array", "items": object}}, "required": ["type", "materials"]},
        {"type": "object", "properties": {"type": {"const": "CreateCustomMaterial"}, "name": {"type": "string"}, "wgsl": {"type": "string"}}, "required": ["type", "name", "wgsl"]},
        {"type": "object", "properties": {"type": {"const": "UpdateCustomMaterial"}, "id": uuid, "name": {"type": "string"}, "wgsl": {"type": "string"}}, "required": ["type", "id"]},
        {"type": "object", "properties": {"type": {"const": "AssignMaterial"}, "id": uuid, "object_ids": {"type": "array", "items": uuid, "minItems": 1}}, "required": ["type", "id", "object_ids"]},
        {"type": "object", "properties": {"type": {"const": "SetCameras"}, "cameras": {"type": "array", "items": object}}, "required": ["type", "cameras"]},
        {"type": "object", "properties": {"type": {"const": "SetAnimation"}, "animation": object}, "required": ["type", "animation"]},
        {"type": "object", "properties": {"type": {"const": "SetSelection"}, "ids": {"type": "array", "items": uuid}}, "required": ["type", "ids"]},
        {"type": "object", "properties": {"type": {"const": "SetActiveCamera"}, "id": {"type": ["string", "null"]}}, "required": ["type", "id"]},
        {"type": "object", "properties": {"type": {"const": "ReplaceScene"}, "scene": object}, "required": ["type", "scene"]}
    ]});
    let empty = json!({"type": "object", "properties": {}, "additionalProperties": false});
    [
        ("get_state", "Read the full live Claydash scene, selection, materials, shared vector variables and bindings, post-processing passes, animation, camera, and revision.", empty.clone()),
        ("get_schema", "Get action names and example serialized objects for each primitive type.", empty.clone()),
        ("list_commands", "List Claydash's existing command palette commands.", empty.clone()),
        ("apply", "Apply typed scene actions as one undoable transaction. Read get_schema first; include expected_revision from get_state.", json!({"type": "object", "properties": {"expected_revision": {"type": "integer"}, "actions": {"type": "array", "items": action, "minItems": 1}}, "required": ["actions"]})),
        ("execute_command", "Run an existing Claydash command by name. Some commands start an interactive gesture.", json!({"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]})),
        ("set_view", "Set the live viewport camera. position and target are [x,y,z]; projection_mode is Perspective or Orthographic.", json!({"type": "object", "properties": {"position": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "target": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "up": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "projection_mode": {"enum": ["Perspective", "Orthographic"]}}, "required": ["position", "target"]})),
        ("build_poisson_mesh", "Queue a Poisson mesh cache build or recompute for a standalone object or Boolean root already set to PoissonMesh. Poll get_poisson_mesh_status for completion. Runtime caches are invalidated by source edits.", json!({"type": "object", "properties": {"id": uuid}, "required": ["id"], "additionalProperties": false})),
        ("get_poisson_mesh_status", "Read Poisson cache progress and completion for a PoissonMesh root. Ready includes vertices, triangles, showing, available, and built sample/mesh resolutions.", json!({"type": "object", "properties": {"id": uuid}, "required": ["id"], "additionalProperties": false})),
        ("export_glb", "Start an asynchronous GLB export to an absolute path on the Claydash host. Returns a job id; poll get_glb_export_status. Geometry: current_representation preserves voxel roots/resolution and uses smooth meshes for other roots; smooth uses Poisson meshes; voxels uses surface cubes with sampled colors. Omit object_ids to use the current selection or whole scene. Captures a scene snapshot; existing files require overwrite:true. Works in desktop and headless sessions.", json!({"type":"object","properties":{
            "path":{"type":"string","minLength":1,"description":"Absolute output file path; parent directory must exist."},
            "geometry":{"enum":["current_representation","smooth","voxels"],"default":"current_representation"},
            "voxel_resolution":{"type":"integer","minimum":8,"maximum":128,"description":"Only for geometry:voxels. Defaults to 32."},
            "object_ids":{"type":"array","items":uuid,"minItems":1},
            "overwrite":{"type":"boolean","default":false}},"required":["path"],"additionalProperties":false})),
        ("get_glb_export_status", "Read GLB export status by job id: running (stage and per-stage progress), completed (file published), failed (error), or cancelled. Retains the most recent 32 terminal jobs for this process.", json!({"type":"object","properties":{"id":uuid},"required":["id"],"additionalProperties":false})),
        ("cancel_glb_export", "Cancel a GLB export by job id. Returns cancelled, or the terminal result if already finished. A cancelled job cannot subsequently publish its file.", json!({"type":"object","properties":{"id":uuid},"required":["id"],"additionalProperties":false})),
        ("capture_viewport", "Render and return a PNG of the viewport. Pass object_ids to isolate groups. mode: simple_shading (default), full_material for material inspection, or outline for all primitive wires including hidden Boolean operands. Explicit mode overrides legacy refine. The live editor view is unchanged.", json!({"type": "object", "properties": {"object_ids": {"type": "array", "items": uuid, "minItems": 1}, "mode": {"enum": ["simple_shading", "full_material", "outline"]}, "refine": {"type": "boolean", "description": "Legacy option: true selects full_material, false selects simple_shading. Ignored when mode is provided."}}, "additionalProperties": false})),
        ("capture_orthographic", "Return one compact PNG with X, Y, Z orthographic views toward the origin. Optional object_ids isolate groups. mode: simple_shading (default), full_material, or outline for all primitive wires including hidden Boolean operands. Explicit mode overrides legacy refine. The live editor view is unchanged.", json!({"type": "object", "properties": {"object_ids": {"type": "array", "items": uuid, "minItems": 1}, "panel_size": {"type": "integer", "minimum": 96, "maximum": 512}, "distance": {"type": "number", "minimum": 0.1, "maximum": 1000}, "mode": {"enum": ["simple_shading", "full_material", "outline"]}, "refine": {"type": "boolean", "description": "Legacy option: true selects full_material, false selects simple_shading. Ignored when mode is provided."}}, "additionalProperties": false})),
        ("undo", "Undo the last scene edit.", empty.clone()),
        ("redo", "Redo the last undone scene edit.", empty.clone()),
        ("save", "Save the live scene to the current path or a specified .claydash path.", json!({"type": "object", "properties": {"path": {"type": "string"}}})),
        ("open", "Open a .claydash project in the live window.", json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})),
    ].into_iter().map(|(name, description, input_schema)| json!({"name": name, "description": description, "inputSchema": input_schema})).collect()
}

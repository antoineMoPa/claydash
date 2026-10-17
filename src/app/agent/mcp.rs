use super::*;

const MODEL_ORGANIZATION: &str = concat!(
    "For efficient modeling, prefer arrangements of sheet bends using native sheet cuts and bends, independent parts, and primitive corner radii. ",
    "Boolean operations are disabled by default, including hard unions, subtraction and intersection. Do not create unions just to organize parts. ",
    "Use shared vector variables with Position bindings and per-part offsets to translate an assembly, or shared rigid frame bindings to translate and rotate it, without Boolean geometry. ",
    "Only call enable_boolean_operations when the user explicitly requests Boolean geometry: nested operations can slow rendering and mesh export. ",
    "Smooth Boolean blends are separately disabled by default for new or enlarged blends; only call enable_slow_smooth_blends when explicitly requested. ",
    "Smooth blends require dense field sampling and can exceed export resource budgets. Primitive corner radii and native sheet cuts/bends/crowns need neither opt-in. ",
    "Existing imported operations remain editable and can be detached or removed without opt-in. ",
    "Both opt-ins persist in saved scenes; New resets them, and ReplaceScene cannot grant them. Enabling does not change geometry. ",
    "Export geometry:'smooth' selects Poisson mesh export and enables neither modeling opt-in. "
);

pub(super) fn schema() -> Value {
    json!({
        "version": 20,
        "operations": ["GetState", "GetSceneSummary", "GetSchema", "GetActionSchema", "GetGaussianCloud", "EnableBooleanOperations", "EnableSlowSmoothBlends", "ListCommands", "Apply", "ExecuteCommand", "SetView", "BuildPoissonMesh", "GetPoissonMeshStatus", "ExportGlb", "GetGlbExportStatus", "CancelGlbExport", "CaptureViewport", "CaptureOrthographic", "Undo", "Redo", "Save", "Open"],
        "actions": ["CreateObject", "PutObject", "SetObjectName", "SetObjectTransform", "SetObjectParams", "SetRenderRepresentation", "SetBoolean", "DeleteObject", "SetWorld", "CreateVectorVariable", "UpdateVectorVariable", "DeleteVectorVariable", "SetVectorBinding", "RemoveVectorBinding", "SetRigidBinding", "RemoveRigidBinding", "SetVariables", "CreatePostProcessPass", "UpdatePostProcessPass", "MovePostProcessPass", "DeletePostProcessPass", "SetMaterials", "CreateCustomMaterial", "UpdateCustomMaterial", "AssignMaterial", "SetCameras", "SetAnimation", "SetSelection", "SetActiveCamera", "ReplaceScene"],
        "variable_spaces": ["Local", "World"],
        "vector_binding_targets": ["Position", "GroupPosition", {"BezierPoint": 0}],
        "variables_example": {"vectors": [{"id": "00000000-0000-0000-0000-000000000001", "name": "Anchor", "value": [0, 1, 0], "space": "World"}], "bindings": []},
        "primitive_kinds": PrimitiveKind::ALL.iter().map(|kind| {
            let mut example = SdfObject::create_kind(*kind);
            example.softness = 0.0;
            json!({"kind": kind, "example": example})
        }).collect::<Vec<_>>(),
        "sheet_emboss_example": {"SheetPanelParams": {
            "size": [0.9, 0.7], "thickness": 0.008,
            "cuts": [{"Circle": {"center": [0.0, -0.32], "radius": 0.28}}],
            "embosses": [{"Circle": {"center": [0.0, -0.32], "radius": 0.28, "depth": 0.035, "transition": 0.075}}],
            "bends": [], "stage": "Formed"
        }},
        "sheet_surface_example": {"SheetPanelParams": {
            "size": [0.9,0.7], "thickness":0.008,
            "surface":{"heights":[0,0.04,0.04,0,0.04,0.12,0.12,0.04,0.04,0.12,0.12,0.04,0,0.04,0.04,0]},
            "path_embosses":[{"stations":[
                {"point":[-0.35,0],"width":0.04,"depth":0.01,"profile":"Rounded"},
                {"point":[0,0.08],"width":0.075,"depth":0.025,"profile":"Crease"},
                {"point":[0.35,0],"width":0.04,"depth":0.01,"profile":"Rounded"}],
                "taper_start":0.12,"taper_end":0.12}], "stage":"Formed"
        }},
        "sheet_seam_example": {"source":"SOURCE_PANEL_UUID","source_edge":"MaxX","edge":"MinX","gap":0.005},
        "render_representations": GroupRenderRepresentation::ALL.iter().map(|mode| json!({"value": mode, "label": mode.label(), "description": mode.description(), "limitation": mode.limitation()})).collect::<Vec<_>>(),
        "notes": format!("{}{}", MODEL_ORGANIZATION, concat!(
            "GetState returns complete typed objects, variables (vectors and bindings), the scene Boolean and smooth-blend opt-in flags, and the raw .claydash scene document. EnableBooleanOperations explicitly permits new Boolean geometry only when requested by the user. EnableSlowSmoothBlends separately opts the current scene in and returns a warning about dense field cost and export resource budgets. ",
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
            "BuildPoissonMesh queues a cache build/recompute for an existing standalone object or Boolean root in PoissonMesh or DirectMesh mode. GetPoissonMeshStatus reads queued/not_built/sampling/reconstructing/ready/failed (direct builds use tessellating/mesh_booleans); ready includes vertex and triangle counts, showing, available, and built resolutions. Caches are runtime-only and may be invalidated by source edits. ",
            "ExportGlb starts an asynchronous GLB file export. path is absolute; geometry is current_representation (default), smooth, direct, or voxels. Forced direct accepts direct_resolution 8..256, default 32, and optional direct_error (positive world-space surface deviation); default error is 0.032/direct_resolution. Analytic surfaces refine by curvature and world scale, not uniform density; solid materials export indexed PBR without baking. Forced voxels accepts voxel_resolution 8..128, default 32. Current representation retains voxel roots and saved resolutions; direct roots retain direct meshes and their saved resolution; other roots become smooth meshes. Optional object_ids selects objects/Boolean subtrees; otherwise current selection or whole scene is exported. The scene is snapshotted at start. Existing files require overwrite:true. Poll GetGlbExportStatus with the returned id for running/completed/failed/cancelled, or CancelGlbExport to cancel. The most recent 32 terminal results are retained until this process exits. Opening or replacing the document cancels pending exports. ",
            "ExactSdf uses the source; BoxDepthAtlas captures from six box faces; SphereDepthAtlas captures with radial rays; GaussianSplats uses layered box-face captures rasterized as hybrid splats. ",
            "Captures retain depth, base color, and the hit material. ",
            "Older documents with removed choices load as ExactSdf. ",
            "BoxParams includes corner_radius; LoftParams contains ordered sections, each with an optional closed profile of 3–32 [Y,Z] points in unit ellipse coordinates. ",
            "Custom profiles in one loft must have matching point counts. ",
            "SheetPanel uses SheetPanelParams {size:[width,height],thickness,cuts,bends,stage:Blank|Cut|Formed}. Material coordinates are centered XY, thickness is along Z. Up to 32 cuts: {Straight:{from:[x,y],to:[x,y]}} retains the left half-plane, {Circle:{center:[x,y],radius}} removes a disk. Bends are {start,radius,angle,direction?}; direction defaults to 0 and is radians from +X towards +Y, perpendicular to the fold line. All-zero directions retain up to 8 ordered nonoverlapping material-X zones inside the blank. Any nonzero direction enables up to 4 sequential spatial bends, applied in array order to already formed coordinates, with line cos(direction)*x+sin(direction)*y=start in sheet-local XY. The stationary side has projected coordinate <= start; the arc zone ends at start+radius*abs(angle); the far side rotates by angle. Nonparallel/crossing regions are supported as spatial deformation, not physically accurate compound stamping; order matters and surfaces can overlap. Cuts stay in original material coordinates. Every bend requires finite start/direction, radius > thickness/2 and 0 < abs(angle) < pi. Directional cylinder centres must stay outside preceding formed slab Z bounds with >=5% radial clearance (conservative bounds ignore cuts); otherwise validation rejects the recipe. Increase radius, reverse sign or reorder operations to avoid singular forming. Stage Blank shows the uncut blank, Cut shows cuts before bending, Formed shows cuts and bends. SetObjectParams changes stage without losing the recipe; then CaptureViewport or CaptureOrthographic supplies a snapshot. Use direction to orient each fold line independently. ",
            "A PutObject can set surface_inlay to a host object id, offset, and thickness. ",
            "SheetPanel also accepts optional embosses (default []), up to 8 {Circle:{center:[x,y],radius,depth,transition}} operations. The inner disk through radius has signed material-Z depth; the next positive transition width uses a cosine-rounded profile back to zero. Positive depth raises, negative depth recesses. Overlapping reliefs add. Embosses act in original material XY before bends and only in Formed stage; cuts remain in original XY and clip relief naturally at blank edges. A matching circular cut creates a formed arch lip. Radius/transition must be positive finite, depth finite nonzero, and the combined inverse metric scale must be >=0.05; very steep transitions are rejected. Bend radial-clearance validation includes embossed thickness bounds. Thickness is retained along material Z, not a physical stamping/thinning simulation. Creation and recipe editing are MCP-only; no sheet UI controls. ",
            "SheetPanel surface (default null) is {heights:[16 finite control heights]}: one tensor-product cubic Bezier height graph over the material rectangle, row-major with X varying fastest. It supports compound crowns/domes with independent X/Y curvature; XY stays unchanged. Control heights are Bezier controls, not interpolation samples. It acts before bends, adds to embosses and is absent in Blank/Cut stages. ",
            "path_embosses (default []) supports up to 4 paths, each {stations:[2..8 {point:[x,y],width,depth,profile:Rounded|Crease}],taper_start,taper_end}. Width is positive half-width; depth is nonzero and same-sign along each path. Width/depth and profile blend interpolate per segment. A continuous maximum-magnitude envelope combines swept segments; overlapping different paths add. Rounded uses cosine, Crease uses squared linear shoulder with a sharp centre. Positive finite taper lengths use smooth arclength end fades and must sum to <= path length. Internal caps/turns may have creases; this is a polyline sweep rather than a smooth spline. Extremely steep or narrow features are rejected by the combined >=5% metric limit. ",
            "seam (default null) is {source:UUID,source_edge:MinX|MaxX|MinY|MaxY,edge:MinX|MaxX|MinY|MaxY,gap:nonnegative}. A persistent directed dependency constrains this patch boundary and adjacent control row to match source heights and surface tangents. The opposite cardinal edges must have equal tangential spans, matching world rotation/scale, aligned tangent coordinates and translations matching gap along the source outward material axis. Gap is in source-local XY units, with equal local heights across the gap; transforms are validated, not automatically repositioned. Both full rectangular edges must survive cuts and stay outside circular/path relief support; both panels must have no bends, and panels plus all Boolean ancestors must use hard unions without lattice, mirror, repetition or surface inlay. Leaf softness without children is inert and allowed. Different normal panel sizes and Z offsets are supported. Cycles, missing sources, chains over 64 links or incompatible edits fail atomically. Source edits re-solve dependants; constrained target control rows are overwritten, other controls retained. Save/Open retains links and re-solves them. Non-MCP incompatible transform edits retain the prior object state. Features remain MCP-only. ",
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
                    "get_scene_summary" => "GetSceneSummary",
                    "get_schema" => "GetSchema",
                    "get_action_schema" => "GetActionSchema",
                    "get_gaussian_cloud" => "GetGaussianCloud",
                    "enable_slow_smooth_blends" => "EnableSlowSmoothBlends",
                    "enable_boolean_operations" => "EnableBooleanOperations",
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
                    "import_gaussian_ply" => "ImportGaussianPly",
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
    let softness = json!({"type": "number", "default": 0, "description": "Boolean softness defaults to 0 for hard operations. New or enlarged positive smooth blends require enable_slow_smooth_blends for the current scene; existing imported blends are retained. Local built-in primitive rounding is independent of Boolean softness."});
    let sdf_object = json!({"type": "object", "properties": {"softness": softness}, "additionalProperties": true});
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
        {"type": "object", "properties": {"type": {"const": "CreateObject"}, "kind": {"enum": PrimitiveKind::ALL}, "name": {"type": "string"}, "position": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "transform": object, "params": object, "render_representation": representations}, "required": ["type", "kind"]},
        {"type": "object", "properties": {"type": {"const": "PutObject"}, "object": sdf_object}, "required": ["type", "object"]},
        {"type": "object", "properties": {"type": {"const": "SetObjectName"}, "id": uuid, "name": {"type": "string"}}, "required": ["type", "id", "name"]},
        {"type": "object", "properties": {"type": {"const": "SetObjectTransform"}, "id": uuid, "transform": object}, "required": ["type", "id", "transform"]},
        {"type": "object", "properties": {"type": {"const": "SetObjectParams"}, "id": uuid, "params": object}, "required": ["type", "id", "params"]},
        {"type": "object", "properties": {"type": {"const": "SetRenderRepresentation"}, "id": uuid, "render_representation": representations, "voxels": {"type": "object", "properties": {"resolution": {"type": "integer", "minimum": 8, "maximum": 128}}, "additionalProperties": false}}, "required": ["type", "id", "render_representation"]},
        {"type": "object", "properties": {"type": {"const": "SetBoolean"}, "id": uuid, "parent": {"type": ["string", "null"]}, "operation": {"enum": ["Union", "Subtract", "Intersect"], "description": "Adding or changing Boolean geometry requires enable_boolean_operations after an explicit user request. Existing operands can be detached with parent:null without opt-in. Do not use unions to organize parts."}, "softness": softness}, "required": ["type", "id", "parent", "operation"]},
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
        ("get_state", "Read the full live Claydash scene, selection, materials, shared vector variables and bindings, post-processing passes, animation, camera, scene Boolean and smooth-blend opt-in flags, and revision.", empty.clone()),
        ("get_scene_summary", "Read scene revision, object IDs/names/transforms, selection, camera and Gaussian counts and bounds without embedded splat bytes.", empty.clone()),
        ("get_schema", "Get action names and example serialized objects for each primitive type.", empty.clone()),
        ("get_action_schema", "Get one tagged Apply action schema. Pass primitive for its serialized params example; SheetPanel includes bend fields and a compact example.", json!({"type":"object","properties":{"action":{"type":"string","description":"Action name such as CreateObject or SetObjectParams."},"primitive":{"enum":PrimitiveKind::ALL}},"required":["action"],"additionalProperties":false})),
        ("get_gaussian_cloud", "Get exact bounds, per-axis 2nd/50th/98th position percentiles and deterministic spatial/color samples for one imported Gaussian object. Coordinates are object-local by default or transformed to world. Does not return embedded bytes.", json!({"type":"object","properties":{"id":uuid,"space":{"enum":["object","world"],"default":"object"},"sample_count":{"type":"integer","minimum":0,"maximum":256,"default":32}},"required":["id"],"additionalProperties":false})),
        ("enable_slow_smooth_blends", "Explicitly enable new or enlarged smooth Boolean blends for this scene. Warning: dense field sampling can be slow and exceed export resource budgets; prefer arrangements of sheet bends, native sheet cuts/bends, independent components, and local built-in rounding. New Boolean geometry, including hard unions, requires enable_boolean_operations and an explicit user request; do not use unions for organization. Enabling does not change existing geometry or automatically apply positive blends. Saved scenes retain opt-in; New resets it; ReplaceScene does not grant opt-in. Existing imported blends are retained. Export geometry:'smooth' does not enable Boolean blends.", empty.clone()),
        ("enable_boolean_operations", "Allow new Boolean unions, subtraction and intersection for this scene only when the user explicitly requests Boolean geometry. Disabled by default because nested operations add render and mesh-export work. Prefer independent parts, native sheet cuts/bends/crowns and primitive corner radii; use shared transform bindings for assemblies. This does not enable smooth blends, change geometry, or replace existing operations. Saved scenes retain opt-in; New resets it; ReplaceScene cannot grant it. Existing imported Boolean geometry can be edited or detached while disabled.", empty.clone()),
        ("list_commands", "List Claydash's existing command palette commands.", empty.clone()),
        ("import_gaussian_ply", "Import a local Graphdeco Gaussian PLY as one selected, undoable asset. Supports ASCII and binary PLY, up to 2,000,000 Gaussians per asset. The project automatically embeds all Gaussian data on Save; no source file is needed afterward. Viewport displays DC color; higher spherical harmonics are preserved. Returns id and revision.", json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})),
        ("apply", "Apply typed scene actions as one undoable transaction. Read get_schema first; include expected_revision from get_state. For efficient modeling, prefer arrangements of sheet bends using native sheet cuts/bends, independent components, and local built-in rounding. New Boolean geometry, including hard unions, requires enable_boolean_operations and an explicit user request; do not use unions for organization. Smooth Boolean blends are unavailable by default for new or enlarged blends; enable_slow_smooth_blends explicitly opts the scene in with a dense field cost/export resource budget warning. Existing imported blends are retained; ReplaceScene does not grant opt-in.", json!({"type": "object", "properties": {"expected_revision": {"type": "integer"}, "actions": {"type": "array", "items": action, "minItems": 1}}, "required": ["actions"]})),
        ("execute_command", "Run an existing Claydash command by name. Some commands start an interactive gesture. The union command requires enable_boolean_operations; completed commands cannot introduce Boolean geometry while disabled.", json!({"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]})),
        ("set_view", "Set the live viewport camera. position and target are [x,y,z]; projection_mode is Perspective or Orthographic.", json!({"type": "object", "properties": {"position": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "target": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "up": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "projection_mode": {"enum": ["Perspective", "Orthographic"]}}, "required": ["position", "target"]})),
        ("build_poisson_mesh", "Queue a Poisson mesh cache build or recompute for a standalone object or Boolean root already set to PoissonMesh or DirectMesh. Poll get_poisson_mesh_status for completion. Runtime caches are invalidated by source edits.", json!({"type": "object", "properties": {"id": uuid}, "required": ["id"], "additionalProperties": false})),
        ("get_poisson_mesh_status", "Read mesh cache progress and completion (direct stages: tessellating/mesh_booleans) for a PoissonMesh or DirectMesh root. Ready includes vertices, triangles, showing, available, and built sample/mesh resolutions.", json!({"type": "object", "properties": {"id": uuid}, "required": ["id"], "additionalProperties": false})),
        ("export_glb", "Start an asynchronous GLB export to an absolute path on the Claydash host. Returns a job id; poll get_glb_export_status. Geometry: current_representation preserves voxel and direct roots/resolution and uses smooth meshes for other roots; smooth uses Poisson meshes and does not enable smooth Boolean blends; direct tessellates source geometry and meshes smooth blends, repetition, mirror, lattice and inlay with baked materials; voxels uses surface cubes with sampled colors. Omit object_ids to use the current selection or whole scene. Captures a scene snapshot; existing files require overwrite:true. Works in desktop and headless sessions.", json!({"type":"object","properties":{
            "path":{"type":"string","minLength":1,"description":"Absolute output file path; parent directory must exist."},
            "geometry":{"enum":["current_representation","smooth","direct","voxels"],"default":"current_representation","description":"Mesh export representation. smooth selects Poisson meshing and does not enable smooth Boolean blends."},
            "direct_resolution":{"type":"integer","minimum":8,"maximum":256,"description":"Only for geometry:direct. Defaults to 32."},
            "direct_error":{"type":"number","exclusiveMinimum":0,"description":"Maximum world-space surface deviation for geometry:direct; default 0.032/direct_resolution."},
            "voxel_resolution":{"type":"integer","minimum":8,"maximum":128,"description":"Only for geometry:voxels. Defaults to 32."},
            "object_ids":{"type":"array","items":uuid,"minItems":1},
            "overwrite":{"type":"boolean","default":false}},"required":["path"],"additionalProperties":false})),
        ("get_glb_export_status", "Read GLB export status by job id: running (stage and per-stage progress), completed (file published), failed (error), or cancelled. Retains the most recent 32 terminal jobs for this process.", json!({"type":"object","properties":{"id":uuid},"required":["id"],"additionalProperties":false})),
        ("cancel_glb_export", "Cancel a GLB export by job id. Returns cancelled, or the terminal result if already finished. A cancelled job cannot subsequently publish its file.", json!({"type":"object","properties":{"id":uuid},"required":["id"],"additionalProperties":false})),
        ("capture_viewport", "Render a viewport PNG. object_ids isolates groups; sheet_stage previews Blank, Cut, or Formed without editing the recipe. mode selects simple_shading (default), full_material, or outline. Explicit mode overrides legacy refine. The live editor view is unchanged.", json!({"type": "object", "properties": {"sheet_stage":{"enum":["Blank","Cut","Formed"]}, "object_ids": {"type": "array", "items": uuid, "minItems": 1}, "mode": {"enum": ["simple_shading", "full_material", "outline"]}, "refine": {"type": "boolean", "description": "Legacy option: true selects full_material, false selects simple_shading. Ignored when mode is provided."}}, "additionalProperties": false})),
        ("capture_orthographic", "Return a PNG with X, Y, Z orthographic views. object_ids isolates groups; sheet_stage previews Blank, Cut, or Formed without editing the recipe. mode selects simple_shading (default), full_material, or outline. Explicit mode overrides legacy refine. The live editor view is unchanged.", json!({"type": "object", "properties": {"sheet_stage":{"enum":["Blank","Cut","Formed"]}, "object_ids": {"type": "array", "items": uuid, "minItems": 1}, "panel_size": {"type": "integer", "minimum": 96, "maximum": 512, "default": 256, "description": "Pixels per orthographic panel; must also fit inside the current viewport."}, "distance": {"type": "number", "minimum": 0.1, "maximum": 1000, "default": 8.0}, "mode": {"enum": ["simple_shading", "full_material", "outline"]}, "refine": {"type": "boolean", "description": "Legacy option: true selects full_material, false selects simple_shading. Ignored when mode is provided."}}, "additionalProperties": false})),
        ("undo", "Undo the last scene edit.", empty.clone()),
        ("redo", "Redo the last undone scene edit.", empty.clone()),
        ("save", "Save the live scene to the current path or a specified .claydash path.", json!({"type": "object", "properties": {"path": {"type": "string"}}})),
        ("open", "Open a .claydash project in the live window.", json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})),
    ].into_iter().map(|(name, description, input_schema)| json!({"name": name, "description": description, "inputSchema": input_schema})).collect()
}

pub(super) fn action_schema(action: &str, primitive: Option<PrimitiveKind>) -> AgentResult {
    let tools = mcp_tools();
    let apply = tools.iter().find(|tool| tool["name"] == "apply").expect("apply tool exists");
    let variants = apply["inputSchema"]["properties"]["actions"]["items"]["oneOf"]
        .as_array().expect("action variants exist");
    let variant = variants.iter().find(|variant| variant["properties"]["type"]["const"] == action)
        .ok_or_else(|| format!("unknown action: {action}"))?;
    let mut result = json!({
        "apply_shape": {"expected_revision": "integer from get_scene_summary (optional)", "actions": "nonempty array of tagged actions"},
        "action": variant,
        "tagging": "Put the action name in the type field, for example {\"type\":\"CreateObject\",\"kind\":\"SheetPanel\"}."
    });
    if let Some(kind) = primitive {
        let mut example = SdfObject::create_kind(kind);
        example.softness = 0.0;
        result["primitive"] = json!(kind);
        result["params_example"] = json!(example.params);
        if action == "CreateObject" {
            result["apply_example"] = json!({"actions":[{"type":"CreateObject","kind":kind,"params":example.params}]});
        }
        if kind == PrimitiveKind::SheetPanel {
            result["sheet_bend"] = json!({
                "shape": {"start":"finite number in sheet-local XY", "radius":"number greater than thickness/2", "angle":"radians, nonzero with magnitude below pi", "direction":"optional radians from +X toward +Y; default 0"},
                "example": {"start":0.15,"radius":0.08,"angle":1.2,"direction":0.0},
                "params_shape": {"SheetPanelParams":{"size":["width","height"],"thickness":"positive number","cuts":[],"bends":["<bend object>"],"stage":"Blank | Cut | Formed"}}
            });
        }
    }
    Ok(result)
}

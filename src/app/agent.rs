//! Local automation surface shared by the CLI and MCP adapter.
//! The socket thread only transports requests. All scene edits run on the app thread.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    time::Duration,
};

use glam::Vec3;
use serde::Deserialize;
use serde_json::{json, Value};
use winit::window::Window;

use super::App;
use crate::{
    camera::ProjectionMode,
    commands, document,
    model::{
        self, AnimationData, BooleanOperation, MaterialAsset, PrimitiveKind, SdfObject, SdfParams,
        Transform, World,
    },
};

pub type AgentResult = Result<Value, String>;

pub struct Inbound {
    pub request: Request,
    pub reply: Sender<AgentResult>,
}

#[derive(Deserialize)]
#[serde(tag = "op", content = "args")]
pub enum Request {
    GetState,
    GetSchema,
    ListCommands,
    Apply(ApplyArgs),
    ExecuteCommand { name: String },
    SetView(ViewArgs),
    CaptureViewport,
    Undo,
    Redo,
    Save { path: Option<PathBuf> },
    Open { path: PathBuf },
}

#[derive(Deserialize)]
pub struct ApplyArgs {
    pub expected_revision: Option<u64>,
    pub actions: Vec<Action>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum Action {
    CreateObject {
        kind: PrimitiveKind,
        name: Option<String>,
        transform: Option<Transform>,
        position: Option<[f32; 3]>,
        params: Option<SdfParams>,
    },
    PutObject {
        object: SdfObject,
    },
    SetObjectName {
        id: uuid::Uuid,
        name: String,
    },
    SetObjectTransform {
        id: uuid::Uuid,
        transform: Transform,
    },
    SetObjectParams {
        id: uuid::Uuid,
        params: SdfParams,
    },
    SetBoolean {
        id: uuid::Uuid,
        parent: Option<uuid::Uuid>,
        operation: BooleanOperation,
        softness: Option<f32>,
    },
    DeleteObject {
        id: uuid::Uuid,
    },
    SetWorld {
        world: World,
    },
    SetMaterials {
        materials: Vec<MaterialAsset>,
    },
    SetCameras {
        cameras: Vec<crate::camera::SceneCamera>,
    },
    SetAnimation {
        animation: AnimationData,
    },
    SetSelection {
        ids: Vec<uuid::Uuid>,
    },
    SetActiveCamera {
        id: Option<uuid::Uuid>,
    },
    ReplaceScene {
        scene: Value,
    },
}

#[derive(Deserialize)]
pub struct ViewArgs {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: Option<[f32; 3]>,
    pub projection_mode: Option<ProjectionMode>,
}

fn socket_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .ok_or("HOME must be set to use the local Claydash agent bridge")?;
    Ok(PathBuf::from(home)
        .join(".claydash-agent")
        .join("agent.sock"))
}

pub(super) fn cleanup_socket() {
    if let Ok(path) = socket_path() {
        let _ = std::fs::remove_file(path);
    }
}

pub fn listen(window: Arc<Window>) -> Result<Receiver<Inbound>, String> {
    let path = socket_path()?;
    let directory = path
        .parent()
        .ok_or("agent socket has no parent directory")?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            return Err(format!("another Claydash instance owns {}", path.display()));
        }
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            let Ok(mut stream) = connection else { continue };
            let tx = tx.clone();
            let window = window.clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<Value, String> {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(120)))
                        .map_err(|error| error.to_string())?;
                    stream
                        .set_write_timeout(Some(Duration::from_secs(120)))
                        .map_err(|error| error.to_string())?;
                    let mut line = String::new();
                    BufReader::new(&stream)
                        .read_line(&mut line)
                        .map_err(|error| error.to_string())?;
                    let request: Request = serde_json::from_str(&line)
                        .map_err(|error| format!("invalid request: {error}"))?;
                    let (reply, response) = mpsc::channel();
                    tx.send(Inbound { request, reply })
                        .map_err(|_| "Claydash is closing".to_string())?;
                    window.request_redraw();
                    response
                        .recv_timeout(Duration::from_secs(115))
                        .map_err(|error| error.to_string())?
                })();
                let output = match result {
                    Ok(value) => json!({"ok": true, "result": value}),
                    Err(error) => json!({"ok": false, "error": error}),
                };
                let _ = writeln!(stream, "{output}");
            });
        }
    });
    Ok(rx)
}

fn call_socket(request: Value) -> AgentResult {
    let mut stream = UnixStream::connect(socket_path()?)
        .map_err(|_| "No native Claydash window is connected. Start Claydash first.".to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(120)))
        .map_err(|error| error.to_string())?;
    writeln!(stream, "{request}").map_err(|error| error.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    let response: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err(response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Claydash request failed")
            .to_string())
    }
}

fn request_payload(op: &str, args: Value) -> Value {
    let args = match op {
        "GetState" | "GetSchema" | "ListCommands" | "CaptureViewport" | "Undo" | "Redo" => {
            Value::Null
        }
        "Save" if args.is_null() => json!({}),
        _ => args,
    };
    json!({"op": op, "args": args})
}

pub fn run_from_args() -> bool {
    let mut args = std::env::args().skip(1);
    let Some(mode) = args.next() else {
        return false;
    };
    match mode.as_str() {
        "mcp" => {
            run_mcp();
            true
        }
        "agent" => {
            let Some(op) = args.next() else {
                eprintln!("usage: claydash agent <operation> [JSON arguments]");
                std::process::exit(2);
            };
            let remaining: Vec<_> = args.collect();
            if op == "CaptureViewport" && remaining.len() == 2 && remaining[0] == "--output" {
                let path = &remaining[1];
                let result = call_socket(request_payload("CaptureViewport", Value::Null))
                    .and_then(|value| {
                        use base64::Engine;
                        let data = value
                            .get("data")
                            .and_then(Value::as_str)
                            .ok_or("capture returned no PNG data")?;
                        base64::engine::general_purpose::STANDARD
                            .decode(data)
                            .map_err(|error| error.to_string())
                    })
                    .and_then(|bytes| {
                        std::fs::write(&path, bytes).map_err(|error| error.to_string())
                    });
                match result {
                    Ok(()) => println!("{path}"),
                    Err(error) => {
                        eprintln!("{error}");
                        std::process::exit(1);
                    }
                }
                return true;
            }
            if remaining.len() > 1 {
                eprintln!("expected a single JSON argument");
                std::process::exit(2);
            }
            let input = remaining
                .first()
                .cloned()
                .unwrap_or_else(|| "null".to_string());
            let parsed: Value = serde_json::from_str(&input).unwrap_or_else(|error| {
                eprintln!("invalid JSON arguments: {error}");
                std::process::exit(2);
            });
            let request = request_payload(&op, parsed);
            match call_socket(request) {
                Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
            true
        }
        _ => false,
    }
}

impl App {
    fn scene_revision(&self) -> u64 {
        (self.agent_revision << 32) | (self.tree.path_version("scene") as u32 as u64)
    }

    pub(super) fn process_agent_requests(&mut self) {
        let Some(receiver) = &self.agent_requests else {
            return;
        };
        let pending: Vec<_> = receiver.try_iter().collect();
        for inbound in pending {
            self.process_agent_request(inbound);
        }
    }

    fn process_agent_request(&mut self, inbound: Inbound) {
        let Inbound { request, reply } = inbound;
        let result = match request {
            Request::GetState => self.agent_state(),
            Request::GetSchema => Ok(schema()),
            Request::ListCommands => Ok(Value::Array(self.commands.commands.iter().map(|(name, command)| {
                json!({"name": name, "title": command.title, "description": command.docs, "shortcut": command.shortcut})
            }).collect())),
            Request::Apply(args) => self.agent_apply(args),
            Request::ExecuteCommand { name } => {
                if !self.commands.commands.contains_key(&name) {
                    Err(format!("unknown command: {name}"))
                } else {
                    commands::execute(&self.commands, &name, &mut self.tree);
                    if name != "undo" && name != "redo" {
                        self.tree.make_undo_redo_snapshot();
                    }
                    Ok(json!({"revision": self.scene_revision()}))
                }
            }
            Request::SetView(args) => self.agent_set_view(args),
            Request::CaptureViewport => {
                if self.agent_capture.is_some() || self.pending_render.is_some() || self.guide_screenshot.is_some()
                    || self.discard_capture || self.renderer.as_ref().is_some_and(|renderer| renderer.capture_pending()) {
                    Err("a render is already in progress".to_string())
                } else {
                    self.agent_capture = Some(reply);
                    if let Some(window) = &self.window { window.request_redraw(); }
                    return;
                }
            }
            Request::Undo => {
                self.tree.undo();
                Ok(json!({"revision": self.scene_revision()}))
            }
            Request::Redo => {
                self.tree.redo();
                Ok(json!({"revision": self.scene_revision()}))
            }
            Request::Save { path } => {
                let path = path.or_else(|| self.document.current_path().map(ToOwned::to_owned));
                match path {
                    Some(path) => document::write_scene(&path, &self.tree).map(|_| {
                        self.document.mark_saved(path.clone());
                        json!({"path": path})
                    }),
                    None => Err("provide a path for the first save".to_string()),
                }
            }
            Request::Open { path } => document::read_scene(&path).map(|scene| {
                self.replace_scene(scene);
                self.document.mark_opened(path.clone());
                self.agent_revision += 1;
                json!({"path": path, "revision": self.scene_revision()})
            }),
        };
        let _ = reply.send(result);
    }

    fn agent_state(&self) -> AgentResult {
        let raw_scene: Value = serde_json::from_slice(&document::serialize_scene(&self.tree)?)
            .map_err(|error| error.to_string())?;
        let animation = match self.tree.get_path("scene.animation") {
            model::ClaydashValue::Animation(value) => Some(value),
            _ => None,
        };
        Ok(json!({
            "revision": self.scene_revision(),
            "document_path": self.document.current_path(),
            "objects": model::objects_ref(&self.tree),
            "selection": model::selected_ref(&self.tree),
            "materials": model::material_assets(&self.tree),
            "cameras": model::scene_cameras(&self.tree),
            "active_camera": model::active_camera_id(&self.tree),
            "world": model::world(&self.tree),
            "animation": animation,
            "view": {"position": self.camera.position, "target": self.camera.target,
                "up": self.camera.up, "projection_mode": self.camera.projection_mode},
            "document": raw_scene,
        }))
    }

    fn agent_apply(&mut self, args: ApplyArgs) -> AgentResult {
        if args
            .expected_revision
            .is_some_and(|expected| expected != self.scene_revision())
        {
            return Err(format!(
                "scene changed; current revision is {}",
                self.scene_revision()
            ));
        }
        if args.actions.is_empty() {
            return Err("actions must not be empty".to_string());
        }
        if args.actions.len() != 1
            && args
                .actions
                .iter()
                .any(|action| matches!(action, Action::ReplaceScene { .. }))
        {
            return Err("ReplaceScene must be the only action".to_string());
        }
        let mut draft = self.tree.clone();
        let mut created = Vec::new();
        let mut replace = false;
        for action in args.actions {
            match action {
                Action::CreateObject {
                    kind,
                    name,
                    transform,
                    position,
                    params,
                } => {
                    let mut object = SdfObject::create_kind(kind);
                    object.material = model::picked_material(&draft);
                    object.material_id = model::picked_material_id(&draft);
                    object.color = object.material.color;
                    if let Some(name) = name {
                        object.name = name;
                    }
                    if let Some(transform) = transform {
                        object.transform = transform;
                    }
                    if let Some(position) = position {
                        object.transform.translation = Vec3::from_array(position);
                    }
                    if let Some(params) = params {
                        if !params_match(kind, &params) {
                            return Err("params do not match primitive kind".to_string());
                        }
                        object.params = params;
                    }
                    created.push(object.uuid);
                    let mut objects = model::objects(&draft);
                    objects.push(object);
                    model::set_objects(&mut draft, objects);
                }
                Action::PutObject { object } => {
                    let mut objects = model::objects(&draft);
                    let Some(existing) = objects
                        .iter_mut()
                        .find(|existing| existing.uuid == object.uuid)
                    else {
                        return Err(format!("object {} does not exist", object.uuid));
                    };
                    *existing = object;
                    model::set_objects(&mut draft, objects);
                }
                Action::SetObjectName { id, name } => {
                    let mut objects = model::objects(&draft);
                    let object = objects
                        .iter_mut()
                        .find(|object| object.uuid == id)
                        .ok_or_else(|| format!("object {id} does not exist"))?;
                    object.name = name;
                    model::set_objects(&mut draft, objects);
                }
                Action::SetObjectTransform { id, transform } => {
                    let mut objects = model::objects(&draft);
                    let object = objects
                        .iter_mut()
                        .find(|object| object.uuid == id)
                        .ok_or_else(|| format!("object {id} does not exist"))?;
                    object.transform = transform;
                    model::set_objects(&mut draft, objects);
                }
                Action::SetObjectParams { id, params } => {
                    let mut objects = model::objects(&draft);
                    let object = objects
                        .iter_mut()
                        .find(|object| object.uuid == id)
                        .ok_or_else(|| format!("object {id} does not exist"))?;
                    object.params = params;
                    model::set_objects(&mut draft, objects);
                }
                Action::SetBoolean {
                    id,
                    parent,
                    operation,
                    softness,
                } => {
                    let mut objects = model::objects(&draft);
                    let object = objects
                        .iter_mut()
                        .find(|object| object.uuid == id)
                        .ok_or_else(|| format!("object {id} does not exist"))?;
                    object.boolean_parent = parent;
                    object.operation = operation;
                    if let Some(softness) = softness {
                        object.softness = softness;
                    }
                    model::set_objects(&mut draft, objects);
                }
                Action::DeleteObject { id } => {
                    let mut objects = model::objects(&draft);
                    if !objects.iter().any(|object| object.uuid == id) {
                        return Err(format!("object {id} does not exist"));
                    }
                    objects.retain(|object| object.uuid != id);
                    for object in &mut objects {
                        if object.boolean_parent == Some(id) {
                            object.boolean_parent = None;
                            object.operation = BooleanOperation::Union;
                        }
                    }
                    model::set_objects(&mut draft, objects);
                    let selection = model::selected(&draft)
                        .into_iter()
                        .filter(|selected| *selected != id)
                        .collect();
                    model::set_selected(&mut draft, selection);
                }
                Action::SetWorld { world } => {
                    draft.set_path("scene.world", model::ClaydashValue::World(world))
                }
                Action::SetMaterials { materials } => {
                    model::set_material_assets(&mut draft, materials)
                }
                Action::SetCameras { cameras } => model::set_scene_cameras(&mut draft, cameras),
                Action::SetAnimation { animation } => draft.set_path(
                    "scene.animation",
                    model::ClaydashValue::Animation(animation),
                ),
                Action::SetSelection { ids } => {
                    for id in &ids {
                        if !model::objects_ref(&draft)
                            .iter()
                            .any(|object| object.uuid == *id)
                            && !model::scene_cameras(&draft)
                                .iter()
                                .any(|camera| camera.uuid == *id)
                        {
                            return Err(format!("selection target {id} does not exist"));
                        }
                    }
                    model::set_selected_exact(&mut draft, ids);
                }
                Action::SetActiveCamera { id } => {
                    if let Some(id) = id {
                        if !model::scene_cameras(&draft)
                            .iter()
                            .any(|camera| camera.uuid == id)
                        {
                            return Err(format!("camera {id} does not exist"));
                        }
                        draft.set_path("scene.active_camera", model::ClaydashValue::Uuid(id));
                    } else {
                        draft.set_path("scene.active_camera", model::ClaydashValue::None);
                    }
                }
                Action::ReplaceScene { scene } => {
                    let bytes = serde_json::to_vec(&scene).map_err(|error| error.to_string())?;
                    let restored = document::deserialize_scene(&bytes)?;
                    draft.set_tree("scene", restored);
                    replace = true;
                }
            }
        }
        validate_scene(&draft)?;
        if replace {
            self.replace_scene(draft.get_tree("scene").ok_or("missing scene")?);
        } else {
            draft.make_undo_redo_snapshot();
            self.tree = draft;
        }
        if replace {
            self.agent_revision += 1;
        }
        Ok(json!({"revision": self.scene_revision(), "created_ids": created}))
    }

    fn agent_set_view(&mut self, args: ViewArgs) -> AgentResult {
        let position = Vec3::from_array(args.position);
        let target = Vec3::from_array(args.target);
        let up = args.up.map(Vec3::from_array).unwrap_or(Vec3::Y);
        if !position.is_finite()
            || !target.is_finite()
            || !up.is_finite()
            || position.distance(target) < 0.001
            || up.length() < 0.001
            || (target - position).cross(up).length() < 0.001
        {
            return Err(
                "view requires finite, distinct position/target and a nonparallel up vector"
                    .to_string(),
            );
        }
        self.camera.position = position;
        self.camera.target = target;
        self.camera.up = up.normalize();
        if let Some(mode) = args.projection_mode {
            self.camera.projection_mode = mode;
        }
        Ok(json!({"revision": self.scene_revision()}))
    }
}

fn params_match(kind: PrimitiveKind, params: &SdfParams) -> bool {
    matches!(
        (kind, params),
        (PrimitiveKind::Box, SdfParams::BoxParams(_))
            | (PrimitiveKind::Sphere, SdfParams::SphereParams(_))
            | (PrimitiveKind::Cylinder, SdfParams::CylinderParams { .. })
            | (PrimitiveKind::Torus, SdfParams::TorusParams { .. })
            | (
                PrimitiveKind::PolygonPrism,
                SdfParams::PolygonPrismParams(_)
            )
            | (PrimitiveKind::BezierCurve, SdfParams::BezierCurveParams(_))
    )
}

fn validate_scene(tree: &model::DataTree) -> Result<(), String> {
    let objects = model::objects_ref(tree);
    let mut ids = std::collections::HashSet::new();
    for object in objects {
        if !ids.insert(object.uuid) {
            return Err(format!("duplicate object id {}", object.uuid));
        }
        let kind = match object.object_type {
            sdf_consts::TYPE_BOX => PrimitiveKind::Box,
            sdf_consts::TYPE_SPHERE => PrimitiveKind::Sphere,
            sdf_consts::TYPE_CYLINDER => PrimitiveKind::Cylinder,
            sdf_consts::TYPE_TORUS => PrimitiveKind::Torus,
            sdf_consts::TYPE_POLYGON_PRISM => PrimitiveKind::PolygonPrism,
            sdf_consts::TYPE_BEZIER_CURVE => PrimitiveKind::BezierCurve,
            _ => return Err(format!("unknown primitive type on {}", object.uuid)),
        };
        if !params_match(kind, &object.params) {
            return Err(format!("primitive parameters do not match {}", object.uuid));
        }
        if !object.transform.translation.is_finite()
            || !object.transform.scale.is_finite()
            || !object.transform.rotation.is_finite()
            || !object.group_transform.translation.is_finite()
            || !object.group_transform.scale.is_finite()
            || !object.group_transform.rotation.is_finite()
        {
            return Err(format!("nonfinite transform on {}", object.uuid));
        }
    }
    for object in objects {
        let mut cursor = object.boolean_parent;
        let mut visited = std::collections::HashSet::new();
        while let Some(parent) = cursor {
            if !visited.insert(parent) || parent == object.uuid {
                return Err(format!("boolean cycle at {}", object.uuid));
            }
            cursor = objects
                .iter()
                .find(|other| other.uuid == parent)
                .ok_or_else(|| format!("missing boolean parent {parent}"))?
                .boolean_parent;
        }
    }
    Ok(())
}

fn schema() -> Value {
    json!({
        "version": 1,
        "operations": ["GetState", "GetSchema", "ListCommands", "Apply", "ExecuteCommand", "SetView", "CaptureViewport", "Undo", "Redo", "Save", "Open"],
        "actions": ["CreateObject", "PutObject", "SetObjectName", "SetObjectTransform", "SetObjectParams", "SetBoolean", "DeleteObject", "SetWorld", "SetMaterials", "SetCameras", "SetAnimation", "SetSelection", "SetActiveCamera", "ReplaceScene"],
        "primitive_kinds": PrimitiveKind::ALL.iter().map(|kind| json!({"kind": kind, "example": SdfObject::create_kind(*kind)})).collect::<Vec<_>>(),
        "notes": "GetState returns complete typed objects and the raw .claydash scene document. CreateObject accepts an optional position [x,y,z], full transform, and shape params. PutObject replaces a complete existing object. Apply actions run as one undoable edit. Send expected_revision from GetState to reject stale edits. ReplaceScene accepts the raw document value and must be the sole action."
    })
}

fn run_mcp() {
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
                json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "claydash", "version": env!("CARGO_PKG_VERSION")}}),
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
                    "capture_viewport" => "CaptureViewport",
                    "undo" => "Undo",
                    "redo" => "Redo",
                    "save" => "Save",
                    "open" => "Open",
                    _ => "",
                };
                if op.is_empty() {
                    Err(format!("unknown tool: {name}"))
                } else {
                    match call_socket(request_payload(op, arguments)) {
                        Ok(value) if name == "capture_viewport" => Ok(
                            json!({"content": [{"type": "image", "data": value["data"], "mimeType": "image/png"}]}),
                        ),
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

fn mcp_tools() -> Vec<Value> {
    let object = json!({"type": "object", "additionalProperties": true});
    let uuid = json!({"type": "string", "format": "uuid"});
    let action = json!({"oneOf": [
        {"type": "object", "properties": {"type": {"const": "CreateObject"}, "kind": {"enum": ["Sphere", "Box", "Cylinder", "Torus", "PolygonPrism", "BezierCurve"]}, "name": {"type": "string"}, "position": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "transform": object, "params": object}, "required": ["type", "kind"]},
        {"type": "object", "properties": {"type": {"const": "PutObject"}, "object": object}, "required": ["type", "object"]},
        {"type": "object", "properties": {"type": {"const": "SetObjectName"}, "id": uuid, "name": {"type": "string"}}, "required": ["type", "id", "name"]},
        {"type": "object", "properties": {"type": {"const": "SetObjectTransform"}, "id": uuid, "transform": object}, "required": ["type", "id", "transform"]},
        {"type": "object", "properties": {"type": {"const": "SetObjectParams"}, "id": uuid, "params": object}, "required": ["type", "id", "params"]},
        {"type": "object", "properties": {"type": {"const": "SetBoolean"}, "id": uuid, "parent": {"type": ["string", "null"]}, "operation": {"enum": ["Union", "Subtract", "Intersect"]}, "softness": {"type": "number"}}, "required": ["type", "id", "parent", "operation"]},
        {"type": "object", "properties": {"type": {"const": "DeleteObject"}, "id": uuid}, "required": ["type", "id"]},
        {"type": "object", "properties": {"type": {"const": "SetWorld"}, "world": object}, "required": ["type", "world"]},
        {"type": "object", "properties": {"type": {"const": "SetMaterials"}, "materials": {"type": "array", "items": object}}, "required": ["type", "materials"]},
        {"type": "object", "properties": {"type": {"const": "SetCameras"}, "cameras": {"type": "array", "items": object}}, "required": ["type", "cameras"]},
        {"type": "object", "properties": {"type": {"const": "SetAnimation"}, "animation": object}, "required": ["type", "animation"]},
        {"type": "object", "properties": {"type": {"const": "SetSelection"}, "ids": {"type": "array", "items": uuid}}, "required": ["type", "ids"]},
        {"type": "object", "properties": {"type": {"const": "SetActiveCamera"}, "id": {"type": ["string", "null"]}}, "required": ["type", "id"]},
        {"type": "object", "properties": {"type": {"const": "ReplaceScene"}, "scene": object}, "required": ["type", "scene"]}
    ]});
    let empty = json!({"type": "object", "properties": {}, "additionalProperties": false});
    [
        ("get_state", "Read the full live Claydash scene, selection, materials, animation, camera, and revision.", empty.clone()),
        ("get_schema", "Get action names and example serialized objects for each primitive type.", empty.clone()),
        ("list_commands", "List Claydash's existing command palette commands.", empty.clone()),
        ("apply", "Apply typed scene actions as one undoable transaction. Read get_schema first; include expected_revision from get_state.", json!({"type": "object", "properties": {"expected_revision": {"type": "integer"}, "actions": {"type": "array", "items": action, "minItems": 1}}, "required": ["actions"]})),
        ("execute_command", "Run an existing Claydash command by name. Some commands start an interactive gesture.", json!({"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]})),
        ("set_view", "Set the live viewport camera. position and target are [x,y,z]; projection_mode is Perspective or Orthographic.", json!({"type": "object", "properties": {"position": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "target": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "up": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "projection_mode": {"enum": ["Perspective", "Orthographic"]}}, "required": ["position", "target"]})),
        ("capture_viewport", "Render and return a PNG of the live viewport after scene refinement.", empty.clone()),
        ("undo", "Undo the last scene edit.", empty.clone()),
        ("redo", "Redo the last undone scene edit.", empty.clone()),
        ("save", "Save the live scene to the current path or a specified .claydash path.", json!({"type": "object", "properties": {"path": {"type": "string"}}})),
        ("open", "Open a .claydash project in the live window.", json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})),
    ].into_iter().map(|(name, description, input_schema)| json!({"name": name, "description": description, "inputSchema": input_schema})).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
                }],
            })
            .is_err());
        app.tree.undo();
        assert_eq!(model::objects_ref(&app.tree).len(), original);
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
}

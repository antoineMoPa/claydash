//! Local automation surface shared by the CLI and MCP adapter.
//! The socket thread only transports requests. All scene edits run on the app thread.

use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use glam::{Vec2, Vec3};
use serde::Deserialize;
use serde_json::{json, Value};
use winit::event_loop::EventLoopProxy;

use super::{App, AppEvent};
use crate::{
    camera::{Camera, ProjectionMode},
    commands, document,
    model::{
        self, AnimationData, BooleanOperation, GroupRenderRepresentation, MaterialAsset,
        PrimitiveKind, SdfObject, SdfParams, Transform, World,
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
    GetSceneSummary,
    GetSchema,
    GetActionSchema { action: String, primitive: Option<PrimitiveKind> },
    GetGaussianCloud { id: uuid::Uuid, space: Option<CloudSpace>, sample_count: Option<usize> },
    ListCommands,
    EnableSlowSmoothBlends,
    EnableBooleanOperations,
    Apply(ApplyArgs),
    ExecuteCommand { name: String },
    SetView(ViewArgs),
    BuildPoissonMesh { id: uuid::Uuid },
    GetPoissonMeshStatus { id: uuid::Uuid },
    ExportGlb(export::ExportGlbArgs),
    GetGlbExportStatus { id: uuid::Uuid },
    CancelGlbExport { id: uuid::Uuid },
    CaptureViewport(CaptureViewportArgs),
    CaptureOrthographic(CaptureOrthographicArgs),
    Undo,
    Redo,
    Save { path: Option<PathBuf> },
    Open { path: PathBuf },
    ImportGaussianPly { path: PathBuf },
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudSpace { Object, World }

fn cloud_bounds(points: impl Iterator<Item = Vec3>) -> Option<Value> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut count = 0;
    for point in points {
        min = min.min(point);
        max = max.max(point);
        count += 1;
    }
    (count > 0).then(|| json!({"min": min, "max": max}))
}

#[derive(Default, Deserialize)]
pub struct CaptureViewportArgs {
    pub sheet_stage: Option<model::SheetStage>,
    pub object_ids: Option<Vec<uuid::Uuid>>,
    pub refine: Option<bool>,
    pub mode: Option<CaptureMode>,
}

#[derive(Default, Deserialize)]
pub struct CaptureOrthographicArgs {
    pub sheet_stage: Option<model::SheetStage>,
    pub object_ids: Option<Vec<uuid::Uuid>>,
    pub panel_size: Option<u32>,
    pub distance: Option<f32>,
    pub refine: Option<bool>,
    pub mode: Option<CaptureMode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    #[default]
    SimpleShading,
    FullMaterial,
    Outline,
}

impl CaptureMode {
    fn from_options(mode: Option<Self>, refine: Option<bool>) -> Self {
        match (mode, refine) {
            (Some(mode), _) => mode,
            (None, Some(true)) => Self::FullMaterial,
            (None, _) => Self::SimpleShading,
        }
    }
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
        render_representation: Option<GroupRenderRepresentation>,
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
    SetRenderRepresentation {
        id: uuid::Uuid,
        render_representation: GroupRenderRepresentation,
        voxels: Option<model::VoxelSettings>,
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
    CreateVectorVariable {
        id: Option<uuid::Uuid>,
        name: String,
        value: Vec3,
        space: Option<model::VariableSpace>,
    },
    UpdateVectorVariable {
        id: uuid::Uuid,
        name: Option<String>,
        value: Option<Vec3>,
        space: Option<model::VariableSpace>,
    },
    DeleteVectorVariable {
        id: uuid::Uuid,
    },
    SetVectorBinding {
        object: uuid::Uuid,
        target: model::VectorBindingTarget,
        variable: uuid::Uuid,
        #[serde(default)]
        offset: Vec3,
    },
    RemoveVectorBinding {
        object: uuid::Uuid,
        target: model::VectorBindingTarget,
    },
    SetRigidBinding { binding: model::RigidBinding },
    RemoveRigidBinding { object: uuid::Uuid, target: model::RigidBindingTarget },
    SetVariables {
        variables: model::SceneVariables,
    },
    SetWorld {
        world: World,
    },
    CreatePostProcessPass {
        name: String,
        wgsl: String,
        enabled: Option<bool>,
    },
    UpdatePostProcessPass {
        id: uuid::Uuid,
        name: Option<String>,
        wgsl: Option<String>,
        enabled: Option<bool>,
    },
    MovePostProcessPass {
        id: uuid::Uuid,
        index: usize,
    },
    DeletePostProcessPass {
        id: uuid::Uuid,
    },
    SetMaterials {
        materials: Vec<MaterialAsset>,
    },
    CreateCustomMaterial {
        name: String,
        wgsl: String,
    },
    UpdateCustomMaterial {
        id: uuid::Uuid,
        name: Option<String>,
        wgsl: Option<String>,
    },
    AssignMaterial {
        id: uuid::Uuid,
        object_ids: Vec<uuid::Uuid>,
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
    if let Some(path) = std::env::var_os("CLAYDASH_AGENT_SOCKET") {
        return Ok(PathBuf::from(path));
    }
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

pub(super) fn listen(proxy: Option<EventLoopProxy<AppEvent>>) -> Result<Receiver<Inbound>, String> {
    let path = socket_path()?;
    let directory = path
        .parent()
        .ok_or("agent socket has no parent directory")?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    // A custom socket may live in an existing shared directory; never chmod it.
    if std::env::var_os("CLAYDASH_AGENT_SOCKET").is_none() {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
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
            let proxy = proxy.clone();
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
                    if let Some(proxy) = proxy {
                        let _ = proxy.send_event(AppEvent::AgentRequest);
                    }
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
        .map_err(|_| "No Claydash instance is connected. Start claydash serve or the desktop app first.".to_string())?;
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
        "GetState" | "GetSceneSummary" | "GetSchema" | "ListCommands" | "EnableSlowSmoothBlends" | "EnableBooleanOperations" | "Undo" | "Redo" => Value::Null,
        "CaptureViewport" | "CaptureOrthographic" if args.is_null() => json!({}),
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
        "serve" | "--headless" | "--agent-headless" | "mcp" => {
            let mode = mode.as_str();
            let remaining: Vec<_> = args.collect();
            if mode == "mcp" && remaining.is_empty() {
                run_mcp(call_socket);
            } else {
                let options = headless::Options::parse(mode, &remaining).unwrap_or_else(|error| {
                    eprintln!("{error}");
                    std::process::exit(2);
                });
                if let Some(options) = options {
                    if let Err(error) = headless::run(options, mode == "mcp") {
                        eprintln!("{error}");
                        std::process::exit(1);
                    }
                }
            }
            true
        }
        "agent" => {
            let Some(op) = args.next() else {
                eprintln!("usage: claydash agent <operation> [JSON arguments]");
                std::process::exit(2);
            };
            if matches!(op.as_str(), "--help" | "-h") {
                if let Some(extra) = args.next() {
                    eprintln!("unexpected argument: {extra}");
                    std::process::exit(2);
                }
                println!("Usage: claydash agent <operation> [JSON arguments]\n\nExamples:\n  claydash agent GetState\n  claydash agent GetSchema\n  claydash agent CaptureViewport --output viewport.png\n\nUse GetSchema to discover operations, including ExportGlb,\nGetGlbExportStatus, and CancelGlbExport. Requires a running server or desktop app.");
                return true;
            }
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
        let changes_view = matches!(
            &request,
            Request::Apply(_)
                | Request::EnableSlowSmoothBlends
                | Request::EnableBooleanOperations
                | Request::ExecuteCommand { .. }
                | Request::SetView(_)
                | Request::BuildPoissonMesh { .. }
                | Request::Undo
                | Request::Redo
                | Request::Open { .. }
                | Request::ImportGaussianPly { .. }
        );
        let result = match request {
            Request::GetState => self.agent_state(),
            Request::GetSceneSummary => self.agent_scene_summary(),
            Request::GetSchema => Ok(schema()),
            Request::GetActionSchema { action, primitive } => mcp::action_schema(&action, primitive),
            Request::GetGaussianCloud { id, space, sample_count } => self.agent_gaussian_cloud(id, space, sample_count),
            Request::ListCommands => Ok(Value::Array(self.commands.commands.iter().map(|(name, command)| {
                json!({"name": name, "title": command.title, "description": command.docs, "shortcut": command.shortcut})
            }).collect())),
            Request::ImportGaussianPly { path } => {
                self.import_gaussian_path(&path).map(|id| json!({"id": id, "revision": self.scene_revision()}))
            }
            Request::Apply(args) => self.agent_apply(args),
            Request::EnableSlowSmoothBlends => self.agent_enable_slow_smooth_blends(),
            Request::EnableBooleanOperations => self.agent_enable_boolean_operations(),
            Request::ExecuteCommand { name } => self.agent_execute_command(&name),
            Request::SetView(args) => self.agent_set_view(args),
            Request::BuildPoissonMesh { id } => self.agent_build_poisson_mesh(id),
            Request::GetPoissonMeshStatus { id } => self.agent_poisson_mesh_status(id),
            Request::ExportGlb(args) => self.agent_export_glb(args),
            Request::GetGlbExportStatus { id } => self.glb_export_status(id),
            Request::CancelGlbExport { id } => self.cancel_glb_export(id),
            Request::CaptureViewport(args) => {
                if self.agent_capture.is_some() || self.pending_render.is_some() || self.guide_screenshot.is_some()
                    || self.discard_capture || self.renderer.as_ref().is_some_and(|renderer| renderer.capture_pending()) {
                    Err("a render is already in progress".to_string())
                } else {
                    match capture::capture_scene(model::objects_ref(&self.tree), args.object_ids.as_deref(), args.sheet_stage) {
                        Ok(objects) => {
                            self.agent_capture = Some(AgentCapture {
                                reply, objects, mode: CaptureMode::from_options(args.mode, args.refine),
                                view: CaptureView::Viewport,
                            });
                            if let Some(renderer) = &mut self.renderer {
                                renderer.invalidate_scene();
                            }
                            return;
                        }
                        Err(error) => Err(error),
                    }
                }
            }
            Request::CaptureOrthographic(args) => {
                if self.agent_capture.is_some() || self.pending_render.is_some() || self.guide_screenshot.is_some()
                    || self.discard_capture || self.renderer.as_ref().is_some_and(|renderer| renderer.capture_pending()) {
                    Err("a render is already in progress".to_string())
                } else {
                    let panel_size = args.panel_size.unwrap_or(256);
                    let distance = args.distance.unwrap_or(8.0);
                    if !(96..=512).contains(&panel_size)
                        || self.renderer.as_ref().is_some_and(|renderer| panel_size as f32 > renderer.size().min_element()) {
                        Err("panel_size must be 96–512 pixels and fit inside the viewport".into())
                    } else if !distance.is_finite() || !(0.1..=1000.0).contains(&distance) {
                        Err("distance must be between 0.1 and 1000".into())
                    } else {
                        match capture::capture_scene(model::objects_ref(&self.tree), args.object_ids.as_deref(), args.sheet_stage) {
                            Ok(objects) => {
                                self.agent_capture = Some(AgentCapture {
                                    reply, objects, mode: CaptureMode::from_options(args.mode, args.refine),
                                    view: CaptureView::Orthographic { panel_size, distance, frames: Vec::with_capacity(3) },
                                });
                                if let Some(renderer) = &mut self.renderer {
                                    renderer.invalidate_scene();
                                }
                                return;
                            }
                            Err(error) => Err(error),
                        }
                    }
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
                    Some(path) => document::write_scene(&path, &self.project_tree_for_save()).map(|_| {
                        self.document.mark_saved(path.clone());
                        self.document.mark_scene_clean(&self.tree);
                        json!({"path": path})
                    }),
                    None => Err("provide a path for the first save".to_string()),
                }
            }
            Request::Open { path } => document::read_scene(&path).map(|scene| {
                self.replace_scene(scene);
                self.document.mark_opened(path.clone());
                self.document.mark_scene_clean(&self.tree);
                self.agent_revision += 1;
                json!({"path": path, "revision": self.scene_revision()})
            }),
        };
        if changes_view && result.is_ok() {
            self.agent_redraw_pending = true;
        }
        let _ = reply.send(result);
    }

    fn agent_poisson_mesh_target(&self, id: uuid::Uuid) -> Result<(), String> {
        let object = model::objects_ref(&self.tree).iter().find(|object| object.uuid == id)
            .ok_or_else(|| format!("unknown object: {id}"))?;
        if object.boolean_parent.is_some() {
            return Err("Poisson mesh cache requires a Boolean group root or standalone object".into());
        }
        if !matches!(object.render_representation, GroupRenderRepresentation::PoissonMesh | GroupRenderRepresentation::DirectMesh) {
            return Err("set the root render_representation to PoissonMesh or DirectMesh first".into());
        }
        Ok(())
    }

    fn agent_build_poisson_mesh(&self, id: uuid::Uuid) -> AgentResult {
        use crate::renderer::poisson_mesh::PoissonMeshAction;
        self.agent_poisson_mesh_target(id)?;
        self.egui.data_mut(|data| {
            let key = egui::Id::new("poisson-mesh-actions");
            let mut actions = data.get_temp::<Vec<PoissonMeshAction>>(key).unwrap_or_default();
            if !actions.iter().any(|action| matches!(action, PoissonMeshAction::Build(root) if *root == id)) {
                actions.push(PoissonMeshAction::Build(id));
            }
            data.insert_temp(key, actions);
        });
        self.egui.request_repaint();
        Ok(json!({"id": id, "status": "queued"}))
    }

    fn agent_poisson_mesh_status(&self, id: uuid::Uuid) -> AgentResult {
        use crate::renderer::poisson_mesh::{PoissonMeshAction, PoissonMeshStatus};
        self.agent_poisson_mesh_target(id)?;
        let (queued, status) = self.egui.data(|data| {
            let queued = data.get_temp::<Vec<PoissonMeshAction>>(egui::Id::new("poisson-mesh-actions"))
                .is_some_and(|actions| actions.iter().any(|action| matches!(action,
                    PoissonMeshAction::Build(root) if *root == id)));
            let status = data.get_temp::<HashMap<uuid::Uuid, PoissonMeshStatus>>(egui::Id::new("poisson-mesh-status"))
                .and_then(|statuses| statuses.get(&id).cloned());
            (queued, status)
        });
        let direct = model::objects_ref(&self.tree).iter().find(|object| object.uuid == id)
            .is_some_and(|object| object.render_representation == GroupRenderRepresentation::DirectMesh);
        let mut result = if queued { json!({"status": "queued"}) } else {
            match status {
                None => json!({"status": "not_built"}),
                Some(PoissonMeshStatus::Sampling { percent }) => json!({"status": if direct { "tessellating" } else { "sampling" }, "percent": percent}),
                Some(PoissonMeshStatus::Reconstructing { percent }) => json!({"status": if direct { "mesh_booleans" } else { "reconstructing" }, "percent": percent}),
                Some(PoissonMeshStatus::Ready { vertices, triangles, showing, available, sample_resolution, mesh_resolution }) =>
                    json!({"status": "ready", "vertices": vertices, "triangles": triangles,
                        "showing": showing, "available": available, "sample_resolution": sample_resolution,
                        "mesh_resolution": mesh_resolution}),
                Some(PoissonMeshStatus::Failed(error)) => json!({"status": "failed", "error": error}),
            }
        };
        result["id"] = json!(id);
        Ok(result)
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
            "modeling_policy": {
                "boolean_operations_enabled": boolean_policy::enabled(&self.tree),
                "boolean_enable_command": "enable_boolean_operations",
                "boolean_warning": boolean_policy::WARNING,
                "slow_smooth_blends_enabled": smooth_blend_policy::enabled(&self.tree),
                "guidance": smooth_blend_policy::GUIDANCE,
                "enable_command": "enable_slow_smooth_blends",
                "warning": smooth_blend_policy::WARNING,
            },
            "viewport": self.renderer.as_ref().map(|renderer| renderer.viewport_diagnostics()),
            "full_material_rendering": commands::full_material_rendering(&self.tree),
            "outline_mode": commands::outline_mode(&self.tree),
            "document_path": self.document.current_path(),
            "objects": model::objects_ref(&self.tree),
            "selection": model::selected_ref(&self.tree),
            "materials": model::material_assets(&self.tree),
            "variables": model::scene_variables(&self.tree),
            "cameras": model::scene_cameras(&self.tree),
            "active_camera": model::active_camera_id(&self.tree),
            "world": model::world(&self.tree),
            "post_processing": model::post_processing_ref(&self.tree),
            "animation": animation,
            "view": {"position": self.camera.position, "target": self.camera.target,
                "up": self.camera.up, "projection_mode": self.camera.projection_mode},
            "document": raw_scene,
        }))
    }

    fn agent_scene_summary(&self) -> AgentResult {
        let objects = model::objects_ref(&self.tree);
        let entries: Vec<Value> = objects.iter().map(|object| {
            let matrix = model::object_world_matrix(objects, object.uuid);
            let cloud = object.imported_splats.as_ref().map(|cloud| {
                let points = cloud.splats.iter().map(|splat| splat.position);
                json!({"count": cloud.splats.len(), "object_bounds": cloud_bounds(points),
                    "world_bounds": cloud_bounds(cloud.splats.iter().map(|splat|
                        matrix.transform_point3(splat.position)))})
            });
            json!({"id": object.uuid, "name": object.name,
                "kind": if object.imported_splats.is_some() { json!("GaussianCloud") } else { json!(PrimitiveKind::from_object_type(object.object_type)) },
                "transform": object.transform, "group_transform": object.group_transform,
                "boolean_parent": object.boolean_parent, "operation": object.operation,
                "imported_gaussian": cloud})
        }).collect();
        Ok(json!({"revision": self.scene_revision(), "objects": entries,
            "selection": model::selected_ref(&self.tree),
            "view": {"position": self.camera.position, "target": self.camera.target,
                "up": self.camera.up, "projection_mode": self.camera.projection_mode},
            "document_path": self.document.current_path()}))
    }

    fn agent_gaussian_cloud(&self, id: uuid::Uuid, space: Option<CloudSpace>, sample_count: Option<usize>) -> AgentResult {
        let count = sample_count.unwrap_or(32);
        if count > 256 { return Err("sample_count must be 0–256".into()); }
        let objects = model::objects_ref(&self.tree);
        let object = objects.iter().find(|object| object.uuid == id)
            .ok_or_else(|| format!("object {id} not found"))?;
        let cloud = object.imported_splats.as_ref().ok_or_else(|| format!("object {id} has no imported Gaussian cloud"))?;
        let world = matches!(space, Some(CloudSpace::World));
        let matrix = model::object_world_matrix(objects, id);
        let positions: Vec<Vec3> = cloud.splats.iter().map(|splat| {
            if world { matrix.transform_point3(splat.position) } else { splat.position }
        }).collect();
        let bounds = cloud_bounds(positions.iter().copied());
        let mut axes = [Vec::with_capacity(positions.len()), Vec::with_capacity(positions.len()), Vec::with_capacity(positions.len())];
        for position in &positions {
            for (axis, values) in axes.iter_mut().enumerate() { values.push(position[axis]); }
        }
        for values in &mut axes { values.sort_unstable_by(f32::total_cmp); }
        let percentiles = [0.02, 0.5, 0.98].map(|quantile| {
            if positions.is_empty() { return None; }
            let index = ((positions.len() - 1) as f32 * quantile).round() as usize;
            Some([axes[0][index], axes[1][index], axes[2][index]])
        });
        let samples: Vec<Value> = (0..count.min(cloud.splats.len())).map(|slot| {
            let index = slot * cloud.splats.len() / count.min(cloud.splats.len());
            let splat = &cloud.splats[index];
            json!({"index": index, "position": positions[index], "color": splat.color,
                "opacity": splat.opacity})
        }).collect();
        Ok(json!({"id": id, "space": if world { "world" } else { "object" },
            "count": cloud.splats.len(), "bounds": bounds,
            "percentiles": {"p02": percentiles[0], "p50": percentiles[1], "p98": percentiles[2]},
            "samples": samples}))
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

mod actions;
mod capture;
mod export;
mod headless;
mod mcp;
#[cfg(test)]
mod tests;
mod validation;
mod smooth_blend_policy;
mod boolean_policy;
#[cfg(test)]
mod boolean_policy_tests;
#[cfg(test)]
mod smooth_blend_tests;

#[cfg(test)]
use capture::capture_objects;
pub use capture::{AgentCapture, CaptureView};
use mcp::{run_mcp, schema};
use validation::validate_scene;

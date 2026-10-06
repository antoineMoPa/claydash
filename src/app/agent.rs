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
    GetSchema,
    ListCommands,
    Apply(ApplyArgs),
    ExecuteCommand { name: String },
    SetView(ViewArgs),
    CaptureViewport(CaptureViewportArgs),
    CaptureOrthographic(CaptureOrthographicArgs),
    Undo,
    Redo,
    Save { path: Option<PathBuf> },
    Open { path: PathBuf },
}

#[derive(Default, Deserialize)]
pub struct CaptureViewportArgs {
    pub object_ids: Option<Vec<uuid::Uuid>>,
    pub refine: Option<bool>,
    pub mode: Option<CaptureMode>,
}

#[derive(Default, Deserialize)]
pub struct CaptureOrthographicArgs {
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

pub(super) fn listen(proxy: EventLoopProxy<AppEvent>) -> Result<Receiver<Inbound>, String> {
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
                    let _ = proxy.send_event(AppEvent::AgentRequest);
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
        "GetState" | "GetSchema" | "ListCommands" | "Undo" | "Redo" => Value::Null,
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
        let changes_view = matches!(
            &request,
            Request::Apply(_)
                | Request::ExecuteCommand { .. }
                | Request::SetView(_)
                | Request::Undo
                | Request::Redo
                | Request::Open { .. }
        );
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
            Request::CaptureViewport(args) => {
                if self.agent_capture.is_some() || self.pending_render.is_some() || self.guide_screenshot.is_some()
                    || self.discard_capture || self.renderer.as_ref().is_some_and(|renderer| renderer.capture_pending()) {
                    Err("a render is already in progress".to_string())
                } else {
                    match args.object_ids.map(|ids| capture_objects(model::objects_ref(&self.tree), &ids)).transpose() {
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
                        match args.object_ids.map(|ids| capture_objects(model::objects_ref(&self.tree), &ids)).transpose() {
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

    fn agent_state(&self) -> AgentResult {
        let raw_scene: Value = serde_json::from_slice(&document::serialize_scene(&self.tree)?)
            .map_err(|error| error.to_string())?;
        let animation = match self.tree.get_path("scene.animation") {
            model::ClaydashValue::Animation(value) => Some(value),
            _ => None,
        };
        Ok(json!({
            "revision": self.scene_revision(),
            "viewport": self.renderer.as_ref().map(|renderer| renderer.viewport_diagnostics()),
            "full_material_rendering": commands::full_material_rendering(&self.tree),
            "outline_mode": commands::outline_mode(&self.tree),
            "document_path": self.document.current_path(),
            "objects": model::objects_ref(&self.tree),
            "selection": model::selected_ref(&self.tree),
            "materials": model::material_assets(&self.tree),
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
mod mcp;
#[cfg(test)]
mod tests;
mod validation;

use capture::capture_objects;
pub use capture::{AgentCapture, CaptureView};
use mcp::{run_mcp, schema};
use validation::validate_scene;

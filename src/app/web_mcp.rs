//! Browser WebMCP requests are queued onto the same event loop that owns the scene.

use std::{cell::RefCell, sync::{mpsc::Sender, Arc}};

use base64::Engine as _;
use js_sys::{Function, Promise};
use serde::Deserialize;
use serde_json::{json, Value};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};
use winit::window::Window;

use super::{App, WebDocumentMessage};
use super::{mesh_actions, mesh_sections};
use crate::model::{self, PrimitiveKind, SdfObject};
use crate::renderer::poisson_mesh::geometry::{MeshSection, MeshShading};

thread_local! {
    static SENDER: RefCell<Option<Sender<WebDocumentMessage>>> = const { RefCell::new(None) };
    static WINDOW: RefCell<Option<Arc<Window>>> = const { RefCell::new(None) };
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    GetSceneSummary,
    ListCommands,
    CreatePrimitive { kind: PrimitiveKind },
    Undo,
    Redo,
    ApplyMeshActions { actions: Vec<MeshAction>, expected_revision: Option<i32> },
    GetMesh { id: uuid::Uuid },
    SetView { position: [f32; 3], target: [f32; 3], up: [f32; 3] },
    PreviewMeshSections { sections: Vec<MeshSection>, #[serde(default)] caps: bool },
    OpenProject { data_base64: String },
    ExportProject,
    SetCaptureClean { clean: bool },
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum MeshAction {
    CreateMesh { name: Option<String>, vertices: Vec<[f32; 3]>, faces: Vec<[u32; 3]>, shading: Option<MeshShading> },
    AddMeshVertices { id: uuid::Uuid, vertices: Vec<[f32; 3]> },
    AddMeshFaces { id: uuid::Uuid, faces: Vec<[u32; 3]> },
    SetMeshShading { id: uuid::Uuid, shading: MeshShading },
    ProjectMeshUvs { id: uuid::Uuid, direction: [f32; 3], up: [f32; 3], scale: [f32; 2], offset: [f32; 2] },
    SetMeshUvs { id: uuid::Uuid, uvs: Vec<[[f32; 2]; 3]> },
    SetMeshTexture { id: uuid::Uuid, png_base64: String },
    CreateSectionMesh { name: Option<String>, sections: Vec<MeshSection>, #[serde(default)] caps: bool, shading: Option<MeshShading> },
    SetMeshSections { id: uuid::Uuid, sections: Vec<MeshSection>, caps: bool },
}

pub(crate) struct Inbound {
    request: Request,
    resolve: Function,
    reject: Function,
}

pub(crate) fn set_sender(sender: Sender<WebDocumentMessage>) {
    SENDER.with(|current| *current.borrow_mut() = Some(sender));
}

pub(crate) fn set_window(window: Arc<Window>) {
    WINDOW.with(|current| *current.borrow_mut() = Some(window));
}

#[wasm_bindgen]
pub fn web_mcp_request(request: String) -> Promise {
    Promise::new(&mut |resolve, reject| {
        let parsed = serde_json::from_str::<Request>(&request);
        match parsed {
            Ok(request) => {
                let sent = SENDER.with(|sender| sender.borrow().as_ref()
                    .is_some_and(|sender| sender.send(WebDocumentMessage::WebMcp(Inbound {
                        request, resolve: resolve.clone(), reject: reject.clone(),
                    })).is_ok()));
                if !sent {
                    let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("Claydash is not ready"));
                } else {
                    WINDOW.with(|window| {
                        if let Some(window) = window.borrow().as_ref() { window.request_redraw(); }
                    });
                }
            }
            Err(error) => {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&error.to_string()));
            }
        }
    })
}

impl App {
    pub(super) fn process_web_mcp(&mut self, inbound: Inbound) {
        let result = self.run_web_mcp(inbound.request);
        match result {
            Ok(value) => match serde_json::to_string(&value) {
                Ok(text) => { let _ = inbound.resolve.call1(&JsValue::NULL, &JsValue::from_str(&text)); }
                Err(error) => { let _ = inbound.reject.call1(&JsValue::NULL, &JsValue::from_str(&error.to_string())); }
            },
            Err(error) => { let _ = inbound.reject.call1(&JsValue::NULL, &JsValue::from_str(&error)); }
        }
    }

    fn run_web_mcp(&mut self, request: Request) -> Result<Value, String> {
        match request {
            Request::GetSceneSummary => {
                let objects = model::objects_ref(&self.tree);
                Ok(json!({
                    "objects": objects.iter().map(|object| json!({
                        "id": object.uuid,
                        "name": object.name,
                        "kind": if object.imported_splats.is_some() { "GaussianCloud".to_owned() }
                            else if object.imported_mesh.as_ref().is_some_and(|mesh| !mesh.sections.is_empty()) {
                                "SectionMesh".to_owned()
                            } else if object.imported_mesh.is_some() { "EditableMesh".to_owned() }
                            else { format!("{:?}", PrimitiveKind::from_object_type(object.object_type)) },
                        "gaussian_count": object.imported_splats.as_ref().map(|cloud| cloud.splats.len()),
                        "mesh_vertices": object.imported_mesh.as_ref().map(|mesh| mesh.positions.len()),
                        "mesh_faces": object.imported_mesh.as_ref().map(|mesh| mesh.triangles.len()),
                        "has_texture": object.imported_mesh.as_ref().map(|mesh| mesh.baked_texture.is_some()),
                        "translation": object.transform.translation,
                    })).collect::<Vec<_>>(),
                    "selection": model::selected_ref(&self.tree).iter().copied()
                        .filter(|id| objects.iter().any(|object| object.uuid == *id))
                        .collect::<Vec<_>>(),
                    "view": {"position": self.camera.position, "target": self.camera.target,
                        "up": self.camera.up, "projection_mode": self.camera.projection_mode,
                        "viewport": self.camera.viewport, "viewport_origin": self.camera.viewport_origin},
                }))
            }
            Request::ListCommands => Ok(Value::Array(self.commands.commands.iter().map(|(name, command)| {
                json!({"name": name, "title": command.title, "description": command.docs})
            }).collect())),
            Request::CreatePrimitive { kind } => {
                let mut objects = model::objects(&self.tree);
                let object = SdfObject::create_kind(kind);
                let id = object.uuid;
                objects.push(object);
                self.tree.make_undo_redo_snapshot();
                model::set_objects(&mut self.tree, objects);
                model::set_selected_exact(&mut self.tree, vec![id]);
                if let Some(renderer) = &mut self.renderer { renderer.invalidate_scene(); }
                Ok(json!({"id": id}))
            }
            Request::Undo => {
                self.tree.undo();
                if let Some(renderer) = &mut self.renderer { renderer.invalidate_scene(); }
                Ok(json!({"ok": true}))
            }
            Request::Redo => {
                self.tree.redo();
                if let Some(renderer) = &mut self.renderer { renderer.invalidate_scene(); }
                Ok(json!({"ok": true}))
            }
            Request::ApplyMeshActions { actions, expected_revision } => {
                let revision = self.tree.path_version("scene");
                if expected_revision.is_some_and(|expected| expected != revision) {
                    return Err(format!("scene changed; current revision is {revision}"));
                }
                if actions.is_empty() { return Err("actions must not be empty".into()); }
                let mut draft = self.tree.clone();
                let mut created = Vec::new();
                for action in actions {
                    match action {
                        MeshAction::CreateMesh { name, vertices, faces, shading } => {
                            created.push(mesh_actions::create_mesh(&mut draft, name, vertices, faces, shading)?);
                        }
                        MeshAction::AddMeshVertices { id, vertices } => mesh_actions::add_vertices(&mut draft, id, vertices)?,
                        MeshAction::AddMeshFaces { id, faces } => mesh_actions::add_faces(&mut draft, id, faces)?,
                        MeshAction::SetMeshShading { id, shading } => mesh_actions::set_shading(&mut draft, id, shading)?,
                        MeshAction::ProjectMeshUvs { id, direction, up, scale, offset } => {
                            mesh_actions::project_uvs(&mut draft, id, direction, up, scale, offset)?;
                        }
                        MeshAction::SetMeshUvs { id, uvs } => mesh_actions::set_uvs(&mut draft, id, uvs)?,
                        MeshAction::SetMeshTexture { id, png_base64 } => mesh_actions::set_texture(&mut draft, id, png_base64)?,
                        MeshAction::CreateSectionMesh { name, sections, caps, shading } => {
                            created.push(mesh_sections::create(&mut draft, name, sections, caps, shading)?);
                        }
                        MeshAction::SetMeshSections { id, sections, caps } => mesh_sections::rebuild(&mut draft, id, sections, caps)?,
                    }
                }
                if let Some(id) = created.last() { model::set_selected_exact(&mut draft, vec![*id]); }
                draft.make_undo_redo_snapshot();
                self.tree = draft;
                if let Some(renderer) = &mut self.renderer { renderer.invalidate_scene(); }
                Ok(json!({"created_ids": created, "revision": self.tree.path_version("scene")}))
            }
            Request::GetMesh { id } => mesh_actions::get_mesh(&self.tree, id),
            Request::PreviewMeshSections { sections, caps } => mesh_sections::preview(sections, caps),
            Request::SetCaptureClean { clean } => {
                self.ui.set_agent_capture_clean(clean);
                Ok(json!({"ok": true}))
            }
            Request::ExportProject => {
                let bytes = crate::document::serialize_scene(&self.project_tree_for_save())?;
                if bytes.len() > 32 * 1024 * 1024 {
                    return Err(format!(
                        "project is {} bytes; WebMCP export supports at most 32 MiB ({} bytes)",
                        bytes.len(), 32 * 1024 * 1024
                    ));
                }
                Ok(json!({
                    "format": "claydash",
                    "byte_length": bytes.len(),
                    "data_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
                }))
            }
            Request::OpenProject { data_base64 } => {
                if data_base64.len() > 45_000_000 {
                    return Err("project exceeds the 32 MiB upload limit".into());
                }
                let bytes = base64::engine::general_purpose::STANDARD.decode(data_base64)
                    .map_err(|error| format!("invalid project base64: {error}"))?;
                if bytes.len() > 32 * 1024 * 1024 {
                    return Err("project exceeds the 32 MiB upload limit".into());
                }
                let scene = crate::document::deserialize_scene(&bytes)?;
                self.replace_scene(scene);
                self.document.start_new();
                self.document.mark_scene_clean(&self.tree);
                if let Some(active) = model::active_camera_id(&self.tree) {
                    if let Some(camera) = model::scene_cameras(&self.tree).into_iter()
                        .find(|camera| camera.uuid == active) {
                        camera.apply_to_view(&mut self.camera);
                    }
                }
                if let Some(renderer) = &mut self.renderer { renderer.invalidate_scene(); }
                Ok(json!({"objects": model::objects_ref(&self.tree).len()}))
            }
            Request::SetView { position, target, up } => {
                let position = glam::Vec3::from_array(position);
                let target = glam::Vec3::from_array(target);
                let up = glam::Vec3::from_array(up);
                if !position.is_finite() || !target.is_finite() || !up.is_finite()
                    || position.distance_squared(target) < 1e-8
                    || up.length_squared() < 1e-8
                    || (target - position).cross(up).length_squared() < 1e-8 {
                    return Err("camera position, target, and up must form a finite view".into());
                }
                self.camera.position = position;
                self.camera.target = target;
                self.camera.up = up;
                if let Some(renderer) = &mut self.renderer { renderer.invalidate_scene(); }
                Ok(json!({"ok": true}))
            }
        }
    }

}

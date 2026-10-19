use std::sync::Arc;

#[cfg(all(not(target_arch = "wasm32"), unix))]
pub(crate) mod agent;
mod events;
mod initialization;
mod pointer_input;
mod rendering;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod navigation_profile;
mod mesh_export_options;
#[cfg(not(target_arch = "wasm32"))]
mod mesh_export;
#[cfg(target_arch = "wasm32")]
#[path = "app/web_mesh_export.rs"]
mod mesh_export;
use initialization::data_tree_with_scene;

#[cfg(target_arch = "wasm32")]
use std::sync::mpsc::{channel, Receiver, Sender};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    atomic::Ordering,
    mpsc::{channel, Receiver},
};

use glam::{Vec2, Vec4};
use observable_key_value_tree::ObservableKVTree;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::PhysicalKey,
    window::{Window, WindowId},
};

#[cfg(target_arch = "wasm32")]
use winit::{
    event_loop::ControlFlow,
    platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys, WindowExtWebSys},
};

use crate::{
    camera::Camera,
    commands::{self, Commands},
    document::{ColorTheme, DocumentState, FileMenuAction},
    interactions::InteractionState,
    model::{objects_ref, ClaydashValue, DataTree, EditorState},
    renderer::Renderer,
    ui::UiState,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::duck;

#[cfg(all(not(target_arch = "wasm32"), unix))]
#[derive(Debug)]
enum AppEvent {
    AgentRequest,
}

#[cfg(any(target_arch = "wasm32", not(unix)))]
type AppEvent = ();

#[cfg(not(target_arch = "wasm32"))]
use crate::document;

#[cfg(target_arch = "wasm32")]
enum WebDocumentMessage {
    ImportedSplats { request: u64, name: String, bytes: Vec<u8> },
    DroppedFile { name: String, bytes: Vec<u8> },
    Opened {
        request: u64,
        name: std::path::PathBuf,
        bytes: Vec<u8>,
        apply_example_camera: bool,
    },
    ExampleError {
        request: u64,
        message: String,
    },
    RenderError {
        id: u64,
        video: bool,
        message: String,
    },
    RenderFinished(u64),
}

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    egui: egui::Context,
    #[cfg(not(target_arch = "wasm32"))]
    egui_state: Option<egui_winit::State>,
    #[cfg(target_arch = "wasm32")]
    egui_state: crate::web_input::WebInput,
    tree: DataTree,
    commands: Commands,
    camera: Camera,
    interactions: InteractionState,
    ui: UiState,
    document: DocumentState,
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    agent_requests: Option<Receiver<agent::Inbound>>,
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    agent_proxy: Option<winit::event_loop::EventLoopProxy<AppEvent>>,
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    agent_capture: Option<agent::AgentCapture>,
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    agent_revision: u64,
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    agent_redraw_pending: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pending_render: Option<PendingRender>,
    #[cfg(not(target_arch = "wasm32"))]
    encoding: Option<NativeEncoding>,
    mesh_export: Option<mesh_export::MeshExportJob>,
    #[cfg(not(target_arch = "wasm32"))]
    mesh_export_history: std::collections::VecDeque<mesh_export::ExportRecord>,
    mesh_export_options: Option<mesh_export_options::ExportOptions>,
    #[cfg(target_arch = "wasm32")]
    pending_render: Option<WebPendingRender>,
    #[cfg(target_arch = "wasm32")]
    encoding: Option<WebEncoding>,
    #[cfg(target_arch = "wasm32")]
    next_render_id: u64,
    discard_capture: bool,
    #[cfg(not(target_arch = "wasm32"))]
    guide_screenshot: Option<std::path::PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    guide_capture_done: bool,
    window_focused: bool,
    window_occluded: bool,
    #[cfg(not(target_arch = "wasm32"))]
    next_refinement_redraw: std::time::Instant,
    #[cfg(not(target_arch = "wasm32"))]
    use_bvh: bool,
    #[cfg(not(target_arch = "wasm32"))]
    benchmark: bool,
    #[cfg(not(target_arch = "wasm32"))]
    ui_benchmark: Option<UiBenchmark>,
    #[cfg(target_arch = "wasm32")]
    renderer_tx: Sender<Renderer>,
    #[cfg(target_arch = "wasm32")]
    renderer_rx: Receiver<Renderer>,
    #[cfg(target_arch = "wasm32")]
    document_tx: Sender<WebDocumentMessage>,
    #[cfg(target_arch = "wasm32")]
    document_rx: Receiver<WebDocumentMessage>,
    #[cfg(target_arch = "wasm32")]
    document_request: u64,
    #[cfg(target_arch = "wasm32")]
    web_loading_complete: bool,
}

#[cfg(all(not(target_arch = "wasm32"), unix))]
impl Drop for App {
    fn drop(&mut self) {
        if self.agent_requests.is_some() {
            agent::cleanup_socket();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
enum PendingRender {
    Still {
        path: std::path::PathBuf,
    },
    Video {
        path: std::path::PathBuf,
        frames_directory: std::path::PathBuf,
        start_frame: u32,
        next_frame: u32,
        end_frame: u32,
        output_index: u32,
        fps: f32,
        restore_frame: f32,
    },
}

#[cfg(not(target_arch = "wasm32"))]
struct NativeEncoding {
    control: Arc<crate::render_export::EncodingControl>,
    completed: Receiver<Result<(), String>>,
    total: u32,
    cancelling: bool,
    format: crate::document::RenderFormat,
}

#[cfg(target_arch = "wasm32")]
enum WebPendingRender {
    Still {
        name: String,
    },
    Video {
        name: String,
        encoder: Option<crate::render_export::WebVideo>,
        start_frame: u32,
        next_frame: u32,
        end_frame: u32,
        fps: f32,
        restore_frame: f32,
    },
}

#[cfg(target_arch = "wasm32")]
struct WebEncoding {
    id: u64,
    cancel: crate::render_export::WebCancel,
}

#[cfg(not(target_arch = "wasm32"))]
struct UiBenchmark {
    pan_camera: bool,
    edit_objects: bool,
    move_group: bool,
    animation: bool,
    selection_click: bool,
    preview_pixels: Vec<u32>,
    frames: usize,
    last_frame: Option<std::time::Instant>,
    samples: Vec<f64>,
    presented_samples: Vec<f64>,
    last_presented: Option<std::time::Instant>,
}

impl App {
    fn pointer_over_ui(&self, position: Vec2, egui_consumed: bool) -> bool {
        self.ui
            .contains_pointer(position, self.egui.pixels_per_point())
            || (egui_consumed && self.ui.overlay_captures_pointer(&self.egui))
    }

    fn project_tree_for_save(&self) -> DataTree {
        let mut tree = self.tree.clone();
        if let Some(renderer) = &self.renderer {
            let source = objects_ref(&self.tree);
            let objects = renderer.optimized_objects_for_save(source, rendering::mesh_source_revision(source));
            crate::model::set_objects(&mut tree, objects);
        }
        tree
    }

    fn cancel_document_work(&mut self) {
        self.mesh_export_options = None;
        self.cancel_render();
        self.cancel_mesh_export_for_document_change();
        // The worker retains its cancellation token. Its eventual completion
        // must neither block a new render nor report errors on the new document.
        self.encoding = None;
        if let Some(renderer) = &mut self.renderer {
            renderer.reset_optimized_fields();
            renderer.invalidate_scene();
        }
        // Renderer reset detaches old capture callbacks entirely.
        self.discard_capture = false;
    }

    fn replace_scene(&mut self, scene: DataTree) {
        self.cancel_document_work();
        self.tree = data_tree_with_scene(scene);
        self.interactions = InteractionState::default();
        self.ui.reset_document_gestures();
        self.clear_document_egui_state();
        self.ui.reset_animation(&self.tree);
    }

    fn import_gaussian_ply(&mut self, name: &str, bytes: &[u8]) -> Result<uuid::Uuid, String> {
        let cloud = claydash_engine::gaussian_ply::read(bytes)?;
        let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        object.name = name.to_owned();
        object.imported_splats = Some(std::sync::Arc::new(cloud));
        let id = object.uuid;
        let mut objects = crate::model::objects(&self.tree);
        objects.push(object);
        self.tree.make_undo_redo_snapshot();
        crate::model::set_objects(&mut self.tree, objects);
        crate::model::set_selected_exact(&mut self.tree, vec![id]);
        Ok(id)
    }

    fn import_image_plane(&mut self, name: &str, bytes: Vec<u8>) -> Result<uuid::Uuid, String> {
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Image exceeds the 16 MB limit".into());
        }
        image::load_from_memory(&bytes).map_err(|error| error.to_string())?;
        let mut object = crate::model::SdfObject::create_image_plane();
        object.name = std::path::Path::new(name).file_stem()
            .unwrap_or_default().to_string_lossy().into_owned();
        let stencil = crate::model::ImageStencil::new(name.to_owned(), bytes, true);
        if let crate::model::SdfParams::BoxParams(params) = &mut object.params {
            params.box_q.x = stencil.size.x * 0.5;
            params.box_q.y = stencil.size.y * 0.5;
        }
        object.image_stencil = Some(stencil);
        let id = object.uuid;
        let mut objects = crate::model::objects(&self.tree);
        objects.push(object);
        self.tree.make_undo_redo_snapshot();
        crate::model::set_objects(&mut self.tree, objects);
        crate::model::set_selected_exact(&mut self.tree, vec![id]);
        Ok(id)
    }

    fn import_glb(&mut self, name: &str, bytes: &[u8]) -> Result<Vec<uuid::Uuid>, String> {
        let gltf = gltf::Gltf::from_slice(bytes).map_err(|error| error.to_string())?;
        let blob = gltf.blob.as_deref().ok_or("GLB has no embedded geometry buffer")?;
        let scene = gltf.default_scene().or_else(|| gltf.scenes().next())
            .ok_or("GLB has no scene")?;
        let mut imported = Vec::new();
        let mut pending = scene.nodes().map(|node| (node, glam::Mat4::IDENTITY)).collect::<Vec<_>>();
        while let Some((node, parent)) = pending.pop() {
            let transform = parent * glam::Mat4::from_cols_array_2d(&node.transform().matrix());
            pending.extend(node.children().map(|child| (child, transform)));
            let Some(mesh) = node.mesh() else { continue };
            for (primitive_index, primitive) in mesh.primitives().enumerate() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let reader = primitive.reader(|buffer| (buffer.index() == 0).then_some(blob));
                let mut positions = reader.read_positions().ok_or("GLB mesh has no positions")?
                    .map(|point| transform.transform_point3(glam::Vec3::from_array(point)))
                    .collect::<Vec<_>>();
                let indices = reader.read_indices().map(|indices| indices.into_u32().collect::<Vec<_>>())
                    .unwrap_or_else(|| (0..positions.len() as u32).collect());
                if positions.len() > 10_000_000 || indices.len() / 3 > 10_000_000
                    || indices.len() % 3 != 0 || indices.iter().any(|index| *index as usize >= positions.len())
                    || positions.iter().any(|position| !position.is_finite()) {
                    return Err("GLB has invalid triangle geometry".into());
                }
                if indices.is_empty() { continue; }
                let (minimum, maximum) = positions.iter().fold(
                    (glam::Vec3::splat(f32::INFINITY), glam::Vec3::splat(f32::NEG_INFINITY)),
                    |(minimum, maximum), point| (minimum.min(*point), maximum.max(*point)),
                );
                let center = (minimum + maximum) * 0.5;
                for position in &mut positions { *position -= center; }
                let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
                object.transform.translation = center;
                object.name = mesh.name().or_else(|| node.name()).map(str::to_owned)
                    .unwrap_or_else(|| format!("{name} mesh {}", primitive_index + 1));
                object.render_representation = crate::model::GroupRenderRepresentation::DirectMesh;
                let reflected = transform.determinant() < 0.0;
                let triangles = indices.chunks_exact(3).map(|face| {
                    if reflected { [face[0], face[2], face[1]] }
                    else { [face[0], face[1], face[2]] }
                })
                    .collect::<Vec<_>>();
                let owners = vec![object.uuid; triangles.len()];
                object.imported_mesh = Some(Arc::new(
                    crate::renderer::poisson_mesh::geometry::Mesh::from_parts(positions, triangles, owners)
                ));
                let color = primitive.material().pbr_metallic_roughness().base_color_factor();
                object.color = glam::Vec4::from_array(color);
                imported.push(object);
            }
        }
        if imported.is_empty() { return Err("GLB contains no triangle meshes".into()); }
        let ids = imported.iter().map(|object| object.uuid).collect::<Vec<_>>();
        let mut objects = crate::model::objects(&self.tree);
        objects.extend(imported);
        self.tree.make_undo_redo_snapshot();
        crate::model::set_objects(&mut self.tree, objects);
        crate::model::set_selected_exact(&mut self.tree, ids.clone());
        Ok(ids)
    }

    fn import_project_contents(&mut self, bytes: &[u8]) -> Result<Vec<uuid::Uuid>, String> {
        let scene = crate::document::deserialize_scene(bytes)?;
        let mut source = DataTree::default();
        source.set_tree("scene", scene);
        let mut incoming = crate::model::objects(&source);
        let mut cameras = crate::model::scene_cameras(&source);
        let mut materials = crate::model::material_assets(&source);
        if incoming.is_empty() && cameras.is_empty() {
            return Err("Project contains no objects or cameras".into());
        }
        let ids = incoming.iter().map(|object| object.uuid)
            .chain(cameras.iter().map(|camera| camera.uuid))
            .chain(materials.iter().map(|material| material.uuid))
            .map(|id| (id, uuid::Uuid::new_v4()))
            .collect::<std::collections::HashMap<_, _>>();
        let selected = incoming.iter().map(|object| ids[&object.uuid])
            .chain(cameras.iter().map(|camera| ids[&camera.uuid])).collect::<Vec<_>>();
        for object in &mut incoming {
            let old_id = object.uuid;
            object.uuid = ids[&old_id];
            object.boolean_parent = object.boolean_parent.and_then(|id| ids.get(&id).copied());
            object.material_id = object.material_id.and_then(|id| ids.get(&id).copied());
            if let Some(inlay) = &mut object.surface_inlay {
                if let Some(id) = ids.get(&inlay.host) { inlay.host = *id; }
                else { object.surface_inlay = None; }
            }
            if let Some(extrusion) = &mut object.path_extrusion {
                extrusion.profile_curve = extrusion.profile_curve.and_then(|id| ids.get(&id).copied());
            }
            match &mut object.params {
                crate::model::SdfParams::TextParams(params) => {
                    params.path = params.path.and_then(|id| ids.get(&id).copied());
                }
                crate::model::SdfParams::SheetPanelParams(params) => {
                    if let Some(seam) = &mut params.seam {
                        if let Some(id) = ids.get(&seam.source) { seam.source = *id; }
                        else { params.seam = None; }
                    }
                }
                _ => {}
            }
            if let Some(mesh) = &mut object.imported_mesh {
                let mesh = Arc::make_mut(mesh);
                for owner in &mut mesh.owners {
                    if let Some(id) = ids.get(owner) { *owner = *id; }
                }
            }
            object.saved_mesh = None;
            object.saved_group_capture = None;
            object.saved_neural_field = None;
        }
        for camera in &mut cameras { camera.uuid = ids[&camera.uuid]; }
        for material in &mut materials { material.uuid = ids[&material.uuid]; }
        let mut objects = crate::model::objects(&self.tree);
        objects.extend(incoming);
        let mut destination_cameras = crate::model::scene_cameras(&self.tree);
        destination_cameras.extend(cameras);
        let mut destination_materials = crate::model::material_assets(&self.tree);
        destination_materials.extend(materials);
        self.tree.make_undo_redo_snapshot();
        crate::model::set_material_assets(&mut self.tree, destination_materials);
        crate::model::set_objects(&mut self.tree, objects);
        crate::model::set_scene_cameras(&mut self.tree, destination_cameras);
        crate::model::set_selected_exact(&mut self.tree, selected.clone());
        Ok(selected)
    }

    fn handle_dropped_bytes(&mut self, name: &str, bytes: Vec<u8>) {
        let extension = std::path::Path::new(name).extension()
            .and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
        let result = match extension.as_str() {
            "ply" => self.import_gaussian_ply(
                std::path::Path::new(name).file_stem().and_then(|stem| stem.to_str()).unwrap_or(name),
                &bytes,
            ).map(|_| ()),
            "png" | "jpg" | "jpeg" | "webp" => self.import_image_plane(name, bytes).map(|_| ()),
            "claydash" => self.import_project_contents(&bytes).map(|_| ()),
            "glb" => self.import_glb(name, &bytes).map(|_| ()),
            _ => return,
        };
        if let Err(error) = result {
            self.document.set_error(&format!("drop {name}"), error);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn install_web_file_drop(&self, _window: &Window) {
        use wasm_bindgen::JsCast;
        let Some(document) = web_sys::window().and_then(|window| window.document()) else { return };
        let dragover = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::DragEvent)>::new(
            |event: web_sys::DragEvent| event.prevent_default(),
        );
        document.add_event_listener_with_callback("dragover", dragover.as_ref().unchecked_ref()).ok();
        dragover.forget();
        let tx = self.document_tx.clone();
        let ctx = self.egui.clone();
        let drop = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::DragEvent)>::new(move |event: web_sys::DragEvent| {
            event.prevent_default();
            let Some(files) = event.data_transfer().and_then(|transfer| transfer.files()) else { return };
            for index in 0..files.length() {
                let Some(file) = files.get(index) else { continue };
                let tx = tx.clone();
                let ctx = ctx.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let name = file.name();
                    let promise = file.array_buffer();
                    let Ok(buffer) = wasm_bindgen_futures::JsFuture::from(promise).await else { return };
                    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                    let _ = tx.send(WebDocumentMessage::DroppedFile { name, bytes });
                    ctx.request_repaint();
                });
            }
        });
        document.add_event_listener_with_callback("drop", drop.as_ref().unchecked_ref()).ok();
        drop.forget();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn handle_dropped_path(&mut self, path: &std::path::Path) {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return };
        match std::fs::read(path) {
            Ok(bytes) => self.handle_dropped_bytes(name, bytes),
            Err(error) => self.document.set_error("read dropped file", error.to_string()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn import_gaussian_path(&mut self, path: &std::path::Path) -> Result<uuid::Uuid, String> {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let name = path.file_stem().unwrap_or_default().to_string_lossy();
        self.import_gaussian_ply(&name, &bytes)
    }

    fn clear_scene(&mut self) {
        self.cancel_document_work();
        self.tree.make_undo_redo_snapshot();
        crate::model::set_objects(&mut self.tree, Vec::new());
        crate::model::set_scene_cameras(&mut self.tree, Vec::new());
        self.tree
            .set_path("scene.active_camera", ClaydashValue::None);
        self.tree.set_path("scene.animation", ClaydashValue::None);
        self.tree.set_path(
            "scene.cursor_position",
            ClaydashValue::Vec3(glam::Vec3::ZERO),
        );
        self.tree.set_path(
            "scene.world",
            ClaydashValue::World(crate::model::World {
                background: crate::model::BackgroundMode::Studio,
                ambient_light: 0.0,
                ..Default::default()
            }),
        );
        crate::model::set_selected(&mut self.tree, Vec::new());
        self.tree.set_transient_path(
            "editor.state",
            ClaydashValue::EditorState(EditorState::Start),
        );
        self.tree
            .set_transient_path("editor.place_cursor", ClaydashValue::Bool(false));
        self.tree.make_undo_redo_snapshot();
        self.interactions = InteractionState::default();
        self.ui.reset_document_gestures();
        self.clear_document_egui_state();
        self.ui.reset_animation(&self.tree);
        self.document.clear_error();
    }

    fn clear_document_egui_state(&self) {
        self.egui.data_mut(|data| {
            data.remove_temp::<crate::renderer::computation::Statuses>(crate::renderer::computation::status_id());
            data.remove_temp::<Vec<crate::renderer::voxels::VoxelAction>>(
                egui::Id::new("voxel-actions"));
            data.remove_temp::<std::collections::HashMap<uuid::Uuid,
                crate::renderer::voxels::VoxelStatus>>(egui::Id::new("voxel-status"));
            data.remove_temp::<Vec<crate::renderer::poisson_mesh::PoissonMeshAction>>(
                egui::Id::new("poisson-mesh-actions"));
            data.remove_temp::<std::collections::HashSet<uuid::Uuid>>(
                egui::Id::new("group-optimization-recompute"));
            data.remove_temp::<std::collections::HashSet<uuid::Uuid>>(
                egui::Id::new("group-optimization-cancel"));
            data.remove_temp::<std::collections::HashMap<uuid::Uuid,
                crate::renderer::poisson_mesh::PoissonMeshStatus>>(
                egui::Id::new("poisson-mesh-status"));
            data.remove_temp::<std::collections::HashMap<uuid::Uuid,
                crate::renderer::NeuralStatus>>(egui::Id::new("neural-sdf-status"));
            data.remove_temp::<std::collections::HashMap<uuid::Uuid,
                crate::model::NeuralTrainingSettings>>(
                egui::Id::new("neural-sdf-applied-settings"));
            data.remove_temp::<std::collections::HashMap<uuid::Uuid, u64>>(
                egui::Id::new("depth-accelerator-ready"));
            data.remove_temp::<std::collections::HashMap<uuid::Uuid, (i32, u32)>>(
                egui::Id::new("depth-accelerator-progress"));
        });
    }

    fn new_document(&mut self) {
        let mut scene = DataTree::default();
        scene.set_path("sdf_objects", ClaydashValue::VecSDFObject(Vec::new()));
        self.replace_scene(scene);
        self.document.start_new();
        self.document.mark_scene_clean(&self.tree);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_path(&mut self, path: std::path::PathBuf) {
        match document::read_scene(&path) {
            Ok(scene) => {
                self.replace_scene(scene);
                self.document.mark_opened(path);
                self.document.mark_scene_clean(&self.tree);
            }
            Err(error) => self.document.set_error("open the project", error),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_path(&mut self, path: std::path::PathBuf) {
        match document::write_scene(&path, &self.project_tree_for_save()) {
            Ok(()) => {
                self.document.mark_saved(path);
                self.document.mark_scene_clean(&self.tree);
            }
            Err(error) => self.document.set_error("save the project", error),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn handle_file_action(&mut self, action: FileMenuAction) {
        match action {
            FileMenuAction::ImportGaussianPly => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Gaussian PLY", &["ply"]).pick_file() {
                    if let Err(error) = self.import_gaussian_path(&path) { self.document.set_error("import Gaussian PLY", error); }
                }
            }
            FileMenuAction::New => self.new_document(),
            FileMenuAction::Clear => self.clear_scene(),
            FileMenuAction::Open => {
                if let Some(path) = document::open_dialog() {
                    self.open_path(path);
                }
            }
            FileMenuAction::OpenExample(example) => {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("examples")
                    .join(example.file);
                match document::read_scene(&path) {
                    Ok(scene) => {
                        self.replace_scene(scene);
                        if let Some(active) = crate::model::active_camera_id(&self.tree) {
                            if let Some(camera) = crate::model::scene_cameras(&self.tree)
                                .into_iter()
                                .find(|camera| camera.uuid == active)
                            {
                                camera.apply_to_view(&mut self.camera);
                            }
                        }
                        self.document.start_new();
                        self.document.mark_scene_clean(&self.tree);
                    }
                    Err(error) => self.document.set_error("open the example", error),
                }
            }
            FileMenuAction::Guide => {
                if let Err(error) = crate::examples::open_guide() {
                    self.document.set_error("open the guide", error);
                }
            }
            FileMenuAction::OpenRecent(path) => self.open_path(path),
            FileMenuAction::Save => {
                if let Some(path) = self.document.current_path().map(std::path::Path::to_owned) {
                    self.save_path(path);
                } else if let Some(path) = document::save_dialog() {
                    self.save_path(path);
                }
            }
            FileMenuAction::SaveAs => {
                if let Some(path) = document::save_dialog() {
                    self.save_path(path);
                }
            }
            FileMenuAction::ExportGlb => self.request_mesh_export(),
            FileMenuAction::Render(format) => {
                if self.pending_render.is_some() || self.encoding.is_some() {
                    return;
                }
                if let Some(path) = document::render_dialog(format) {
                    self.pending_render = Some(match format {
                        crate::document::RenderFormat::WebP => PendingRender::Still { path },
                        crate::document::RenderFormat::Mp4 => {
                            let animation = crate::animation::animation_data(&self.tree);
                            let restore_frame = self.ui.animation_frame();
                            self.ui
                                .set_animation_frame(&mut self.tree, animation.start_frame as f32);
                            if matches!(
                                self.tree.get_path("editor.camera_view"),
                                ClaydashValue::Bool(true)
                            ) {
                                if let Some(active) = crate::model::active_camera_id(&self.tree) {
                                    if let Some(camera) = crate::model::scene_cameras(&self.tree)
                                        .iter()
                                        .find(|camera| camera.uuid == active)
                                    {
                                        camera.apply_to_view(&mut self.camera);
                                    }
                                }
                            }
                            PendingRender::Video {
                                path,
                                frames_directory: std::env::temp_dir()
                                    .join(format!("claydash-video-{}", uuid::Uuid::new_v4())),
                                start_frame: animation.start_frame,
                                next_frame: animation.start_frame,
                                end_frame: animation.end_frame.max(animation.start_frame),
                                output_index: 0,
                                fps: animation.fps,
                                restore_frame,
                            }
                        }
                    });
                }
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn handle_file_action(&mut self, action: FileMenuAction) {
        if matches!(
            action,
            FileMenuAction::New
                | FileMenuAction::Clear
                | FileMenuAction::Open
                | FileMenuAction::OpenExample(_)
        ) {
            self.document_request = self.document_request.wrapping_add(1);
        }
        let request = self.document_request;
        match action {
            FileMenuAction::ImportGaussianPly => {
                let tx = self.document_tx.clone();
                let ctx = self.egui.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Some(file) = rfd::AsyncFileDialog::new().add_filter("Gaussian PLY", &["ply"]).pick_file().await {
                        let name = file.file_name();
                        let bytes = file.read().await;
                        let _ = tx.send(WebDocumentMessage::ImportedSplats { request, name, bytes });
                        ctx.request_repaint();
                    }
                });
            }
            FileMenuAction::ExportGlb => self.request_mesh_export(),
            FileMenuAction::Guide => crate::examples::open_guide(),
            FileMenuAction::OpenExample(example) => {
                let tx = self.document_tx.clone();
                let ctx = self.egui.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let message = match crate::examples::fetch_example(example.file).await {
                        Ok(bytes) => WebDocumentMessage::Opened {
                            request,
                            name: example.file.into(),
                            bytes: bytes.to_vec(),
                            apply_example_camera: true,
                        },
                        Err(error) => WebDocumentMessage::ExampleError {
                            request,
                            message: format!("{}: {error:?}", example.title),
                        },
                    };
                    let _ = tx.send(message);
                    ctx.request_repaint();
                });
            }
            FileMenuAction::New => self.new_document(),
            FileMenuAction::Clear => self.clear_scene(),
            FileMenuAction::Open => {
                let tx = self.document_tx.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let file = rfd::AsyncFileDialog::new()
                        .add_filter("Claydash project", &["claydash"])
                        .pick_file()
                        .await;
                    if let Some(file) = file {
                        let name = std::path::PathBuf::from(file.file_name());
                        let bytes = file.read().await;
                        let _ = tx.send(WebDocumentMessage::Opened {
                            request,
                            name,
                            bytes,
                            apply_example_camera: false,
                        });
                    }
                });
            }
            FileMenuAction::Save | FileMenuAction::SaveAs => {
                let file_name = self
                    .document
                    .current_path()
                    .and_then(std::path::Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "untitled.claydash".to_string());
                self.download_web_project(file_name);
            }
            FileMenuAction::SaveNamed(file_name) => self.download_web_project(file_name),
            FileMenuAction::OpenRecent(_) => {}
            FileMenuAction::Render(crate::document::RenderFormat::WebP) => {
                if self.pending_render.is_some() || self.encoding.is_some() {
                    return;
                }
                let stem = self
                    .document
                    .current_path()
                    .and_then(std::path::Path::file_stem)
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "render".to_owned());
                self.pending_render = Some(WebPendingRender::Still {
                    name: format!("{stem}.webp"),
                });
            }
            FileMenuAction::Render(crate::document::RenderFormat::Mp4) => {
                if self.pending_render.is_some() || self.encoding.is_some() {
                    return;
                }
                let stem = self
                    .document
                    .current_path()
                    .and_then(std::path::Path::file_stem)
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "render".to_owned());
                let animation = crate::animation::animation_data(&self.tree);
                let fps = if animation.fps.is_finite() {
                    animation.fps.clamp(1.0, 240.0)
                } else {
                    24.0
                };
                let restore_frame = self.ui.animation_frame();
                self.ui
                    .set_animation_frame(&mut self.tree, animation.start_frame as f32);
                if matches!(
                    self.tree.get_path("editor.camera_view"),
                    ClaydashValue::Bool(true)
                ) {
                    if let Some(active) = crate::model::active_camera_id(&self.tree) {
                        if let Some(camera) = crate::model::scene_cameras(&self.tree)
                            .iter()
                            .find(|camera| camera.uuid == active)
                        {
                            camera.apply_to_view(&mut self.camera);
                        }
                    }
                }
                self.pending_render = Some(WebPendingRender::Video {
                    name: format!("{stem}.mp4"),
                    encoder: None,
                    start_frame: animation.start_frame,
                    next_frame: animation.start_frame,
                    end_frame: animation.end_frame.max(animation.start_frame),
                    fps,
                    restore_frame,
                });
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn download_web_project(&mut self, file_name: String) {
        let bytes = match crate::document::serialize_scene(&self.project_tree_for_save()) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.document.set_error("save the project", error);
                return;
            }
        };
        match crate::document::download_bytes(&file_name, &bytes) {
            Ok(()) => {
                self.document
                    .mark_saved(std::path::PathBuf::from(file_name));
                self.document.mark_scene_clean(&self.tree);
            }
            Err(error) => self.document.set_error("save the project", error),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn process_web_document_messages(&mut self) {
        while let Ok(message) = self.document_rx.try_recv() {
            match message {
                WebDocumentMessage::DroppedFile { name, bytes } => {
                    self.handle_dropped_bytes(&name, bytes);
                }
                WebDocumentMessage::ImportedSplats { request, name, bytes } => {
                    if request == self.document_request {
                        if let Err(error) = self.import_gaussian_ply(&name, &bytes) {
                            self.document.set_error("import Gaussian PLY", error);
                        }
                    }
                }
                WebDocumentMessage::Opened {
                    request,
                    name,
                    bytes,
                    apply_example_camera,
                } => {
                    if request != self.document_request {
                        continue;
                    }
                    match crate::document::deserialize_scene(&bytes) {
                        Ok(scene) => {
                            self.replace_scene(scene);
                            if apply_example_camera {
                                if let Some(active) = crate::model::active_camera_id(&self.tree) {
                                    if let Some(camera) = crate::model::scene_cameras(&self.tree)
                                        .into_iter()
                                        .find(|camera| camera.uuid == active)
                                    {
                                        camera.apply_to_view(&mut self.camera);
                                    }
                                }
                            }
                            self.document.mark_opened(name);
                            self.document.mark_scene_clean(&self.tree);
                        }
                        Err(error) => self.document.set_error("open the project", error),
                    }
                }
                WebDocumentMessage::ExampleError { request, message } => {
                    if request == self.document_request {
                        self.document.set_error("open the example", message);
                    }
                }
                WebDocumentMessage::RenderFinished(id) => {
                    if self.encoding.as_ref().is_some_and(|job| job.id == id) {
                        self.encoding = None;
                    }
                }
                WebDocumentMessage::RenderError { id, video, message } => {
                    if self.encoding.as_ref().is_some_and(|job| job.id == id) {
                        self.encoding = None;
                        self.document.set_error(
                            if video {
                                "render the animation"
                            } else {
                                "render the scene"
                            },
                            message,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod file_action_tests {
    use super::*;

    #[test]
    fn initial_box_uses_optimized_mesh() {
        let app = App::new();
        let objects = objects_ref(&app.tree);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].render_representation,
            crate::model::GroupRenderRepresentation::DirectMesh);
    }

    fn triangle_glb() -> Vec<u8> {
        let mut binary = Vec::new();
        for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            binary.extend_from_slice(&value.to_le_bytes());
        }
        let json = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0, "translation": [2, 0, 0]}],
            "meshes": [{"name": "Triangle", "primitives": [{"attributes": {"POSITION": 0}}]}],
            "buffers": [{"byteLength": binary.len()}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": binary.len()}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0,0,0], "max": [1,1,0]}]
        }).to_string();
        let mut json = json.into_bytes();
        json.resize((json.len() + 3) & !3, b' ');
        let total = 12 + 8 + json.len() + 8 + binary.len();
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(b"glTF");
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&(total as u32).to_le_bytes());
        bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"JSON");
        bytes.extend_from_slice(&json);
        bytes.extend_from_slice(&(binary.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"BIN\0");
        bytes.extend_from_slice(&binary);
        bytes
    }

    #[test]
    fn dropped_glb_creates_persistent_transformable_mesh() {
        let mut app = App::new();
        let id = app.import_glb("triangle.glb", &triangle_glb()).unwrap()[0];
        let object = crate::model::objects(&app.tree).into_iter().find(|object| object.uuid == id).unwrap();
        let mesh = object.imported_mesh.as_ref().unwrap();
        assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
        assert_eq!(mesh.positions[0], glam::Vec3::new(-0.5, -0.5, 0.0));
        assert_eq!(object.transform.translation, glam::Vec3::new(2.5, 0.5, 0.0));
        assert_eq!(crate::model::raymarch(
            glam::Vec3::new(2.2, 0.2, 1.0), glam::Vec3::NEG_Z, &[object.clone()]), Some(id));
        let duplicate = object.duplicate();
        assert_eq!(duplicate.imported_mesh.unwrap().owners, vec![duplicate.uuid]);
        let bytes = crate::document::serialize_scene(&app.tree).unwrap();
        let scene = crate::document::deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);
        assert_eq!(crate::model::objects(&restored).into_iter().find(|object| object.uuid == id)
            .unwrap().imported_mesh.unwrap().triangles, vec![[0, 1, 2]]);
    }

    #[test]
    fn dropped_image_uses_pixel_aspect_ratio_for_plane() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(4, 2).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut app = App::new();
        let id = app.import_image_plane("wide.png", png).unwrap();
        let object = crate::model::objects(&app.tree).into_iter().find(|object| object.uuid == id).unwrap();
        let stencil = object.image_stencil.unwrap();
        assert!(stencil.image_plane);
        assert_eq!(stencil.size, glam::Vec2::new(1.0, 0.5));
        let crate::model::SdfParams::BoxParams(params) = object.params else { panic!("image plane must be a box") };
        assert_eq!(params.box_q.x, 0.5);
        assert_eq!(params.box_q.y, 0.25);
    }

    #[test]
    fn dropped_ply_adds_selected_splat_asset() {
        let names = ["x", "y", "z", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2", "rot_3", "f_dc_0", "f_dc_1", "f_dc_2", "opacity"];
        let ply = format!("ply\nformat ascii 1.0\nelement vertex 1\n{}end_header\n0 0 0 -2 -3 -4 1 0 0 0 1 0 0 3\n",
            names.map(|name| format!("property float {name}\n")).join(""));
        let mut app = App::new();
        let before = objects_ref(&app.tree).len();
        app.handle_dropped_bytes("cloud.PLY", ply.into_bytes());
        let objects = objects_ref(&app.tree);
        assert_eq!(objects.len(), before + 1);
        assert_eq!(objects.last().unwrap().imported_splats.as_ref().unwrap().splats.len(), 1);
        assert_eq!(crate::model::selected_ref(&app.tree), &[objects.last().unwrap().uuid]);
    }

    #[test]
    fn dropped_project_merges_objects_without_replacing_current_document() {
        let mut app = App::new();
        let old_objects = objects_ref(&app.tree).to_vec();
        let old_path = app.document.current_path().map(std::path::Path::to_path_buf);
        let mut scene = DataTree::default();
        let parent = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        let mut child = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        child.boolean_parent = Some(parent.uuid);
        scene.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(vec![parent, child]));
        let bytes = crate::document::serialize_scene(&scene).unwrap();
        app.handle_dropped_bytes("empty.claydash", bytes);
        let objects = objects_ref(&app.tree);
        assert_eq!(objects.len(), old_objects.len() + 2);
        assert_eq!(objects[0].uuid, old_objects[0].uuid);
        assert_eq!(objects.last().unwrap().boolean_parent, Some(objects[objects.len() - 2].uuid));
        assert_eq!(app.document.current_path(), old_path.as_deref());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn opening_suspension_example_has_no_camera() {
        let mut app = App::new();
        let example = crate::examples::EXAMPLES.iter()
            .find(|example| example.file == "single_wheel_suspension.claydash").unwrap();
        app.handle_file_action(FileMenuAction::OpenExample(*example));
        assert!(crate::model::scene_cameras(&app.tree).is_empty());
        assert!(crate::model::active_camera_id(&app.tree).is_none());
        assert!(app.document.current_path().is_none());
    }

    #[test]
    fn opening_another_scene_discards_old_document_actions() {
        let mut app = App::new();
        let old = objects_ref(&app.tree)[0].uuid;
        app.tree.set_path("scene.old_document_marker", ClaydashValue::Bool(true));
        app.tree.set_transient_path("editor.old_document_marker", ClaydashValue::Bool(true));
        app.tree.make_undo_redo_snapshot();
        app.tree.set_path("scene.pending_old_marker", ClaydashValue::Bool(true));
        app.tree.undo(); // Leave old forward history as well as a pending edit.
        app.tree.set_path("scene.pending_old_marker", ClaydashValue::Bool(true));
        app.egui.data_mut(|data| {
            data.insert_temp(egui::Id::new("voxel-actions"),
                vec![crate::renderer::voxels::VoxelAction::Build(old)]);
            data.insert_temp(egui::Id::new("voxel-status"),
                std::collections::HashMap::from([(old,
                    crate::renderer::voxels::VoxelStatus::Sampling { percent: 1 })]));
            data.insert_temp(egui::Id::new("poisson-mesh-actions"),
                vec![crate::renderer::poisson_mesh::PoissonMeshAction::Build(old)]);
            data.insert_temp(egui::Id::new("group-optimization-recompute"),
                std::collections::HashSet::from([old]));
            data.insert_temp(egui::Id::new("group-optimization-cancel"),
                std::collections::HashSet::from([old]));
            data.insert_temp(egui::Id::new("neural-sdf-status"),
                std::collections::HashMap::<uuid::Uuid, crate::renderer::NeuralStatus>::new());
            data.insert_temp(egui::Id::new("depth-accelerator-ready"),
                std::collections::HashMap::from([(old, 1u64)]));
            data.insert_temp(egui::Id::new("poisson-mesh-status"),
                std::collections::HashMap::from([(old,
                    crate::renderer::poisson_mesh::PoissonMeshStatus::Sampling {
                        percent: 1,
                    })]));
        });
        let mut next = DataTree::default();
        let replacement = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        let replacement_id = replacement.uuid;
        next.set_path("sdf_objects", ClaydashValue::VecSDFObject(vec![replacement]));
        app.replace_scene(next);
        assert_eq!(objects_ref(&app.tree)[0].uuid, replacement_id);
        assert!(matches!(app.tree.get_path("scene.old_document_marker"), ClaydashValue::None));
        assert!(matches!(app.tree.get_path("editor.old_document_marker"), ClaydashValue::None));
        let scene_bytes = crate::document::serialize_scene(&app.tree).unwrap();
        for _ in 0..3 {
            app.tree.undo();
            app.tree.redo();
            assert_eq!(crate::document::serialize_scene(&app.tree).unwrap(), scene_bytes);
        }
        assert!(app.tree.get_tree("scene.old_document_marker").is_none());
        assert!(app.tree.get_tree("scene.pending_old_marker").is_none());
        assert!(app.tree.get_tree("editor.old_document_marker").is_none());
        app.egui.data(|data| {
            assert!(data.get_temp::<Vec<crate::renderer::voxels::VoxelAction>>(
                egui::Id::new("voxel-actions")).is_none());
            assert!(data.get_temp::<std::collections::HashMap<uuid::Uuid,
                crate::renderer::voxels::VoxelStatus>>(egui::Id::new("voxel-status")).is_none());
            assert!(data.get_temp::<Vec<crate::renderer::poisson_mesh::PoissonMeshAction>>(
                egui::Id::new("poisson-mesh-actions")).is_none());
            for key in ["group-optimization-recompute", "group-optimization-cancel"] {
                assert!(data.get_temp::<std::collections::HashSet<uuid::Uuid>>(
                    egui::Id::new(key)).is_none());
            }
            assert!(data.get_temp::<std::collections::HashMap<uuid::Uuid,
                crate::renderer::NeuralStatus>>(egui::Id::new("neural-sdf-status")).is_none());
            assert!(data.get_temp::<std::collections::HashMap<uuid::Uuid, u64>>(
                egui::Id::new("depth-accelerator-ready")).is_none());
            assert!(data.get_temp::<std::collections::HashMap<uuid::Uuid,
                crate::renderer::poisson_mesh::PoissonMeshStatus>>(
                egui::Id::new("poisson-mesh-status")).is_none());
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn switching_scenes_cancels_and_detaches_pending_renders() {
        let mut app = App::new();
        let control = Arc::new(crate::render_export::EncodingControl::default());
        let (sender, completed) = channel();
        app.encoding = Some(NativeEncoding {
            control: control.clone(), completed, total: 1, cancelling: false,
            format: crate::document::RenderFormat::WebP,
        });
        app.pending_render = Some(PendingRender::Still { path: "unused.webp".into() });
        app.discard_capture = true;
        app.replace_scene(DataTree::default());
        assert!(control.cancelled.load(Ordering::Relaxed));
        assert!(app.encoding.is_none());
        assert!(app.pending_render.is_none());
        assert!(!app.discard_capture);
        assert!(sender.send(Err("obsolete encoding error".into())).is_err());

        let frames_directory = std::env::temp_dir().join(format!("claydash-cancel-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&frames_directory).unwrap();
        std::fs::write(frames_directory.join("frame"), b"partial frame").unwrap();
        app.pending_render = Some(PendingRender::Video {
            path: "unused.mp4".into(), frames_directory: frames_directory.clone(),
            start_frame: 0, next_frame: 1, end_frame: 2, output_index: 1,
            fps: 24.0, restore_frame: 0.0,
        });
        app.replace_scene(DataTree::default());
        assert!(app.pending_render.is_none());
        assert!(!frames_directory.exists());
    }

    #[test]
    fn clear_resets_scene_and_lighting_and_is_undoable() {
        let mut app = App::new();
        let objects = objects_ref(&app.tree).to_vec();
        assert!(!objects.is_empty());
        let previous_world = crate::model::World {
            background: crate::model::BackgroundMode::Sky,
            ambient_light: 1.7,
            ..Default::default()
        };
        app.tree
            .set_path("scene.world", ClaydashValue::World(previous_world));
        crate::model::set_selected(&mut app.tree, vec![objects[0].uuid]);
        let path = app
            .document
            .current_path()
            .map(std::path::Path::to_path_buf);

        app.handle_file_action(FileMenuAction::Clear);

        assert!(objects_ref(&app.tree).is_empty());
        assert!(crate::model::selected_ref(&app.tree).is_empty());
        assert!(crate::model::scene_cameras(&app.tree).is_empty());
        assert_eq!(app.document.current_path(), path.as_deref());
        let cleared_world = crate::model::world(&app.tree);
        assert_eq!(
            cleared_world.background,
            crate::model::BackgroundMode::Studio
        );
        assert_eq!(cleared_world.ambient_light, 0.0);
        let bytes = crate::document::serialize_scene(&app.tree).unwrap();
        let scene = crate::document::deserialize_scene(&bytes).unwrap();
        assert!(
            matches!(scene.get_path("world"), ClaydashValue::World(world) if world == cleared_world)
        );
        assert!(
            matches!(scene.get_path("sdf_objects"), ClaydashValue::VecSDFObject(objects) if objects.is_empty())
        );

        app.tree.undo();
        assert_eq!(
            serde_json::to_value(objects_ref(&app.tree)).unwrap(),
            serde_json::to_value(&objects).unwrap()
        );
        assert_eq!(crate::model::world(&app.tree), previous_world);
        app.tree.redo();
        assert!(objects_ref(&app.tree).is_empty());
        assert_eq!(crate::model::world(&app.tree), cleared_world);
    }

    #[test]
    fn new_creates_empty_saveable_scene() {
        let mut app = App::new();
        assert!(!objects_ref(&app.tree).is_empty());

        app.handle_file_action(FileMenuAction::New);

        assert!(objects_ref(&app.tree).is_empty());
        assert!(app.document.current_path().is_none());
        let bytes = crate::document::serialize_scene(&app.tree).unwrap();
        let scene = crate::document::deserialize_scene(&bytes).unwrap();
        assert!(matches!(
            scene.get_path("sdf_objects"),
            ClaydashValue::VecSDFObject(objects) if objects.is_empty()
        ));
    }
}

pub fn run() {
    #[cfg(target_os = "macos")]
    let event_loop = {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        let mut builder = EventLoop::<AppEvent>::with_user_event();
        if std::env::args().any(|argument| {
            argument.starts_with("--guide-screenshot=")
        }) {
            builder.with_activation_policy(ActivationPolicy::Prohibited);
        }
        builder.build().expect("create event loop")
    };
    #[cfg(not(target_os = "macos"))]
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("create event loop");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = App::new();
        #[cfg(unix)]
        {
            app.agent_proxy = Some(event_loop.create_proxy());
        }
        event_loop.run_app(&mut app).expect("run app");
    }
    #[cfg(target_arch = "wasm32")]
    event_loop.spawn_app(App::new());
}

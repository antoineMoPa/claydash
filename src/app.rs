use std::sync::Arc;

#[cfg(all(not(target_arch = "wasm32"), unix))]
pub(crate) mod agent;
mod events;
mod initialization;
mod rendering;
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
    duck,
    interactions::InteractionState,
    model::{objects_ref, ClaydashValue, DataTree, EditorState},
    renderer::Renderer,
    ui::UiState,
};

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
    Opened {
        request: u64,
        name: std::path::PathBuf,
        bytes: Vec<u8>,
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
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    agent_headless: bool,
    window_focused: bool,
    window_occluded: bool,
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
    edit_objects: bool,
    move_group: bool,
    animation: bool,
    selection_click: bool,
    preview_pixels: Vec<u32>,
    frames: usize,
    last_frame: Option<std::time::Instant>,
    samples: Vec<f64>,
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
            let objects = renderer.optimized_objects_for_save(objects_ref(&self.tree));
            crate::model::set_objects(&mut tree, objects);
        }
        tree
    }

    fn replace_scene(&mut self, scene: DataTree) {
        self.cancel_render();
        self.cancel_mesh_export_for_document_change();
        self.tree = data_tree_with_scene(scene);
        self.interactions = InteractionState::default();
        self.ui.reset_document_gestures();
        self.clear_document_egui_state();
        self.ui.reset_animation(&self.tree);
        if let Some(renderer) = &mut self.renderer {
            renderer.reset_optimized_fields();
            renderer.invalidate_scene();
        }
    }

    fn clear_scene(&mut self) {
        self.cancel_render();
        self.cancel_mesh_export_for_document_change();
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
        if let Some(renderer) = &mut self.renderer {
            renderer.reset_optimized_fields();
            renderer.invalidate_scene();
        }
        self.document.clear_error();
    }

    fn clear_document_egui_state(&self) {
        self.egui.data_mut(|data| {
            data.remove_temp::<crate::renderer::computation::Statuses>(crate::renderer::computation::status_id());
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
            FileMenuAction::ExportGlb => self.start_mesh_export(),
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
            FileMenuAction::ExportGlb => self.start_mesh_export(),
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
                WebDocumentMessage::Opened {
                    request,
                    name,
                    bytes,
                } => {
                    if request != self.document_request {
                        continue;
                    }
                    match crate::document::deserialize_scene(&bytes) {
                        Ok(scene) => {
                            self.replace_scene(scene);
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
    fn opening_another_scene_discards_old_document_actions() {
        let mut app = App::new();
        let old = objects_ref(&app.tree)[0].uuid;
        app.tree.set_path("scene.old_document_marker", ClaydashValue::Bool(true));
        app.tree.set_transient_path("editor.old_document_marker", ClaydashValue::Bool(true));
        app.egui.data_mut(|data| {
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
        app.egui.data(|data| {
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
            argument.starts_with("--guide-screenshot=") || argument == "--agent-headless"
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

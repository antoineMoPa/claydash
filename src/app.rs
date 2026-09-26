use std::sync::Arc;

#[cfg(all(not(target_arch = "wasm32"), unix))]
pub(crate) mod agent;
mod events;
mod initialization;
mod rendering;
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

#[cfg(not(target_arch = "wasm32"))]
use crate::document;

#[cfg(target_arch = "wasm32")]
enum WebDocumentMessage {
    Opened {
        name: std::path::PathBuf,
        bytes: Vec<u8>,
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
    agent_capture: Option<std::sync::mpsc::Sender<agent::AgentResult>>,
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    agent_revision: u64,
    #[cfg(not(target_arch = "wasm32"))]
    pending_render: Option<PendingRender>,
    #[cfg(not(target_arch = "wasm32"))]
    encoding: Option<NativeEncoding>,
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

    fn replace_scene(&mut self, scene: DataTree) {
        self.cancel_render();
        self.tree = data_tree_with_scene(scene);
        self.interactions = InteractionState::default();
        self.ui.reset_document_gestures();
        self.ui.reset_animation(&self.tree);
        if let Some(renderer) = &mut self.renderer {
            renderer.invalidate_scene();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_path(&mut self, path: std::path::PathBuf) {
        match document::read_scene(&path) {
            Ok(scene) => {
                self.replace_scene(scene);
                self.document.mark_opened(path);
            }
            Err(error) => self.document.set_error("open the project", error),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_path(&mut self, path: std::path::PathBuf) {
        match document::write_scene(&path, &self.tree) {
            Ok(()) => self.document.mark_saved(path),
            Err(error) => self.document.set_error("save the project", error),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn handle_file_action(&mut self, action: FileMenuAction) {
        match action {
            FileMenuAction::Open => {
                if let Some(path) = document::open_dialog() {
                    self.open_path(path);
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
        match action {
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
                        let _ = tx.send(WebDocumentMessage::Opened { name, bytes });
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
        let bytes = match crate::document::serialize_scene(&self.tree) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.document.set_error("save the project", error);
                return;
            }
        };
        match crate::document::download_bytes(&file_name, &bytes) {
            Ok(()) => self
                .document
                .mark_saved(std::path::PathBuf::from(file_name)),
            Err(error) => self.document.set_error("save the project", error),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn process_web_document_messages(&mut self) {
        while let Ok(message) = self.document_rx.try_recv() {
            match message {
                WebDocumentMessage::Opened { name, bytes } => {
                    match crate::document::deserialize_scene(&bytes) {
                        Ok(scene) => {
                            self.replace_scene(scene);
                            self.document.mark_opened(name);
                        }
                        Err(error) => self.document.set_error("open the project", error),
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

pub fn run() {
    #[cfg(target_os = "macos")]
    let event_loop = {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        let mut builder = EventLoop::builder();
        if std::env::args().any(|argument| {
            argument.starts_with("--guide-screenshot=") || argument == "--agent-headless"
        }) {
            builder.with_activation_policy(ActivationPolicy::Prohibited);
        }
        builder.build().expect("create event loop")
    };
    #[cfg(not(target_os = "macos"))]
    let event_loop = EventLoop::new().expect("create event loop");
    #[cfg(not(target_arch = "wasm32"))]
    event_loop.run_app(&mut App::new()).expect("run app");
    #[cfg(target_arch = "wasm32")]
    event_loop.spawn_app(App::new());
}

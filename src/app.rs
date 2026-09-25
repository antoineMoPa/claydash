use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use std::sync::mpsc::{channel, Receiver, Sender};

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
    document::{DocumentState, FileMenuAction},
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
    #[cfg(not(target_arch = "wasm32"))]
    pending_render: Option<PendingRender>,
    #[cfg(not(target_arch = "wasm32"))]
    guide_screenshot: Option<std::path::PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    guide_capture_done: bool,
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

#[cfg(not(target_arch = "wasm32"))]
enum PendingRender {
    Still {
        path: std::path::PathBuf,
    },
    Video {
        path: std::path::PathBuf,
        frames_directory: std::path::PathBuf,
        next_frame: u32,
        end_frame: u32,
        output_index: u32,
        fps: f32,
        restore_frame: f32,
    },
}

#[cfg(not(target_arch = "wasm32"))]
struct UiBenchmark {
    edit_objects: bool,
    preview_pixels: Vec<u32>,
    frames: usize,
    last_frame: Option<std::time::Instant>,
    samples: Vec<f64>,
}

#[cfg(not(target_arch = "wasm32"))]
fn guide_face_scene() -> Vec<crate::model::SdfObject> {
    use crate::model::{
        BooleanOperation, BoxParams, PolygonPrismParams, PrimitiveKind, SdfObject, SdfParams,
    };

    let mut cut_source = SdfObject::create_kind(PrimitiveKind::Box);
    cut_source.name = "Face cut source".into();
    cut_source.params = SdfParams::BoxParams(BoxParams {
        box_q: glam::Vec3::splat(0.55),
    });
    cut_source.transform.translation = glam::Vec3::new(-1.0, 0.0, 0.0);
    cut_source.softness = 0.0;

    let mut cut = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    cut.name = "Face extrusion".into();
    cut.params = SdfParams::PolygonPrismParams(PolygonPrismParams {
        vertices: vec![
            Vec2::new(-0.35, -0.3),
            Vec2::new(0.35, -0.3),
            Vec2::new(0.0, 0.35),
        ],
        half_depth: 0.3,
    });
    cut.transform.translation = glam::Vec3::new(-1.0, 0.0, 0.85);
    cut.boolean_parent = Some(cut_source.uuid);
    cut.operation = BooleanOperation::Union;

    let mut box_source = SdfObject::create_kind(PrimitiveKind::Box);
    box_source.name = "Box face source".into();
    box_source.params = SdfParams::BoxParams(BoxParams {
        box_q: glam::Vec3::splat(0.5),
    });
    box_source.transform.translation = glam::Vec3::new(1.0, -0.15, 0.0);

    let mut box_extrusion = SdfObject::create_kind(PrimitiveKind::Box);
    box_extrusion.name = "Box face extrusion".into();
    box_extrusion.params = SdfParams::BoxParams(BoxParams {
        box_q: glam::Vec3::new(0.32, 0.38, 0.5),
    });
    box_extrusion.transform.translation = glam::Vec3::new(1.0, 0.72, 0.0);

    vec![cut_source, cut, box_source, box_extrusion]
}

impl App {
    pub fn new() -> Self {
        let scene = serde_json::from_str(duck::DEFAULT_DUCK).expect("parse default scene");
        #[allow(unused_mut)]
        let mut tree = data_tree_with_scene(scene);

        #[cfg(not(target_arch = "wasm32"))]
        let brute_force_benchmark =
            std::env::args().any(|argument| argument == "--stress-benchmark-brute-force");
        #[cfg(not(target_arch = "wasm32"))]
        let benchmark = brute_force_benchmark
            || std::env::args().any(|argument| {
                argument == "--stress-benchmark" || argument == "--stress-benchmark-suite"
            });
        #[cfg(not(target_arch = "wasm32"))]
        if benchmark || std::env::args().any(|arg| arg == "--stress-ui-benchmark") {
            let case = std::env::args()
                .find_map(|arg| arg.strip_prefix("--benchmark-case=").map(str::to_owned));
            let scene = case
                .and_then(|case| {
                    crate::model::renderer_benchmark_scenes()
                        .into_iter()
                        .find(|(name, _)| *name == case)
                        .map(|(_, scene)| scene)
                })
                .unwrap_or_else(crate::model::renderer_stress_scene);
            crate::model::set_objects(&mut tree, scene);
            crate::model::set_selected(&mut tree, Vec::new());
        }
        let camera = Camera::new();
        #[cfg(not(target_arch = "wasm32"))]
        let camera = if std::env::args().any(|arg| arg == "--benchmark-orthographic") {
            Camera {
                projection_mode: crate::camera::ProjectionMode::Orthographic,
                ..camera
            }
        } else {
            camera
        };
        #[cfg(not(target_arch = "wasm32"))]
        let camera = if std::env::args().any(|argument| argument == "--ui-preview") {
            let face_preview =
                std::env::args().any(|argument| argument == "--guide-panel=face-cut");
            let repeat_preview =
                std::env::args().any(|argument| argument == "--guide-panel=repeat");
            let simple_kind = std::env::args().find_map(|argument| match argument.as_str() {
                "--guide-panel=shapes" => Some(crate::model::PrimitiveKind::Box),
                "--guide-panel=gizmos" => Some(crate::model::PrimitiveKind::Cylinder),
                "--guide-panel=modifiers" => Some(crate::model::PrimitiveKind::Sphere),
                _ => None,
            });
            let mut scene = if repeat_preview {
                let duck: DataTree =
                    serde_json::from_str(duck::DEFAULT_DUCK).expect("parse default duck preview");
                let mut scene = crate::model::objects(&data_tree_with_scene(duck));
                let root = scene[0].uuid;
                scene.retain(|object| object.uuid == root || object.boolean_parent == Some(root));
                scene
            } else if face_preview {
                guide_face_scene()
            } else if let Some(kind) = simple_kind {
                let mut shape = crate::model::SdfObject::create_kind(kind);
                shape.name = format!("{} example", kind.label());
                shape.params = match kind {
                    crate::model::PrimitiveKind::Box => {
                        crate::model::SdfParams::BoxParams(crate::model::BoxParams {
                            box_q: glam::Vec3::splat(0.55),
                        })
                    }
                    crate::model::PrimitiveKind::Cylinder => {
                        crate::model::SdfParams::CylinderParams {
                            radius: 0.45,
                            half_height: 0.65,
                        }
                    }
                    crate::model::PrimitiveKind::Sphere => {
                        crate::model::SdfParams::SphereParams(crate::model::SphereParams {
                            radius: 0.5,
                        })
                    }
                    _ => shape.params,
                };
                vec![shape]
            } else {
                crate::model::ui_preview_scene()
            };
            let operand_preview =
                std::env::args().any(|argument| argument == "--guide-panel=operand");
            if std::env::args().any(|argument| argument == "--guide-panel=modifiers") {
                if let Some((min, max)) = crate::model::lattice_bounds(&scene, scene[0].uuid) {
                    scene[0].lattice = Some(crate::model::Lattice::new(min, max, 3));
                }
            }
            if repeat_preview {
                let (min, max) =
                    crate::model::lattice_bounds(&scene, scene[0].uuid).expect("duck bounds");
                scene[0].repetition.enabled = true;
                scene[0].repetition.spacing = (max - min) * 1.1;
            }
            let selected = scene[if face_preview {
                1
            } else if operand_preview {
                7
            } else if simple_kind.is_some() || repeat_preview {
                0
            } else {
                2
            }]
            .uuid;
            if face_preview {
                crate::model::set_selected_exact(&mut tree, vec![selected]);
            } else {
                crate::model::set_selected(&mut tree, vec![selected]);
            }
            crate::model::set_objects(&mut tree, scene);
            if face_preview {
                crate::model::set_selected_modeling_face(
                    &mut tree,
                    Some(crate::model::ModelingFaceSelection::PolygonPrism(
                        crate::model::PolygonPrismFaceSelection {
                            object: selected,
                            face: crate::model::PolygonPrismFace::Cap { positive: true },
                        },
                    )),
                );
            }
            if std::env::args().any(|argument| argument == "--guide-panel=animation") {
                let binding = crate::model::AnimationBinding {
                    object: selected,
                    property: crate::model::AnimatableProperty::Position(
                        crate::model::VectorAxis::X,
                    ),
                };
                crate::animation::insert_keyframe(&mut tree, binding, 0, 1.35);
                crate::animation::insert_keyframe(&mut tree, binding, 30, -1.35);
            }
            tree.make_undo_redo_snapshot();
            Camera {
                position: if simple_kind.is_some() {
                    glam::Vec3::new(0.0, 1.3, 4.5)
                } else {
                    glam::Vec3::new(0.0, 1.6, 8.5)
                },
                ..camera
            }
        } else {
            camera
        };
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::args().any(|argument| argument == "--guide-panel=camera") {
            let scene_camera = crate::camera::SceneCamera::from_view("Camera 1", &camera);
            crate::model::set_selected(&mut tree, vec![scene_camera.uuid]);
            crate::model::set_scene_cameras(&mut tree, vec![scene_camera]);
        }
        let mut commands = Commands::new();
        commands::register_all(&mut commands);
        #[cfg(target_arch = "wasm32")]
        let (renderer_tx, renderer_rx) = channel();
        #[cfg(target_arch = "wasm32")]
        let (document_tx, document_rx) = channel();
        let document = DocumentState::default();
        let mut ui = UiState::default();
        ui.set_animation_timeline_open(document.animation_timeline_open());
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::args().any(|argument| argument.starts_with("--guide-screenshot=")) {
            let show_timeline = !std::env::args().any(|argument| {
                argument.starts_with("--guide-panel=") && argument != "--guide-panel=animation"
            });
            ui.set_animation_timeline_open(show_timeline);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(panel) = std::env::args()
            .find_map(|argument| argument.strip_prefix("--guide-panel=").map(str::to_owned))
        {
            ui.focus_guide_panel(&panel);
        }
        Self {
            window: None,
            renderer: None,
            egui: egui::Context::default(),
            #[cfg(not(target_arch = "wasm32"))]
            egui_state: None,
            #[cfg(target_arch = "wasm32")]
            egui_state: crate::web_input::WebInput::default(),
            tree,
            commands,
            camera,
            interactions: InteractionState::default(),
            ui,
            document,
            #[cfg(not(target_arch = "wasm32"))]
            pending_render: None,
            #[cfg(not(target_arch = "wasm32"))]
            guide_screenshot: std::env::args().find_map(|argument| {
                argument
                    .strip_prefix("--guide-screenshot=")
                    .map(std::path::PathBuf::from)
            }),
            #[cfg(not(target_arch = "wasm32"))]
            guide_capture_done: false,
            window_focused: true,
            window_occluded: false,
            #[cfg(not(target_arch = "wasm32"))]
            use_bvh: !brute_force_benchmark,
            #[cfg(not(target_arch = "wasm32"))]
            benchmark,
            #[cfg(not(target_arch = "wasm32"))]
            ui_benchmark: std::env::args()
                .any(|arg| arg == "--stress-ui-benchmark")
                .then(|| UiBenchmark {
                    edit_objects: std::env::args().any(|arg| arg == "--benchmark-edit"),
                    preview_pixels: Vec::new(),
                    frames: 0,
                    last_frame: None,
                    samples: Vec::new(),
                }),
            #[cfg(target_arch = "wasm32")]
            renderer_tx,
            #[cfg(target_arch = "wasm32")]
            renderer_rx,
            #[cfg(target_arch = "wasm32")]
            document_tx,
            #[cfg(target_arch = "wasm32")]
            document_rx,
        }
    }

    fn redraw(&mut self) {
        #[cfg(target_arch = "wasm32")]
        self.process_web_document_messages();
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        renderer.sync_material_asset_previews(&crate::model::material_assets(&self.tree));
        if self.camera.viewport == Vec2::ONE {
            self.camera.viewport = renderer.size();
        }
        if !self.ui.selection_gesture_active() {
            if self.interactions.update(&mut self.camera, &mut self.tree) {
                crate::ui::exit_camera_view(&mut self.tree);
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        let input = match &mut self.egui_state {
            Some(egui_state) => egui_state.take_egui_input(&window),
            None => return,
        };
        #[cfg(target_arch = "wasm32")]
        let input = self.egui_state.take(&window);
        let egui = self.egui.clone();
        if let Some(ids) = renderer.material_preview_ids() {
            egui.data_mut(|data| {
                data.insert_temp(crate::renderer::MaterialPreviewIds::egui_id(), ids)
            });
        }
        let mut file_action = None;
        let interaction_guide = self.interactions.active_guide();
        let mut output = egui.run_ui(input, |ui| {
            file_action = self.ui.draw(
                ui,
                &mut self.tree,
                &mut self.commands,
                &mut self.camera,
                &mut self.document,
                interaction_guide,
            );
        });
        if let Some(action) = file_action {
            self.handle_file_action(action);
        }
        self.document
            .set_animation_timeline_open(self.ui.animation_timeline_open());
        // Toolbar/palette commands run inside the UI pass. Place their new
        // objects before rendering, using the just-updated viewport geometry.
        self.interactions
            .place_pending_spawn(&self.camera, &mut self.tree);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(egui_state) = &mut self.egui_state {
            egui_state.handle_platform_output(&window, std::mem::take(&mut output.platform_output));
        }

        #[cfg(not(target_arch = "wasm32"))]
        let effective_selection = if self.pending_render.is_some() {
            Vec::new()
        } else {
            commands::effective_selected_ids(&self.tree)
        };
        #[cfg(target_arch = "wasm32")]
        let effective_selection = commands::effective_selected_ids(&self.tree);
        #[cfg(not(target_arch = "wasm32"))]
        let capture_render = self.pending_render.is_some() || self.guide_screenshot.is_some();
        #[cfg(not(target_arch = "wasm32"))]
        let capture_ui = self.guide_screenshot.is_some();
        #[cfg(target_arch = "wasm32")]
        let capture_render = false;
        #[cfg(target_arch = "wasm32")]
        let capture_ui = false;
        if let Some(renderer) = &mut self.renderer {
            let export_version = if capture_render { i32::MIN } else { 0 };
            let scene_versions = [
                self.tree.path_version("scene.sdf_objects"),
                self.tree
                    .path_version("scene.selected_uuids")
                    .wrapping_add(export_version),
            ];
            let captured_frame = renderer.render(
                &self.camera,
                objects_ref(&self.tree),
                &effective_selection,
                scene_versions,
                &self.egui,
                &mut output,
                capture_render,
                capture_ui,
            );
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(frame) = captured_frame {
                if let Some(path) = self.guide_screenshot.take() {
                    let result = path
                        .parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                        .map(std::fs::create_dir_all)
                        .transpose()
                        .and_then(|_| {
                            crate::render_export::png_bytes(&frame).map_err(std::io::Error::other)
                        })
                        .and_then(|bytes| std::fs::write(&path, bytes));
                    result.unwrap_or_else(|error| {
                        panic!(
                            "Could not write guide screenshot {}: {error}",
                            path.display()
                        )
                    });
                    eprintln!("Guide screenshot: {}", path.display());
                    self.guide_capture_done = true;
                }
                if let Some(pending) = self.pending_render.take() {
                    let frame = crate::render_export::crop_to_viewport(frame, &self.camera);
                    match pending {
                        PendingRender::Still { path } => {
                            if let Err(error) = crate::render_export::write_render(
                                &path,
                                crate::document::RenderFormat::WebP,
                                &frame,
                            ) {
                                self.document.set_error("render the scene", error);
                            }
                        }
                        PendingRender::Video {
                            path,
                            frames_directory,
                            next_frame,
                            end_frame,
                            output_index,
                            fps,
                            restore_frame,
                        } => {
                            let write = crate::render_export::write_video_frame(
                                &frames_directory,
                                output_index,
                                &frame,
                            );
                            if let Err(error) = write {
                                self.document.set_error("render the animation", error);
                                let _ = std::fs::remove_dir_all(&frames_directory);
                                self.ui.set_animation_frame(&mut self.tree, restore_frame);
                            } else if next_frame < end_frame {
                                let next_frame = next_frame + 1;
                                self.ui
                                    .set_animation_frame(&mut self.tree, next_frame as f32);
                                self.pending_render = Some(PendingRender::Video {
                                    path,
                                    frames_directory,
                                    next_frame,
                                    end_frame,
                                    output_index: output_index + 1,
                                    fps,
                                    restore_frame,
                                });
                            } else {
                                if let Err(error) =
                                    crate::render_export::write_mp4(&path, &frames_directory, fps)
                                {
                                    self.document.set_error("encode the animation", error);
                                }
                                let _ = std::fs::remove_dir_all(&frames_directory);
                                self.ui.set_animation_frame(&mut self.tree, restore_frame);
                            }
                        }
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            let _ = captured_frame;
        }
        self.tree.reset_update_cycle();
    }

    fn pointer_over_ui(&self, position: Vec2, egui_consumed: bool) -> bool {
        self.ui
            .contains_pointer(position, self.egui.pixels_per_point())
            || (egui_consumed && self.ui.overlay_captures_pointer(&self.egui))
    }

    fn replace_scene(&mut self, scene: DataTree) {
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
            FileMenuAction::Render(_) => {
                self.document.set_error(
                    "render the scene",
                    "render export is currently desktop-only",
                );
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
            }
        }
    }
}

fn data_tree_with_scene(scene: DataTree) -> DataTree {
    let mut tree = ObservableKVTree::default();
    tree.set_tree("scene", scene);
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
    tree.set_path(
        "editor.color",
        ClaydashValue::Vec4(Vec4::new(0.8, 0.0, 0.3, 1.0)),
    );
    tree.make_undo_redo_snapshot();
    tree
}

#[cfg(target_arch = "wasm32")]
fn web_canvas_size(window: &Window) -> Option<winit::dpi::PhysicalSize<u32>> {
    let Some(browser) = web_sys::window() else {
        return None;
    };
    if window.canvas().is_none() {
        return None;
    }
    let width = browser
        .inner_width()
        .ok()
        .and_then(|value| value.as_f64())
        .unwrap_or(1280.0);
    let height = browser
        .inner_height()
        .ok()
        .and_then(|value| value.as_f64())
        .unwrap_or(720.0);
    let scale = browser.device_pixel_ratio();
    let width = (width * scale).round() as u32;
    let height = (height * scale).round() as u32;
    Some(winit::dpi::PhysicalSize::new(width, height))
}

#[cfg(target_arch = "wasm32")]
fn sync_web_canvas(window: &Window) -> Option<winit::dpi::PhysicalSize<u32>> {
    let canvas = window.canvas()?;
    let size = web_canvas_size(window)?;
    let width = size.width;
    let height = size.height;
    let changed = canvas.width() != width || canvas.height() != height;
    if changed {
        canvas.set_width(width);
        canvas.set_height(height);
        Some(size)
    } else {
        None
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title("Claydash");
        #[cfg(not(target_arch = "wasm32"))]
        let attributes = if self.benchmark || self.ui_benchmark.is_some() {
            let size = std::env::args()
                .find_map(|argument| {
                    let value = argument.strip_prefix("--benchmark-size=")?;
                    let (width, height) = value.split_once('x')?;
                    Some((
                        width.parse::<u32>().expect("benchmark width"),
                        height.parse::<u32>().expect("benchmark height"),
                    ))
                })
                .unwrap_or((384, 216));
            attributes.with_inner_size(winit::dpi::PhysicalSize::new(size.0, size.1))
        } else {
            attributes.with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0))
        };
        #[cfg(target_arch = "wasm32")]
        let attributes = attributes.with_append(true);
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
        #[cfg(target_arch = "wasm32")]
        sync_web_canvas(&window);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.egui_state = Some(egui_winit::State::new(
                self.egui.clone(),
                egui::ViewportId::ROOT,
                &window,
                None,
                None,
                None,
            ));
            let mut renderer = pollster::block_on(Renderer::new(
                window.clone(),
                self.use_bvh,
                self.benchmark || self.ui_benchmark.is_some(),
            ));
            if self.benchmark {
                self.camera.viewport = renderer.size();
                let versions = [
                    self.tree.path_version("scene.sdf_objects"),
                    self.tree.path_version("scene.selected_uuids"),
                ];
                if std::env::args().any(|argument| argument == "--stress-benchmark-suite") {
                    for (case, scene) in crate::model::renderer_benchmark_scenes() {
                        if let Some(filter) = std::env::args().find_map(|arg| {
                            arg.strip_prefix("--benchmark-case=").map(str::to_owned)
                        }) {
                            if case != filter {
                                continue;
                            }
                        }
                        eprintln!("Case: {case}");
                        renderer.benchmark_scene(
                            &self.camera,
                            &scene,
                            &[],
                            [i32::MIN + 1, 0],
                            &case,
                        );
                    }
                } else {
                    renderer.benchmark_scene(
                        &self.camera,
                        objects_ref(&self.tree),
                        &commands::effective_selected_ids(&self.tree),
                        versions,
                        "stress",
                    );
                }
                event_loop.exit();
            }
            self.renderer = Some(renderer);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let renderer_tx = self.renderer_tx.clone();
            let renderer_window = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let renderer = Renderer::new(renderer_window, true, false).await;
                let _ = renderer_tx.send(renderer);
            });
            event_loop.set_control_flow(ControlFlow::Poll);
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        let egui_consumed = self
            .egui_state
            .as_mut()
            .is_some_and(|state| state.on_window_event(&window, &event).consumed);
        #[cfg(target_arch = "wasm32")]
        let egui_consumed = {
            self.egui_state.on_window_event(&window, &event);
            match event {
                WindowEvent::CursorMoved { .. }
                | WindowEvent::CursorLeft { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::MouseWheel { .. } => self.egui.egui_wants_pointer_input(),
                WindowEvent::KeyboardInput { .. } | WindowEvent::Ime(_) => {
                    self.egui.egui_wants_keyboard_input()
                }
                _ => false,
            }
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                if focused {
                    window.request_redraw();
                } else {
                    self.interactions.suspend_navigation();
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.window_occluded = occluded;
                if !occluded {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                #[cfg(not(target_arch = "wasm32"))]
                let guide_capture = self.guide_screenshot.is_some();
                #[cfg(target_arch = "wasm32")]
                let guide_capture = false;
                if (!self.window_focused || self.window_occluded) && !guide_capture {
                    return;
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(benchmark) = &mut self.ui_benchmark {
                    let now = std::time::Instant::now();
                    if let Some(last) = benchmark.last_frame {
                        if benchmark.frames > 10 {
                            benchmark
                                .samples
                                .push(now.duration_since(last).as_secs_f64() * 1000.0);
                        }
                    }
                    benchmark.last_frame = Some(now);
                    let angle = (benchmark.frames as f32 * 0.08).sin() * 0.3;
                    if benchmark.edit_objects {
                        let mut scene = objects_ref(&self.tree).to_vec();
                        if let Some(object) = scene.first_mut() {
                            object.transform.rotation = glam::Quat::from_rotation_y(angle);
                        }
                        crate::model::set_objects(&mut self.tree, scene);
                    } else {
                        self.camera.position =
                            glam::Quat::from_rotation_y(angle) * glam::Vec3::new(-3.3, 0.8, 1.7);
                    }
                    if benchmark.frames > 10 {
                        if let Some(renderer) = &self.renderer {
                            benchmark.preview_pixels.push(renderer.preview_pixels());
                        }
                    }
                    benchmark.frames += 1;
                    if benchmark.frames >= 130 {
                        benchmark.samples.sort_by(f64::total_cmp);
                        let samples = &benchmark.samples;
                        eprintln!(
                            "Native UI loop: p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms, {} frames",
                            samples[samples.len() / 2],
                            samples[(samples.len() - 1) * 95 / 100],
                            samples[samples.len() - 1],
                            samples.len()
                        );
                        benchmark.preview_pixels.sort_unstable();
                        if !benchmark.preview_pixels.is_empty() {
                            eprintln!(
                                "Median preview pixels: {}",
                                benchmark.preview_pixels[benchmark.preview_pixels.len() / 2]
                            );
                        }
                        event_loop.exit();
                        return;
                    }
                }
                self.redraw();
                #[cfg(not(target_arch = "wasm32"))]
                if self.guide_capture_done {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let position = Vec2::new(position.x as f32, position.y as f32);
                let over_ui = self.ui.selection_gesture_active()
                    || self.pointer_over_ui(position, egui_consumed);
                self.interactions.cursor_moved(position, over_ui);
            }
            WindowEvent::MouseWheel { delta, .. }
                if !self.ui.selection_gesture_active()
                    && !self.pointer_over_ui(self.interactions.mouse_position, egui_consumed) =>
            {
                match delta {
                    MouseScrollDelta::LineDelta(_, y) if y != 0.0 => {
                        crate::ui::exit_camera_view(&mut self.tree);
                        self.camera.zoom(y);
                    }
                    MouseScrollDelta::PixelDelta(delta) if delta.y != 0.0 => {
                        crate::ui::exit_camera_view(&mut self.tree);
                        self.camera.zoom(delta.y as f32 / 53.0);
                    }
                    _ => {}
                }
            }
            WindowEvent::PinchGesture { delta, .. }
                if !self.ui.selection_gesture_active()
                    && !self.pointer_over_ui(self.interactions.mouse_position, egui_consumed) =>
            {
                if delta != 0.0 {
                    crate::ui::exit_camera_view(&mut self.tree);
                    self.camera.zoom(delta as f32);
                }
            }
            WindowEvent::MouseInput {
                button: MouseButton::Right,
                state,
                ..
            } => {
                let over_ui = self.ui.selection_gesture_active()
                    || self.pointer_over_ui(self.interactions.mouse_position, egui_consumed);
                self.interactions.set_right_button(
                    state == ElementState::Pressed,
                    over_ui,
                    &self.camera,
                    &self.tree,
                );
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Released,
                ..
            } => {
                // Finish a grab even when its release lands over an editor panel.
                self.interactions.pointer_up(&self.camera, &mut self.tree);
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } if !self.ui.selection_gesture_active()
                && !self.pointer_over_ui(self.interactions.mouse_position, egui_consumed) =>
            {
                if self.ui.box_selection_enabled(&self.tree) {
                    return;
                }
                let ghost = self.ui.ghost_at(
                    self.interactions.mouse_position,
                    self.egui.pixels_per_point(),
                );
                self.interactions
                    .pointer_down(&self.camera, &mut self.tree, ghost);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if event.state == ElementState::Pressed
                        && !event.repeat
                        && !self.ui.selection_gesture_active()
                    {
                        let wants_keyboard = self.egui.egui_wants_keyboard_input();
                        let timeline_shortcut = matches!(
                            key,
                            winit::keyboard::KeyCode::KeyG
                                | winit::keyboard::KeyCode::Backspace
                                | winit::keyboard::KeyCode::Delete
                        ) && self.ui.animation_timeline_contains_pointer(
                            self.interactions.mouse_position,
                            self.egui.pixels_per_point(),
                        );
                        let entered_box_selection = key == winit::keyboard::KeyCode::KeyB
                            && !wants_keyboard
                            && !self.interactions.command_modifier_down()
                            && self.ui.begin_box_selection(
                                &self.tree,
                                self.interactions.mouse_position,
                                self.egui.pixels_per_point(),
                                self.egui.input(|input| input.modifiers.shift),
                            );
                        if !entered_box_selection && !timeline_shortcut {
                            self.interactions.key_pressed(
                                key,
                                wants_keyboard,
                                &self.commands,
                                &mut self.tree,
                            );
                        }
                    } else if event.state == ElementState::Released {
                        self.interactions.key_released(key);
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = event_loop;
        #[cfg(target_arch = "wasm32")]
        if let Some(window) = &self.window {
            if let Some(size) = sync_web_canvas(window) {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size);
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        if self.renderer.is_none() {
            if let Ok(mut renderer) = self.renderer_rx.try_recv() {
                if let Some(window) = &self.window {
                    if let Some(size) = web_canvas_size(window) {
                        renderer.resize(size);
                    }
                }
                self.renderer = Some(renderer);
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let guide_capture = self.guide_screenshot.is_some();
        #[cfg(target_arch = "wasm32")]
        let guide_capture = false;
        if self.window_focused && !self.window_occluded || guide_capture {
            let Some(window) = &self.window else {
                return;
            };
            window.request_redraw();
        }
    }
}

pub fn run() {
    let event_loop = EventLoop::new().expect("create event loop");
    #[cfg(not(target_arch = "wasm32"))]
    event_loop.run_app(&mut App::new()).expect("run app");
    #[cfg(target_arch = "wasm32")]
    event_loop.spawn_app(App::new());
}

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
    Saved(std::path::PathBuf),
    Error(String),
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
struct UiBenchmark {
    edit_objects: bool,
    preview_pixels: Vec<u32>,
    frames: usize,
    last_frame: Option<std::time::Instant>,
    samples: Vec<f64>,
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
            let scene = crate::model::ui_preview_scene();
            crate::model::set_selected(&mut tree, vec![scene[2].uuid]);
            crate::model::set_objects(&mut tree, scene);
            tree.make_undo_redo_snapshot();
            Camera {
                position: glam::Vec3::new(0.0, 1.6, 8.5),
                ..camera
            }
        } else {
            camera
        };
        let mut commands = Commands::new();
        commands::register_all(&mut commands);
        #[cfg(target_arch = "wasm32")]
        let (renderer_tx, renderer_rx) = channel();
        #[cfg(target_arch = "wasm32")]
        let (document_tx, document_rx) = channel();
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
            ui: UiState::default(),
            document: DocumentState::default(),
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
        let Some(renderer) = &self.renderer else {
            return;
        };
        if self.camera.viewport == Vec2::ONE {
            self.camera.viewport = renderer.size();
        }
        if !self.ui.selection_gesture_active() {
            self.interactions.update(&mut self.camera, &mut self.tree);
        }

        #[cfg(not(target_arch = "wasm32"))]
        let input = match &mut self.egui_state {
            Some(egui_state) => egui_state.take_egui_input(&window),
            None => return,
        };
        #[cfg(target_arch = "wasm32")]
        let input = self.egui_state.take(&window);
        let egui = self.egui.clone();
        let mut file_action = None;
        let mut output = egui.run_ui(input, |ui| {
            file_action = self.ui.draw(
                ui,
                &mut self.tree,
                &mut self.commands,
                &mut self.camera,
                &mut self.document,
            );
        });
        if let Some(action) = file_action {
            self.handle_file_action(action);
        }
        // Toolbar/palette commands run inside the UI pass. Place their new
        // objects before rendering, using the just-updated viewport geometry.
        self.interactions
            .place_pending_spawn(&self.camera, &mut self.tree);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(egui_state) = &mut self.egui_state {
            egui_state.handle_platform_output(&window, std::mem::take(&mut output.platform_output));
        }

        let effective_selection = commands::effective_selected_ids(&self.tree);
        if let Some(renderer) = &mut self.renderer {
            let scene_versions = [
                self.tree.path_version("scene.sdf_objects"),
                self.tree.path_version("scene.selected_uuids"),
            ];
            renderer.render(
                &self.camera,
                objects_ref(&self.tree),
                &effective_selection,
                scene_versions,
                &self.egui,
                &mut output,
            );
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
                let bytes = match crate::document::serialize_scene(&self.tree) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        self.document.set_error("save the project", error);
                        return;
                    }
                };
                let file_name = self
                    .document
                    .current_path()
                    .and_then(std::path::Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "untitled.claydash".to_string());
                let tx = self.document_tx.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let file = rfd::AsyncFileDialog::new()
                        .add_filter("Claydash project", &["claydash"])
                        .set_file_name(&file_name)
                        .save_file()
                        .await;
                    if let Some(file) = file {
                        let name = std::path::PathBuf::from(file.file_name());
                        match file.write(&bytes).await {
                            Ok(()) => {
                                let _ = tx.send(WebDocumentMessage::Saved(name));
                            }
                            Err(error) => {
                                let _ = tx.send(WebDocumentMessage::Error(format!("{error:?}")));
                            }
                        }
                    }
                });
            }
            FileMenuAction::OpenRecent(_) => {}
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
                WebDocumentMessage::Saved(name) => self.document.mark_saved(name),
                WebDocumentMessage::Error(error) => {
                    self.document.set_error("save the project", error)
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
                if !self.window_focused || self.window_occluded {
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
                    MouseScrollDelta::LineDelta(_, y) => self.camera.zoom(y),
                    MouseScrollDelta::PixelDelta(delta) => self.camera.zoom(delta.y as f32 / 53.0),
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
                        let entered_box_selection = key == winit::keyboard::KeyCode::KeyB
                            && !wants_keyboard
                            && !self.interactions.command_modifier_down()
                            && self.ui.enter_box_selection_mode(&self.tree);
                        if !entered_box_selection {
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
        if self.window_focused && !self.window_occluded {
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

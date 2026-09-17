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
    duck,
    interactions::InteractionState,
    model::{objects, selected, ClaydashValue, DataTree, EditorState},
    renderer::Renderer,
    ui::UiState,
};

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
    #[cfg(target_arch = "wasm32")]
    renderer_tx: Sender<Renderer>,
    #[cfg(target_arch = "wasm32")]
    renderer_rx: Receiver<Renderer>,
}

impl App {
    pub fn new() -> Self {
        let mut tree = ObservableKVTree::default();
        let scene = serde_json::from_str(duck::DEFAULT_DUCK).expect("parse default scene");
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

        let mut commands = Commands::new();
        commands::register_all(&mut commands);
        #[cfg(target_arch = "wasm32")]
        let (renderer_tx, renderer_rx) = channel();
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
            camera: Camera::new(),
            interactions: InteractionState::default(),
            ui: UiState::default(),
            #[cfg(target_arch = "wasm32")]
            renderer_tx,
            #[cfg(target_arch = "wasm32")]
            renderer_rx,
        }
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(renderer) = &self.renderer else {
            return;
        };
        self.camera.viewport = renderer.size();
        self.interactions.update(&mut self.camera, &mut self.tree);

        #[cfg(not(target_arch = "wasm32"))]
        let input = match &mut self.egui_state {
            Some(egui_state) => egui_state.take_egui_input(&window),
            None => return,
        };
        #[cfg(target_arch = "wasm32")]
        let input = self.egui_state.take(&window);
        let egui = self.egui.clone();
        let mut output = egui.run_ui(input, |ui| {
            self.ui.draw(ui, &mut self.tree, &mut self.commands);
        });
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(egui_state) = &mut self.egui_state {
            egui_state.handle_platform_output(&window, std::mem::take(&mut output.platform_output));
        }

        if let Some(renderer) = &mut self.renderer {
            renderer.render(
                &self.camera,
                &objects(&self.tree),
                &selected(&self.tree),
                &self.egui,
                &mut output,
            );
        }
        self.tree.reset_update_cycle();
    }

    fn pointer_over_ui(&self, position: Vec2, egui_consumed: bool) -> bool {
        egui_consumed
            || self
                .ui
                .contains_pointer(position, self.egui.pixels_per_point())
    }
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
            self.renderer = Some(pollster::block_on(Renderer::new(window.clone())));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let renderer_tx = self.renderer_tx.clone();
            let renderer_window = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let renderer = Renderer::new(renderer_window).await;
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
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let position = Vec2::new(position.x as f32, position.y as f32);
                let over_ui = self.pointer_over_ui(position, egui_consumed);
                self.interactions.cursor_moved(position, over_ui);
            }
            WindowEvent::MouseWheel { delta, .. }
                if !self.pointer_over_ui(self.interactions.mouse_position, egui_consumed) =>
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
                let over_ui = self.pointer_over_ui(self.interactions.mouse_position, egui_consumed);
                self.interactions
                    .set_right_button(state == ElementState::Pressed, over_ui);
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } if !self.pointer_over_ui(self.interactions.mouse_position, egui_consumed) => {
                self.interactions.pointer_down(&self.camera, &mut self.tree);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if event.state == ElementState::Pressed {
                        self.interactions.key_pressed(
                            key,
                            self.egui.egui_wants_keyboard_input(),
                            &self.commands,
                            &mut self.tree,
                        );
                    } else {
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
        if let Some(window) = &self.window {
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

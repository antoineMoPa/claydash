use std::sync::Arc;

use glam::{Vec2, Vec4};
use observable_key_value_tree::ObservableKVTree;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::PhysicalKey,
    window::{Window, WindowId},
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
    egui_state: Option<egui_winit::State>,
    tree: DataTree,
    commands: Commands,
    camera: Camera,
    interactions: InteractionState,
    ui: UiState,
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
        Self {
            window: None,
            renderer: None,
            egui: egui::Context::default(),
            egui_state: None,
            tree,
            commands,
            camera: Camera::new(),
            interactions: InteractionState::default(),
            ui: UiState::default(),
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

        let input = match &mut self.egui_state {
            Some(egui_state) => egui_state.take_egui_input(&window),
            None => return,
        };
        let egui = self.egui.clone();
        let mut output = egui.run_ui(input, |ui| {
            self.ui.draw(ui, &mut self.tree, &mut self.commands);
        });
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

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("Claydash"))
                .expect("create window"),
        );
        self.egui_state = Some(egui_winit::State::new(
            self.egui.clone(),
            egui::ViewportId::ROOT,
            &window,
            None,
            None,
            None,
        ));
        self.renderer = Some(Renderer::new(window.clone()));
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let egui_consumed = self
            .egui_state
            .as_mut()
            .is_some_and(|state| state.on_window_event(&window, &event).consumed);
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

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

pub fn run() {
    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.run_app(&mut App::new()).expect("run app");
}

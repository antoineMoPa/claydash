use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect};
use winit::{
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent},
    keyboard::{Key as WinitKey, KeyCode, ModifiersState, PhysicalKey},
    window::Window,
};

pub struct WebInput {
    input: RawInput,
    modifiers: Modifiers,
    pointer: Pos2,
}

impl Default for WebInput {
    fn default() -> Self {
        Self {
            input: RawInput::default(),
            modifiers: Modifiers::default(),
            pointer: Pos2::ZERO,
        }
    }
}

impl WebInput {
    pub fn take(&mut self, window: &Window) -> RawInput {
        let scale = window.scale_factor() as f32;
        let size = window.inner_size().to_logical::<f32>(window.scale_factor());
        self.input.screen_rect = Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(size.width, size.height),
        ));
        if let Some(viewport) = self.input.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.native_pixels_per_point = Some(scale);
            viewport.inner_rect = self.input.screen_rect;
            viewport.focused = Some(self.input.focused);
        }
        self.input.take()
    }

    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) {
        let scale = window.scale_factor() as f32;
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = Pos2::new(position.x as f32 / scale, position.y as f32 / scale);
                self.input.events.push(Event::PointerMoved(self.pointer));
            }
            WindowEvent::CursorLeft { .. } => self.input.events.push(Event::PointerGone),
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(button) = pointer_button(*button) {
                    self.input.events.push(Event::PointerButton {
                        pos: self.pointer,
                        button,
                        pressed: *state == ElementState::Pressed,
                        modifiers: self.modifiers,
                    });
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let (unit, delta) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (MouseWheelUnit::Line, egui::vec2(*x, *y)),
                    MouseScrollDelta::PixelDelta(delta) => (
                        MouseWheelUnit::Point,
                        egui::vec2(delta.x as f32 / scale, delta.y as f32 / scale),
                    ),
                };
                self.input.events.push(Event::MouseWheel {
                    unit,
                    delta,
                    phase: touch_phase(*phase),
                    modifiers: self.modifiers,
                });
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = egui_modifiers(modifiers.state());
                self.input
                    .events
                    .push(Event::ModifiersChanged(self.modifiers));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(key) = egui_key(code) {
                        self.input.events.push(Event::Key {
                            key,
                            physical_key: Some(key),
                            pressed,
                            repeat: event.repeat,
                            modifiers: self.modifiers,
                        });
                    }
                }
                if pressed && !self.modifiers.ctrl && !self.modifiers.command {
                    if let WinitKey::Character(text) = &event.logical_key {
                        self.input.events.push(Event::Text(text.to_string()));
                    }
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.input.events.push(Event::Text(text.clone()));
            }
            WindowEvent::Focused(focused) => {
                self.input.focused = *focused;
                self.input.events.push(Event::WindowFocused(*focused));
            }
            _ => {}
        }
    }
}

fn pointer_button(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Right => Some(PointerButton::Secondary),
        MouseButton::Middle => Some(PointerButton::Middle),
        MouseButton::Back => Some(PointerButton::Extra1),
        MouseButton::Forward => Some(PointerButton::Extra2),
        MouseButton::Other(_) => None,
    }
}

fn touch_phase(phase: TouchPhase) -> egui::TouchPhase {
    match phase {
        TouchPhase::Started => egui::TouchPhase::Start,
        TouchPhase::Moved => egui::TouchPhase::Move,
        TouchPhase::Ended => egui::TouchPhase::End,
        TouchPhase::Cancelled => egui::TouchPhase::Cancel,
    }
}

fn egui_modifiers(modifiers: ModifiersState) -> Modifiers {
    let command = modifiers.super_key();
    Modifiers {
        alt: modifiers.alt_key(),
        ctrl: modifiers.control_key(),
        shift: modifiers.shift_key(),
        mac_cmd: command,
        command,
    }
}

fn egui_key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Enter => Key::Enter,
        KeyCode::Space => Key::Space,
        KeyCode::Insert => Key::Insert,
        KeyCode::Delete => Key::Delete,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyB => Key::B,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyF => Key::F,
        KeyCode::KeyG => Key::G,
        KeyCode::KeyH => Key::H,
        KeyCode::KeyI => Key::I,
        KeyCode::KeyJ => Key::J,
        KeyCode::KeyK => Key::K,
        KeyCode::KeyL => Key::L,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyN => Key::N,
        KeyCode::KeyO => Key::O,
        KeyCode::KeyP => Key::P,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyU => Key::U,
        KeyCode::KeyV => Key::V,
        KeyCode::KeyW => Key::W,
        KeyCode::KeyX => Key::X,
        KeyCode::KeyY => Key::Y,
        KeyCode::KeyZ => Key::Z,
        _ => return None,
    })
}

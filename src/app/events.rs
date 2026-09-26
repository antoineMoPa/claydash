use super::*;

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

impl ApplicationHandler<AppEvent> for App {
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
        #[cfg(not(target_arch = "wasm32"))]
        let attributes = attributes.with_visible(
            self.guide_screenshot.is_none() && {
                #[cfg(unix)]
                {
                    !self.agent_headless
                }
                #[cfg(not(unix))]
                {
                    true
                }
            },
        );
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
            if self.guide_screenshot.is_some() {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
            }
            #[cfg(unix)]
            if self.agent_headless {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                    std::time::Instant::now() + std::time::Duration::from_millis(16),
                ));
            }
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
        #[cfg(all(not(target_arch = "wasm32"), unix))]
        if !self.benchmark && self.ui_benchmark.is_none() && self.guide_screenshot.is_none() {
            match super::agent::listen(self.agent_proxy.as_ref().unwrap().clone()) {
                Ok(receiver) => self.agent_requests = Some(receiver),
                Err(error) => eprintln!("Claydash agent bridge unavailable: {error}"),
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        #[cfg(all(not(target_arch = "wasm32"), unix))]
        match event {
            AppEvent::AgentRequest => self.process_agent_requests(),
        }
        #[cfg(any(target_arch = "wasm32", not(unix)))]
        let _ = event;
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
                #[cfg(all(not(target_arch = "wasm32"), unix))]
                self.process_agent_requests();
                #[cfg(not(target_arch = "wasm32"))]
                let guide_capture = self.guide_screenshot.is_some() || {
                    #[cfg(unix)]
                    {
                        self.agent_capture.is_some()
                    }
                    #[cfg(not(unix))]
                    {
                        false
                    }
                };
                #[cfg(target_arch = "wasm32")]
                let guide_capture = false;
                if (!self.window_focused || self.window_occluded) && !guide_capture {
                    return;
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(benchmark) = &mut self.ui_benchmark {
                    let now = std::time::Instant::now();
                    if benchmark.selection_click && matches!(benchmark.frames, 60 | 90) {
                        let pointer = self.camera.viewport_origin + self.camera.viewport * 0.5;
                        InteractionState::select_at(
                            &self.camera,
                            &mut self.tree,
                            pointer,
                            None,
                            false,
                        );
                    }
                    if let Some(last) = benchmark.last_frame {
                        if benchmark.frames > 10 {
                            benchmark
                                .samples
                                .push(now.duration_since(last).as_secs_f64() * 1000.0);
                        }
                    }
                    benchmark.last_frame = Some(now);
                    if !benchmark.selection_click {
                        let angle = (benchmark.frames as f32 * 0.08).sin() * 0.3;
                        if benchmark.edit_objects {
                            let mut scene = objects_ref(&self.tree).to_vec();
                            if let Some(object) = scene.first_mut() {
                                object.transform.rotation = glam::Quat::from_rotation_y(angle);
                            }
                            crate::model::set_objects(&mut self.tree, scene);
                        } else {
                            self.camera.position = glam::Quat::from_rotation_y(angle)
                                * glam::Vec3::new(-3.3, 0.8, 1.7);
                        }
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
        #[cfg(all(not(target_arch = "wasm32"), unix))]
        if self.agent_headless {
            self.process_agent_requests();
            if self.agent_capture.is_some() && self.renderer.is_some() {
                self.redraw();
            }
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(16),
            ));
            return;
        }
        #[cfg(all(not(target_arch = "wasm32"), unix))]
        if self.agent_capture.is_some() {
            if self.renderer.is_some() {
                self.redraw();
            }
            if self.agent_capture.is_some() {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
                return;
            }
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.guide_screenshot.is_some() {
            if self.renderer.is_some() {
                self.redraw();
                if self.guide_capture_done {
                    event_loop.exit();
                }
            }
            return;
        }
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
        let guide_capture = self.guide_screenshot.is_some() || {
            #[cfg(unix)]
            {
                self.agent_capture.is_some()
            }
            #[cfg(not(unix))]
            {
                false
            }
        };
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

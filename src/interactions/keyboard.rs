use super::*;

impl InteractionState {
    pub fn suspend_navigation(&mut self) {
        self.keys.clear();
        self.mouse_delta = Vec2::ZERO;
        self.right_down = false;
        self.right_pan_reference = None;
        self.shift_pan = None;
    }

    pub fn command_modifier_down(&self) -> bool {
        [
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ]
        .iter()
        .any(|key| self.keys.contains(key))
    }

    fn primary_modifier_down(&self) -> bool {
        [
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ]
        .iter()
        .any(|key| self.keys.contains(key))
    }

    pub fn key_pressed(
        &mut self,
        key: KeyCode,
        egui_wants_keyboard: bool,
        command_map: &Commands,
        tree: &mut DataTree,
    ) {
        let first_press = self.keys.insert(key);
        let modal_confirm_key = matches!(key, KeyCode::Enter | KeyCode::Escape)
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::EditorState(
                    EditorState::ExtendingCurve
                        | EditorState::Extruding
                        | EditorState::DraggingFace
                )
            );
        if (egui_wants_keyboard && !modal_confirm_key) || !first_press {
            return;
        }
        if self.view_rotation.is_some() || self.view_rotation_requested {
            match key {
                KeyCode::Escape => {
                    if let Some(session) = self.view_rotation.take() {
                        self.restore_view_rotation(session, tree);
                    }
                    self.view_rotation_requested = false;
                    tree.set_transient_path("editor.view_rotation", ClaydashValue::Bool(false));
                }
                KeyCode::Enter => self.finish_view_rotation(tree),
                _ => {}
            }
            return;
        }
        if key == KeyCode::KeyR
            && !self.command_modifier_down()
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
            )
        {
            let selection = selected(tree);
            let viewing_selected_camera = matches!(
                tree.get_path("editor.camera_view"),
                ClaydashValue::Bool(true)
            ) && crate::model::active_camera_id(tree)
                .is_some_and(|id| selection == [id]);
            if selection.is_empty() || viewing_selected_camera {
                self.view_rotation_requested = true;
                return;
            }
        }
        if key == KeyCode::Enter && commands::close_curve_extension(tree) {
            return;
        }
        if key == KeyCode::Escape
            && matches!(
                tree.get_path("editor.place_cursor"),
                ClaydashValue::Bool(true)
            )
        {
            tree.set_transient_path("editor.place_cursor", ClaydashValue::Bool(false));
            return;
        }
        if key == KeyCode::Escape && crate::ui::scene_actions::pending_boolean(tree).is_some() {
            crate::ui::scene_actions::cancel_boolean_pick(tree);
            return;
        }
        if key == KeyCode::Escape
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
            )
        {
            set_selected(tree, vec![]);
            return;
        }
        if key == KeyCode::Enter
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
            )
            && commands::close_selected_curve(tree)
        {
            return;
        }
        let has_command_modifier = self.command_modifier_down();
        let rotating = matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Rotating)
        );
        if !has_command_modifier {
            let operation = match key {
                KeyCode::Equal | KeyCode::NumpadAdd if !rotating => {
                    Some(crate::model::BooleanOperation::Union)
                }
                KeyCode::Minus | KeyCode::NumpadSubtract if !rotating => {
                    Some(crate::model::BooleanOperation::Subtract)
                }
                KeyCode::NumpadMultiply if !rotating => {
                    Some(crate::model::BooleanOperation::Intersect)
                }
                KeyCode::Digit8
                    if !rotating
                        && (self.keys.contains(&KeyCode::ShiftLeft)
                            || self.keys.contains(&KeyCode::ShiftRight)) =>
                {
                    Some(crate::model::BooleanOperation::Intersect)
                }
                _ => None,
            };
            if let Some(operation) = operation {
                crate::ui::scene_actions::begin_boolean_pick(tree, operation);
                return;
            }
        }
        let shift = self.keys.contains(&KeyCode::ShiftLeft)
            || self.keys.contains(&KeyCode::ShiftRight)
            || self.keys.contains(&KeyCode::SuperLeft)
            || self.keys.contains(&KeyCode::SuperRight);
        let transforming = matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(
                EditorState::Grabbing
                    | EditorState::Scaling
                    | EditorState::Rotating
                    | EditorState::Extruding
                    | EditorState::DraggingFace
                    | EditorState::ExtendingCurve
            )
        );
        if rotating && !has_command_modifier {
            match key {
                KeyCode::Minus | KeyCode::NumpadSubtract => {
                    if let NumericRotationInput::Editing(input) = &mut self.numeric_rotation {
                        if input.starts_with('-') {
                            input.remove(0);
                        } else {
                            input.insert(0, '-');
                        }
                    }
                    return;
                }
                KeyCode::Backspace => {
                    if let NumericRotationInput::Editing(input) = &mut self.numeric_rotation {
                        input.pop();
                    }
                    return;
                }
                _ => {
                    if let Some(character) = rotation_input_character(key) {
                        if let NumericRotationInput::Editing(input) = &mut self.numeric_rotation {
                            if character != '.' || !input.contains('.') {
                                input.push(character);
                            }
                        }
                        return;
                    }
                }
            }
        }
        let primary_modifier = self.primary_modifier_down();
        let name = match key {
            KeyCode::KeyG => "grab",
            KeyCode::KeyS => "scale",
            KeyCode::KeyR => "rotate",
            KeyCode::KeyE if !transforming => "extrude",
            KeyCode::KeyX => "constrain_x",
            KeyCode::KeyY if transforming => "constrain_y",
            KeyCode::KeyZ if transforming => "constrain_z",
            KeyCode::KeyZ if shift => "undo",
            KeyCode::KeyY if shift => "redo",
            KeyCode::KeyY => "constrain_y",
            KeyCode::KeyZ if !has_command_modifier => "toggle_outline",
            KeyCode::KeyA if shift => "select_all_or_none",
            KeyCode::KeyD if shift => "duplicate",
            KeyCode::KeyI if primary_modifier => "invert_selection",
            KeyCode::Escape => "quit",
            KeyCode::Enter => "finish",
            KeyCode::Backspace => "delete",
            _ => return,
        };
        crate::ui::scene_actions::cancel_boolean_pick(tree);
        commands::execute(command_map, name, tree);
        if name == "grab"
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::EditorState(EditorState::Grabbing)
            )
        {
            self.transform_session = None;
            self.pending_grab_pointer = Some(self.mouse_position);
        }
        if name == "grab"
            && matches!(
                tree.get_path("editor.curve_grab_initial"),
                ClaydashValue::VecSDFObject(_)
            )
            && self.curve_grab_mouse_start.is_none()
        {
            self.curve_grab_mouse_start = Some(self.mouse_position);
        }
        if name == "quit" || name == "finish" {
            self.curve_grab_mouse_start = None;
            self.transform_session = None;
            self.pending_grab_pointer = None;
        }
        if name == "grab"
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::EditorState(EditorState::DraggingFace)
            )
        {
            self.extrusion_session = None;
        }
        match name {
            "rotate" => {
                self.numeric_rotation = NumericRotationInput::Editing(String::new());
            }
            "constrain_x" | "constrain_y" | "constrain_z" => {}
            _ => {
                self.numeric_rotation = NumericRotationInput::Idle;
            }
        }
    }

    pub fn key_released(&mut self, key: KeyCode) {
        self.keys.remove(&key);
    }

    pub fn cursor_moved(&mut self, position: Vec2, over_ui: bool) {
        let delta = position - self.mouse_position;
        self.mouse_position = position;
        if !over_ui {
            self.mouse_delta += delta;
        }
    }

    pub fn set_right_button(
        &mut self,
        pressed: bool,
        over_ui: bool,
        camera: &Camera,
        tree: &DataTree,
    ) {
        if !pressed || !over_ui {
            self.right_down = pressed;
            self.right_pan_reference = if pressed {
                pan_reference(camera, tree, self.mouse_position)
            } else {
                None
            };
        }
    }

    pub fn update(&mut self, camera: &mut Camera, tree: &mut DataTree) -> bool {
        let mut camera_moved = false;
        if let Some(initial) = self.pending_view_restore.take() {
            *camera = initial;
        }
        if self.view_rotation_requested {
            self.view_rotation_requested = false;
            let initial_scene_camera = if matches!(
                tree.get_path("editor.camera_view"),
                ClaydashValue::Bool(true)
            ) {
                crate::model::active_camera_id(tree).and_then(|id| {
                    crate::model::scene_cameras(tree)
                        .into_iter()
                        .find(|candidate| candidate.uuid == id)
                })
            } else {
                None
            };
            self.view_rotation = Some(ViewRotationSession {
                initial_view: camera.clone(),
                initial_scene_camera,
            });
            tree.set_transient_path("editor.view_rotation", ClaydashValue::Bool(true));
            self.mouse_delta = Vec2::ZERO;
        }
        if self.view_rotation.is_some() {
            if self.mouse_delta != Vec2::ZERO {
                let center = camera.viewport_origin + camera.viewport * 0.5;
                let previous = self.mouse_position - self.mouse_delta - center;
                let current = self.mouse_position - center;
                let angle = if previous.length() >= 12.0 && current.length() >= 12.0 {
                    previous.perp_dot(current).atan2(previous.dot(current))
                } else {
                    self.mouse_delta.x * 0.005
                };
                camera.rotate_in_place(-angle);
                self.write_view_rotation_camera(camera, tree);
                camera_moved = true;
            }
            self.mouse_delta = Vec2::ZERO;
            return camera_moved;
        }
        if let Some(session) = &mut self.shift_pan {
            if self.mouse_position.distance(session.start) >= PAN_DRAG_THRESHOLD {
                if let Some(reference) = session.reference {
                    camera.pan_to_cursor(self.mouse_position, reference);
                } else {
                    camera.pan(self.mouse_position - session.last_applied);
                }
                session.last_applied = self.mouse_position;
                camera_moved = true;
            }
        } else {
            let transforming = matches!(
                tree.get_path("editor.state"),
                ClaydashValue::EditorState(
                    EditorState::Grabbing
                        | EditorState::Scaling
                        | EditorState::Rotating
                        | EditorState::Extruding
                        | EditorState::DraggingFace
                        | EditorState::ExtendingCurve
                )
            );
            if !transforming
                && (self.keys.contains(&KeyCode::ControlLeft)
                    || self.keys.contains(&KeyCode::ControlRight))
                && self.mouse_delta != Vec2::ZERO
            {
                camera.orbit(self.mouse_delta);
                camera_moved = true;
            }
            if self.right_down && self.mouse_delta != Vec2::ZERO {
                if let Some(reference) = self.right_pan_reference {
                    camera.pan_to_cursor(self.mouse_position, reference);
                } else {
                    camera.pan(self.mouse_delta);
                }
                camera_moved = true;
            }
        }
        self.mouse_delta = Vec2::ZERO;
        self.update_transformation(camera, tree);
        camera_moved
    }

    fn write_view_rotation_camera(&self, camera: &Camera, tree: &mut DataTree) {
        let Some(session) = &self.view_rotation else {
            return;
        };
        let Some(initial) = &session.initial_scene_camera else {
            return;
        };
        let mut cameras = crate::model::scene_cameras(tree);
        if let Some(current) = cameras
            .iter_mut()
            .find(|candidate| candidate.uuid == initial.uuid)
        {
            let view = crate::camera::SceneCamera::from_view(&initial.name, camera);
            current.transform = view.transform;
            current.focal_distance = view.focal_distance;
            tree.set_transient_path("scene.cameras", ClaydashValue::VecCamera(cameras));
        }
    }

    fn restore_view_rotation(&mut self, session: ViewRotationSession, tree: &mut DataTree) {
        if let Some(initial) = session.initial_scene_camera {
            let mut cameras = crate::model::scene_cameras(tree);
            if let Some(current) = cameras
                .iter_mut()
                .find(|candidate| candidate.uuid == initial.uuid)
            {
                *current = initial;
                tree.set_transient_path("scene.cameras", ClaydashValue::VecCamera(cameras));
            }
        }
        // The viewport camera is restored on the next update, before rendering.
        self.pending_view_restore = Some(session.initial_view);
    }

    pub(super) fn finish_view_rotation(&mut self, tree: &mut DataTree) {
        if let Some(session) = self.view_rotation.take() {
            if let Some(initial) = session.initial_scene_camera {
                let final_cameras = crate::model::scene_cameras(tree);
                let mut initial_cameras = final_cameras.clone();
                if let Some(current) = initial_cameras
                    .iter_mut()
                    .find(|candidate| candidate.uuid == initial.uuid)
                {
                    *current = initial;
                    tree.set_transient_path(
                        "scene.cameras",
                        ClaydashValue::VecCamera(initial_cameras),
                    );
                    crate::model::set_scene_cameras(tree, final_cameras);
                    tree.make_undo_redo_snapshot();
                }
            }
        }
        self.view_rotation_requested = false;
        tree.set_transient_path("editor.view_rotation", ClaydashValue::Bool(false));
    }

    pub fn view_rotation_active(&self) -> bool {
        self.view_rotation.is_some()
            || self.view_rotation_requested
            || self.pending_view_restore.is_some()
    }
}

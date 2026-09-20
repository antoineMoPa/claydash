use super::*;

impl InteractionState {
    pub(super) fn update_transformation(&mut self, camera: &Camera, tree: &mut DataTree) {
        self.place_pending_spawn(camera, tree);
        let mode = match tree.get_path("editor.state") {
            ClaydashValue::EditorState(mode) => mode,
            _ => EditorState::Start,
        };
        if mode == EditorState::Start {
            self.transform_session = None;
            self.numeric_rotation = NumericRotationInput::Idle;
            return;
        }
        if self.transform_session.as_ref().is_none_or(|session| {
            session.mode != mode || session.selection != crate::model::selected_ref(tree)
        }) {
            self.begin_transform_session(mode, camera, tree);
        }
        if let Some(session) = &mut self.transform_session {
            if mode == EditorState::Rotating {
                let ctrl_snap = self.keys.contains(&KeyCode::ControlLeft)
                    || self.keys.contains(&KeyCode::ControlRight);
                let pointer_moved =
                    self.mouse_position.distance(session.last_mouse_position) > 0.001;
                session.rotation_snap_active =
                    rotation_snap_active(session.rotation_snap_active, ctrl_snap, pointer_moved);
                session.last_mouse_position = self.mouse_position;
            }
        }
        let Some(session) = self.transform_session.clone() else {
            return;
        };

        let constrain_x = matches!(
            tree.get_path("editor.constrain_x"),
            ClaydashValue::Bool(true)
        );
        let constrain_y = matches!(
            tree.get_path("editor.constrain_y"),
            ClaydashValue::Bool(true)
        );
        let constrain_z = matches!(
            tree.get_path("editor.constrain_z"),
            ClaydashValue::Bool(true)
        );
        let constrained = constrain_x || constrain_y || constrain_z;
        let mask = if constrained {
            Vec3::new(
                constrain_x as u8 as f32,
                constrain_y as u8 as f32,
                constrain_z as u8 as f32,
            )
        } else {
            Vec3::ONE
        };
        let current_cursor = if mode == EditorState::Grabbing {
            camera.cursor_on_plane(self.mouse_position, session.center)
        } else {
            camera.cursor_at_depth(self.mouse_position, session.center)
        };
        let mut scene = objects(tree);

        for target in &session.targets {
            let operation = match mode {
                EditorState::Grabbing => {
                    Mat4::from_translation((current_cursor - session.initial_cursor) * mask)
                }
                EditorState::Scaling => {
                    let factor = (current_cursor.distance(session.center) / session.initial_radius)
                        .max(0.001);
                    let factors = Vec3::ONE + (Vec3::splat(factor) - Vec3::ONE) * mask;
                    Mat4::from_translation(session.center)
                        * Mat4::from_scale(factors)
                        * Mat4::from_translation(-session.center)
                }
                EditorState::Rotating => {
                    let current_angle = camera.cursor_angle(self.mouse_position, session.center);
                    let raw_angle = current_angle - session.initial_angle;
                    let angle = rotation_drag_angle(raw_angle, session.rotation_snap_active);
                    let axis = if constrained {
                        mask.normalize_or_zero()
                    } else {
                        (camera.target - camera.position).normalize()
                    };
                    let numeric_angle = match &self.numeric_rotation {
                        NumericRotationInput::Editing(input) => input.parse::<f32>().ok(),
                        NumericRotationInput::Idle => None,
                    };
                    let pointer_angle = if constrained { -angle } else { angle };
                    let rotation = Quat::from_axis_angle(
                        axis,
                        numeric_angle.map_or(pointer_angle, f32::to_radians),
                    );
                    Mat4::from_translation(session.center)
                        * Mat4::from_quat(rotation)
                        * Mat4::from_translation(-session.center)
                }
                EditorState::Start => Mat4::IDENTITY,
            };
            let local = target.parent_world.inverse() * operation * target.world;
            let (scale, rotation, translation) = local.to_scale_rotation_translation();
            commands::set_transform_target(
                &mut scene,
                target.kind,
                target.id,
                crate::model::Transform {
                    translation,
                    rotation,
                    scale,
                },
            );
        }
        set_objects(tree, scene);
    }
}

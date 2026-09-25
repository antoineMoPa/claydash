use super::*;

impl InteractionState {
    pub(super) fn update_transformation(&mut self, camera: &Camera, tree: &mut DataTree) {
        self.place_pending_spawn(camera, tree);
        let mode = match tree.get_path("editor.state") {
            ClaydashValue::EditorState(mode) => mode,
            _ => EditorState::Start,
        };
        if mode == EditorState::ExtendingCurve {
            self.transform_session = None;
            self.extrusion_session = None;
            self.active_guide = None;
            self.update_curve_extension(camera, tree);
            return;
        }
        if mode == EditorState::Grabbing
            && matches!(
                tree.get_path("editor.curve_grab_initial"),
                ClaydashValue::VecSDFObject(_)
            )
        {
            self.transform_session = None;
            self.extrusion_session = None;
            self.active_guide = None;
            self.update_curve_grab(camera, tree);
            return;
        }
        if matches!(mode, EditorState::Extruding | EditorState::DraggingFace) {
            self.transform_session = None;
            self.numeric_rotation = NumericRotationInput::Idle;
            self.update_extrusion(mode, camera, tree);
            return;
        }
        self.extrusion_session = None;
        if mode == EditorState::Start {
            self.transform_session = None;
            self.curve_grab_mouse_start = None;
            self.active_guide = None;
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
        let grab_operation = if mode == EditorState::Grabbing {
            let raw_translation = (current_cursor - session.initial_cursor) * mask;
            let raw_anchors: Vec<_> = session
                .anchors
                .iter()
                .map(|anchor| *anchor + raw_translation)
                .collect();
            let bypass_guides =
                self.keys.contains(&KeyCode::AltLeft) || self.keys.contains(&KeyCode::AltRight);
            let snap = if bypass_guides {
                None
            } else if constrained {
                crate::guides::snap_along_line(
                    camera,
                    &raw_anchors,
                    mask,
                    &session.guides,
                    1.0,
                    self.active_guide,
                )
            } else {
                crate::guides::snap_in_view_plane(
                    camera,
                    &raw_anchors,
                    &session.guides,
                    1.0,
                    self.active_guide,
                )
            };
            self.active_guide = snap.map(|snap| snap.active);
            let correction = snap.map_or(Vec3::ZERO, |snap| snap.correction);
            Some(Mat4::from_translation(raw_translation + correction))
        } else {
            self.active_guide = None;
            None
        };
        let mut scene = objects(tree);
        let mut cameras = crate::model::scene_cameras(tree);

        for target in &session.targets {
            let operation = match mode {
                EditorState::Grabbing => grab_operation.unwrap_or(Mat4::IDENTITY),
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
                    let pointer_angle = if constrained { angle } else { -angle };
                    let rotation = Quat::from_axis_angle(
                        axis,
                        numeric_angle.map_or(pointer_angle, f32::to_radians),
                    );
                    Mat4::from_translation(session.center)
                        * Mat4::from_quat(rotation)
                        * Mat4::from_translation(-session.center)
                }
                EditorState::Start
                | EditorState::Extruding
                | EditorState::DraggingFace
                | EditorState::ExtendingCurve => Mat4::IDENTITY,
            };
            let local = target.parent_world.inverse() * operation * target.world;
            let (scale, rotation, translation) = local.to_scale_rotation_translation();
            commands::set_transform_target(
                &mut scene,
                &mut cameras,
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
        crate::model::set_scene_cameras(tree, cameras);
    }

    fn update_curve_extension(&mut self, camera: &Camera, tree: &mut DataTree) {
        let Some(point) = crate::model::selected_curve_point(tree) else {
            return;
        };
        let ClaydashValue::VecSDFObject(initial) = tree.get_path("editor.curve_extension_initial")
        else {
            return;
        };
        let Some(initial) = initial.iter().find(|object| object.uuid == point.object) else {
            return;
        };
        let crate::model::SdfParams::BezierCurveParams(initial_curve) = &initial.params else {
            return;
        };
        let mut scene = objects(tree);
        let matrix = crate::model::object_world_matrix(&scene, point.object);
        let Some(object) = scene.iter_mut().find(|object| object.uuid == point.object) else {
            return;
        };
        let crate::model::SdfParams::BezierCurveParams(curve) = &mut object.params else {
            return;
        };
        let from_start = point.index == 0;
        let Some(anchor) = (if from_start {
            initial_curve.points.first()
        } else {
            initial_curve.points.last()
        }) else {
            return;
        };
        let anchor = *anchor;
        if curve.points.len() < 2 {
            return;
        }
        let world_anchor = matrix.transform_point3(anchor);
        let world_pointer = camera.cursor_on_plane(self.mouse_position, world_anchor);
        let local_pointer = matrix.inverse().transform_point3(world_pointer);
        if from_start {
            curve.points[0] = local_pointer;
            curve.points[1] = local_pointer.lerp(anchor, 1.0 / 3.0);
        } else {
            let last = curve.points.len() - 1;
            curve.points[last] = local_pointer;
            curve.points[last - 1] = anchor.lerp(local_pointer, 2.0 / 3.0);
        }
        set_objects(tree, scene);
    }

    fn update_curve_grab(&mut self, camera: &Camera, tree: &mut DataTree) {
        let start_mouse = *self
            .curve_grab_mouse_start
            .get_or_insert(self.mouse_position);
        let Some(point) = crate::model::selected_curve_point(tree) else {
            return;
        };
        let ClaydashValue::VecSDFObject(initial) = tree.get_path("editor.curve_grab_initial")
        else {
            return;
        };
        let Some(initial) = initial.iter().find(|object| object.uuid == point.object) else {
            return;
        };
        let crate::model::SdfParams::BezierCurveParams(initial_curve) = &initial.params else {
            return;
        };
        let Some(&initial_point) = initial_curve.points.get(point.index) else {
            return;
        };
        let mut scene = objects(tree);
        let matrix = crate::model::object_world_matrix(&scene, point.object);
        let world_point = matrix.transform_point3(initial_point);
        let world_start = camera.cursor_on_plane(start_mouse, world_point);
        let world_current = camera.cursor_on_plane(self.mouse_position, world_point);
        let world_delta = world_current - world_start;
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
        let world_delta = if constrain_x || constrain_y || constrain_z {
            world_delta
                * Vec3::new(
                    constrain_x as u8 as f32,
                    constrain_y as u8 as f32,
                    constrain_z as u8 as f32,
                )
        } else {
            world_delta
        };
        let local_delta = matrix.inverse().transform_vector3(world_delta);
        let Some(object) = scene.iter_mut().find(|object| object.uuid == point.object) else {
            return;
        };
        let crate::model::SdfParams::BezierCurveParams(curve) = &mut object.params else {
            return;
        };
        let Some(current_point) = curve.points.get_mut(point.index) else {
            return;
        };
        *current_point = initial_point + local_delta;
        if curve.closed && point.index == 0 {
            let last = curve.points.len() - 1;
            curve.points[last] = curve.points[0];
        }
        set_objects(tree, scene);
    }

    fn update_extrusion(&mut self, mode: EditorState, camera: &Camera, tree: &mut DataTree) {
        let Some(face) = crate::model::selected_modeling_face(tree) else {
            tree.set_path(
                "editor.state",
                ClaydashValue::EditorState(EditorState::Start),
            );
            self.extrusion_session = None;
            self.active_guide = None;
            return;
        };
        let (axis, positive) = match face {
            crate::model::ModelingFaceSelection::Box(face) => (face.axis, face.positive),
            crate::model::ModelingFaceSelection::CylinderCap(face) => {
                (crate::model::VectorAxis::Y, face.positive)
            }
            crate::model::ModelingFaceSelection::PolygonPrism(_) => return,
        };
        if self
            .extrusion_session
            .as_ref()
            .is_none_or(|session| session.mode != mode || session.object != face.object())
        {
            let scene = objects(tree);
            let Some(object) = scene.iter().find(|object| object.uuid == face.object()) else {
                return;
            };
            let initial_half_extent = match (face, &object.params) {
                (
                    crate::model::ModelingFaceSelection::Box(_),
                    crate::model::SdfParams::BoxParams(params),
                ) => params.box_q[axis.index()],
                (
                    crate::model::ModelingFaceSelection::CylinderCap(_),
                    crate::model::SdfParams::CylinderParams { half_height, .. },
                ) => *half_height,
                _ => return,
            };
            let mut local_direction = Vec3::ZERO;
            local_direction[axis.index()] = if positive { 1.0 } else { -1.0 };
            let matrix = crate::model::object_world_matrix(&scene, face.object());
            let outer = matrix.transform_point3(local_direction * initial_half_extent);
            let direction = matrix.transform_vector3(local_direction);
            let Some(center) = camera.project(outer, 1.0) else {
                return;
            };
            let Some(ahead) = camera.project(outer + direction * 0.1, 1.0) else {
                return;
            };
            let mut projected_axis = Vec2::new(ahead.x - center.x, ahead.y - center.y) * 10.0;
            if projected_axis.length_squared() < 4.0 {
                let screen_up = camera.view().inverse().y_axis.truncate();
                if let Some(fallback) = camera.project(outer + screen_up * 0.1, 1.0) {
                    projected_axis = Vec2::new(fallback.x - center.x, fallback.y - center.y) * 10.0;
                }
            }
            if projected_axis.length_squared() < 0.0001 {
                return;
            }
            let mut excluded = vec![face.object()];
            match tree.get_path("editor.extrusion_source_face") {
                ClaydashValue::BoxFaceSelection(source) => excluded.push(source.object),
                ClaydashValue::ModelingFaceSelection(source) => excluded.push(source.object()),
                _ => {}
            }
            let guides = crate::guides::face_center_guides(&scene, &excluded);
            self.extrusion_session = Some(ExtrusionSession {
                mode,
                object: face.object(),
                axis,
                positive,
                start_mouse_position: self.mouse_position,
                projected_axis,
                initial_transform: object.transform,
                initial_half_extent,
                initial_face_center: outer,
                world_direction_per_unit: direction,
                guides,
            });
        }
        let Some(session) = self.extrusion_session.clone() else {
            return;
        };
        let mut amount = (self.mouse_position - session.start_mouse_position)
            .dot(session.projected_axis)
            / session.projected_axis.length_squared();
        let bypass_guides =
            self.keys.contains(&KeyCode::AltLeft) || self.keys.contains(&KeyCode::AltRight);
        let raw_face_center =
            session.initial_face_center + session.world_direction_per_unit * amount;
        let snap = if bypass_guides {
            None
        } else {
            crate::guides::snap_along_line(
                camera,
                &[raw_face_center],
                session.world_direction_per_unit,
                &session.guides,
                1.0,
                self.active_guide,
            )
        };
        self.active_guide = snap.map(|snap| snap.active);
        if let Some(snap) = snap {
            let direction_length = session.world_direction_per_unit.length().max(0.0001);
            amount += snap
                .correction
                .dot(session.world_direction_per_unit / direction_length)
                / direction_length;
        }
        let half_extent = (session.initial_half_extent + amount * 0.5).max(0.01);
        let half_extent_delta = half_extent - session.initial_half_extent;
        let mut local_direction = Vec3::ZERO;
        local_direction[session.axis.index()] = if session.positive { 1.0 } else { -1.0 };
        let mut scene = objects(tree);
        let Some(object) = scene
            .iter_mut()
            .find(|object| object.uuid == session.object)
        else {
            return;
        };
        match &mut object.params {
            crate::model::SdfParams::BoxParams(params) => {
                params.box_q[session.axis.index()] = half_extent;
            }
            crate::model::SdfParams::CylinderParams { half_height, .. }
                if session.axis == crate::model::VectorAxis::Y =>
            {
                *half_height = half_extent;
            }
            _ => return,
        }
        object.transform = session.initial_transform;
        object.transform.translation += session
            .initial_transform
            .matrix()
            .transform_vector3(local_direction * half_extent_delta);
        set_objects(tree, scene);
    }
}

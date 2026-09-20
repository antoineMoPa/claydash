use super::*;

impl InteractionState {
    pub fn pointer_down(
        &mut self,
        camera: &Camera,
        tree: &mut DataTree,
        ghost: Option<uuid::Uuid>,
    ) {
        if matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Grabbing)
        ) {
            return; // A grab commits on release, not on press.
        }
        if !matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Start) | ClaydashValue::None
        ) {
            tree.set_path(
                "editor.state",
                ClaydashValue::EditorState(EditorState::Start),
            );
            tree.make_undo_redo_snapshot();
            self.numeric_rotation = NumericRotationInput::Idle;
            return;
        }
        let shift =
            self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
        if shift {
            self.shift_pan = Some(ShiftPanSession {
                start: self.mouse_position,
                last_applied: self.mouse_position,
                ghost,
                reference: pan_reference(camera, tree, self.mouse_position),
            });
            return;
        }
        Self::select_at(camera, tree, self.mouse_position, ghost, shift);
    }

    pub(crate) fn select_at(
        camera: &Camera,
        tree: &mut DataTree,
        position: Vec2,
        ghost: Option<uuid::Uuid>,
        shift: bool,
    ) {
        let (origin, direction) = camera.ray(position);
        let scene = objects(tree);
        let ghost = ghost.filter(|id| scene.iter().any(|object| object.uuid == *id));
        if let Some(pick) = crate::ui::scene_actions::pending_boolean(tree) {
            let ghost = ghost
                .filter(|id| crate::ui::scene_actions::can_attach(&scene, pick.target, &[*id]));
            // Ignore the target's group so an overlapping target cannot swallow
            // every click intended for the second object.
            let target_root = crate::ui::scene_actions::viewport_group_root(&scene, pick.target);
            let candidates: Vec<_> = scene
                .iter()
                .filter(|object| {
                    let root = crate::ui::scene_actions::viewport_group_root(&scene, object.uuid);
                    root != target_root
                })
                .cloned()
                .collect();
            let hit = ghost.or_else(|| raymarch(origin, direction, &candidates));
            if let Some(hit) = hit {
                let operand = ghost
                    .unwrap_or_else(|| crate::ui::scene_actions::viewport_group_root(&scene, hit));
                crate::ui::scene_actions::apply_boolean_pick(tree, operand);
            }
            return;
        }
        let Some(hit) = ghost.or_else(|| raymarch(origin, direction, &scene)) else {
            set_selected(tree, vec![]);
            return;
        };
        let mut selection = selected(tree);
        if shift {
            let target = crate::ui::scene_actions::viewport_group_root(&scene, hit);
            if selection.contains(&target) {
                selection.retain(|uuid| *uuid != target);
            } else {
                selection.push(target);
            }
        } else {
            let target =
                crate::ui::scene_actions::viewport_selection_target(&scene, hit, &selection);
            if target.scope == crate::model::SelectionScope::Exact {
                set_selected_exact(tree, vec![target.id]);
            } else {
                set_selected(tree, vec![target.id]);
            }
            return;
        }
        set_selected(tree, selection);
    }

    pub fn pointer_up(&mut self, camera: &Camera, tree: &mut DataTree) {
        if let Some(session) = self.shift_pan.take() {
            if self.mouse_position.distance(session.start) < PAN_DRAG_THRESHOLD {
                Self::select_at(camera, tree, session.start, session.ghost, true);
            }
            return;
        }
        if matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Grabbing)
        ) {
            self.update_transformation(camera, tree);
            tree.set_path(
                "editor.state",
                ClaydashValue::EditorState(EditorState::Start),
            );
            self.transform_session = None;
            self.numeric_rotation = NumericRotationInput::Idle;
            tree.make_undo_redo_snapshot();
        }
    }

    pub(super) fn begin_transform_session(
        &mut self,
        mode: EditorState,
        camera: &Camera,
        tree: &mut DataTree,
    ) {
        let scene = objects(tree);
        let targets = commands::transform_targets(tree);
        if targets.is_empty() {
            tree.set_path(
                "editor.state",
                ClaydashValue::EditorState(EditorState::Start),
            );
            return;
        }

        let selection = commands::effective_selected_ids(tree);
        let selected_world: Vec<_> = scene
            .iter()
            .filter(|object| selection.contains(&object.uuid))
            .map(|object| {
                crate::model::object_world_matrix(&scene, object.uuid).transform_point3(Vec3::ZERO)
            })
            .collect();
        let center = selected_world.iter().copied().sum::<Vec3>() / selected_world.len() as f32;
        for target in &targets {
            let path = match target.kind {
                commands::TransformTargetKind::Object => "editor.initial_transform",
                commands::TransformTargetKind::Group => "editor.initial_group_transform",
            };
            tree.set_path(
                &format!("{path}.{}", target.id),
                ClaydashValue::Transform(target.transform),
            );
        }
        let initial_cursor = if mode == EditorState::Grabbing {
            camera.cursor_on_plane(self.mouse_position, center)
        } else {
            camera.cursor_at_depth(self.mouse_position, center)
        };
        self.transform_session = Some(TransformSession {
            mode,
            selection: selected(tree),
            center,
            initial_cursor,
            initial_angle: camera.cursor_angle(self.mouse_position, center),
            initial_radius: initial_cursor.distance(center).max(0.001),
            last_mouse_position: self.mouse_position,
            rotation_snap_active: self.keys.contains(&KeyCode::ControlLeft)
                || self.keys.contains(&KeyCode::ControlRight),
            targets,
        });
    }

    pub fn place_pending_spawn(&mut self, camera: &Camera, tree: &mut DataTree) {
        let ClaydashValue::Uuid(id) = tree.get_path("editor.spawn_at_cursor") else {
            return;
        };
        tree.set_path("editor.spawn_at_cursor", ClaydashValue::None);
        if !matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Grabbing)
        ) {
            return;
        }
        let mut scene = objects(tree);
        let Some(object) = scene.iter_mut().find(|object| object.uuid == id) else {
            return;
        };
        object.transform.translation = camera.cursor_at_depth(self.mouse_position, camera.target);
        set_objects(tree, scene);
        self.begin_transform_session(EditorState::Grabbing, camera, tree);
    }
}

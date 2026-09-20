use super::*;

impl UiState {
    fn draw_transform_gizmo(
        &mut self,
        ui: &mut egui::Ui,
        geometry: &GizmoGeometry,
        pointer: Option<egui::Pos2>,
        camera: &Camera,
        snap_rotation: bool,
    ) {
        let hovered = pointer.and_then(|point| gizmo_action_at(point, geometry));
        let (active, active_start_pointer, active_initial_angle, active_initial_axis_vector) =
            match &self.selection_tools.gesture {
                Some(Gesture::Transform(session)) => (
                    Some(session.action),
                    Some(session.start_pointer),
                    Some(session.initial_angle),
                    session.initial_axis_vector,
                ),
                _ => (None, None, None, None),
            };
        let emphasized = |action| hovered == Some(action) || active == Some(action);
        let stroke_width = |action| {
            if action == GizmoAction::MoveFree {
                1.7
            } else if emphasized(action) {
                2.8
            } else {
                1.6
            }
        };
        let rotation_stroke_width = |action| if emphasized(action) { 3.6 } else { 2.6 };
        let painter = ui.painter();

        for arc in &geometry.rotation_arcs {
            let action = GizmoAction::RotateAxis(arc.axis);
            let points = if active == Some(action) {
                pointer
                    .zip(active_initial_angle)
                    .map(|(point, initial_angle)| {
                        let delta = point - geometry.center;
                        let current_angle = delta.y.atan2(delta.x);
                        let raw_angle = current_angle - initial_angle;
                        let applied_angle = active_initial_axis_vector
                            .and_then(|initial| {
                                axis_rotation_drag_angle(
                                    camera,
                                    Vec2::new(point.x, point.y) * geometry.pixels_per_point,
                                    geometry.center_world,
                                    world_axis(arc.axis),
                                    initial,
                                    snap_rotation,
                                )
                            })
                            .unwrap_or_else(|| {
                                -crate::interactions::rotation_drag_angle(raw_angle, snap_rotation)
                            });
                        rotation_arc_points(
                            camera,
                            geometry.center_world,
                            geometry.pixels_per_point,
                            arc.u,
                            arc.v,
                            geometry.world_ring_radius,
                            arc.middle_angle + applied_angle,
                        )
                    })
                    .unwrap_or_else(|| arc.points.clone())
            } else {
                arc.points.clone()
            };
            painter.add(egui::Shape::line(
                points.clone(),
                Stroke::new(
                    rotation_stroke_width(action),
                    gizmo_color(axis_color(arc.axis), emphasized(action)),
                ),
            ));
            register_polyline_regions(&mut self.regions, &points);
        }

        let view_action = GizmoAction::RotateView;
        let (view_rotate, view_ring) = if active == Some(view_action) {
            if let (Some(point), Some(initial_angle)) = (pointer, active_initial_angle) {
                let delta = point - geometry.center;
                let current_angle = delta.y.atan2(delta.x);
                let angle = view_rotation_gizmo_angle(initial_angle, current_angle, snap_rotation);
                let direction = egui::vec2(angle.cos(), angle.sin());
                (
                    geometry.center + direction * VIEW_RING_RADIUS,
                    screen_arc(
                        geometry.center,
                        VIEW_RING_RADIUS,
                        angle,
                        ROTATION_ARC_DEGREES.to_radians(),
                    ),
                )
            } else {
                (geometry.view_rotate, geometry.view_ring.clone())
            }
        } else {
            (geometry.view_rotate, geometry.view_ring.clone())
        };
        let view_stroke = Stroke::new(
            rotation_stroke_width(view_action),
            gizmo_color(Color32::WHITE, emphasized(view_action)),
        );
        painter.add(egui::Shape::line(view_ring.clone(), view_stroke));
        register_polyline_regions(&mut self.regions, &view_ring);

        for axis in &geometry.axes {
            let scale_action = GizmoAction::ScaleAxis(axis.axis);
            let move_action = GizmoAction::MoveAxis(axis.axis);
            let scale_color = gizmo_color(axis_color(axis.axis), emphasized(scale_action));
            let move_color = gizmo_color(axis_color(axis.axis), emphasized(move_action));
            let scale_stroke = Stroke::new(stroke_width(scale_action), scale_color);
            let move_stroke = Stroke::new(stroke_width(move_action), move_color);
            let scale_handle = if active == Some(scale_action) {
                pointer
                    .zip(active_start_pointer)
                    .map(|(point, start)| {
                        axis.scale_handle + axis.direction * (point - start).dot(axis.direction)
                    })
                    .unwrap_or(axis.scale_handle)
            } else {
                axis.scale_handle
            };
            painter.line_segment([geometry.center, scale_handle], scale_stroke);
            let arrow_base = axis.move_tip - axis.direction * 11.0;
            paint_dashed_line(painter, scale_handle, arrow_base, move_stroke);

            let scale_rect = egui::Rect::from_center_size(scale_handle, egui::vec2(13.0, 13.0));
            painter.rect_stroke(scale_rect, 1.0, scale_stroke, egui::StrokeKind::Inside);

            let perpendicular = egui::vec2(-axis.direction.y, axis.direction.x);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    axis.move_tip,
                    arrow_base + perpendicular * 6.0,
                    arrow_base - perpendicular * 6.0,
                ],
                move_color,
                move_stroke,
            ));
            self.regions.push(
                egui::Rect::from_center_size(
                    scale_handle,
                    egui::Vec2::splat(HANDLE_HIT_RADIUS * 2.0),
                )
                .intersect(ui.clip_rect()),
            );
            self.regions.push(
                egui::Rect::from_center_size(
                    axis.move_tip,
                    egui::Vec2::splat(HANDLE_HIT_RADIUS * 2.0),
                )
                .intersect(ui.clip_rect()),
            );
        }

        let uniform_action = GizmoAction::ScaleUniform;
        let uniform_scale = if active == Some(uniform_action) {
            pointer.unwrap_or(geometry.uniform_scale)
        } else {
            geometry.uniform_scale
        };
        let uniform_stroke = Stroke::new(
            stroke_width(uniform_action),
            gizmo_color(Color32::WHITE, emphasized(uniform_action)),
        );
        paint_dashed_line(painter, geometry.center, uniform_scale, uniform_stroke);
        let uniform_rect = egui::Rect::from_center_size(uniform_scale, egui::vec2(15.0, 15.0));
        painter.rect_stroke(uniform_rect, 1.0, uniform_stroke, egui::StrokeKind::Inside);
        self.regions.push(
            egui::Rect::from_center_size(uniform_scale, egui::Vec2::splat(HANDLE_HIT_RADIUS * 2.0))
                .intersect(ui.clip_rect()),
        );

        painter.circle_filled(view_rotate, 7.0, gizmo_fill(emphasized(view_action)));
        painter.circle_stroke(view_rotate, 7.0, view_stroke);
        self.regions.push(
            egui::Rect::from_center_size(view_rotate, egui::Vec2::splat(HANDLE_HIT_RADIUS * 2.0))
                .intersect(ui.clip_rect()),
        );

        painter.circle_filled(geometry.center, 3.5, Color32::from_black_alpha(115));
        painter.circle_stroke(
            geometry.center,
            3.5,
            Stroke::new(1.3, gizmo_color(Color32::WHITE, hovered.is_some())),
        );

        let free_action = GizmoAction::MoveFree;
        paint_move_glyph(
            painter,
            geometry.free_move,
            Stroke::new(
                stroke_width(free_action),
                gizmo_color(Color32::WHITE, emphasized(free_action)),
            ),
        );
        // Keep this last: focused gesture tests and assistive tooling identify
        // the free-move control as the final discrete gizmo region.
        self.regions.push(
            egui::Rect::from_center_size(geometry.free_move, egui::vec2(28.0, 28.0))
                .intersect(ui.clip_rect()),
        );
    }

    pub(in crate::ui) fn draw_selection_tools(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
    ) {
        let scale = ui.ctx().pixels_per_point();
        let (pointer, pressed, released, cancel, additive, navigating, ctrl_snap_rotation) = ui
            .input(|i| {
                (
                    i.pointer.interact_pos(),
                    i.pointer.primary_pressed(),
                    i.pointer.primary_released(),
                    i.key_pressed(egui::Key::Escape) || !i.focused,
                    i.modifiers.shift,
                    i.modifiers.ctrl || i.pointer.secondary_down(),
                    i.modifiers.ctrl,
                )
            });
        let snap_rotation = match &mut self.selection_tools.gesture {
            Some(Gesture::Transform(session))
                if matches!(
                    session.action,
                    GizmoAction::RotateView | GizmoAction::RotateAxis(_)
                ) =>
            {
                let pointer_moved =
                    pointer.is_some_and(|point| point.distance(session.last_pointer) > 0.001);
                session.rotation_snap_active = crate::interactions::rotation_snap_active(
                    session.rotation_snap_active,
                    ctrl_snap_rotation,
                    pointer_moved,
                );
                if let Some(point) = pointer {
                    session.last_pointer = point;
                }
                session.rotation_snap_active
            }
            _ => ctrl_snap_rotation,
        };
        if cancel {
            if let Some(Gesture::Transform(session)) = self.selection_tools.gesture.take() {
                let mut scene = objects(tree);
                let mut cameras = crate::model::scene_cameras(tree);
                for target in session.targets {
                    commands::set_transform_target(
                        &mut scene,
                        &mut cameras,
                        target.kind,
                        target.id,
                        target.transform,
                    );
                }
                set_objects(tree, scene);
                crate::model::set_scene_cameras(tree, cameras);
            }
            self.selection_tools.gesture = None;
            return;
        }
        if !matches!(
            tree.get_path("editor.state"),
            ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
        ) {
            return;
        }
        let scene = objects(tree);
        let selection = selected(tree);
        let transform_targets = commands::transform_targets(tree);
        if !self.selection_tools.box_mode() && !transform_targets.is_empty() {
            let center = transform_targets
                .iter()
                .map(|target| target.world.transform_point3(Vec3::ZERO))
                .sum::<Vec3>()
                / transform_targets.len() as f32;
            if let Some(geometry) = selection_gizmo_geometry(camera, center, scale) {
                self.draw_transform_gizmo(ui, &geometry, pointer, camera, snap_rotation);
                let hovered_action = pointer.and_then(|point| gizmo_action_at(point, &geometry));
                if let (Some(point), Some(action)) = (pointer, hovered_action) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    ui.interact(
                        egui::Rect::from_center_size(point, egui::vec2(2.0, 2.0)),
                        egui::Id::new(("selection-transform-tooltip", action)),
                        egui::Sense::hover(),
                    )
                    .on_hover_text(action.tooltip());
                    if pressed && !navigating && !self.selection_tools.active() {
                        let physical = Vec2::new(point.x, point.y) * scale;
                        let delta = point - geometry.center;
                        let axis_screen_direction = match action {
                            GizmoAction::MoveAxis(axis) | GizmoAction::ScaleAxis(axis) => geometry
                                .axes
                                .iter()
                                .find(|gizmo| gizmo.axis == axis)
                                .map(|gizmo| gizmo.direction),
                            _ => None,
                        };
                        let initial_axis_vector = match action {
                            GizmoAction::RotateAxis(axis) => cursor_vector_on_axis_plane(
                                camera,
                                physical,
                                center,
                                world_axis(axis),
                            ),
                            _ => None,
                        };
                        self.selection_tools.gesture = Some(Gesture::Transform(TransformGesture {
                            action,
                            start: camera.cursor_on_plane(physical, center),
                            start_pointer: point,
                            last_pointer: point,
                            center,
                            initial_angle: delta.y.atan2(delta.x),
                            initial_axis_vector,
                            initial_radius: delta.length().max(0.001),
                            axis_screen_direction,
                            world_units_per_point: geometry.world_units_per_point,
                            rotation_snap_active: snap_rotation,
                            targets: transform_targets.clone(),
                        }));
                    }
                }
            }
        }
        if self.selection_tools.box_mode()
            && !egui::Popup::is_any_open(ui.ctx())
            && pressed
            && !navigating
            && !self.selection_tools.active()
        {
            if let Some(p) = pointer.filter(|p| {
                ui.clip_rect().contains(*p) && !self.regions.iter().any(|r| r.contains(*p))
            }) {
                self.selection_tools.gesture = Some(Gesture::Box {
                    start: p,
                    end: p,
                    additive,
                });
            }
        }
        match &mut self.selection_tools.gesture {
            Some(Gesture::Transform(session)) => {
                if let Some(p) = pointer {
                    let mut scene = scene.clone();
                    let mut cameras = crate::model::scene_cameras(tree);
                    let operation = match session.action {
                        GizmoAction::MoveFree => {
                            let physical = Vec2::new(p.x, p.y) * scale;
                            let delta =
                                camera.cursor_on_plane(physical, session.center) - session.start;
                            glam::Mat4::from_translation(delta)
                        }
                        GizmoAction::MoveAxis(axis) => {
                            let screen_direction =
                                session.axis_screen_direction.unwrap_or_default();
                            let amount = (p - session.start_pointer).dot(screen_direction)
                                * session.world_units_per_point;
                            glam::Mat4::from_translation(world_axis(axis) * amount)
                        }
                        GizmoAction::ScaleUniform => {
                            let center_screen = camera.project(session.center, scale).unwrap_or(p);
                            let factor =
                                (p.distance(center_screen) / session.initial_radius).max(0.001);
                            glam::Mat4::from_translation(session.center)
                                * glam::Mat4::from_scale(Vec3::splat(factor))
                                * glam::Mat4::from_translation(-session.center)
                        }
                        GizmoAction::ScaleAxis(axis) => {
                            let screen_direction =
                                session.axis_screen_direction.unwrap_or_default();
                            let factor = (1.0
                                + (p - session.start_pointer).dot(screen_direction)
                                    / SCALE_HANDLE_DISTANCE)
                                .max(0.001);
                            let mut factors = Vec3::ONE;
                            factors[axis] = factor;
                            glam::Mat4::from_translation(session.center)
                                * glam::Mat4::from_scale(factors)
                                * glam::Mat4::from_translation(-session.center)
                        }
                        GizmoAction::RotateView | GizmoAction::RotateAxis(_) => {
                            let center_screen = camera.project(session.center, scale).unwrap_or(p);
                            let delta = p - center_screen;
                            let raw_angle = delta.y.atan2(delta.x) - session.initial_angle;
                            let angle =
                                crate::interactions::rotation_drag_angle(raw_angle, snap_rotation);
                            let (rotation_axis, applied_angle) = match session.action {
                                GizmoAction::RotateAxis(axis) => {
                                    let rotation_axis = world_axis(axis);
                                    let world_angle = session
                                        .initial_axis_vector
                                        .and_then(|initial| {
                                            axis_rotation_drag_angle(
                                                camera,
                                                Vec2::new(p.x, p.y) * scale,
                                                session.center,
                                                rotation_axis,
                                                initial,
                                                snap_rotation,
                                            )
                                        })
                                        .unwrap_or(-angle);
                                    (rotation_axis, world_angle)
                                }
                                GizmoAction::RotateView => {
                                    let view = camera.target - camera.position;
                                    (view / view.length().max(0.0001), angle)
                                }
                                _ => unreachable!(),
                            };
                            glam::Mat4::from_translation(session.center)
                                * glam::Mat4::from_quat(glam::Quat::from_axis_angle(
                                    rotation_axis,
                                    applied_angle,
                                ))
                                * glam::Mat4::from_translation(-session.center)
                        }
                    };
                    for target in &session.targets {
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
                if released {
                    tree.make_undo_redo_snapshot();
                }
            }
            Some(Gesture::Box {
                start,
                end,
                additive,
            }) => {
                if let Some(p) = pointer {
                    *end = ui.clip_rect().clamp(p);
                }
                let rect = egui::Rect::from_two_pos(*start, *end);
                let color = ui.visuals().selection.bg_fill;
                ui.painter()
                    .rect_filled(rect, 0.0, color.gamma_multiply(0.15));
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
                if released {
                    // Match UI coordinates so the click tolerance stays consistent
                    // across display scales. Exactly 3 px remains a box gesture.
                    if start.distance(*end) < 3.0 {
                        self.selection_tools.tool = SelectionTool::Select;
                        crate::interactions::InteractionState::select_at(
                            camera,
                            tree,
                            Vec2::new(end.x, end.y) * scale,
                            self.ghosts.pick(*end),
                            *additive,
                        );
                    } else {
                        set_selected(
                            tree,
                            box_selection(
                                &scene,
                                camera,
                                rect,
                                scale,
                                if *additive { selection } else { vec![] },
                            ),
                        );
                    }
                }
            }
            None => {}
        }
        if released {
            self.selection_tools.gesture = None;
        }
    }
}

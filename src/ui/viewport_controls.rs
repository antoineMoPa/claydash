use super::*;

impl UiState {
    pub(super) fn handle_animation_shortcuts(&mut self, ctx: &egui::Context, tree: &mut DataTree) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (toggle, step, insert_keyframe) = ctx.input(|input| {
            let toggle = input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Space,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if modifiers.is_none()
                )
            });
            let step = input.events.iter().find_map(|event| {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    return None;
                };
                if !modifiers.is_none() && *modifiers != egui::Modifiers::SHIFT {
                    return None;
                }
                let direction = match key {
                    egui::Key::ArrowLeft => -1.0,
                    egui::Key::ArrowRight => 1.0,
                    _ => return None,
                };
                Some(direction * if modifiers.shift { 10.0 } else { 1.0 })
            });
            let insert_keyframe = input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::I,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if modifiers.is_none()
                )
            });
            (toggle, step, insert_keyframe)
        });
        if insert_keyframe && !selected(tree).is_empty() {
            self.insert_keyframe_menu_position = Some(
                ctx.pointer_latest_pos()
                    .unwrap_or_else(|| ctx.content_rect().center()),
            );
        } else if toggle {
            self.animation.toggle_playback();
        } else if let Some(step) = step {
            self.animation.playing = false;
            self.animation
                .set_frame(tree, self.animation.current_frame.round() + step);
        }
    }

    pub(super) fn draw_insert_keyframe_menu(&mut self, ctx: &egui::Context, tree: &mut DataTree) {
        let Some(cursor_position) = self.insert_keyframe_menu_position else {
            return;
        };
        let mut open = true;
        let mut properties = None;
        let mut insert_lattice = false;
        let lattice_available = objects(tree).iter().any(|object| {
            selected(tree).contains(&object.uuid)
                && object
                    .lattice
                    .as_ref()
                    .is_some_and(|lattice| lattice.current_shape_key.is_some())
        });
        let style = ctx.style_of(ctx.theme());
        let response = egui::Window::new("Insert keyframe")
            .id(egui::Id::new("insert-keyframe-menu"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .current_pos(cursor_position + egui::vec2(8.0, 8.0))
            .frame(egui::Frame::popup(&style).inner_margin(egui::Margin::symmetric(8, 6)))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.set_min_width(174.0);
                ui.strong("Insert Keyframe");
                for (label, value) in [
                    ("Location", TransformKeySet::Location),
                    ("Rotation", TransformKeySet::Rotation),
                    ("Scale", TransformKeySet::Scale),
                    (
                        "Location, Rotation & Scale",
                        TransformKeySet::LocationRotationScale,
                    ),
                ] {
                    if ui.button(label).clicked() {
                        properties = Some(value);
                    }
                }
                if lattice_available && ui.button("Lattice shape").clicked() {
                    insert_lattice = true;
                }
            });
        if let Some(response) = response {
            self.regions.push(response.response.rect);
            let close_from_input = ctx.input(|input| {
                input.key_pressed(egui::Key::Escape)
                    || input.pointer.any_pressed()
                        && input
                            .pointer
                            .interact_pos()
                            .is_none_or(|position| !response.response.rect.contains(position))
            });
            open &= !close_from_input;
        }
        if let Some(properties) = properties {
            insert_transform_keyframes(tree, &mut self.animation, properties);
            open = false;
        }
        if insert_lattice {
            let frame = self.animation.current_frame.round().max(0.0) as u32;
            for id in selected(tree) {
                if animation::insert_lattice_keyframe(tree, id, frame) {
                    self.animation.selected_keyframes = vec![SelectedKeyframe {
                        binding: AnimationBinding {
                            object: id,
                            property: AnimatableProperty::LatticeShape,
                        },
                        frame,
                    }];
                }
            }
            tree.make_undo_redo_snapshot();
            open = false;
        }
        self.insert_keyframe_menu_position = open.then_some(cursor_position);
    }

    pub(super) fn draw_top_controls(
        &mut self,
        ctx: &egui::Context,
        tree: &mut DataTree,
        camera: &mut Camera,
    ) {
        let viewport = self.viewport_rect.unwrap();
        let left = egui::Area::new("top-left-tools".into())
            .order(egui::Order::Foreground)
            .fixed_pos(self.viewport_rect.unwrap().left_top() + egui::vec2(6.0, 6.0))
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport);
                ui.set_max_width((viewport.width() - 12.0).max(80.0));
                egui::Frame::NONE.show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    ui.horizontal_wrapped(|ui| {
                        if viewport.width() >= 400.0 {
                            for kind in PrimitiveKind::SPAWNABLE {
                                if view_button(
                                    ui,
                                    primitive_icon_source(kind),
                                    &format!("Add {}", kind.label()),
                                )
                                .clicked()
                                {
                                    commands::spawn(tree, kind.object_type());
                                }
                            }
                        } else {
                            ui.scope(|ui| {
                                style_view_buttons(ui);
                                egui::containers::menu::MenuButton::from_button(view_icon_button(
                                    egui::include_image!("../../assets/icons/lucide/plus.svg"),
                                    false,
                                ))
                                .ui(ui, |ui| {
                                    for kind in PrimitiveKind::SPAWNABLE {
                                        if ui
                                            .add(egui::Button::image_and_text(
                                                icon_image(
                                                    primitive_icon_source(kind),
                                                    ui.visuals().text_color(),
                                                ),
                                                kind.label(),
                                            ))
                                            .clicked()
                                        {
                                            commands::spawn(tree, kind.object_type());
                                            ui.close();
                                        }
                                    }
                                })
                                .0
                                .on_hover_text("Add object");
                            });
                        }
                        if view_button(
                            ui,
                            egui::include_image!("../../assets/icons/lucide/command.svg"),
                            "Command palette (Cmd/Ctrl+Shift+P)",
                        )
                        .clicked()
                        {
                            self.palette.open();
                        }
                        if view_button(
                            ui,
                            egui::include_image!("../../assets/icons/lucide/undo-2.svg"),
                            "Undo",
                        )
                        .clicked()
                        {
                            undo_redo::undo(tree);
                        }
                        if view_button(
                            ui,
                            egui::include_image!("../../assets/icons/lucide/redo-2.svg"),
                            "Redo",
                        )
                        .clicked()
                        {
                            undo_redo::redo(tree);
                        }
                    });
                });
            });
        self.regions.push(left.response.rect);

        // Move the right-hand group below the tools when space is tight.
        let projection_y = if viewport.width() < left.response.rect.width() + 130.0 {
            left.response.rect.height() + 10.0
        } else {
            6.0
        };
        let right = egui::Area::new("top-right-tools".into())
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(viewport.right_top() + egui::vec2(-6.0, projection_y))
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport);
                ui.horizontal(|ui| {
                    ui.set_height(26.0);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (projection_icon, next_mode) = match camera.projection_mode {
                        crate::camera::ProjectionMode::Perspective => (
                            egui::include_image!("../../assets/icons/lucide/box.svg"),
                            "orthographic",
                        ),
                        crate::camera::ProjectionMode::Orthographic => (
                            egui::include_image!("../../assets/icons/lucide/square.svg"),
                            "perspective",
                        ),
                    };
                    let tooltip =
                        format!("{} — click for {next_mode}", camera.projection_mode.label());
                    if view_button(ui, projection_icon, &tooltip).clicked() {
                        exit_camera_view(tree);
                        camera.toggle_projection();
                    }
                    if view_button(
                        ui,
                        egui::include_image!("../../assets/icons/lucide/rotate-3d.svg"),
                        "Snap to isometric view",
                    )
                    .clicked()
                    {
                        exit_camera_view(tree);
                        camera.snap(ViewAngle::Isometric);
                    }
                    let camera_view = matches!(
                        tree.get_path("editor.camera_view"),
                        crate::model::ClaydashValue::Bool(true)
                    );
                    if selectable_view_button(
                        ui,
                        egui::include_image!("../../assets/icons/lucide/camera.svg"),
                        "Toggle active camera view",
                        camera_view,
                    )
                    .clicked()
                    {
                        toggle_camera_view(tree, camera);
                    }
                });
            });
        self.regions.push(right.response.rect);
        self.draw_selection_toolbar(ctx, viewport, tree);
    }

    pub(super) fn draw_view_gizmo(
        &mut self,
        ctx: &egui::Context,
        tree: &mut DataTree,
        camera: &mut Camera,
    ) {
        let viewport = self.viewport_rect.unwrap();
        if viewport.height() < 250.0 || viewport.width() < 150.0 {
            return;
        }
        let offset = camera.position - camera.target;
        let yaw = offset.x.atan2(offset.z).to_degrees();
        let pitch = (offset.y / offset.length().max(0.001)).asin().to_degrees();
        let area = egui::Area::new("view-gizmo".into())
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(self.viewport_rect.unwrap().left_bottom() + egui::vec2(10.0, -10.0))
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport);
                egui::Frame::NONE
                    .inner_margin(egui::Margin::same(7))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("{yaw:.0}°  {pitch:.0}°"))
                                .small()
                                .color(Color32::LIGHT_GRAY),
                        );
                        orientation_axes(ui, tree, camera);
                    });
            });
        self.regions.push(area.response.rect);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransformKeySet {
    Location,
    Rotation,
    Scale,
    LocationRotationScale,
}

pub(super) fn insert_transform_keyframes(
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
    key_set: TransformKeySet,
) {
    let targets = commands::transform_targets(tree);
    let frame = runtime.current_frame.round().max(0.0) as u32;
    let include_location = matches!(
        key_set,
        TransformKeySet::Location | TransformKeySet::LocationRotationScale
    );
    let include_rotation = matches!(
        key_set,
        TransformKeySet::Rotation | TransformKeySet::LocationRotationScale
    );
    let include_scale = matches!(
        key_set,
        TransformKeySet::Scale | TransformKeySet::LocationRotationScale
    );
    for target in targets {
        let (rotation_x, rotation_y, rotation_z) =
            target.transform.rotation.to_euler(EulerRot::XYZ);
        let rotation_degrees = Vec3::new(
            rotation_x.to_degrees(),
            rotation_y.to_degrees(),
            rotation_z.to_degrees(),
        );
        for axis in VectorAxis::ALL {
            let (position, rotation, scale) = match target.kind {
                commands::TransformTargetKind::Group => (
                    AnimatableProperty::GroupPosition(axis),
                    AnimatableProperty::GroupRotation(axis),
                    AnimatableProperty::GroupScale(axis),
                ),
                commands::TransformTargetKind::Object | commands::TransformTargetKind::Camera => (
                    AnimatableProperty::Position(axis),
                    AnimatableProperty::Rotation(axis),
                    AnimatableProperty::Scale(axis),
                ),
            };
            for (property, value) in [
                include_location.then_some((position, target.transform.translation[axis.index()])),
                include_rotation.then_some((rotation, rotation_degrees[axis.index()])),
                include_scale.then_some((scale, target.transform.scale[axis.index()])),
            ]
            .into_iter()
            .flatten()
            {
                let binding = AnimationBinding {
                    object: target.id,
                    property,
                };
                animation::insert_keyframe(tree, binding, frame, value);
                runtime.selected_keyframes = vec![SelectedKeyframe { binding, frame }];
            }
        }
    }
    tree.make_undo_redo_snapshot();
}

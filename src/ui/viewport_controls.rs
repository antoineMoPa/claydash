use super::*;

impl UiState {
    pub(super) fn handle_animation_shortcuts(&mut self, ctx: &egui::Context, tree: &mut DataTree) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (toggle, step) = ctx.input(|input| {
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
            (toggle, step)
        });
        if toggle {
            self.animation.toggle_playback();
        } else if let Some(step) = step {
            self.animation.playing = false;
            self.animation
                .set_frame(tree, self.animation.current_frame.round() + step);
        }
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
                            for kind in PrimitiveKind::ALL {
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
                                    for kind in PrimitiveKind::ALL {
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
        let projection_y = if viewport.width() < left.response.rect.width() + 72.0 {
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
                        camera.toggle_projection();
                    }
                    if view_button(
                        ui,
                        egui::include_image!("../../assets/icons/lucide/rotate-3d.svg"),
                        "Snap to isometric view",
                    )
                    .clicked()
                    {
                        camera.snap(ViewAngle::Isometric);
                    }
                });
            });
        self.regions.push(right.response.rect);
        self.draw_selection_toolbar(ctx, viewport);
    }

    pub(super) fn draw_view_gizmo(&mut self, ctx: &egui::Context, camera: &mut Camera) {
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
                        orientation_axes(ui, camera);
                    });
            });
        self.regions.push(area.response.rect);
    }
}

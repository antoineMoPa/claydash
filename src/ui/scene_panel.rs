use super::*;

pub(super) fn scene_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    let selection = selected(tree);
    let scene = objects(tree);
    if let Some(pick) = scene_actions::pending_boolean(tree) {
        let target = scene
            .iter()
            .find(|object| object.uuid == pick.target)
            .unwrap();
        ui.colored_label(
            Color32::LIGHT_BLUE,
            format!("{}: {}", pick.operation.label(), target.display_name()),
        );
        ui.label("Click an operand in the scene or tree. Escape cancels.")
            .on_hover_text("The target and its ancestors cannot be used as operands. Choose another object or group.");
        if ui.button("Cancel operation").clicked() {
            scene_actions::cancel_boolean_pick(tree);
        }
        ui.separator();
    }
    if selection.len() >= 2 {
        if let Some(target) = scene.iter().find(|object| object.uuid == selection[0]) {
            ui.label(RichText::new(format!("→ {}", target.display_name())).strong());
        }
        ui.horizontal_wrapped(|ui| {
            for (label, operation) in [
                ("Union", BooleanOperation::Union),
                ("Subtract", BooleanOperation::Subtract),
                ("Intersect", BooleanOperation::Intersect),
            ] {
                if ui
                    .add_enabled(selection.len() >= 2, egui::Button::new(label))
                    .on_hover_text(match operation {
                        BooleanOperation::Union => "Add the selected shapes to the target.",
                        BooleanOperation::Subtract => {
                            "Cut all selected operands out of the first selected target."
                        }
                        BooleanOperation::Intersect => {
                            "Keep only the volume shared by target and operands."
                        }
                    })
                    .clicked()
                {
                    apply_boolean(tree, operation);
                }
            }
        });
        ui.separator();
    }
    for object in scene
        .iter()
        .filter(|object| object.boolean_parent.is_none())
    {
        subtree_rows(ui, tree, &scene, object, &selection, 0);
    }
}

pub(super) fn subtree_rows(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    scene: &[SdfObject],
    object: &SdfObject,
    selection: &[uuid::Uuid],
    depth: usize,
) {
    if depth >= scene.len() {
        return;
    }
    object_row(ui, tree, object, selection, depth);
    for child in scene
        .iter()
        .filter(|child| child.boolean_parent == Some(object.uuid))
    {
        subtree_rows(ui, tree, scene, child, selection, depth + 1);
    }
}

pub(super) fn operation_style(operation: BooleanOperation) -> (&'static str, Color32) {
    match operation {
        BooleanOperation::Union => ("+", Color32::from_rgb(140, 210, 175)),
        BooleanOperation::Subtract => ("−", Color32::from_rgb(255, 192, 115)),
        BooleanOperation::Intersect => ("∩", Color32::from_rgb(159, 219, 255)),
    }
}

pub(super) fn operand_shape_menu(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    target: uuid::Uuid,
    operation: BooleanOperation,
) {
    for kind in PrimitiveKind::ALL {
        if ui
            .add(egui::Button::image_and_text(
                icon_image(primitive_icon_source(kind), ui.visuals().text_color()),
                kind.label(),
            ))
            .clicked()
        {
            scene_actions::add_operand(tree, target, kind, operation);
            ui.close();
        }
    }
}

pub(super) fn operand_operation_menu(ui: &mut egui::Ui, tree: &mut DataTree, object: &SdfObject) {
    for operation in [
        BooleanOperation::Union,
        BooleanOperation::Subtract,
        BooleanOperation::Intersect,
    ] {
        if ui
            .selectable_label(object.operation == operation, operation.label())
            .clicked()
        {
            edit_boolean_operand(tree, object.uuid, Some(operation));
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Detach").clicked() {
        edit_boolean_operand(tree, object.uuid, None);
        ui.close();
    }
}

pub(super) fn object_row(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    object: &SdfObject,
    selection: &[uuid::Uuid],
    depth: usize,
) {
    ui.push_id(object.uuid, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("...", |ui| {
                    if ui.button("Duplicate").clicked() {
                        commands::duplicate_object(tree, object.uuid);
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Add cutter", |ui| {
                        operand_shape_menu(ui, tree, object.uuid, BooleanOperation::Subtract)
                    });
                    ui.menu_button("Add union", |ui| {
                        operand_shape_menu(ui, tree, object.uuid, BooleanOperation::Union)
                    });
                    ui.menu_button("Add intersection", |ui| {
                        operand_shape_menu(ui, tree, object.uuid, BooleanOperation::Intersect)
                    });
                    let operands: Vec<_> = selection
                        .iter()
                        .copied()
                        .filter(|id| *id != object.uuid)
                        .collect();
                    if scene_actions::can_attach(&objects(tree), object.uuid, &operands) {
                        ui.separator();
                        for (label, operation) in [
                            ("Subtract selected", BooleanOperation::Subtract),
                            ("Union selected", BooleanOperation::Union),
                            ("Intersect selected", BooleanOperation::Intersect),
                        ] {
                            if ui.button(label).clicked() {
                                scene_actions::attach(tree, object.uuid, &operands, operation);
                                ui.close();
                            }
                        }
                    }
                    if object.boolean_parent.is_some() {
                        ui.separator();
                        operand_operation_menu(ui, tree, object);
                    }
                })
                .response
                .on_hover_text("Object actions");
                ui.menu_button(
                    RichText::new("−")
                        .color(operation_style(BooleanOperation::Subtract).1)
                        .strong(),
                    |ui| {
                        operand_shape_menu(ui, tree, object.uuid, BooleanOperation::Subtract);
                    },
                )
                .response
                .on_hover_text("Add a subtractive shape");

                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 24.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space((depth as f32 * 12.0).min(ui.available_width() * 0.3));
                        if depth > 0 {
                            let (symbol, color) = operation_style(object.operation);
                            let badge = if object.operation == BooleanOperation::Intersect {
                                egui::Button::image(
                                    egui::Image::new(egui::include_image!(
                                        "../../assets/icons/intersect.svg"
                                    ))
                                    .fit_to_exact_size(egui::vec2(12.0, 12.0))
                                    .tint(color),
                                )
                            } else {
                                egui::Button::new(RichText::new(symbol).color(color).strong())
                            };
                            egui::containers::menu::MenuButton::from_button(badge)
                                .ui(ui, |ui| {
                                    operand_operation_menu(ui, tree, object);
                                })
                                .0
                                .on_hover_text(object.operation.label());
                        }
                        primitive_icon(ui, PrimitiveKind::from_object_type(object.object_type));
                        let selected_now = selection.contains(&object.uuid);
                        let rename_id = ui.id().with("rename");
                        let rename_focus_id = ui.id().with("rename-focus");
                        let response = if let Some(mut draft) =
                            ui.ctx().data(|data| data.get_temp::<String>(rename_id))
                        {
                            let response = ui.add_sized(
                                egui::vec2(ui.available_width().max(1.0), 24.0),
                                egui::TextEdit::singleline(&mut draft),
                            );
                            let needs_focus = ui.ctx().data(|data| {
                                !data.get_temp::<bool>(rename_focus_id).unwrap_or(false)
                            });
                            if needs_focus {
                                response.request_focus();
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(rename_focus_id, true);
                                });
                            }
                            let cancel = response.has_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Escape));
                            let commit = response.has_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter));
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(rename_id, draft.clone());
                            });
                            if cancel || commit || response.lost_focus() {
                                ui.ctx().data_mut(|data| {
                                    data.remove_temp::<String>(rename_id);
                                    data.remove_temp::<bool>(rename_focus_id);
                                });
                                if !cancel {
                                    rename_object(tree, object.uuid, draft);
                                }
                            }
                            None
                        } else {
                            Some(
                                ui.add_sized(
                                    egui::vec2(ui.available_width().max(1.0), 24.0),
                                    egui::Button::new(object.display_name())
                                        .selected(selected_now)
                                        .frame(false)
                                        .truncate()
                                        .sense(egui::Sense::click_and_drag()),
                                )
                                .on_hover_text(format!(
                                "{}\nDouble-click to rename · Drag onto another object to combine",
                                object.display_name()
                            )),
                            )
                        };
                        let Some(response) = response else {
                            return;
                        };
                        if response.double_clicked() {
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(rename_id, object.display_name());
                                data.insert_temp(rename_focus_id, false);
                            });
                        }
                        if response.clicked() && !response.double_clicked() {
                            response.surrender_focus();
                            if !scene_actions::apply_boolean_pick(tree, object.uuid) {
                                let extend = ui.input(|input| {
                                    input.modifiers.shift || input.modifiers.command
                                });
                                let mut next = if extend {
                                    selection.to_vec()
                                } else {
                                    Vec::new()
                                };
                                if extend && selected_now {
                                    next.retain(|id| *id != object.uuid);
                                } else {
                                    next.push(object.uuid);
                                }
                                set_selected(tree, next);
                            }
                        }
                        response.dnd_set_drag_payload(scene_actions::DragObjects(
                            if selected_now {
                                selection.to_vec()
                            } else {
                                vec![object.uuid]
                            },
                        ));
                        let popup_id = ui.id().with("drop-operation");
                        if let Some(payload) =
                            response.dnd_hover_payload::<scene_actions::DragObjects>()
                        {
                            let valid =
                                scene_actions::can_attach(&objects(tree), object.uuid, &payload.0);
                            ui.ctx().set_cursor_icon(if valid {
                                egui::CursorIcon::Copy
                            } else {
                                egui::CursorIcon::NotAllowed
                            });
                            if valid {
                                ui.painter().rect_stroke(
                                    response.rect,
                                    4,
                                    Stroke::new(2.0, Color32::from_rgb(255, 192, 115)),
                                    egui::StrokeKind::Inside,
                                );
                                if let Some(payload) =
                                    response.dnd_release_payload::<scene_actions::DragObjects>()
                                {
                                    ui.ctx().data_mut(|data| {
                                        data.insert_temp(popup_id, (*payload).clone())
                                    });
                                    egui::Popup::open_id(ui.ctx(), popup_id);
                                }
                            }
                        }
                        egui::Popup::from_response(&response)
                            .id(popup_id)
                            .open_memory(None)
                            .show(|ui| {
                                ui.strong(object.display_name());
                                for operation in [
                                    BooleanOperation::Union,
                                    BooleanOperation::Subtract,
                                    BooleanOperation::Intersect,
                                ] {
                                    let (symbol, color) = operation_style(operation);
                                    if ui
                                        .button(
                                            RichText::new(format!(
                                                "{}  {}",
                                                if operation == BooleanOperation::Intersect {
                                                    "&"
                                                } else {
                                                    symbol
                                                },
                                                operation.label()
                                            ))
                                            .color(color),
                                        )
                                        .clicked()
                                    {
                                        if let Some(payload) = ui.ctx().data_mut(|data| {
                                            data.remove_temp::<scene_actions::DragObjects>(popup_id)
                                        }) {
                                            scene_actions::attach(
                                                tree,
                                                object.uuid,
                                                &payload.0,
                                                operation,
                                            );
                                        }
                                        ui.close();
                                    }
                                }
                            });
                        response.context_menu(|ui| {
                            if ui.button("Duplicate").clicked() {
                                commands::duplicate_object(tree, object.uuid);
                                ui.close();
                            }
                            ui.separator();
                            ui.menu_button("Add cutter", |ui| {
                                operand_shape_menu(
                                    ui,
                                    tree,
                                    object.uuid,
                                    BooleanOperation::Subtract,
                                )
                            });
                            if object.boolean_parent.is_some() {
                                ui.separator();
                                operand_operation_menu(ui, tree, object);
                            }
                        });
                    },
                );
            });
        });
    });
}

pub(super) fn edit_boolean_operand(
    tree: &mut DataTree,
    id: uuid::Uuid,
    operation: Option<BooleanOperation>,
) {
    let mut scene = objects(tree);
    if let Some(object) = scene.iter_mut().find(|object| object.uuid == id) {
        object.operation = operation.unwrap_or(BooleanOperation::Union);
        if operation.is_none() {
            let _ = object;
            scene_actions::reparent_preserving_world(&mut scene, id, None);
        }
        set_objects(tree, scene);
        tree.make_undo_redo_snapshot();
    }
}

pub(super) fn rename_object(tree: &mut DataTree, id: uuid::Uuid, name: String) {
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == id) else {
        return;
    };
    if object.name == name {
        return;
    }
    object.name = name;
    set_objects(tree, scene);
    tree.make_undo_redo_snapshot();
}

pub(super) fn apply_boolean(tree: &mut DataTree, operation: BooleanOperation) {
    let selection = selected(tree);
    if selection.len() < 2 {
        return;
    }
    scene_actions::attach(tree, selection[0], &selection[1..], operation);
}

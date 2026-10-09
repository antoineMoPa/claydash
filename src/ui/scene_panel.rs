use super::*;

pub(super) fn scene_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    let filter = object_list_filter(ui);
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
    if selection.len() >= 2 && crate::model::selected_variable(tree).is_none() {
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
    let visible = filtered_object_ids(&scene, &filter);
    for object in scene
        .iter()
        .filter(|object| object.boolean_parent.is_none())
    {
        filtered_subtree_rows(ui, tree, &scene, object, &selection, &visible, 0);
    }
    variable_rows(ui, tree, &filter);
    let cameras: Vec<_> = crate::model::scene_cameras(tree)
        .into_iter()
        .filter(|camera| name_matches(&camera.name, &filter))
        .collect();
    if !cameras.is_empty() {
        ui.separator();
        ui.label(RichText::new("Cameras").strong());
        for camera in cameras {
            let response = ui.selectable_label(
                crate::model::selected_variable(tree).is_none() && selection.contains(&camera.uuid),
                &camera.name,
            );
            if response.clicked() {
                set_selected(tree, vec![camera.uuid]);
                tree.set_path(
                    "scene.active_camera",
                    crate::model::ClaydashValue::Uuid(camera.uuid),
                );
            }
            response.on_hover_text("Select camera object");
        }
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
    for kind in PrimitiveKind::SPAWNABLE {
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
        let rename_id = ui.id().with("rename");
        let rename_focus_id = ui.id().with("rename-focus");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("...", |ui| {
                    if ui.button("Rename").clicked() {
                        begin_row_rename(
                            ui.ctx(),
                            rename_id,
                            rename_focus_id,
                            object.display_name(),
                        );
                        ui.close();
                    }
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
                        let selected_now = crate::model::selected_variable(tree).is_none()
                            && selection.contains(&object.uuid);
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
                            let cancel = (response.has_focus() || response.lost_focus())
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
                        if response.double_clicked() || response.triple_clicked() {
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(rename_id, object.display_name());
                                data.insert_temp(rename_focus_id, false);
                            });
                        }
                        if response.clicked()
                            && !response.double_clicked()
                            && !response.triple_clicked()
                        {
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
                            if ui.button("Rename").clicked() {
                                begin_row_rename(
                                    ui.ctx(),
                                    rename_id,
                                    rename_focus_id,
                                    object.display_name(),
                                );
                                ui.close();
                            }
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
    tree.make_undo_redo_snapshot();
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

fn name_matches(name: &str, query: &str) -> bool {
    query.is_empty() || name.to_lowercase().contains(&query.to_lowercase())
}

fn filtered_object_ids(scene: &[SdfObject], query: &str) -> std::collections::HashSet<uuid::Uuid> {
    let mut visible = std::collections::HashSet::new();
    for object in scene
        .iter()
        .filter(|object| name_matches(&object.display_name(), query))
    {
        let mut current = Some(object.uuid);
        let mut ancestry = std::collections::HashSet::new();
        while let Some(id) = current {
            if !ancestry.insert(id) {
                break;
            }
            visible.insert(id);
            current = scene
                .iter()
                .find(|object| object.uuid == id)
                .and_then(|object| object.boolean_parent);
        }
    }
    visible
}

fn filtered_subtree_rows(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    scene: &[SdfObject],
    object: &SdfObject,
    selection: &[uuid::Uuid],
    visible: &std::collections::HashSet<uuid::Uuid>,
    depth: usize,
) {
    if depth >= scene.len() || !visible.contains(&object.uuid) {
        return;
    }
    object_row(ui, tree, object, selection, depth);
    for child in scene
        .iter()
        .filter(|child| child.boolean_parent == Some(object.uuid))
    {
        filtered_subtree_rows(ui, tree, scene, child, selection, visible, depth + 1);
    }
}

fn object_list_filter(ui: &mut egui::Ui) -> String {
    let id = egui::Id::new("scene-name-filter");
    let mut query = ui
        .ctx()
        .data(|data| data.get_temp::<String>(id))
        .unwrap_or_default();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(36.0), 24.0),
        egui::Sense::hover(),
    );
    ui.put(
        rect,
        egui::TextEdit::singleline(&mut query)
            .id(id.with("input"))
            .hint_text("Filter...")
            .vertical_align(egui::Align::Center)
            .margin(egui::Margin {
                left: 6,
                right: 28,
                top: 2,
                bottom: 2,
            }),
    );
    let clear = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 14.0, rect.center().y),
        egui::vec2(20.0, 20.0),
    );
    ui.ctx().data_mut(|data| {
        data.insert_temp(id.with("rect"), rect);
        data.insert_temp(id.with("clear-rect"), clear);
    });
    if !query.is_empty()
        && ui
            .put(
                clear,
                egui::Button::image(
                    egui::Image::new(egui::include_image!("../../assets/icons/lucide/x.svg"))
                        .fit_to_exact_size(egui::vec2(14.0, 14.0))
                        .tint(ui.visuals().text_color()),
                )
                .frame(false),
            )
            .on_hover_text("Clear filter")
            .clicked()
    {
        query.clear();
        ui.memory_mut(|memory| memory.request_focus(id.with("input")));
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(id, query.clone()));
    query
}

fn begin_row_rename(ctx: &egui::Context, rename: egui::Id, focus: egui::Id, name: String) {
    ctx.data_mut(|data| {
        data.insert_temp(rename, name);
        data.insert_temp(focus, false);
    });
}

fn variable_rows(ui: &mut egui::Ui, tree: &mut DataTree, filter: &str) {
    use crate::model::{
        scene_variables, selected_variable, set_selected_variable,
    };
    let before = scene_variables(tree);
    let mut variables = before.clone();
    let mut begin_edit = false;
    let mut remove = None;
    let world_locked_ids: Vec<_> = variables.vectors.iter().filter(|v| crate::model::is_variable_constraint_reference(&variables,v.id)).map(|v| v.id).collect();
    for variable in variables
        .vectors
        .iter_mut()
        .filter(|variable| name_matches(point_row_name(&variable.name), filter))
    {
        ui.push_id(("variable-row", variable.id), |ui| {
            let rename = ui.id().with("rename");
            let focus = ui.id().with("rename-focus");
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.add(icon_image(
                    egui::include_image!("../../assets/icons/lucide/circle-dot.svg"),
                    ui.visuals().text_color(),
                ));
                let mut draft = ui.ctx().data(|data| data.get_temp::<String>(rename));
                if let Some(name) = &mut draft {
                    let response = ui.add_sized(
                        egui::vec2((ui.available_width() - 30.0).max(1.0), 24.0),
                        egui::TextEdit::singleline(name),
                    );
                    if !ui
                        .ctx()
                        .data(|data| data.get_temp::<bool>(focus))
                        .unwrap_or(false)
                    {
                        response.request_focus();
                        ui.ctx().data_mut(|data| data.insert_temp(focus, true));
                    }
                    let cancel = (response.has_focus() || response.lost_focus())
                        && ui.input(|input| input.key_pressed(egui::Key::Escape));
                    let commit = response.has_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(rename, name.clone()));
                    if cancel || commit || response.lost_focus() {
                        ui.ctx().data_mut(|data| {
                            data.remove_temp::<String>(rename);
                            data.remove_temp::<bool>(focus);
                        });
                        if !cancel && variable.name != *name {
                            begin_edit = true;
                            variable.name = name.clone();
                        }
                    }
                } else {
                    let response = ui.add_sized(
                        egui::vec2((ui.available_width() - 30.0).max(1.0), 24.0),
                        egui::Button::new(point_row_name(&variable.name))
                            .sense(egui::Sense::click_and_drag())
                            .selected(selected_variable(tree) == Some(variable.id))
                            .frame(false)
                            .truncate(),
                    );
                    if response.double_clicked() || response.triple_clicked() {
                        begin_row_rename(ui.ctx(), rename, focus, variable.name.clone());
                    } else if response.clicked() {
                        set_selected(tree, vec![]);
                        set_selected_variable(tree, Some(variable.id));
                        response.surrender_focus();
                    }
                    response
                        .on_hover_text("Double-click to rename")
                        .context_menu(|ui| {
                            variable_row_menu(
                                ui,
                                variable,
                                world_locked_ids.contains(&variable.id),
                                rename,
                                focus,
                                &mut remove,
                                &mut begin_edit,
                            );
                        });
                }
                ui.menu_button("...", |ui| {
                    variable_row_menu(ui, variable, world_locked_ids.contains(&variable.id), rename, focus, &mut remove, &mut begin_edit)
                });
            });
        });
    }
    if let Some(id) = remove {
        variables.vectors.retain(|variable| variable.id != id);
        crate::model::remove_variable_references(&mut variables, id);
        if selected_variable(tree) == Some(id) {
            set_selected_variable(tree, None);
        }
    }
    if variables != before {
        if begin_edit {
            tree.make_undo_redo_snapshot();
        }
        if let Err(error) = crate::model::try_set_scene_variables(tree, variables) {
            eprintln!("Point edit rejected: {error}");
        }
        tree.make_undo_redo_snapshot();
    }
}

fn variable_row_menu(
    ui: &mut egui::Ui,
    variable: &mut crate::model::VectorVariable,
    world_locked: bool,
    rename: egui::Id,
    focus: egui::Id,
    remove: &mut Option<uuid::Uuid>,
    begin_edit: &mut bool,
) {
    if ui.button("Rename").clicked() {
        begin_row_rename(ui.ctx(), rename, focus, variable.name.clone());
        ui.close();
    }
    ui.separator();
    let before = variable.space;
    ui.selectable_value(
        &mut variable.space,
        crate::model::VariableSpace::World,
        "World coordinates",
    );
    ui.add_enabled_ui(!world_locked, |ui| {
        ui.selectable_value(&mut variable.space, crate::model::VariableSpace::Local,"Local coordinates");
    }).response.on_hover_text("Constraint points require World coordinates");
    *begin_edit |= before != variable.space;
    ui.separator();
    if ui.button("Delete").clicked() {
        *remove = Some(variable.id);
        *begin_edit = true;
        ui.close();
    }
}

fn point_row_name(name: &str) -> &str {
    if name.is_empty() {
        "Point"
    } else {
        name
    }
}

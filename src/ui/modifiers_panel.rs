use super::*;

pub(super) fn modifiers_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
    animation::migrate_legacy_lattice_tracks(tree);
    ui.spacing_mut().item_spacing.y = 8.0;
    ui.label(RichText::new("Modifiers").strong());
    let selection = selected(tree);
    if selection.len() != 1 {
        ui.label("Select one object or Boolean group to edit its modifiers.");
        return;
    }
    if crate::model::scene_cameras(tree)
        .iter()
        .any(|camera| camera.uuid == selection[0])
    {
        ui.label("Camera objects have no modifiers.");
        return;
    }
    let mut scene = objects(tree);
    let target = commands::selected_group_id(tree).unwrap_or(selection[0]);
    let bounds = crate::model::lattice_bounds(&scene, target);
    let repeat_spacing =
        bounds.map(|(min, max)| ((max - min) * 1.1).clamp(Vec3::splat(0.01), Vec3::splat(20.0)));
    let has_children = crate::model::has_boolean_children(&scene, target);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == target) else {
        return;
    };
    let can_lattice = object.boolean_parent.is_none();
    let can_repeat = !has_children || object.boolean_parent.is_none();
    let mut changed = false;
    let mut snapshot = false;
    ui.menu_button("+ Add modifier", |ui| {
        if ui
            .add_enabled(
                can_lattice && object.lattice.is_none(),
                egui::Button::new("Lattice"),
            )
            .clicked()
        {
            if let Some((min, max)) = bounds {
                object.lattice = Some(crate::model::Lattice::new(min, max, 2));
                changed = true;
                snapshot = true;
            }
            ui.close();
        }
        if ui
            .add_enabled(
                can_repeat && !object.repetition.enabled,
                egui::Button::new("Repeat"),
            )
            .clicked()
        {
            object.repetition = crate::model::Repetition {
                enabled: true,
                spacing: repeat_spacing
                    .unwrap_or_else(|| crate::model::Repetition::default().spacing),
                ..Default::default()
            };
            changed = true;
            snapshot = true;
            ui.close();
        }
    });

    let mut remove_lattice = false;
    let mut deleted_shape_key = None;
    if let Some(lattice) = &mut object.lattice {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Lattice").strong().size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Remove").clicked() {
                        remove_lattice = true;
                    }
                });
            });
            if remove_lattice {
                return;
            }
            let mut resolution = lattice.resolution as i32;
            let field_width = (ui.available_width() - 74.0).max(110.0);
            let response = egui::Grid::new(ui.id().with("lattice-density"))
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Density");
                    let response = ui.add_sized(
                        [field_width, 24.0],
                        egui::Slider::new(&mut resolution, 2..=9).text("points/axis"),
                    );
                    ui.end_row();
                    response
                })
                .inner;
            if response.changed() {
                lattice.resize(resolution as u8);
                changed = true;
            }
            snapshot |= response.drag_stopped() || response.clicked() && response.changed();
            ui.label(format!(
                "{} × {} × {} control points",
                resolution, resolution, resolution
            ));
            ui.separator();
            ui.label(RichText::new("Shape keys").strong());
            if ui.button("+ Add shape from current lattice").clicked() {
                lattice.add_shape_key();
                changed = true;
                snapshot = true;
            }
            if lattice.current_shape_key.is_none() {
                ui.weak(format!(
                    "Previewing position {:.2}. Select a shape to edit it.",
                    lattice.shape_position
                ));
            }
            if ui
                .selectable_label(lattice.current_shape_key == Some(0), "0 · Reset grid")
                .clicked()
            {
                lattice.select_shape_key(0);
                changed = true;
                snapshot = true;
            }
            let mut select_shape_key = None;
            for (index, key) in lattice.shape_keys.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::selectable(
                                lattice.current_shape_key == Some(index + 1),
                                format!("{} ·", index + 1),
                            )
                            .min_size(egui::vec2(36.0, 24.0)),
                        )
                        .clicked()
                    {
                        select_shape_key = Some(index + 1);
                    }
                    let remaining_width = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(remaining_width, 24.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui
                                .add(egui::Button::new("Delete").min_size(egui::vec2(0.0, 24.0)))
                                .on_hover_text("Delete shape key")
                                .clicked()
                            {
                                deleted_shape_key = Some(index + 1);
                            }
                            let name_width = ui.available_width().max(24.0);
                            if ui
                                .add_sized(
                                    [name_width, 24.0],
                                    egui::TextEdit::singleline(&mut key.name),
                                )
                                .changed()
                            {
                                changed = true;
                                snapshot = true;
                            }
                        },
                    );
                });
            }
            if let Some(index) = select_shape_key {
                lattice.select_shape_key(index);
                changed = true;
                snapshot = true;
            }
            if let Some(index) = deleted_shape_key {
                lattice.shape_keys.remove(index - 1);
                let selected = lattice.current_shape_key.unwrap_or(0);
                lattice.current_shape_key = Some(if selected >= index {
                    selected.saturating_sub(1)
                } else {
                    selected
                });
                lattice.select_shape_key(lattice.current_shape_key.unwrap_or(0));
                changed = true;
                snapshot = true;
            }
            ui.separator();
            if ui.button("Reset all points").clicked() {
                lattice.offsets.fill(Vec3::ZERO);
                lattice.save_selected_shape_key();
                changed = true;
                snapshot = true;
            }
        });
    } else if !object.repetition.enabled {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("No modifiers").strong());
        });
    }
    if remove_lattice {
        object.lattice = None;
        changed = true;
        snapshot = true;
    }
    let show_repeat = object.repetition.enabled;
    if changed {
        set_objects(tree, scene);
    }
    if remove_lattice {
        animation::remove_lattice_track(tree, target);
    }
    if let Some(index) = deleted_shape_key {
        animation::remap_lattice_shape_keys_after_delete(tree, target, index);
    }
    if snapshot {
        tree.make_undo_redo_snapshot();
    }
    if show_repeat {
        let mut remove_repeat = false;
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Repeat").strong().size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    remove_repeat = ui.small_button("Remove").clicked();
                });
            });
            if !remove_repeat {
                repetition_panel(ui, tree, runtime);
            }
        });
        if remove_repeat {
            let mut scene = objects(tree);
            if let Some(object) = scene.iter_mut().find(|object| object.uuid == target) {
                object.repetition.enabled = false;
                set_objects(tree, scene);
                animation::remove_repetition_tracks(tree, target);
                tree.make_undo_redo_snapshot();
            }
        }
    }
}

use super::*;

pub(super) fn modifiers_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    ui.spacing_mut().item_spacing.y = 8.0;
    ui.heading("Modifiers");
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
    let Some(object) = scene.iter_mut().find(|object| object.uuid == target) else {
        return;
    };
    if object.boolean_parent.is_some() {
        ui.label("Select the whole object or its top level Boolean group to add a modifier.");
        return;
    }
    let mut changed = false;
    let mut snapshot = false;
    ui.menu_button("+ Add modifier", |ui| {
        if ui
            .add_enabled(object.lattice.is_none(), egui::Button::new("Lattice"))
            .clicked()
        {
            if let Some((min, max)) = bounds {
                object.lattice = Some(crate::model::Lattice::new(min, max, 2));
                changed = true;
                snapshot = true;
            }
            ui.close();
        }
    });

    let mut remove_lattice = false;
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
            if ui.button("Reset all points").clicked() {
                lattice.offsets.fill(Vec3::ZERO);
                changed = true;
                snapshot = true;
            }
        });
    } else {
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
    if changed {
        set_objects(tree, scene);
    }
    if snapshot {
        tree.make_undo_redo_snapshot();
    }
}

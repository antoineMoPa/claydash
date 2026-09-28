use super::*;

pub(super) fn group_optimizations_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    ui.separator();
    ui.label(RichText::new("Group optimizations").strong());
    let selection = selected(tree);
    if selection.len() != 1 {
        ui.weak("Select one object or Boolean group to explore a representation.");
        return;
    }
    if crate::model::scene_cameras(tree)
        .iter()
        .any(|camera| camera.uuid == selection[0])
    {
        return;
    }
    let mut scene = objects(tree);
    let target = commands::selected_group_id(tree).unwrap_or(selection[0]);
    let Some(index) = scene.iter().position(|object| object.uuid == target) else {
        return;
    };
    let saved_mode = scene[index].render_representation;
    let mut selected_mode = saved_mode;
    egui::ComboBox::from_id_salt(ui.auto_id_with("group-render-representation"))
        .selected_text(saved_mode.label())
        .show_ui(ui, |ui| {
            for mode in crate::model::GroupRenderRepresentation::ALL {
                ui.selectable_value(&mut selected_mode, mode, mode.label())
                    .on_hover_text(mode.description());
            }
        });
    ui.weak(selected_mode.limitation());
    if selected_mode != scene[index].render_representation {
        scene[index].render_representation = selected_mode;
        set_objects(tree, scene);
        tree.make_undo_redo_snapshot();
    }
}

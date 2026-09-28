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
    let saved_settings = scene[index].neural_sdf;
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
    if selected_mode == crate::model::GroupRenderRepresentation::NeuralSdf {
        let settings = &mut scene[index].neural_sdf;
        let preset = crate::model::NeuralModelPreset::matching(settings.training);
        ui.label("Model size");
        egui::ComboBox::from_id_salt(ui.auto_id_with("neural-model-size"))
            .selected_text(preset.map_or("Custom", |preset| preset.label()))
            .show_ui(ui, |ui| {
                for choice in crate::model::NeuralModelPreset::ALL {
                    if ui.selectable_label(preset == Some(choice), choice.label()).clicked() {
                        settings.training = choice.settings();
                        settings.hit_distance_cells = 0.5;
                    }
                }
            }).response.on_hover_text("Model size presets tested on the default duck. Larger models trade training and rendering speed for fit quality. Click Recompute to apply; edit parameters below for Custom settings.");
        egui::Grid::new(ui.auto_id_with("neural-settings")).num_columns(2).show(ui, |ui| {
            ui.label("Activation");
            egui::ComboBox::from_id_salt("neural-activation").selected_text(settings.training.activation.label()).show_ui(ui, |ui| {
                for activation in crate::model::NeuralActivation::ALL {
                    ui.selectable_value(&mut settings.training.activation, activation, activation.label());
                }
            }); ui.end_row();
            ui.label("Hidden layers");
            ui.add(egui::DragValue::new(&mut settings.training.layers).update_while_editing(false).range(1..=4)); ui.end_row();
            ui.label("Width");
            ui.add(egui::DragValue::new(&mut settings.training.width).update_while_editing(false).range(4..=32)); ui.end_row();
            ui.label("Samples per side").on_hover_text("Training distance grid resolution. Total samples grow cubically; applies on Recompute.");
            ui.add(egui::DragValue::new(&mut settings.training.samples_per_side).update_while_editing(false).range(8..=1024)); ui.end_row();
            ui.label("Epochs").on_hover_text("One full pass through all grid samples per epoch.");
            ui.add(egui::DragValue::new(&mut settings.training.epochs).update_while_editing(false).range(1..=512)); ui.end_row();
            ui.label("Learning rate");
            ui.add(egui::DragValue::new(&mut settings.training.learning_rate).update_while_editing(false).range(0.00001..=0.1).speed(0.0001).max_decimals(5)); ui.end_row();
            ui.label("Seed").on_hover_text("Controls initialization and sample order. The same seed and settings reproduce the same fit.");
            ui.add(egui::DragValue::new(&mut settings.training.seed).update_while_editing(false)); ui.end_row();
            ui.label("Hit distance (cells)").on_hover_text("Distance from zero accepted as a hit, in grid-cell units. Larger values stop sooner and expand the silhouette. Applies immediately without retraining.");
            ui.add(egui::DragValue::new(&mut settings.hit_distance_cells).update_while_editing(false).range(0.001..=4.0).speed(0.001).max_decimals(3)); ui.end_row();
        });
        let status = ui.ctx().data(|data| data.get_temp::<std::collections::HashMap<uuid::Uuid, crate::renderer::NeuralStatus>>(egui::Id::new("neural-sdf-status")))
            .and_then(|statuses| statuses.get(&target).cloned());
        use crate::renderer::NeuralStatus;
        let computing = matches!(status, None | Some(NeuralStatus::Pending | NeuralStatus::Training));
        if ui.add_enabled(!computing, egui::Button::new("Recompute")).on_hover_text("Restart this group's bake using the current settings. Exact SDF is shown until the result is ready.").clicked() {
            ui.ctx().data_mut(|data| {
                let id = egui::Id::new("neural-sdf-recompute");
                let mut requests = data.get_temp::<std::collections::HashSet<uuid::Uuid>>(id).unwrap_or_default();
                requests.insert(target);
                data.insert_temp(id, requests);
            });
            ui.ctx().request_repaint();
        }
        match status {
            Some(NeuralStatus::Ready {
                rms,
                max,
                milliseconds,
            }) => {
                ui.weak(format!(
                    "Ready · RMS {rms:.4}, max {max:.4} · {milliseconds:.0} ms"
                ));
            }
            Some(NeuralStatus::Failed(reason)) => {
                ui.weak(format!("Exact SDF fallback: {reason}"));
            }
            Some(NeuralStatus::Training) => {
                ui.weak("Training neural field…");
            }
            _ => {
                ui.weak("Neural bake pending…");
            }
        }
    }
    if selected_mode != scene[index].render_representation
        || saved_settings != scene[index].neural_sdf
    {
        scene[index].render_representation = selected_mode;
        set_objects(tree, scene);
        tree.make_undo_redo_snapshot();
    }
}

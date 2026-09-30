use super::*;
use crate::model::{world, BackgroundMode, RenderPipelineMode};

pub(super) fn world_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    let mut settings = world(tree);
    let before = settings;
    ui.label(RichText::new("Rendering").strong());
    egui::ComboBox::from_label("Pipeline")
        .selected_text(settings.render_pipeline.label())
        .show_ui(ui, |ui| {
            for mode in RenderPipelineMode::ALL {
                ui.selectable_value(&mut settings.render_pipeline, mode, mode.label());
            }
        });
    if settings.render_pipeline == RenderPipelineMode::Deferred {
        ui.checkbox(
            &mut settings.screen_space_ambient_occlusion,
            "Screen-space ambient occlusion",
        );
        ui.checkbox(
            &mut settings.screen_space_reflections,
            "Screen-space reflections (experimental)",
        );
        if let Some(reason) =
            crate::model::deferred_fallback_reason(crate::model::objects_ref(tree))
        {
            ui.label(format!("Using exact shading: {reason} in this scene."));
        }
    }
    ui.separator();
    ui.label(RichText::new("Sky & Sun").strong());
    ui.add_space(8.0);
    egui::ComboBox::from_label("Background")
        .selected_text(settings.background.label())
        .show_ui(ui, |ui| {
            for mode in BackgroundMode::ALL {
                ui.selectable_value(&mut settings.background, mode, mode.label());
            }
        });
    match settings.background {
        BackgroundMode::Studio => {}
        BackgroundMode::Flat => {
            ui.horizontal(|ui| {
                ui.label("Color");
                ui.color_edit_button_rgb(&mut settings.flat_color);
            });
        }
        BackgroundMode::Transparent => {}
        BackgroundMode::NightSky => {
            ui.add(
                egui::Slider::new(&mut settings.night_star_density, 0.0..=0.5).text("Star density"),
            );
            ui.add(egui::Slider::new(&mut settings.night_star_size, 0.25..=2.5).text("Star size"));
            ui.add(
                egui::Slider::new(&mut settings.night_star_brightness, 0.0..=6.0)
                    .text("Star brightness"),
            );
            ui.horizontal(|ui| {
                ui.label("Star color");
                ui.color_edit_button_rgb(&mut settings.night_star_color);
            });
            ui.add(
                egui::Slider::new(&mut settings.night_horizon_glow, 0.0..=3.0).text("Horizon glow"),
            );
        }
        BackgroundMode::Sky => {
            ui.add(egui::Slider::new(&mut settings.latitude, -90.0..=90.0).text("Latitude °"));
            ui.add(egui::Slider::new(&mut settings.day_of_year, 1..=365).text("Day of year"));
            ui.add(egui::Slider::new(&mut settings.solar_time, 0.0..=24.0).text("Solar hour"));
            ui.add(
                egui::Slider::new(&mut settings.azimuth, -180.0..=180.0).text("North rotation °"),
            );
            ui.separator();
            ui.add(egui::Slider::new(&mut settings.turbidity, 1.0..=10.0).text("Atmosphere haze"));
            ui.add(
                egui::Slider::new(&mut settings.sun_temperature, 2000.0..=10000.0)
                    .text("Sun color K"),
            );
            ui.add(egui::Slider::new(&mut settings.sun_intensity, 0.0..=4.0).text("Sun intensity"));
        }
    }
    ui.add(egui::Slider::new(&mut settings.ambient_light, 0.0..=30.0).text("Ambient light"));
    if settings != before {
        tree.set_path("scene.world", ClaydashValue::World(settings));
    }
    ui.separator();
    post_processing_panel(ui, tree);
}

fn post_processing_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    use crate::model::{post_processing, set_post_processing, PostProcessPass};
    ui.label(RichText::new("Post-processing WGSL").strong());
    let mut passes = post_processing(tree);
    let validation_id = egui::Id::new("post-processing-document-validation");
    let cached = ui.ctx().data_mut(|data| {
        data.get_temp::<(Vec<PostProcessPass>, Result<(), String>)>(validation_id)
    });
    let validation = if let Some((checked, result)) = cached {
        if checked == passes {
            result
        } else {
            crate::renderer::post_processing::validate_passes(&passes)
        }
    } else {
        crate::renderer::post_processing::validate_passes(&passes)
    };
    ui.ctx()
        .data_mut(|data| data.insert_temp(validation_id, (passes.clone(), validation.clone())));
    if let Err(error) = validation {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    if ui
        .add_enabled(passes.len() < 16, egui::Button::new("Add effect"))
        .clicked()
    {
        let pass = PostProcessPass::new("Warm tint".into(), PostProcessPass::EXAMPLE.into());
        passes.push(pass);
        if crate::renderer::post_processing::validate_passes(&passes).is_ok() {
            set_post_processing(tree, passes.clone());
            tree.make_undo_redo_snapshot();
        }
    }
    enum Change {
        Move(usize, usize),
        Remove(usize),
    }
    let mut change = None;
    for index in 0..passes.len() {
        let id = passes[index].uuid;
        let edit_id = egui::Id::new(("post-processing-edit", id));
        let error_id = egui::Id::new(("post-processing-error", id));
        let label = format!("{}. {}", index + 1, passes[index].name);
        ui.collapsing(label, |ui| {
            if ui.checkbox(&mut passes[index].enabled, "Enabled").changed() {
                set_post_processing(tree, passes.clone());
                tree.make_undo_redo_snapshot();
            }
            let (mut name, mut wgsl) = ui
                .ctx()
                .data_mut(|data| data.get_temp::<(String, String)>(edit_id))
                .unwrap_or_else(|| (passes[index].name.clone(), passes[index].wgsl.clone()));
            let name_changed = ui.text_edit_singleline(&mut name).changed();
            ui.label("WGSL body");
            let source_changed = ui
                .add(
                    egui::TextEdit::multiline(&mut wgsl)
                        .code_editor()
                        .desired_rows(8),
                )
                .changed();
            if name_changed || source_changed {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(edit_id, (name.clone(), wgsl.clone())));
            }
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    let mut edited = passes.clone();
                    edited[index].name = name;
                    edited[index].wgsl = wgsl;
                    match crate::renderer::post_processing::validate_passes(&edited) {
                        Ok(()) => {
                            set_post_processing(tree, edited);
                            tree.make_undo_redo_snapshot();
                            ui.ctx().data_mut(|data| {
                                data.remove::<(String, String)>(edit_id);
                                data.remove::<String>(error_id);
                            });
                        }
                        Err(error) => {
                            ui.ctx().data_mut(|data| data.insert_temp(error_id, error));
                        }
                    }
                }
                if index > 0 && ui.button("Move up").clicked() {
                    change = Some(Change::Move(index, index - 1));
                }
                if index + 1 < passes.len() && ui.button("Move down").clicked() {
                    change = Some(Change::Move(index, index + 1));
                }
                if ui.button("Remove").clicked() {
                    change = Some(Change::Remove(index));
                }
            });
            if let Some(error) = ui.ctx().data_mut(|data| data.get_temp::<String>(error_id)) {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
    }
    if let Some(change) = change {
        match change {
            Change::Move(from, to) => passes.swap(from, to),
            Change::Remove(index) => {
                passes.remove(index);
            }
        }
        set_post_processing(tree, passes);
        tree.make_undo_redo_snapshot();
    }
}

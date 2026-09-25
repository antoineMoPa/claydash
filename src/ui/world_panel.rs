use super::*;
use crate::model::{world, BackgroundMode};

pub(super) fn world_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    let mut settings = world(tree);
    let before = settings;
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
        BackgroundMode::Studio => {
            ui.label("The original studio environment and lighting.");
        }
        BackgroundMode::Flat => {
            ui.horizontal(|ui| {
                ui.label("Color");
                ui.color_edit_button_rgb(&mut settings.flat_color);
            });
        }
        BackgroundMode::Transparent => {
            ui.label("The background exports with a transparent alpha channel in WebP images.");
        }
        BackgroundMode::Sky => {
            ui.label("Sun position follows latitude, day of year, and local solar time.");
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
    if settings != before {
        tree.set_path("scene.world", ClaydashValue::World(settings));
    }
}

use super::*;
use crate::model::{MetalFinish, MetalSpecies, MetalStudy, MetalTangent};

pub(super) fn library(ui: &mut egui::Ui, tree: &mut DataTree, filter: &str) -> bool {
    material_group(
        ui,
        tree,
        "Metals",
        false,
        filter,
        MetalSpecies::ALL.into_iter()
            .map(|species| (species.label(), Material::metal_preset(species)))
            .chain(
                MetalStudy::ALL.into_iter()
                    .map(|preset| (preset.label(), preset.material())),
            ),
    )
}
pub(super) fn picker(ui: &mut egui::Ui, tree: &mut DataTree, filter: &str) {
    for species in MetalSpecies::ALL {
        if material_matches_filter(species.label(), filter)
            && ui.button(species.label()).clicked()
        {
            apply_material(tree, Material::metal_preset(species));
            ui.close();
        }
    }
    for study in MetalStudy::ALL {
        if material_matches_filter(study.label(), filter)
            && ui.button(study.label()).clicked()
        {
            apply_material(tree, study.material());
            ui.close();
        }
    }
}
pub(super) fn controls(ui: &mut egui::Ui, material: &mut Material) -> bool {
    let before = *material;
    ui.separator();
    ui.label(RichText::new("Metal surface").strong());
    egui::ComboBox::from_id_salt("metal-species")
        .selected_text(material.metal.species.label())
        .show_ui(ui, |ui| {
            for species in MetalSpecies::ALL {
                ui.selectable_value(&mut material.metal.species, species, species.label());
            }
        });
    let previous_finish = material.metal.finish;
    egui::ComboBox::from_id_salt("metal-finish")
        .selected_text(material.metal.finish.label())
        .show_ui(ui, |ui| {
            for finish in MetalFinish::ALL {
                ui.selectable_value(&mut material.metal.finish, finish, finish.label());
            }
        });
    if material.metal.finish != previous_finish {
        material.apply_metal_finish(material.metal.finish);
    }
    ui.weak("Choosing a finish sets its roughness, anisotropy and relief defaults.");
    egui::ComboBox::from_id_salt("metal-study")
        .selected_text(
            MetalStudy::ALL
                .iter()
                .find(|study| study.material() == *material)
                .map(|study| study.label())
                .unwrap_or("Custom metal settings"),
        )
        .show_ui(ui, |ui| {
            for study in MetalStudy::ALL {
                if ui
                    .selectable_label(*material == study.material(), study.label())
                    .clicked()
                {
                    *material = study.material();
                }
            }
        });
    egui::CollapsingHeader::new("Brushing and relief").default_open(true).show(ui, |ui| {
        egui::ComboBox::from_id_salt("metal-tangent").selected_text(material.metal.tangent.label()).show_ui(ui, |ui| {
            for tangent in MetalTangent::ALL {
                ui.selectable_value(&mut material.metal.tangent, tangent, tangent.label());
            }
        });
        let mut degrees = material.metal.brush_angle.to_degrees();
        if ui.add(egui::Slider::new(&mut degrees, -180.0..=180.0).text("Brush angle °")).changed() {
            material.metal.brush_angle = degrees.to_radians();
        }
        ui.add(egui::Slider::new(&mut material.metal.texture_scale, 0.05..=16.0)
            .logarithmic(true)
            .text("Texture scale"))
            .on_hover_text("1 is the default handheld-scale detail. Lower values enlarge the pattern; higher values make it finer.");
        for (label, value, range) in [
            ("Anisotropy", &mut material.metal.anisotropy, 0.0..=0.95),
            ("Relief strength", &mut material.metal.relief_strength, 0.0..=2.0),
            ("Scratches", &mut material.metal.scratches, 0.0..=1.0),
            ("Image relief", &mut material.metal.image_relief, 0.0..=2.0),
        ] { ui.add(egui::Slider::new(value, range).text(label)); }
        ui.weak("Upload an Image stencil in Object settings. Image relief > 0 reads grayscale as height using its placement; 0 keeps the image as a color decal. Images belong to objects and do not appear in library spheres.");
    });
    ui.collapsing("Oxidation and paint", |ui| {
        ui.add(
            egui::Slider::new(&mut material.metal.oxidation, 0.0..=1.0).text("Oxidation / tarnish"),
        );
        if material.metal.species == MetalSpecies::Gold {
            ui.weak("Gold discoloration is an artistic dirt proxy.");
        }
        ui.add(
            egui::Slider::new(&mut material.metal.paint_coverage, 0.0..=1.0).text("Paint coverage"),
        );
        ui.horizontal(|ui| {
            ui.label("Paint color");
            let mut color = material.metal.paint_color.to_array();
            if ui.color_edit_button_rgb(&mut color).changed() {
                material.metal.paint_color = Vec3::from_array(color);
            }
        });
        ui.add(
            egui::Slider::new(&mut material.metal.paint_roughness, 0.025..=1.0)
                .text("Paint roughness"),
        );
    });
    *material != before
}

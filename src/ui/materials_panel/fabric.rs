use crate::model::{FabricColoration, FabricConstruction, FabricFinish, FabricPreset, Material};
use egui::RichText;

pub(super) fn fabric_controls(ui: &mut egui::Ui, material: &mut Material) -> bool {
    let mut changed = false;
    ui.separator();
    ui.label(RichText::new("Fabric construction").strong());
    egui::ComboBox::from_id_salt("fabric-preset")
        .selected_text(material.fabric.preset.label())
        .show_ui(ui, |ui| {
            for preset in FabricPreset::ALL {
                if ui
                    .selectable_label(material.fabric.preset == preset, preset.label())
                    .clicked()
                {
                    let selected = Material::fabric_preset(preset);
                    material.fabric = selected.fabric;
                    material.color = selected.color;
                    changed = true;
                }
            }
        });
    egui::ComboBox::from_id_salt("fabric-construction")
        .selected_text(material.fabric.construction.label())
        .show_ui(ui, |ui| {
            for value in FabricConstruction::ALL {
                changed |= ui
                    .selectable_value(&mut material.fabric.construction, value, value.label())
                    .changed();
            }
        });
    egui::ComboBox::from_id_salt("fabric-finish")
        .selected_text(material.fabric.finish.label())
        .show_ui(ui, |ui| {
            for value in FabricFinish::ALL {
                changed |= ui
                    .selectable_value(&mut material.fabric.finish, value, value.label())
                    .changed();
            }
        });
    egui::ComboBox::from_id_salt("fabric-coloration")
        .selected_text(material.fabric.coloration.label())
        .show_ui(ui, |ui| {
            for value in FabricColoration::ALL {
                changed |= ui
                    .selectable_value(&mut material.fabric.coloration, value, value.label())
                    .changed();
            }
        });
    if material.fabric.coloration == FabricColoration::Heather {
        let mut yarn = material.fabric.light_yarn_color.to_array();
        changed |= ui
            .color_edit_button_rgb(&mut yarn)
            .on_hover_text("Light yarn color, independent of dyed yarn")
            .changed();
        material.fabric.light_yarn_color = yarn.into();
        changed |= ui
            .add(
                egui::Slider::new(&mut material.fabric.light_yarn_fraction, 0.0..=0.8)
                    .text("Light yarn fraction"),
            )
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut material.fabric.strand_length, 1.0..=12.0)
                    .text("Strand length"),
            )
            .changed();
    }
    ui.label(RichText::new("Yarn and relief").strong());
    let base_pitch = crate::model::FabricSettings::preset(material.fabric.preset).pitch_x;
    let previous_scale = base_pitch / material.fabric.pitch_x.max(0.001);
    let mut texture_scale = previous_scale;
    if ui
        .add(egui::Slider::new(&mut texture_scale, 0.25..=8.0).text("Texture scale"))
        .on_hover_text("Higher values make the weave or knit finer")
        .changed()
    {
        let ratio = previous_scale / texture_scale;
        material.fabric.pitch_x *= ratio;
        material.fabric.pitch_y *= ratio;
        changed = true;
    }
    egui::CollapsingHeader::new("Advanced pitch").show(ui, |ui| {
        for (label, value) in [
            ("Pitch X", &mut material.fabric.pitch_x),
            ("Pitch Y", &mut material.fabric.pitch_y),
        ] {
            changed |= ui
                .add(egui::Slider::new(value, 0.002..=0.16).text(label))
                .changed();
        }
    });
    for (label, value, range) in [
        (
            "Relief depth",
            &mut material.fabric.relief_depth,
            0.0..=0.04,
        ),
        ("Normal tilt", &mut material.fabric.normal_gain, 0.0..=2.0),
        ("Fiber detail", &mut material.fabric.fiber_detail, 0.0..=1.5),
        ("Nap length", &mut material.fabric.nap_length, 0.0..=1.0),
    ] {
        changed |= ui
            .add(egui::Slider::new(value, range).text(label))
            .changed();
    }
    ui.label(RichText::new("Fiber sheen").strong());
    for (label, value) in [
        ("Sheen weight", &mut material.fabric.sheen_weight),
        ("Sheen spread", &mut material.fabric.sheen_spread),
        ("Fiber alignment", &mut material.fabric.fiber_alignment),
    ] {
        changed |= ui
            .add(egui::Slider::new(value, 0.0..=1.0).text(label))
            .changed();
    }
    let mut degrees = material.fabric.orientation.to_degrees();
    changed |= ui
        .add(egui::Slider::new(&mut degrees, -180.0..=180.0).text("Pattern direction"))
        .changed();
    material.fabric.orientation = degrees.to_radians();
    changed
}

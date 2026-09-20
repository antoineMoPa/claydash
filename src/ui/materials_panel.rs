use super::*;

pub(super) fn materials_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
    ui.label(RichText::new("Material library").strong());
    ui.horizontal_wrapped(|ui| {
        for kind in MaterialKind::ALL {
            let preset = Material::preset(kind);
            let response = material_preview(ui, kind, preset);
            if response.clicked() {
                apply_material(tree, preset);
            }
        }
    });
    ui.separator();
    if commands::selected_group_id(tree).is_some() {
        ui.label("Drill into the group to edit a primitive material.");
        return;
    }
    let selection = selected(tree);
    if selection.is_empty() {
        ui.label("Pick a material for new objects, or select objects to edit their material.");
        return;
    }
    let mut scene = objects(tree);
    let Some(first) = scene.iter().find(|object| selection.contains(&object.uuid)) else {
        return;
    };
    let first_id = first.uuid;
    let mut material = first.material;
    material.color = first.color;
    let mut keyframes = Vec::new();
    let mut rgba = material.color.to_array();
    let color_binding = AnimationBinding {
        object: first_id,
        property: AnimatableProperty::MaterialColor(ColorChannel::Red),
    };
    let color_response = animatable_widget(ui, tree, runtime, color_binding, |ui| {
        ui.color_edit_button_rgba_unmultiplied(&mut rgba)
    });
    let mut changed = color_response.changed();
    material.color = Vec4::from_array(rgba);
    if color_response.hovered() && ui.input(plain_i_pressed) {
        for object in scene
            .iter()
            .filter(|object| selection.contains(&object.uuid))
        {
            for channel in ColorChannel::ALL {
                keyframes.push(KeyframeRequest {
                    binding: AnimationBinding {
                        object: object.uuid,
                        property: AnimatableProperty::MaterialColor(channel),
                    },
                    value: material.color[channel.index()],
                });
            }
        }
    }
    color_response.on_hover_text("Press I to keyframe all four color channels");
    for (label, property, value, range) in [
        (
            "Roughness",
            AnimatableProperty::MaterialRoughness,
            &mut material.roughness,
            0.0..=1.0,
        ),
        (
            "Metallic",
            AnimatableProperty::MaterialMetallic,
            &mut material.metallic,
            0.0..=1.0,
        ),
        (
            "Reflectivity",
            AnimatableProperty::MaterialReflectivity,
            &mut material.reflectivity,
            0.0..=1.0,
        ),
        (
            "Refractive index",
            AnimatableProperty::MaterialRefractiveIndex,
            &mut material.refractive_index,
            1.0..=2.5,
        ),
        (
            "Opacity",
            AnimatableProperty::MaterialOpacity,
            &mut material.opacity,
            0.02..=1.0,
        ),
    ] {
        let label = ui.label(label);
        ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(24.0);
        let binding = AnimationBinding {
            object: first_id,
            property,
        };
        let response = animatable_widget(ui, tree, runtime, binding, |ui| {
            ui.add(egui::Slider::new(value, range))
                .labelled_by(label.id)
        });
        if response.hovered() && ui.input(plain_i_pressed) {
            for object in scene
                .iter()
                .filter(|object| selection.contains(&object.uuid))
            {
                keyframes.push(KeyframeRequest {
                    binding: AnimationBinding {
                        object: object.uuid,
                        property,
                    },
                    value: *value,
                });
            }
        }
        response
            .clone()
            .on_hover_text("Press I to insert a keyframe at the current frame");
        changed |= response.changed();
    }
    if changed {
        tree.set_path(
            "editor.material",
            crate::model::ClaydashValue::Material(material),
        );
        for object in &mut scene {
            if selection.contains(&object.uuid) {
                object.material = material;
                object.color = material.color;
            }
        }
        set_objects(tree, scene);
    }
    apply_keyframe_requests(tree, runtime, keyframes);
}

pub(super) fn apply_material(tree: &mut DataTree, material: Material) {
    tree.set_path(
        "editor.material",
        crate::model::ClaydashValue::Material(material),
    );
    if commands::selected_group_id(tree).is_some() {
        tree.make_undo_redo_snapshot();
        return;
    }
    let selection = selected(tree);
    let mut scene = objects(tree);
    for object in &mut scene {
        if selection.contains(&object.uuid) {
            object.material = material;
            object.color = material.color;
        }
    }
    set_objects(tree, scene);
    tree.make_undo_redo_snapshot();
}

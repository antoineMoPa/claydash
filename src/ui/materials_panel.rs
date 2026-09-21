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
            if material_preview(ui, kind, preset).clicked() {
                apply_material(tree, preset);
            }
        }
    });
    ui.add_space(4.0);
    let picker_id = ui.id().with("material-filter");
    let mut filter = ui
        .ctx()
        .data(|data| data.get_temp::<String>(picker_id))
        .unwrap_or_default();
    let selected_name = crate::model::picked_material_id(tree)
        .and_then(|id| {
            crate::model::material_assets(tree)
                .into_iter()
                .find(|asset| asset.uuid == id)
                .map(|asset| asset.name)
        })
        .unwrap_or_else(|| "Choose material…".into());
    ui.menu_button(selected_name, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut filter)
                .hint_text("Filter materials")
                .desired_width(190.0),
        );
        ui.separator();
        for kind in MaterialKind::ALL {
            if (filter.is_empty() || kind.label().contains(&filter))
                && ui.button(kind.label()).clicked()
            {
                apply_material(tree, Material::preset(kind));
                ui.close();
            }
        }
        let assets = crate::model::material_assets(tree);
        if !assets.is_empty() {
            ui.separator();
            for asset in assets {
                if (filter.is_empty() || asset.name.contains(&filter))
                    && ui.button(&asset.name).clicked()
                {
                    tree.set_path(
                        "editor.material_id",
                        crate::model::ClaydashValue::Uuid(asset.uuid),
                    );
                    let selection = commands::effective_selected_ids(tree);
                    let mut scene = objects(tree);
                    assign_material(&mut scene, &selection, asset.material, Some(asset.uuid));
                    set_objects(tree, scene);
                    tree.set_path(
                        "editor.material",
                        crate::model::ClaydashValue::Material(asset.material),
                    );
                    tree.make_undo_redo_snapshot();
                    ui.close();
                }
            }
        }
    });
    ui.ctx()
        .data_mut(|data| data.insert_temp(picker_id, filter));
    ui.separator();
    let selection = commands::effective_selected_ids(tree);
    if selection.is_empty() {
        ui.label("Pick a material for new objects, or select objects to edit their material.");
        return;
    }
    let mut scene = objects(tree);
    let Some(first) = scene.iter().find(|object| selection.contains(&object.uuid)) else {
        return;
    };
    let first_id = first.uuid;
    let mut material_id = first.material_id;
    let mut material = first.material;
    material.color = first.color;
    if let Some(id) = material_id {
        if let Some(asset) = crate::model::material_assets(tree)
            .into_iter()
            .find(|asset| asset.uuid == id)
        {
            let mut name = asset.name;
            ui.horizontal(|ui| {
                ui.label("Name");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut name)
                            .desired_width(ui.available_width().max(48.0)),
                    )
                    .on_hover_text("Rename this scene material")
                    .changed()
                {
                    crate::model::rename_material_asset(tree, id, name);
                }
            });
        }
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Copy")
            .on_hover_text("Copy this shared material")
            .clicked()
        {
            let was_unlinked = material_id.is_none();
            let id =
                material_id.unwrap_or_else(|| crate::model::ensure_material_asset(tree, material));
            material_id = Some(id);
            assign_material(&mut scene, &selection, material, Some(id));
            set_objects(tree, scene.clone());
            tree.set_path("editor.material_id", crate::model::ClaydashValue::Uuid(id));
            if was_unlinked {
                tree.make_undo_redo_snapshot();
            }
            tree.set_path(
                "editor.material_clipboard",
                crate::model::ClaydashValue::Material(material),
            );
            tree.set_path(
                "editor.material_clipboard_id",
                crate::model::ClaydashValue::Uuid(id),
            );
        }
        let clipboard = match tree.get_path("editor.material_clipboard") {
            crate::model::ClaydashValue::Material(value) => Some(value),
            _ => None,
        };
        if ui
            .add_enabled(clipboard.is_some(), egui::Button::new("Paste"))
            .clicked()
        {
            if let Some(value) = clipboard {
                let id = match tree.get_path("editor.material_clipboard_id") {
                    crate::model::ClaydashValue::Uuid(id) => id,
                    _ => crate::model::ensure_material_asset(tree, value),
                };
                assign_material(&mut scene, &selection, value, Some(id));
                material = value;
                material_id = Some(id);
                set_objects(tree, scene.clone());
                tree.set_path(
                    "editor.material",
                    crate::model::ClaydashValue::Material(value),
                );
                tree.set_path("editor.material_id", crate::model::ClaydashValue::Uuid(id));
                tree.make_undo_redo_snapshot();
            }
        }
        if ui
            .button("Unlink")
            .on_hover_text("Make a unique copy that no longer changes with the shared material")
            .clicked()
        {
            let unique_id =
                crate::model::create_unlinked_material_asset(tree, material_id, material);
            material_id = Some(unique_id);
            assign_material(&mut scene, &selection, material, material_id);
            set_objects(tree, scene.clone());
            tree.set_path(
                "editor.material_id",
                crate::model::ClaydashValue::Uuid(unique_id),
            );
            tree.make_undo_redo_snapshot();
        }
    });
    if commands::selected_group_id(tree).is_some() {
        ui.weak(format!(
            "Editing material for all {} objects in this group",
            selection.len()
        ));
    } else if let Some(id) = material_id {
        let linked = scene
            .iter()
            .filter(|object| object.material_id == Some(id))
            .count();
        ui.weak(format!("Linked material · {linked} object(s)"));
    }
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
        let id = material_id.unwrap_or_else(|| crate::model::ensure_material_asset(tree, material));
        crate::model::update_material_asset(tree, id, material);
        for object in &mut scene {
            if object.material_id == Some(id) || selection.contains(&object.uuid) {
                object.material_id = Some(id);
                object.material = material;
                object.color = material.color;
            }
        }
        tree.set_path("editor.material_id", crate::model::ClaydashValue::Uuid(id));
        set_objects(tree, scene);
    }
    apply_keyframe_requests(tree, runtime, keyframes);
}

pub(super) fn apply_material(tree: &mut DataTree, material: Material) {
    let material_id = crate::model::ensure_material_asset(tree, material);
    tree.set_path(
        "editor.material",
        crate::model::ClaydashValue::Material(material),
    );
    tree.set_path(
        "editor.material_id",
        crate::model::ClaydashValue::Uuid(material_id),
    );
    let selection = commands::effective_selected_ids(tree);
    let mut scene = objects(tree);
    assign_material(&mut scene, &selection, material, Some(material_id));
    set_objects(tree, scene);
    tree.make_undo_redo_snapshot();
}

fn assign_material(
    scene: &mut [SdfObject],
    selection: &[uuid::Uuid],
    material: Material,
    material_id: Option<uuid::Uuid>,
) {
    for object in scene {
        if selection.contains(&object.uuid) {
            object.material_id = material_id;
            object.material = material;
            object.color = material.color;
        }
    }
}

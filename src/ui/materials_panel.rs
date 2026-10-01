use super::*;
mod fabric;
mod metal;

const BASIC_MATERIAL_KINDS: [MaterialKind; 4] = [
    MaterialKind::Transparent,
    MaterialKind::Metallic,
    MaterialKind::Solid,
    MaterialKind::Brick,
];

fn material_matches_filter(label: &str, filter: &str) -> bool {
    label.to_lowercase().contains(&filter.trim().to_lowercase())
}

fn material_group(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    title: &str,
    default_open: bool,
    filter: &str,
    presets: impl IntoIterator<Item = (&'static str, Material)>,
) -> bool {
    let presets: Vec<_> = presets
        .into_iter()
        .filter(|(label, _)| material_matches_filter(label, filter))
        .collect();
    if presets.is_empty() {
        return false;
    }
    egui::CollapsingHeader::new(title)
        .default_open(default_open)
        .open((!filter.trim().is_empty()).then_some(true))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, material) in presets {
                    if material_preview(ui, label, material.kind, material).clicked() {
                        apply_material(tree, material);
                    }
                }
            });
        });
    true
}

pub(super) fn materials_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
    ui.label(RichText::new("Material library").strong());
    let picker_id = ui.id().with("material-filter");
    let mut filter = ui
        .ctx()
        .data(|data| data.get_temp::<String>(picker_id))
        .unwrap_or_default();
    ui.add(
        egui::TextEdit::singleline(&mut filter)
            .hint_text("Filter materials")
            .desired_width(ui.available_width()),
    );
    let mut matches = material_group(
        ui,
        tree,
        "Basic",
        true,
        &filter,
        BASIC_MATERIAL_KINDS
            .into_iter()
            .map(|kind| (kind.label(), Material::preset(kind))),
    );
    matches |= material_group(
        ui,
        tree,
        "Wood",
        false,
        &filter,
        WoodSpecies::ALL.into_iter()
            .map(|species| (species.label(), Material::wood_preset(species))),
    );
    matches |= material_group(
        ui,
        tree,
        "Fabrics",
        false,
        &filter,
        crate::model::FabricPreset::ALL.into_iter()
            .map(|preset| (preset.label(), Material::fabric_preset(preset))),
    );
    matches |= metal::library(ui, tree, &filter);
    matches |= material_group(
        ui,
        tree,
        "Misc",
        false,
        &filter,
        [(MaterialKind::Diagnostic.label(), Material::preset(MaterialKind::Diagnostic))],
    );
    if !matches {
        ui.weak("No matching presets");
    }
    ui.separator();
    let selected_name = crate::model::picked_material_id(tree)
        .and_then(|id| {
            crate::model::material_assets(tree)
                .into_iter()
                .find(|asset| asset.uuid == id)
                .map(|asset| asset.name)
        })
        .unwrap_or_else(|| "Choose material…".into());
    ui.menu_button(selected_name, |ui| {
        for kind in BASIC_MATERIAL_KINDS.into_iter().chain([MaterialKind::Diagnostic]) {
            if material_matches_filter(kind.label(), &filter)
                && ui.button(kind.label()).clicked()
            {
                apply_material(tree, Material::preset(kind));
                ui.close();
            }
        }
        for species in WoodSpecies::ALL {
            if material_matches_filter(species.label(), &filter)
                && ui.button(species.label()).clicked()
            {
                apply_material(tree, Material::wood_preset(species));
                ui.close();
            }
        }
        for preset_kind in crate::model::FabricPreset::ALL {
            if material_matches_filter(preset_kind.label(), &filter)
                && ui.button(preset_kind.label()).clicked()
            {
                apply_material(tree, Material::fabric_preset(preset_kind));
                ui.close();
            }
        }
        metal::picker(ui, tree, &filter);
        let assets = crate::model::material_assets(tree);
        if !assets.is_empty() {
            ui.separator();
            let previews = ui.ctx().data(|data| {
                data.get_temp::<crate::renderer::MaterialPreviewIds>(
                    crate::renderer::MaterialPreviewIds::egui_id(),
                )
            });
            for asset in assets {
                if !material_matches_filter(&asset.name, &filter) {
                    continue;
                }
                let clicked = ui
                    .horizontal(|ui| {
                        if let Some(texture) =
                            previews.as_ref().and_then(|ids| ids.for_asset(asset.uuid))
                        {
                            ui.add(egui::Image::new((texture, egui::vec2(32.0, 21.0))));
                        }
                        ui.button(&asset.name).clicked()
                    })
                    .inner;
                if clicked {
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
    let selection = commands::effective_selected_ids(tree);
    let custom_count = crate::model::material_assets(tree)
        .iter()
        .filter(|asset| asset.material.kind == MaterialKind::Custom)
        .count();
    if ui
        .add_enabled(custom_count < 16, egui::Button::new("New WGSL material"))
        .on_disabled_hover_text("A scene can contain at most 16 custom WGSL materials")
        .clicked()
    {
        let asset = crate::model::MaterialAsset::custom("Custom WGSL".to_owned());
        let id = asset.uuid;
        let material = asset.material;
        let mut assets = crate::model::material_assets(tree);
        assets.push(asset);
        crate::model::set_material_assets(tree, assets);
        tree.set_path("editor.material_id", crate::model::ClaydashValue::Uuid(id));
        tree.set_path(
            "editor.material",
            crate::model::ClaydashValue::Material(material),
        );
        let mut scene = objects(tree);
        assign_material(&mut scene, &selection, material, Some(id));
        set_objects(tree, scene);
        tree.make_undo_redo_snapshot();
    }
    let editing_id = objects(tree)
        .iter()
        .find(|object| selection.contains(&object.uuid))
        .and_then(|object| object.material_id)
        .or_else(|| crate::model::picked_material_id(tree));
    if let Some(asset) = editing_id.and_then(|id| {
        crate::model::material_assets(tree)
            .into_iter()
            .find(|asset| asset.uuid == id && asset.material.kind == MaterialKind::Custom)
    }) {
        custom_shader_editor(ui, tree, &asset);
    }
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
    if material.kind != MaterialKind::Custom {
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
            .add_enabled(
                material.kind != MaterialKind::Custom || custom_count < 16,
                egui::Button::new("Unlink"),
            )
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
    if material.kind == MaterialKind::Metal {
        ui.label("Metal tint");
    }
    let mut rgba = material.color.to_array();
    let color_binding = AnimationBinding {
        object: first_id,
        property: AnimatableProperty::MaterialColor(ColorChannel::Red),
    };
    let color_response = animatable_widget(ui, tree, runtime, color_binding, |ui| {
        if material.kind == MaterialKind::Metal {
            let mut rgb = [rgba[0], rgba[1], rgba[2]];
            let response = ui.color_edit_button_rgb(&mut rgb);
            rgba[..3].copy_from_slice(&rgb);
            response
        } else {
            ui.color_edit_button_rgba_unmultiplied(&mut rgba)
        }
    });
    let mut changed = color_response.changed();
    material.color = Vec4::from_array(rgba);
    if color_response.hovered() && ui.input(plain_i_pressed) {
        for object in scene
            .iter()
            .filter(|object| selection.contains(&object.uuid))
        {
            for channel in ColorChannel::ALL {
                if material.kind == MaterialKind::Metal && channel == ColorChannel::Alpha { continue; }
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
    color_response.on_hover_text(if material.kind == MaterialKind::Metal {
        "Press I to keyframe the three tint channels"
    } else {
        "Press I to keyframe all four color channels"
    });
    if material.kind == MaterialKind::Metal {
        changed |= metal::controls(ui, &mut material);
    }
    if material.kind == MaterialKind::Fabric {
        changed |= fabric::fabric_controls(ui, &mut material);
    }
    if material.kind == MaterialKind::Brick {
        ui.separator();
        ui.label(RichText::new("Brick bond").strong());
        for (label, value, range) in [
            ("Brick width", &mut material.brick.width, 0.30..=0.80),
            (
                "Course height",
                &mut material.brick.course_height,
                0.16..=0.38,
            ),
            (
                "Mortar width",
                &mut material.brick.mortar_width,
                0.006..=0.045,
            ),
            ("Wear", &mut material.brick.wear, 0.0..=1.0),
            ("Joint depth", &mut material.brick.relief, 0.0..=0.055),
            ("Edge rounding", &mut material.brick.bevel, 0.002..=0.035),
            ("Clay porosity", &mut material.brick.porosity, 0.0..=1.0),
            ("Firing variation", &mut material.brick.firing, 0.0..=1.0),
            (
                "Efflorescence",
                &mut material.brick.efflorescence,
                0.0..=1.0,
            ),
        ] {
            changed |= ui
                .add(egui::Slider::new(value, range).text(label))
                .changed();
        }
        ui.horizontal(|ui| {
            ui.label("Mortar color");
            let mut mortar = material.brick.mortar_color.to_array();
            changed |= ui.color_edit_button_rgb(&mut mortar).changed();
            material.brick.mortar_color = Vec3::from_array(mortar);
        });
        ui.separator();
    }
    if material.kind == MaterialKind::Wood {
        ui.separator();
        ui.label(RichText::new("Wood grain").strong());
        ui.horizontal(|ui| {
            ui.label("Species");
            egui::ComboBox::from_id_salt("wood-species")
                .selected_text(material.wood.species.label())
                .show_ui(ui, |ui| {
                    for species in WoodSpecies::ALL {
                        if ui
                            .selectable_label(material.wood.species == species, species.label())
                            .clicked()
                        {
                            let previous_species = material.wood.species;
                            let preset = Material::wood_preset(species);
                            material.color = preset.color;
                            material.wood = preset.wood;
                            if let Some(id) = material_id {
                                if crate::model::material_assets(tree).iter().any(|asset| {
                                    asset.uuid == id && asset.name == previous_species.label()
                                }) {
                                    crate::model::rename_material_asset(
                                        tree,
                                        id,
                                        species.label().into(),
                                    );
                                }
                            }
                            changed = true;
                        }
                    }
                });
        });
        egui::CollapsingHeader::new("Growth and cut")
            .default_open(true)
            .show(ui, |ui| {
                let mut cut_degrees = material.wood.cut_angle.to_degrees();
                changed |= ui
                    .add(egui::Slider::new(&mut cut_degrees, -72.0..=72.0).text("Cut angle °"))
                    .changed();
                material.wood.cut_angle = cut_degrees.to_radians();
                let mut ring_millimeters = material.wood.ring_spacing * 100.0;
                changed |= ui
                    .add(
                        egui::Slider::new(&mut ring_millimeters, 1.0..=15.0)
                            .text("Mean ring width (mm)"),
                    )
                    .changed();
                material.wood.ring_spacing = ring_millimeters / 100.0;
                for (label, value, range) in [
                    ("Ring color", &mut material.wood.ring_contrast, 0.0..=1.0),
                    ("Ring ridge", &mut material.wood.ring_relief, 0.0..=1.0),
                    (
                        "Ring variation",
                        &mut material.wood.ring_variation,
                        0.0..=1.0,
                    ),
                    ("Knots", &mut material.wood.knots, 0.0..=1.0),
                    ("End checks", &mut material.wood.end_checks, 0.0..=1.0),
                ] {
                    changed |= ui
                        .add(egui::Slider::new(value, range).text(label))
                        .changed();
                }
            });
        ui.collapsing("Fiber and pores", |ui| {
            for (label, value, range) in [
                ("Bump", &mut material.wood.bump, 0.0..=1.6),
                ("Fiber relief", &mut material.wood.fiber_relief, 0.0..=1.0),
                ("Fiber pigment", &mut material.wood.fiber_pigment, 0.0..=1.0),
                (
                    "Directionality",
                    &mut material.wood.fiber_directionality,
                    2.0..=20.0,
                ),
                (
                    "Scale falloff",
                    &mut material.wood.scale_falloff,
                    0.35..=1.55,
                ),
                ("Vessel pores", &mut material.wood.pores, 0.0..=1.0),
                ("Figure", &mut material.wood.figure, 0.0..=1.0),
            ] {
                changed |= ui
                    .add(egui::Slider::new(value, range).text(label))
                    .changed();
            }
        });
        ui.collapsing("Surface and finish", |ui| {
            changed |= ui
                .add(
                    egui::Slider::new(&mut material.wood.sanding_grit, 0.0..=1.0)
                        .text("Sanding grit"),
                )
                .changed();
            let mut sanding_degrees = material.wood.sanding_angle.to_degrees();
            changed |= ui
                .add(
                    egui::Slider::new(&mut sanding_degrees, -90.0..=90.0)
                        .text("Sanding direction °"),
                )
                .changed();
            material.wood.sanding_angle = sanding_degrees.to_radians();
            ui.horizontal(|ui| {
                ui.label("Stain color");
                egui::ComboBox::from_id_salt("wood-stain")
                    .selected_text(material.wood.stain_color.label())
                    .show_ui(ui, |ui| {
                        for stain in WoodStain::ALL {
                            changed |= ui
                                .selectable_value(
                                    &mut material.wood.stain_color,
                                    stain,
                                    stain.label(),
                                )
                                .changed();
                        }
                    });
            });
            for (label, value) in [
                ("Stain load", &mut material.wood.stain_load),
                ("Poly build", &mut material.wood.coat),
                ("Poly sheen", &mut material.wood.coat_sheen),
                ("Poly amber", &mut material.wood.coat_amber),
            ] {
                changed |= ui
                    .add(egui::Slider::new(value, 0.0..=1.0).text(label))
                    .changed();
            }
        });
        ui.separator();
    }
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
        if material.kind == MaterialKind::Metal && property != AnimatableProperty::MaterialRoughness
        {
            continue;
        }
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

fn custom_shader_editor(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    asset: &crate::model::MaterialAsset,
) {
    ui.separator();
    ui.label(RichText::new(format!("WGSL · {}", asset.name)).strong());
    let name_id = ui.id().with(("custom-wgsl-name", asset.uuid));
    let mut name = ui
        .ctx()
        .data(|data| data.get_temp::<String>(name_id))
        .unwrap_or_else(|| asset.name.clone());
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0));
        if ui
            .add_enabled(
                !name.trim().is_empty() && name != asset.name,
                egui::Button::new("Rename"),
            )
            .clicked()
        {
            crate::model::rename_material_asset(tree, asset.uuid, name.clone());
            tree.make_undo_redo_snapshot();
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(name_id, name));
    let draft_id = ui.id().with(("custom-wgsl-draft", asset.uuid));
    let error_id = ui.id().with(("custom-wgsl-error", asset.uuid));
    let current = asset.wgsl.as_deref().unwrap_or_default();
    let mut draft = ui
        .ctx()
        .data(|data| data.get_temp::<String>(draft_id))
        .unwrap_or_else(|| current.to_owned());
    ui.add(
        egui::TextEdit::multiline(&mut draft)
            .code_editor()
            .desired_rows(12)
            .desired_width(f32::INFINITY),
    );
    let dirty = draft != current;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(dirty, egui::Button::new("Apply WGSL"))
            .clicked()
        {
            let mut assets = crate::model::material_assets(tree);
            if let Some(candidate) = assets
                .iter_mut()
                .find(|candidate| candidate.uuid == asset.uuid)
            {
                candidate.wgsl = Some(draft.clone());
            }
            match crate::renderer::validate_custom_materials(&assets) {
                Ok(()) => {
                    crate::model::set_material_assets(tree, assets);
                    tree.make_undo_redo_snapshot();
                    ui.ctx().data_mut(|data| data.remove::<String>(error_id));
                }
                Err(error) => {
                    ui.ctx().data_mut(|data| data.insert_temp(error_id, error));
                }
            }
        }
        if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
            draft = current.to_owned();
            ui.ctx().data_mut(|data| data.remove::<String>(error_id));
        }
    });
    if let Some(error) = ui.ctx().data(|data| data.get_temp::<String>(error_id)) {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    if let Some(error) = ui
        .ctx()
        .data(|data| data.get_temp::<Option<String>>(egui::Id::new("custom-material-render-error")))
        .flatten()
    {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    ui.ctx().data_mut(|data| data.insert_temp(draft_id, draft));
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

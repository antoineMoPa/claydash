use super::*;

pub(super) fn modifiers_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
) {
    animation::migrate_legacy_lattice_tracks(tree);
    ui.spacing_mut().item_spacing.y = 8.0;
    ui.label(RichText::new("Modifiers").strong());
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
    let repeat_spacing =
        bounds.map(|(min, max)| ((max - min) * 1.1).clamp(Vec3::splat(0.01), Vec3::splat(20.0)));
    let has_children = crate::model::has_boolean_children(&scene, target);
    let profile_curves: Vec<_> = scene
        .iter()
        .filter(|candidate| {
            candidate.uuid != target && matches!(candidate.params, SdfParams::BezierCurveParams(_))
        })
        .map(|candidate| (candidate.uuid, candidate.display_name()))
        .collect();
    let parents: std::collections::HashMap<_, _> = scene
        .iter()
        .map(|object| (object.uuid, object.boolean_parent))
        .collect();
    let root_for = |mut id| {
        for _ in 0..scene.len() {
            let Some(Some(parent)) = parents.get(&id) else {
                break;
            };
            id = *parent;
        }
        id
    };
    let mut unavailable_roots = std::collections::HashSet::new();
    unavailable_roots.insert(root_for(target));
    for member in &scene {
        if member.surface_inlay.is_some()
            || member.lattice.is_some()
            || member.mirror.is_some()
            || member.repetition.enabled
            || matches!(member.params, SdfParams::BezierCurveParams(_))
        {
            unavailable_roots.insert(root_for(member.uuid));
        }
    }
    let surface_hosts: Vec<_> = scene
        .iter()
        .filter(|candidate| {
            candidate.boolean_parent.is_none() && !unavailable_roots.contains(&candidate.uuid)
        })
        .map(|candidate| (candidate.uuid, candidate.display_name()))
        .collect();
    let Some(object) = scene.iter_mut().find(|object| object.uuid == target) else {
        return;
    };
    let is_curve = matches!(object.params, SdfParams::BezierCurveParams(_));
    let can_lattice = !is_curve && object.boolean_parent.is_none();
    let can_mirror = !is_curve && object.boolean_parent.is_none();
    let can_repeat = !is_curve && (!has_children || object.boolean_parent.is_none());
    let mut changed = false;
    let mut snapshot = false;
    ui.menu_button("+ Add modifier", |ui| {
        if is_curve
            && ui
                .add_enabled(
                    object.path_extrusion.is_none(),
                    egui::Button::new("Path Extrusion"),
                )
                .clicked()
        {
            object.path_extrusion = Some(crate::model::PathExtrusion::default());
            changed = true;
            snapshot = true;
            ui.close();
        }
        if ui
            .add_enabled(
                !is_curve
                    && !has_children
                    && object.boolean_parent.is_none()
                    && object.surface_inlay.is_none()
                    && !surface_hosts.is_empty()
                    && object.lattice.is_none()
                    && object.mirror.is_none()
                    && !object.repetition.enabled,
                egui::Button::new("Surface inlay"),
            )
            .clicked()
        {
            object.surface_inlay = Some(crate::model::SurfaceInlay {
                host: surface_hosts[0].0,
                offset: 0.018,
                thickness: 0.012,
            });
            changed = true;
            snapshot = true;
            ui.close();
        }
        if ui
            .add_enabled(
                can_mirror && object.mirror.is_none(),
                egui::Button::new("Mirror"),
            )
            .clicked()
        {
            object.mirror = Some(crate::model::Mirror::default());
            changed = true;
            snapshot = true;
            ui.close();
        }
        if ui
            .add_enabled(
                can_lattice && object.lattice.is_none(),
                egui::Button::new("Lattice"),
            )
            .clicked()
        {
            if let Some((min, max)) = bounds {
                object.lattice = Some(crate::model::Lattice::new(min, max, 2));
                changed = true;
                snapshot = true;
            }
            ui.close();
        }
        if ui
            .add_enabled(
                can_repeat && !object.repetition.enabled,
                egui::Button::new("Repeat"),
            )
            .clicked()
        {
            object.repetition = crate::model::Repetition {
                enabled: true,
                spacing: repeat_spacing
                    .unwrap_or_else(|| crate::model::Repetition::default().spacing),
                ..Default::default()
            };
            changed = true;
            snapshot = true;
            ui.close();
        }
    });

    let mut remove_inlay = false;
    if let Some(inlay) = &mut object.surface_inlay {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Surface inlay").strong().size(16.0));
                remove_inlay = ui.small_button("Remove").clicked();
            });
            if !remove_inlay {
                let selected_name = surface_hosts
                    .iter()
                    .find(|(id, _)| *id == inlay.host)
                    .map_or("Missing host", |(_, name)| name.as_str());
                egui::ComboBox::from_id_salt("surface-inlay-host")
                    .selected_text(selected_name)
                    .show_ui(ui, |ui| {
                        for (id, name) in &surface_hosts {
                            changed |= ui.selectable_value(&mut inlay.host, *id, name).changed();
                        }
                    });
                changed |= ui
                    .add(egui::Slider::new(&mut inlay.offset, -0.2..=0.2).text("Offset"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut inlay.thickness, 0.001..=0.1).text("Thickness"))
                    .changed();
                ui.weak("Uses this object's shape as the inlay boundary on the host surface.");
            }
        });
    }
    if remove_inlay {
        object.surface_inlay = None;
        changed = true;
        snapshot = true;
    }
    let mut remove_lattice = false;
    let mut deleted_shape_key = None;
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
            ui.label(RichText::new("Shape keys").strong());
            if ui.button("+ Add shape from current lattice").clicked() {
                lattice.add_shape_key();
                changed = true;
                snapshot = true;
            }
            if lattice.current_shape_key.is_none() {
                ui.weak(format!(
                    "Previewing position {:.2}. Select a shape to edit it.",
                    lattice.shape_position
                ));
            }
            if ui
                .selectable_label(lattice.current_shape_key == Some(0), "0 · Reset grid")
                .clicked()
            {
                lattice.select_shape_key(0);
                changed = true;
                snapshot = true;
            }
            let mut select_shape_key = None;
            for (index, key) in lattice.shape_keys.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::selectable(
                                lattice.current_shape_key == Some(index + 1),
                                format!("{} ·", index + 1),
                            )
                            .min_size(egui::vec2(36.0, 24.0)),
                        )
                        .clicked()
                    {
                        select_shape_key = Some(index + 1);
                    }
                    let remaining_width = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(remaining_width, 24.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui
                                .add(egui::Button::new("Delete").min_size(egui::vec2(0.0, 24.0)))
                                .on_hover_text("Delete shape key")
                                .clicked()
                            {
                                deleted_shape_key = Some(index + 1);
                            }
                            let name_width = ui.available_width().max(24.0);
                            if ui
                                .add_sized(
                                    [name_width, 24.0],
                                    egui::TextEdit::singleline(&mut key.name),
                                )
                                .changed()
                            {
                                changed = true;
                                snapshot = true;
                            }
                        },
                    );
                });
            }
            if let Some(index) = select_shape_key {
                lattice.select_shape_key(index);
                changed = true;
                snapshot = true;
            }
            if let Some(index) = deleted_shape_key {
                lattice.shape_keys.remove(index - 1);
                let selected = lattice.current_shape_key.unwrap_or(0);
                lattice.current_shape_key = Some(if selected >= index {
                    selected.saturating_sub(1)
                } else {
                    selected
                });
                lattice.select_shape_key(lattice.current_shape_key.unwrap_or(0));
                changed = true;
                snapshot = true;
            }
            ui.separator();
            if ui.button("Reset all points").clicked() {
                lattice.offsets.fill(Vec3::ZERO);
                lattice.save_selected_shape_key();
                changed = true;
                snapshot = true;
            }
        });
    } else if !object.repetition.enabled
        && object.mirror.is_none()
        && object.path_extrusion.is_none()
    {
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
    let mut remove_path_extrusion = false;
    if let Some(path) = &mut object.path_extrusion {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Path Extrusion").strong().size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    remove_path_extrusion = ui.small_button("Remove").clicked();
                });
            });
            if !remove_path_extrusion {
                let size =
                    ui.add(egui::Slider::new(&mut path.radius, 0.01..=2.0).text("Profile size"));
                changed |= size.changed();
                snapshot |= size.drag_stopped();
                ui.label("Cross section");
                ui.horizontal(|ui| {
                    changed |= ui
                        .selectable_value(&mut path.profile_curve, None, "Built-in")
                        .changed();
                    if path.profile_curve.is_none() {
                        changed |= ui
                            .selectable_value(
                                &mut path.profile,
                                crate::model::BezierProfile::Round,
                                "Round",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut path.profile,
                                crate::model::BezierProfile::Square,
                                "Square",
                            )
                            .changed();
                    }
                });
                egui::ComboBox::from_label("Profile curve")
                    .selected_text(
                        path.profile_curve
                            .and_then(|id| {
                                profile_curves
                                    .iter()
                                    .find(|(candidate, _)| *candidate == id)
                                    .map(|(_, name)| name.as_str())
                            })
                            .unwrap_or("None"),
                    )
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(&mut path.profile_curve, None, "None")
                            .changed();
                        for (id, name) in &profile_curves {
                            changed |= ui
                                .selectable_value(&mut path.profile_curve, Some(*id), name)
                                .changed();
                        }
                    });
                if path.profile_curve.is_some() {
                    ui.weak(
                        "The profile curve's local XY shape is closed and swept along this path.",
                    );
                    if path.profile_curve.is_some_and(|id| {
                        !profile_curves.iter().any(|(candidate, _)| *candidate == id)
                    }) {
                        ui.colored_label(
                            Color32::LIGHT_RED,
                            "Profile curve is missing. Choose another curve.",
                        );
                    }
                }
                snapshot |= changed;
            }
        });
    }
    if remove_path_extrusion {
        object.path_extrusion = None;
        changed = true;
        snapshot = true;
    }
    let mut remove_mirror = false;
    if let Some(mirror) = &mut object.mirror {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Mirror").strong().size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    remove_mirror = ui.small_button("Remove").clicked();
                });
            });
            if !remove_mirror {
                ui.label("Reflect the positive side across local axes");
                ui.horizontal(|ui| {
                    for (axis, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                        let response = ui.checkbox(&mut mirror.axes[axis], label);
                        if response.changed() {
                            changed = true;
                            snapshot = true;
                        }
                    }
                });
            }
        });
    }
    if remove_mirror {
        object.mirror = None;
        changed = true;
        snapshot = true;
    }
    let show_repeat = object.repetition.enabled;
    if changed {
        set_objects(tree, scene);
    }
    if remove_lattice {
        animation::remove_lattice_track(tree, target);
    }
    if let Some(index) = deleted_shape_key {
        animation::remap_lattice_shape_keys_after_delete(tree, target, index);
    }
    if snapshot {
        tree.make_undo_redo_snapshot();
    }
    if show_repeat {
        let mut remove_repeat = false;
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Repeat").strong().size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    remove_repeat = ui.small_button("Remove").clicked();
                });
            });
            if !remove_repeat {
                repetition_panel(ui, tree, runtime);
            }
        });
        if remove_repeat {
            let mut scene = objects(tree);
            if let Some(object) = scene.iter_mut().find(|object| object.uuid == target) {
                object.repetition.enabled = false;
                set_objects(tree, scene);
                animation::remove_repetition_tracks(tree, target);
                tree.make_undo_redo_snapshot();
            }
        }
    }
}

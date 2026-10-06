use super::*;
use crate::renderer::computation::{self, Stage};

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
    let saved_accelerator = scene[index].sphere_accelerator;
    let saved_box_accelerator = scene[index].box_accelerator;
    let saved_splats = scene[index].gaussian_splats;
    let saved_mode = scene[index].render_representation;
    let mut selected_mode = saved_mode;
    let mut optimization_percent = None;
    let capture_revision = tree
        .path_version("scene.sdf_objects")
        .wrapping_add(tree.path_version("scene.materials"))
        .wrapping_add(tree.path_version("scene.selected_uuids"));
    egui::ComboBox::from_id_salt(ui.auto_id_with("group-render-representation"))
        .selected_text(saved_mode.label())
        .show_ui(ui, |ui| {
            for mode in crate::model::GroupRenderRepresentation::ALL {
                ui.selectable_value(&mut selected_mode, mode, mode.label())
                    .on_hover_text(mode.description());
            }
        });
    if matches!(selected_mode, crate::model::GroupRenderRepresentation::GaussianSplats | crate::model::GroupRenderRepresentation::PoissonMesh) {
        let settings = &mut scene[index].gaussian_splats;
        ui.horizontal(|ui| {
            ui.label(if selected_mode == crate::model::GroupRenderRepresentation::PoissonMesh { "Sample resolution" } else { "Splat resolution" }).on_hover_text("Grid of rays from each of the six bounding-box sides. Higher resolution preserves more detail and uses more memory. Click Recompute to apply.");
            ui.add(egui::DragValue::new(&mut settings.resolution).update_while_editing(false)
                .range(1..=u32::MAX).speed(1.0));
        });
        let status = if selected_mode == saved_mode && selected_mode == crate::model::GroupRenderRepresentation::GaussianSplats {
            crate::renderer::depth_accelerator_status(ui.ctx(), &scene, target, capture_revision)
        } else { crate::renderer::DepthAcceleratorStatus::NotComputedYet };
        if selected_mode == crate::model::GroupRenderRepresentation::GaussianSplats {
            recompute_button(ui, target, matches!(status, crate::renderer::DepthAcceleratorStatus::Baking(_)));
            if let crate::renderer::DepthAcceleratorStatus::Baking(percent) = status {
                optimization_percent = Some(percent);
            }
        }
        if selected_mode == crate::model::GroupRenderRepresentation::GaussianSplats { ui.weak(match status {
            crate::renderer::DepthAcceleratorStatus::Ready => "100%".to_owned(),
            crate::renderer::DepthAcceleratorStatus::Baking(percent) => format!("{percent}%"),
            crate::renderer::DepthAcceleratorStatus::NotComputedYet => "0%".to_owned(),
        }); }
        #[cfg(not(target_arch = "wasm32"))]
        if selected_mode == crate::model::GroupRenderRepresentation::PoissonMesh {
            poisson_mesh_controls(ui, target);
        }
    }
    if selected_mode.is_depth_accelerator() {
        let settings = match selected_mode {
            crate::model::GroupRenderRepresentation::SphereAccelerator => &mut scene[index].sphere_accelerator,
            crate::model::GroupRenderRepresentation::BoxAccelerator => &mut scene[index].box_accelerator,
            _ => unreachable!(),
        };
        ui.horizontal(|ui| {
            ui.label("Exact march distance").on_hover_text("World-space distance from the baked depth surface where tracing switches to the source SDF. Applies immediately without rebaking; independent of surface hit tolerance.");
            ui.add(egui::DragValue::new(&mut settings.configurable_epsilon)
                .update_while_editing(false).range(0.0001_f32..=1000.0_f32)
                .speed(0.01).max_decimals(4));
        });
    }
    if selected_mode == crate::model::GroupRenderRepresentation::NeuralSdf {
        let settings = &mut scene[index].neural_sdf;
        let preset = crate::model::NeuralModelPreset::matching(settings.training);
        ui.label("Model size");
        egui::ComboBox::from_id_salt(ui.auto_id_with("neural-model-size"))
            .selected_text(preset.map_or("Custom", |preset| preset.label()))
            .show_ui(ui, |ui| {
                for choice in crate::model::NeuralModelPreset::ALL {
                    if ui.selectable_label(preset == Some(choice), choice.label()).clicked() {
                        let raymarch_last_segment = settings.training.raymarch_last_segment;
                        let distance_offset = settings.training.distance_offset;
                        settings.training = choice.settings();
                        settings.training.raymarch_last_segment = raymarch_last_segment;
                        settings.training.distance_offset = distance_offset;
                        settings.hit_distance_cells = crate::model::NeuralSdfSettings::default().hit_distance_cells;
                    }
                }
            }).response.on_hover_text("Model size presets. Larger models trade training and rendering speed for fit quality. Click Recompute to apply; edit parameters below for Custom settings.");
        egui::Grid::new(ui.auto_id_with("neural-settings")).num_columns(2).show(ui, |ui| {
            ui.label("Activation");
            egui::ComboBox::from_id_salt("neural-activation").selected_text(settings.training.activation.label()).show_ui(ui, |ui| {
                for activation in crate::model::NeuralActivation::ALL {
                    ui.selectable_value(&mut settings.training.activation, activation, activation.label());
                }
            }); ui.end_row();
            ui.label("Hidden layers");
            ui.add(egui::DragValue::new(&mut settings.training.layers).update_while_editing(false).range(1..=crate::model::NeuralTrainingSettings::MAX_LAYERS)); ui.end_row();
            ui.label("Width");
            ui.add(egui::DragValue::new(&mut settings.training.width).update_while_editing(false).range(4..=crate::model::NeuralTrainingSettings::MAX_WIDTH)); ui.end_row();
            ui.label("Samples").on_hover_text("Total candidate points distributed uniformly at random in the bounding box. Training discards half the points farther than 0.4 scene units from the surface; applies on Recompute.");
            ui.add(egui::DragValue::new(&mut settings.training.samples).update_while_editing(false).range(512..=crate::model::NeuralTrainingSettings::MAX_SAMPLES)); ui.end_row();
            ui.label("Epochs").on_hover_text("One full pass through all random samples per epoch.");
            ui.add(egui::DragValue::new(&mut settings.training.epochs).update_while_editing(false).range(1..=512)); ui.end_row();
            ui.label("Learning rate");
            learning_rate_input(ui, &mut settings.training.learning_rate); ui.end_row();
            ui.label("Seed").on_hover_text("Controls initialization, random sample positions, and sample order. The same seed and settings reproduce the same fit.");
            ui.add(egui::DragValue::new(&mut settings.training.seed).update_while_editing(false)); ui.end_row();
            ui.label("Hit distance").on_hover_text("Distance from zero accepted as a hit, in equivalent sample-spacing units. Larger values stop sooner and expand the silhouette. Applies immediately without retraining.");
            ui.add(egui::DragValue::new(&mut settings.hit_distance_cells).update_while_editing(false).range(0.001..=4.0).speed(0.001).max_decimals(3)); ui.end_row();
            ui.label("Raymarch last segment").on_hover_text("Use the fitted field to approach, then finish against the original source. Applies after Recompute.");
            ui.checkbox(&mut settings.training.raymarch_last_segment, ""); ui.end_row();
            if settings.training.raymarch_last_segment {
                ui.label("Training offset").on_hover_text("Subtract this world-space distance from every training target so the fitted surface expands and hands off before the source. Applies after Recompute.");
                ui.add(egui::DragValue::new(&mut settings.training.distance_offset).update_while_editing(false).range(0.0..=10.0).speed(0.01).max_decimals(3)); ui.end_row();
            }
        });
        let status = ui.ctx().data(|data| data.get_temp::<std::collections::HashMap<uuid::Uuid, crate::renderer::NeuralStatus>>(egui::Id::new("neural-sdf-status")))
            .and_then(|statuses| statuses.get(&target).cloned());
        use crate::renderer::NeuralStatus;
        let computing = matches!(
            status,
            Some(NeuralStatus::Pending | NeuralStatus::Training { .. })
        );
        recompute_button(ui, target, computing);
        match status {
            Some(NeuralStatus::Ready { .. }) => {
                ui.weak("100%");
            }
            Some(NeuralStatus::Failed(_)) => {}
            Some(NeuralStatus::Training { percent }) => {
                optimization_percent = Some(percent);
                ui.weak(format!("{percent}%"));
            }
            Some(NeuralStatus::Pending) => {
                optimization_percent = Some(0);
                ui.weak("0%");
            }
            _ => {
                ui.weak("0%");
            }
        }
    }
    if selected_mode.is_depth_accelerator() || matches!(selected_mode,
        crate::model::GroupRenderRepresentation::BoxDepthAtlas | crate::model::GroupRenderRepresentation::SphereDepthAtlas) {
        let status = if selected_mode == saved_mode {
            crate::renderer::depth_accelerator_status(ui.ctx(), &scene, target, capture_revision)
        } else { crate::renderer::DepthAcceleratorStatus::NotComputedYet };
        recompute_button(ui, target, matches!(status, crate::renderer::DepthAcceleratorStatus::Baking(_)));
        ui.weak(status.label());
        if let crate::renderer::DepthAcceleratorStatus::Baking(percent) = status {
            optimization_percent = Some(percent);
        }
    }
    if selected_mode != crate::model::GroupRenderRepresentation::PoissonMesh {
        optimization_remaining(ui, target, optimization_percent.is_some(),
            if selected_mode == crate::model::GroupRenderRepresentation::NeuralSdf { Stage::Training } else { Stage::Capture });
    }
    if selected_mode != scene[index].render_representation
        || saved_splats != scene[index].gaussian_splats
        || saved_settings != scene[index].neural_sdf
        || saved_accelerator != scene[index].sphere_accelerator
        || saved_box_accelerator != scene[index].box_accelerator
    {
        scene[index].render_representation = selected_mode;
        set_objects(tree, scene);
        tree.make_undo_redo_snapshot();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn poisson_mesh_controls(ui: &mut egui::Ui, target: uuid::Uuid) {
    use crate::renderer::poisson_mesh::{PoissonMeshAction, PoissonMeshStatus};
    let mut status = ui.ctx().data(|data| data.get_temp::<std::collections::HashMap<uuid::Uuid, PoissonMeshStatus>>(
        egui::Id::new("poisson-mesh-status")))
        .and_then(|statuses| statuses.get(&target).cloned());
    ui.label(RichText::new("Poisson mesh").strong());
    let busy = matches!(status, Some(PoissonMeshStatus::Sampling { .. } | PoissonMeshStatus::Reconstructing));
    let mut action = None;
    match computation_buttons(ui, busy) {
        Some(ComputationAction::Recompute) => {
            action = Some(PoissonMeshAction::Build(target));
            status = Some(PoissonMeshStatus::Sampling { percent: 0 });
        }
        Some(ComputationAction::Cancel) => {
            action = Some(PoissonMeshAction::Cancel(target));
            status = None;
        }
        None => {}
    }
    let stage = if matches!(status, Some(PoissonMeshStatus::Reconstructing)) {
        Stage::Reconstruction
    } else { Stage::Sampling };
    let active = matches!(status, Some(PoissonMeshStatus::Sampling { .. } | PoissonMeshStatus::Reconstructing));
    match status {
        Some(PoissonMeshStatus::Sampling { percent, .. }) => { ui.weak(format!("Capturing surface samples · {percent}%")); }
        Some(PoissonMeshStatus::Reconstructing) => {
            ui.weak("Reconstructing mesh");
        }
        Some(PoissonMeshStatus::Ready { vertices, triangles, showing, available, .. }) => {
            ui.weak(format!("{vertices} vertices · {triangles} triangles"));
            if available {
                let label = if showing { "Show exact source" } else { "Show mesh" };
                if ui.button(label).clicked() {
                    action = Some(if showing { PoissonMeshAction::Hide(target) } else { PoissonMeshAction::Show(target) });
                }
            } else {
                ui.weak("The mesh is ready, but this renderer cannot currently display it in the viewport.");
            }
        }
        Some(PoissonMeshStatus::Failed(error)) => { ui.colored_label(ui.visuals().error_fg_color, error); }
        None => { ui.weak("Click Recompute to build a mesh from the source surface."); }
    }
    optimization_remaining(ui, target, active, stage);
    if let Some(action) = action {
        ui.ctx().data_mut(|data| {
            let id = egui::Id::new("poisson-mesh-actions");
            let mut actions = data.get_temp::<Vec<PoissonMeshAction>>(id).unwrap_or_default();
            actions.push(action);
            data.insert_temp(id, actions);
        });
        ui.ctx().request_repaint();
    }
}

fn optimization_remaining(ui: &mut egui::Ui, target: uuid::Uuid, active: bool, stage: Stage) {
    if !active { return; }
    let snapshot = ui.ctx().data(|data| data.get_temp::<computation::Statuses>(computation::status_id()))
        .and_then(|statuses| statuses.get(&target).filter(|status| status.stage == stage).cloned());
    ui.weak(snapshot.map_or_else(|| "Estimating time remaining…".into(), |status| status.remaining_label()));
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
}

#[derive(Clone, Copy)]
enum ComputationAction { Recompute, Cancel }

fn computation_buttons(ui: &mut egui::Ui, computing: bool) -> Option<ComputationAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        if ui.add_enabled(!computing, egui::Button::new("Recompute"))
            .on_hover_text("Compute using the current settings. Runs only when clicked.").clicked() {
            action = Some(ComputationAction::Recompute);
        }
        if computing && ui.button("Cancel").on_hover_text("Cancel this computation.").clicked() {
            action = Some(ComputationAction::Cancel);
        }
    });
    action
}

fn recompute_button(ui: &mut egui::Ui, target: uuid::Uuid, computing: bool) {
    if let Some(action) = computation_buttons(ui, computing) {
        ui.ctx().data_mut(|data| {
            let id = egui::Id::new(match action {
                ComputationAction::Recompute => "group-optimization-recompute",
                ComputationAction::Cancel => "group-optimization-cancel",
            });
            let mut requests = data.get_temp::<std::collections::HashSet<uuid::Uuid>>(id).unwrap_or_default();
            requests.insert(target);
            data.insert_temp(id, requests);
        });
        ui.ctx().request_repaint();
    }
}

fn learning_rate_input(ui: &mut egui::Ui, value: &mut f32) -> egui::Response {
    ui.add(
        egui::DragValue::new(value)
            .update_while_editing(false)
            // Match the stored f32 endpoints so roundoff cannot discard the edit buffer.
            .range(0.00001_f32..=0.1_f32)
            .clamp_existing_to_range(false)
            .speed(0.0001)
            .max_decimals(5),
    )
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn splat_panel_uses_the_shared_recompute_button_and_only_requests_on_click() {
        let context = egui::Context::default();
        let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        object.render_representation = crate::model::GroupRenderRepresentation::GaussianSplats;
        let id = object.uuid;
        let mut tree = DataTree::default();
        crate::model::set_objects(&mut tree, vec![object]);
        crate::model::set_selected_exact(&mut tree, vec![id]);
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 1000.0))),
            events, ..Default::default()
        };
        let mut output = context.run_ui(input(vec![]), |ui| group_optimizations_panel(ui, &mut tree));
        output.textures_delta.clear();
        let button = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Recompute" => Some(text.pos + text.galley.size() * 0.5),
            _ => None,
        }).expect("same Recompute button as neural optimization");
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "0%")));
        let request_id = egui::Id::new("group-optimization-recompute");
        assert!(context.data(|data| data.get_temp::<std::collections::HashSet<uuid::Uuid>>(request_id)).is_none());
        for pressed in [true, false] {
            let mut output = context.run_ui(input(vec![egui::Event::PointerMoved(button), egui::Event::PointerButton {
                pos: button, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
            }]), |ui| group_optimizations_panel(ui, &mut tree));
            output.textures_delta.clear();
        }
        assert_eq!(context.data(|data| data.get_temp::<std::collections::HashSet<uuid::Uuid>>(request_id)),
            Some(std::collections::HashSet::from([id])));
    }

    #[test]
    fn learning_rate_keeps_partial_text_until_committed() {
        for initial in [0.00001_f32, 0.001, 0.1] {
            let context = egui::Context::default();
            let mut value = initial;
            let mut widget = egui::Id::NULL;
            let input = |events| egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))),
                events,
                ..Default::default()
            };
            let mut output = context.run_ui(input(vec![]), |ui| {
                widget = learning_rate_input(ui, &mut value).id;
            });
            output.textures_delta.clear();
            context.memory_mut(|memory| memory.request_focus(widget));
            let mut output = context.run_ui(input(vec![]), |ui| { learning_rate_input(ui, &mut value); });
            output.textures_delta.clear();
            let mut output = context.run_ui(input(vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }]), |ui| { learning_rate_input(ui, &mut value); });
            output.textures_delta.clear();
            for text in ["0", ".", "0", "0", "2"] {
                let mut output = context.run_ui(input(vec![egui::Event::Text(text.into())]), |ui| {
                    learning_rate_input(ui, &mut value);
                });
                output.textures_delta.clear();
                assert_eq!(value, initial, "partial text must not change the saved rate");
                if text == "2" {
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Text(text) if text.galley.text() == "0.002")),
                        "typed text was reset for rate {initial}: {:?}", output.shapes.iter().filter_map(|shape| match &shape.shape { egui::Shape::Text(text) => Some(text.galley.text()), _ => None }).collect::<Vec<_>>());
                }
            }
            let mut output = context.run_ui(input(vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]), |ui| { learning_rate_input(ui, &mut value); });
            output.textures_delta.clear();
            assert_eq!(value, 0.002);
        }
    }

    #[test]
    fn percentage_is_shown_below_recompute_without_status_text() {
        let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        object.render_representation = crate::model::GroupRenderRepresentation::NeuralSdf;
        let id = object.uuid;
        let mut tree = DataTree::default();
        crate::model::set_objects(&mut tree, vec![object]);
        crate::model::set_selected_exact(&mut tree, vec![id]);
        let context = egui::Context::default();
        context.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("neural-sdf-status"),
                std::collections::HashMap::from([(
                    id,
                    crate::renderer::NeuralStatus::Training { percent: 30 },
                )]),
            );
        });
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 1000.0),
                )),
                ..Default::default()
            },
            |ui| group_optimizations_panel(ui, &mut tree),
        );
        output.textures_delta.clear();
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((text.galley.text(), text.pos.y)),
                _ => None,
            })
            .collect();
        let button_y = labels
            .iter()
            .find(|(text, _)| *text == "Recompute")
            .unwrap()
            .1;
        let progress_y = labels.iter().find(|(text, _)| *text == "30%").unwrap().1;
        assert!(progress_y > button_y);
        assert!(!labels
            .iter()
            .any(|(text, _)| text.contains("Training neural") || text.contains("pending")));
    }
}

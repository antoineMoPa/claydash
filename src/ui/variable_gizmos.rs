use super::*;
use crate::model::{
    scene_variables, selected_variable, set_selected_variable, variable_viewport_frame,
};

impl UiState {
    /// Point markers select variables; movement uses the regular transform tool.
    pub(super) fn draw_variable_gizmos(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
        blocker_count: usize,
    ) -> bool {
        let variables = scene_variables(tree);
        let scale = ui.ctx().pixels_per_point();
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let navigating = ui.input(|input| {
            input.modifiers.ctrl
                || input.modifiers.command
                || input.pointer.secondary_down()
                || input.pointer.middle_down()
        });
        let blockers = self.regions[..blocker_count.min(self.regions.len())].to_vec();
        let active = selected_variable(tree);
        let modal_grab = active.is_some()
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::EditorState(crate::model::EditorState::Grabbing)
            );
        let over_move = active
            .filter(|id| !crate::model::is_variable_derived(&variables, *id))
            .and_then(|id| variables.vectors.iter().find(|variable| variable.id == id))
            .zip(pointer)
            .is_some_and(|(variable, point)| {
                selection_tools::movement_gizmo_contains(
                    camera,
                    variable_viewport_frame(tree, variable).transform_point3(variable.value),
                    scale,
                    point,
                )
            });
        let mut hit_any = over_move;
        for variable in &variables.vectors {
            let world = variable_viewport_frame(tree, variable).transform_point3(variable.value);
            let Some(center) = camera.project(world, scale) else {
                continue;
            };
            if !ui.clip_rect().contains(center) || blockers.iter().any(|rect| rect.contains(center))
            {
                continue;
            }
            let hit = egui::Rect::from_center_size(center, egui::vec2(20.0, 20.0))
                .intersect(ui.clip_rect());
            let hovered = pointer.is_some_and(|point| hit.contains(point));
            let selected = active == Some(variable.id);
            if !navigating && !modal_grab {
                self.regions.push(hit);
                hit_any |= hovered;
                let response = ui
                    .interact(
                        hit,
                        egui::Id::new(("shared-point", variable.id)),
                        egui::Sense::click(),
                    )
                    .on_hover_text(
                        if crate::model::is_variable_derived(&variables, variable.id) {
                            let driver = variables
                                .four_bar_constraints
                                .iter()
                                .find(|constraint| {
                                    constraint.lower_joint == variable.id
                                        || constraint.upper_joint == variable.id
                                })
                                .and_then(|constraint| {
                                    variables
                                        .vectors
                                        .iter()
                                        .find(|point| point.id == constraint.driver)
                                })
                                .map(|point| point.name.as_str())
                                .unwrap_or("the linkage driver");
                            format!("{} · Driven by linkage; move {}", variable.name, driver)
                        } else {
                            variable.name.clone()
                        },
                    );
                if response.clicked() || response.is_pointer_button_down_on() {
                    set_selected_variable(tree, Some(variable.id));
                }
            }
            ui.painter().circle_filled(
                center,
                if selected { 8.0 } else { 6.0 },
                Color32::from_black_alpha(230),
            );
            ui.painter().circle_filled(
                center,
                if selected { 5.5 } else { 4.0 },
                Color32::from_rgb(255, 190, 90),
            );
            if selected || hovered {
                ui.painter()
                    .circle_stroke(center, 8.0, Stroke::new(1.0, Color32::WHITE));
            }
            if selected {
                ui.painter().text(
                    center + egui::vec2(11.0, -15.0),
                    egui::Align2::LEFT_BOTTOM,
                    &variable.name,
                    egui::FontId::proportional(13.0),
                    Color32::WHITE,
                );
            }
        }
        if !navigating
            && !modal_grab
            && ui.input(|input| input.pointer.primary_pressed())
            && !hit_any
            && !self.selection_tools.active()
            && pointer.is_some_and(|point| {
                ui.clip_rect().contains(point) && !blockers.iter().any(|rect| rect.contains(point))
            })
        {
            set_selected_variable(tree, None);
        }
        modal_grab || (!navigating && (hit_any || self.selection_tools.active()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{set_scene_variables, SceneVariables, VariableSpace, VectorVariable};

    #[test]
    fn modal_grab_keeps_point_visible_and_cannot_select_another_marker() {
        let ctx = egui::Context::default();
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let variable = VectorVariable {
            id: uuid::Uuid::new_v4(),
            name: "Selected point".into(),
            value: Vec3::ZERO,
            space: VariableSpace::World,
        };
        let other = VectorVariable {
            id: uuid::Uuid::new_v4(),
            name: "Other".into(),
            value: Vec3::X,
            space: VariableSpace::World,
        };
        set_scene_variables(
            &mut tree,
            SceneVariables {
                vectors: vec![variable.clone(), other.clone()],
                bindings: vec![],
                ..SceneVariables::default()
            },
        );
        set_selected_variable(&mut tree, Some(variable.id));
        tree.set_transient_path(
            "editor.state",
            ClaydashValue::EditorState(crate::model::EditorState::Grabbing),
        );
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800.0, 600.0);
        let point = camera.project(variable.value, 1.0).unwrap();
        let other_point = camera.project(other.value, 1.0).unwrap();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events: vec![
                    egui::Event::PointerMoved(other_point),
                    egui::Event::PointerButton {
                        pos: other_point,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            },
            |ui| {
                assert!(state.draw_variable_gizmos(ui, &mut tree, &camera, 0));
            },
        );
        output.textures_delta.clear();
        assert!(output.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Circle(circle) if circle.center == point)
        ));
        assert_eq!(selected_variable(&tree), Some(variable.id));
        assert!(
            state.regions.is_empty(),
            "modal markers must not swallow grab commit clicks"
        );
    }
}

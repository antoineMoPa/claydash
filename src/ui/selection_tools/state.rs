use super::*;

impl UiState {
    pub fn selection_gesture_active(&self) -> bool {
        self.selection_tools.active()
    }

    pub fn box_selection_enabled(&self, tree: &DataTree) -> bool {
        self.selection_tools.box_mode()
            && scene_actions::pending_boolean(tree).is_none()
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
            )
    }

    pub fn enter_box_selection_mode(&mut self, tree: &DataTree) -> bool {
        if self.selection_tools.active()
            || scene_actions::pending_boolean(tree).is_some()
            || !matches!(
                tree.get_path("editor.state"),
                ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
            )
        {
            return false;
        }
        self.selection_tools.tool = SelectionTool::Box;
        true
    }

    pub fn begin_box_selection(
        &mut self,
        tree: &DataTree,
        physical_pointer: Vec2,
        pixels_per_point: f32,
        additive: bool,
    ) -> bool {
        if !self.enter_box_selection_mode(tree) {
            return false;
        }
        let point = egui::pos2(
            physical_pointer.x / pixels_per_point,
            physical_pointer.y / pixels_per_point,
        );
        self.selection_tools.gesture = Some(Gesture::Box {
            start: point,
            end: point,
            additive,
        });
        true
    }

    pub(in crate::ui) fn draw_selection_toolbar(
        &mut self,
        ctx: &egui::Context,
        viewport: egui::Rect,
    ) {
        let area = egui::Area::new("selection-tools".into())
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(viewport.right_bottom() - egui::vec2(6.0, 6.0))
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport);
                ui.add_enabled_ui(!self.selection_tools.active(), |ui| {
                    ui.horizontal(|ui| {
                        ui.set_height(26.0);
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (tool, source, tooltip) in [
                            (
                                SelectionTool::Select,
                                egui::include_image!(
                                    "../../../assets/icons/lucide/mouse-pointer-2.svg"
                                ),
                                "Select objects",
                            ),
                            (
                                SelectionTool::Box,
                                egui::include_image!("../../../assets/icons/lucide/scan.svg"),
                                "Box select (B) · drag a rectangle · Shift adds",
                            ),
                        ] {
                            if selectable_view_button(
                                ui,
                                source,
                                tooltip,
                                self.selection_tools.tool == tool,
                            )
                            .clicked()
                            {
                                self.selection_tools.tool = tool;
                            }
                        }
                    });
                });
            });
        self.regions.push(area.response.rect);
    }
}

use super::*;

pub(super) struct WorkspaceView<'a> {
    pub(super) tree: &'a mut DataTree,
    pub(super) animation: &'a mut AnimationRuntime,
}

impl PaneView<EditorPane> for WorkspaceView<'_> {
    fn tab(&mut self, _id: PaneId, pane: &EditorPane) -> Tab {
        Tab::new(pane.title()).closable(*pane != EditorPane::Viewport)
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, id: PaneId, pane: &EditorPane) {
        if *pane == EditorPane::Viewport {
            return;
        }
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, Color32::from_rgb(25, 26, 29));
        if *pane == EditorPane::Animation {
            animation_panel(ui, self.tree, self.animation);
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(("editor-pane-scroll", id))
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.spacing_mut().text_edit_width = ui.available_width();
                        ui.spacing_mut().slider_width = ui
                            .spacing()
                            .slider_width
                            .min((ui.available_width() - 100.0).max(24.0));
                        match pane {
                            EditorPane::Animation => {}
                            EditorPane::Scene => scene_panel(ui, self.tree),
                            EditorPane::Object => object_panel(ui, self.tree, self.animation),
                            EditorPane::Materials => materials_panel(ui, self.tree, self.animation),
                            EditorPane::Repetition => {
                                repetition_panel(ui, self.tree, self.animation)
                            }
                            EditorPane::Operand => operand_panel(ui, self.tree, self.animation),
                            EditorPane::Viewport => {}
                        }
                    });
            });
    }
}

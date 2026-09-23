use super::*;
use crate::model::{ClaydashValue, EditorState};

const BAR_HEIGHT: f32 = 24.0;

impl UiState {
    fn status_line(&self, tree: &DataTree) -> Option<(String, String)> {
        if matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Extruding)
        ) {
            return Some((
                "Extrude face".into(),
                "Move mouse · Alt bypasses guides · click or Enter confirms · Esc cancels".into(),
            ));
        }
        if let Some(pick) = scene_actions::pending_boolean(tree) {
            return Some((
                pick.operation.label().into(),
                "Click another object · Esc cancels".into(),
            ));
        }
        self.face_cut_status(tree)
            .map(|message| ("Face cut".into(), message))
    }

    pub(super) fn draw_status_bar(&self, ui: &mut egui::Ui, tree: &DataTree) {
        let line = self.status_line(tree);
        egui::Panel::bottom("claydash-status-bar")
            .exact_size(BAR_HEIGHT)
            .resizable(false)
            .drag_to_open(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 26, 29))
                    .stroke(Stroke::NONE)
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ui, |ui| {
                if let Some((title, message)) = line {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.label(
                            egui::RichText::new(title)
                                .strong()
                                .size(12.0)
                                .color(Color32::WHITE),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(message)
                                    .size(12.0)
                                    .color(Color32::from_gray(190)),
                            )
                            .truncate(),
                        );
                    });
                }
            });
    }
}

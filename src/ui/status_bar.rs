use super::*;
use crate::model::{ClaydashValue, EditorState};

const BAR_HEIGHT: f32 = 24.0;

#[derive(Clone, Copy)]
pub(crate) enum RenderProgress {
    Image,
    Video {
        completed: u32,
        total: u32,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Encoding {
        completed: u32,
        total: u32,
    },
    Finalizing,
    #[cfg(not(target_arch = "wasm32"))]
    Cancelling,
}

impl UiState {
    fn selection_label(tree: &DataTree) -> Option<String> {
        if crate::model::cloud_edit(tree).is_some() {
            return Some("Point Cloud".into());
        }
        if let Some(face) = crate::model::selected_modeling_face(tree) {
            let scene = crate::model::objects_ref(tree);
            if scene.iter().any(|object| object.uuid == face.object()) {
                return Some("Face".into());
            }
        }
        let selection = crate::model::selected_ref(tree);
        match selection.len() {
            0 => None,
            1 => crate::model::objects_ref(tree)
                .iter()
                .find(|object| object.uuid == selection[0])
                .map(|object| object.name.clone()),
            count => Some(format!("{count} objects")),
        }
    }

    pub(super) fn draw_viewport_selection_label(&self, ui: &egui::Ui, tree: &DataTree) {
        let Some(label) = Self::selection_label(tree) else {
            return;
        };
        let rect = ui.clip_rect();
        let galley = ui.painter().layout_no_wrap(
            label,
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
        let badge = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.bottom() - 20.0),
            galley.size() + egui::vec2(20.0, 10.0),
        );
        ui.painter()
            .rect_filled(badge, 6.0, Color32::from_black_alpha(180));
        ui.painter().galley(
            badge.center() - galley.size() * 0.5,
            galley,
            Color32::WHITE,
        );
    }

    fn status_line(&self, tree: &DataTree) -> Option<(String, String)> {
        let snap_hint = if crate::model::snapping_enabled(tree, false, false) {
            "snapping on · Alt bypasses"
        } else {
            "hold Ctrl to snap"
        };
        if matches!(
            tree.get_path("editor.view_rotation"),
            ClaydashValue::Bool(true)
        ) {
            return Some((
                "Rotate view".into(),
                "Move around the view center to roll · click or Enter confirms · Esc cancels"
                    .into(),
            ));
        }
        if matches!(
            tree.get_path("editor.curve_grab_initial"),
            ClaydashValue::VecSDFObject(_)
        ) {
            return Some((
                "Move curve point".into(),
                "Move mouse · X/Y/Z constrains · click or Enter confirms · Esc cancels".into(),
            ));
        }
        if matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::ExtendingCurve)
        ) {
            return Some((
                "Extend curve".into(),
                "Move mouse to place the new point · click to place · Enter closes the loop · Esc cancels".into(),
            ));
        }
        if matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Extruding)
        ) {
            return Some((
                "Extrude face".into(),
                format!("Move mouse · {snap_hint} · click or Enter confirms · Esc cancels"),
            ));
        }
        if matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::DraggingFace)
        ) {
            return Some((
                "Drag face".into(),
                format!("Move mouse · {snap_hint} · click or Enter confirms · Esc cancels"),
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

    pub(super) fn draw_status_bar(
        &self,
        ui: &mut egui::Ui,
        tree: &DataTree,
        progress: Option<RenderProgress>,
    ) -> bool {
        let line = self.status_line(tree);
        let mut cancel = false;
        let fill = if ui.visuals().dark_mode {
            Color32::from_rgb(25, 26, 29)
        } else {
            ui.visuals().panel_fill
        };
        egui::Panel::bottom("claydash-status-bar")
            .exact_size(if progress.is_some() { 30.0 } else { BAR_HEIGHT })
            .resizable(false)
            .drag_to_open(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(fill)
                    .stroke(Stroke::NONE)
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ui, |ui| {
                if let Some(progress) = progress {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (title, completed, total) = match progress {
                            RenderProgress::Image => ("Rendering image", None, None),
                            RenderProgress::Video { completed, total } => {
                                ("Rendering animation", Some(completed), Some(total))
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            RenderProgress::Encoding { completed, total } => {
                                ("Encoding MP4", Some(completed), Some(total))
                            }
                            RenderProgress::Finalizing => ("Finishing render", None, None),
                            #[cfg(not(target_arch = "wasm32"))]
                            RenderProgress::Cancelling => ("Cancelling render", None, None),
                        };
                        #[cfg(not(target_arch = "wasm32"))]
                        let cancelling = matches!(progress, RenderProgress::Cancelling);
                        #[cfg(target_arch = "wasm32")]
                        let cancelling = false;
                        if !cancelling {
                            cancel = ui.small_button("Cancel").clicked();
                        }
                        if let (Some(completed), Some(total)) = (completed, total) {
                            ui.label(format!("{completed}/{total} frames"));
                            ui.add(
                                egui::ProgressBar::new(completed as f32 / total.max(1) as f32)
                                    .desired_width(150.0)
                                    .show_percentage(),
                            );
                        } else {
                            ui.spinner();
                        }
                        ui.label(egui::RichText::new(title).strong().size(12.0));
                    });
                } else if let Some((title, message)) = line {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.label(
                            egui::RichText::new(title)
                                .strong()
                                .size(12.0)
                                .color(ui.visuals().text_color()),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(message)
                                    .size(12.0)
                                    .color(ui.visuals().weak_text_color()),
                            )
                            .truncate(),
                        );
                    });
                }
            });
        cancel
            || (progress.is_some() && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_selection_prefers_face_and_counts_multiple_objects() {
        let first = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        let second = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        let mut tree = DataTree::default();
        crate::model::set_objects(&mut tree, vec![first.clone(), second.clone()]);
        crate::model::set_selected_exact(&mut tree, vec![first.uuid]);
        assert_eq!(
            UiState::selection_label(&tree).as_deref(),
            Some(first.name.as_str())
        );
        crate::model::set_selected_box_face(
            &mut tree,
            Some(crate::model::BoxFaceSelection {
                object: first.uuid,
                axis: crate::model::VectorAxis::Z,
                positive: true,
            }),
        );
        assert_eq!(UiState::selection_label(&tree).as_deref(), Some("Face"));
        crate::model::set_selected(&mut tree, vec![first.uuid, second.uuid]);
        assert_eq!(
            UiState::selection_label(&tree).as_deref(),
            Some("2 objects")
        );
    }
}

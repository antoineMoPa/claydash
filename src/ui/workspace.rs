use super::*;

pub(super) struct WorkspaceView<'a> {
    pub(super) tree: &'a mut DataTree,
    pub(super) animation: &'a mut AnimationRuntime,
    pub(super) document: &'a mut DocumentState,
    pub(super) viewport_frame: Option<egui_frames::FrameId>,
}

impl PaneView<EditorPane> for WorkspaceView<'_> {
    fn tab(&mut self, _id: PaneId, pane: &EditorPane) -> Tab {
        Tab::new(pane.title()).closable(*pane != EditorPane::Viewport)
    }

    fn tab_strip_end(&mut self, ui: &mut egui::Ui, frame: egui_frames::FrameId, _primary: bool) {
        let header_fill = if ui.visuals().dark_mode {
            Color32::from_rgb(25, 26, 29)
        } else {
            ui.visuals().panel_fill
        };
        let strip = ui.clip_rect();
        let interior = egui::Rect::from_min_max(
            strip.min + egui::vec2(1.0, 1.0),
            strip.max - egui::vec2(1.0, 0.0),
        );
        ui.painter().rect_filled(interior, 0.0, header_fill);
        if self.viewport_frame == Some(frame) {
            let current = match ui.ctx().theme() {
                egui::Theme::Dark => crate::document::ColorTheme::Dark,
                egui::Theme::Light => crate::document::ColorTheme::Light,
            };
            let next = current.toggled();
            let tooltip = match next {
                crate::document::ColorTheme::Light => "Switch to light mode",
                crate::document::ColorTheme::Dark => "Switch to dark mode",
            };
            if theme_header_button(ui, current, header_fill)
                .on_hover_text(tooltip)
                .clicked()
            {
                self.document.set_color_theme(next);
                ui.ctx().set_theme(next.egui_theme());
                ui.ctx().request_repaint();
            }
        }
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, id: PaneId, pane: &EditorPane) {
        if *pane == EditorPane::Viewport {
            return;
        }
        let fill = if ui.visuals().dark_mode {
            Color32::from_rgb(25, 26, 29)
        } else {
            ui.visuals().panel_fill
        };
        ui.painter().rect_filled(ui.max_rect(), 0.0, fill);
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
                            EditorPane::Operand => operand_panel(ui, self.tree, self.animation),
                            EditorPane::Viewport => {}
                        }
                    });
            });
    }
}

fn theme_header_button(
    ui: &mut egui::Ui,
    theme: crate::document::ColorTheme,
    header_fill: Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(17.0, 15.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let ink = if response.hovered() {
        ui.visuals().hyperlink_color
    } else {
        ui.visuals().weak_text_color()
    };
    let center = rect.center();
    match theme {
        crate::document::ColorTheme::Light => {
            ui.painter().circle_filled(center, 5.5, ink);
            ui.painter()
                .circle_filled(center + egui::vec2(3.025, -1.65), 4.675, header_fill);
        }
        crate::document::ColorTheme::Dark => {
            ui.painter().circle_filled(center, 3.5, ink);
            for step in 0..8 {
                let angle = std::f32::consts::TAU * step as f32 / 8.0;
                let direction = egui::vec2(angle.cos(), angle.sin());
                ui.painter().line_segment(
                    [center + direction * 5.0, center + direction * 7.0],
                    Stroke::new(1.0, ink),
                );
            }
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_frame_header_has_its_own_fill() {
        let ctx = egui::Context::default();
        ctx.set_theme(egui::Theme::Light);
        let mut frames =
            Frames::new().with_style(FramesStyle::from_visuals(&egui::Visuals::light()));
        frames.style_mut().background = Color32::TRANSPARENT;
        frames.style_mut().frame_fill = Color32::TRANSPARENT;
        let mut layout = Layout::with_pane(EditorPane::Viewport);
        let mut tree = DataTree::default();
        let mut animation = AnimationRuntime::default();
        let mut document = DocumentState::default();
        let viewport_frame = layout
            .find_pane(|pane| *pane == EditorPane::Viewport)
            .and_then(|(pane, _)| layout.frame_of(pane));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_min_size(egui::vec2(500.0, 300.0));
            let mut view = WorkspaceView {
                tree: &mut tree,
                animation: &mut animation,
                document: &mut document,
                viewport_frame,
            };
            frames.show(ui, &mut layout, &mut view);
        });
        output.textures_delta.clear();
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Rect(rect)
                if rect.fill == egui::Visuals::light().panel_fill
                    && rect.rect.height() < 40.0
                    && rect.rect.width() > 400.0
        )));
    }
}

use super::*;

mod frame;
mod guides;
mod shape;

use frame::{point_on_face, point_world, source_and_frame};
use guides::guided_outline_point;
use shape::{create_face_shape, polygon_is_valid};

const FACE_CUT_OVERLAP: f32 = 0.003;
const FACE_CUT_INITIAL_DEPTH: f32 = 0.04;
const CLOSE_DISTANCE: f32 = 10.0;
const ANGLE_GUIDE_DISTANCE: f32 = 10.0;
const ANCHOR_GUIDE_DISTANCE: f32 = 10.0;
const FRACTION_MIN_SPACING: f32 = 24.0;

impl UiState {
    pub(super) fn cancel_face_cut(&mut self, tree: &mut DataTree) {
        if let Some(FaceCutDraft {
            phase: FaceCutPhase::Depth { object, .. },
            ..
        }) = self.selection_tools.face_cut.take()
        {
            let mut scene = objects(tree);
            scene.retain(|candidate| candidate.uuid != object);
            crate::model::set_objects_transient(tree, scene);
        }
        self.selection_tools.face_cut = None;
    }

    fn finish_face_cut(&mut self, tree: &mut DataTree) {
        let Some(FaceCutDraft {
            phase: FaceCutPhase::Depth { object, .. },
            ..
        }) = self.selection_tools.face_cut.take()
        else {
            return;
        };
        set_objects(tree, objects(tree));
        crate::model::set_selected_exact(tree, vec![object]);
        tree.make_undo_redo_snapshot();
        self.selection_tools.tool = SelectionTool::Select;
    }

    pub(in crate::ui) fn face_cut_status(&self, tree: &DataTree) -> Option<String> {
        if !self.selection_tools.face_cut_mode() {
            return None;
        }
        let draft = self.selection_tools.face_cut.as_ref();
        match draft {
            Some(FaceCutDraft {
                phase: FaceCutPhase::Depth { object, .. },
                ..
            }) => {
                let scene = objects(tree);
                let shape = scene.iter().find(|shape| shape.uuid == *object)?;
                let SdfParams::PolygonPrismParams(params) = &shape.params else {
                    return None;
                };
                let depth = (params.half_depth - FACE_CUT_OVERLAP).max(0.0) * 2.0;
                Some(format!(
                    "{} {:.2} · click or Enter confirms · Esc cancels",
                    if shape.operation == BooleanOperation::Subtract {
                        "Cut depth"
                    } else {
                        "Extrusion"
                    },
                    depth
                ))
            }
            Some(FaceCutDraft { vertices, .. }) if vertices.len() >= 3 && !polygon_is_valid(vertices) => {
                Some("Outline cannot cross itself · Backspace removes a point · Esc cancels".into())
            }
            _ => Some("Points snap to edges, corners, fractions, and earlier points · Alt bypasses guides · Enter closes · Esc cancels".into()),
        }
    }

    pub(super) fn draw_face_cut(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
    ) {
        let scale = ui.ctx().pixels_per_point();
        let (pointer, pressed, enter, backspace, escape, bypass_guides) = ui.input(|input| {
            (
                input.pointer.interact_pos(),
                input.pointer.primary_pressed(),
                input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Backspace),
                input.key_pressed(egui::Key::Escape),
                input.modifiers.alt,
            )
        });
        if escape {
            self.cancel_face_cut(tree);
            self.selection_tools.tool = SelectionTool::Select;
            return;
        }
        let scene = objects(tree);
        let selected_face = crate::model::selected_modeling_face(tree);
        if self.selection_tools.face_cut.is_none() {
            let Some(face) = selected_face else {
                self.selection_tools.tool = SelectionTool::Select;
                return;
            };
            self.selection_tools.face_cut = Some(FaceCutDraft {
                face,
                vertices: Vec::new(),
                phase: FaceCutPhase::Outline,
            });
        }
        let Some(draft) = self.selection_tools.face_cut.as_mut() else {
            return;
        };
        if !scene
            .iter()
            .any(|object| object.uuid == draft.face.object())
        {
            self.cancel_face_cut(tree);
            self.selection_tools.tool = SelectionTool::Select;
            return;
        }

        let mut close_outline = enter && draft.vertices.len() >= 3;
        if backspace && matches!(draft.phase, FaceCutPhase::Outline) {
            draft.vertices.pop();
        }
        let pointer_in_view = pointer.filter(|point| {
            ui.clip_rect().contains(*point)
                && !self.regions.iter().any(|rect| rect.contains(*point))
        });
        let mut guides = Vec::new();
        let mut anchor_guide = None;
        let hovered_local = if matches!(draft.phase, FaceCutPhase::Outline) {
            pointer_in_view
                .and_then(|point| {
                    point_on_face(
                        camera,
                        Vec2::new(point.x, point.y) * scale,
                        &scene,
                        draft.face,
                    )
                })
                .map(|raw| {
                    if !bypass_guides {
                        let (point, active_guides, active_anchor) = guided_outline_point(
                            camera,
                            scale,
                            &scene,
                            draft.face,
                            &draft.vertices,
                            raw,
                        );
                        guides = active_guides;
                        anchor_guide = active_anchor;
                        point
                    } else {
                        raw
                    }
                })
        } else {
            None
        };
        if matches!(draft.phase, FaceCutPhase::Outline) && pressed {
            if let Some(point) = pointer_in_view {
                let first_screen = draft.vertices.first().and_then(|first| {
                    point_world(&scene, draft.face, *first)
                        .and_then(|world| camera.project(world, scale))
                });
                if draft.vertices.len() >= 3
                    && first_screen.is_some_and(|first| first.distance(point) <= CLOSE_DISTANCE)
                {
                    close_outline = true;
                } else if draft.vertices.len() < crate::model::MAX_POLYGON_PRISM_VERTICES {
                    if let Some(local) = hovered_local {
                        draft.vertices.push(local);
                    }
                }
            }
        }

        let mut just_closed = false;
        if close_outline && matches!(draft.phase, FaceCutPhase::Outline) {
            let start_pointer = pointer.unwrap_or(ui.clip_rect().center());
            if let Some((_, frame)) = source_and_frame(&scene, draft.face) {
                let matrix = crate::model::object_world_matrix(&scene, draft.face.object());
                let center = matrix.transform_point3(frame.origin);
                let direction = matrix.transform_vector3(frame.normal);
                if let (Some(at), Some(ahead)) = (
                    camera.project(center, scale),
                    camera.project(center + direction * 0.1, scale),
                ) {
                    let mut projected_axis = (ahead - at) * 10.0;
                    if projected_axis.length_sq() < 4.0 {
                        let screen_up = camera.view().inverse().y_axis.truncate();
                        if let Some(fallback) = camera.project(center + screen_up * 0.1, scale) {
                            projected_axis = (fallback - at) * 10.0;
                        }
                    }
                    if projected_axis.length_sq() > 0.0001 {
                        let id = uuid::Uuid::new_v4();
                        if let Some(shape) = create_face_shape(
                            &scene,
                            draft.face,
                            &draft.vertices,
                            FACE_CUT_INITIAL_DEPTH,
                            id,
                        ) {
                            let mut preview = scene.clone();
                            preview.push(shape);
                            crate::model::set_objects_transient(tree, preview);
                            draft.phase = FaceCutPhase::Depth {
                                object: id,
                                start_pointer,
                                projected_axis,
                            };
                            just_closed = true;
                        }
                    }
                }
            }
        }

        if let FaceCutPhase::Depth {
            object,
            start_pointer,
            projected_axis,
        } = draft.phase
        {
            if let Some(point) = pointer {
                let depth = FACE_CUT_INITIAL_DEPTH
                    + (point - start_pointer).dot(projected_axis) / projected_axis.length_sq();
                let mut preview = objects(tree);
                preview.retain(|candidate| candidate.uuid != object);
                if let Some(shape) =
                    create_face_shape(&preview, draft.face, &draft.vertices, depth, object)
                {
                    preview.push(shape);
                    crate::model::set_objects_transient(tree, preview);
                }
            }
            if !just_closed && (enter || (pressed && pointer_in_view.is_some())) {
                self.finish_face_cut(tree);
                return;
            }
        }

        let scene = objects(tree);
        let points: Vec<_> = draft
            .vertices
            .iter()
            .filter_map(|point| point_world(&scene, draft.face, *point))
            .filter_map(|world| camera.project(world, scale))
            .collect();
        let stroke = Stroke::new(2.0, Color32::from_rgb(255, 190, 72));
        for segment in points.windows(2) {
            ui.painter().line_segment([segment[0], segment[1]], stroke);
        }
        if matches!(draft.phase, FaceCutPhase::Outline) {
            if let (Some(last), Some(local)) = (points.last(), hovered_local) {
                if let Some(point) = point_world(&scene, draft.face, local)
                    .and_then(|world| camera.project(world, scale))
                {
                    ui.painter().line_segment([*last, point], stroke);
                }
            }
            let guide_color = Color32::from_rgb(94, 203, 255);
            for guide in &guides {
                ui.painter()
                    .line_segment([guide.start, guide.end], Stroke::new(1.5, guide_color));
            }
            if let Some(guide) = &anchor_guide {
                ui.painter().line_segment(
                    [
                        guide.screen - egui::vec2(6.0, 0.0),
                        guide.screen + egui::vec2(6.0, 0.0),
                    ],
                    Stroke::new(1.5, guide_color),
                );
                ui.painter().line_segment(
                    [
                        guide.screen - egui::vec2(0.0, 6.0),
                        guide.screen + egui::vec2(0.0, 6.0),
                    ],
                    Stroke::new(1.5, guide_color),
                );
            }
            let mut labels: Vec<&str> = Vec::new();
            for guide in &guides {
                if !labels.contains(&guide.label) {
                    labels.push(guide.label);
                }
            }
            if let Some(guide) = &anchor_guide {
                if !labels.contains(&guide.label.as_str()) {
                    labels.push(&guide.label);
                }
            }
            if let Some(at) = anchor_guide
                .as_ref()
                .map(|guide| guide.screen)
                .or_else(|| guides.first().map(|guide| guide.end))
            {
                let font = egui::FontId::proportional(12.0);
                let width = labels
                    .iter()
                    .map(|label| {
                        ui.painter()
                            .layout_no_wrap((*label).into(), font.clone(), guide_color)
                            .size()
                            .x
                    })
                    .fold(0.0_f32, f32::max);
                let line_height = 14.0;
                let stack_height = line_height * labels.len() as f32;
                let x = if at.x + 8.0 + width <= ui.clip_rect().right() {
                    at.x + 8.0
                } else {
                    at.x - 8.0 - width
                };
                let y = if at.y - 8.0 - stack_height >= ui.clip_rect().top() {
                    at.y - 8.0 - stack_height
                } else {
                    at.y + 8.0
                };
                for (index, label) in labels.iter().enumerate() {
                    ui.painter().text(
                        egui::pos2(x, y + index as f32 * line_height),
                        egui::Align2::LEFT_TOP,
                        *label,
                        font.clone(),
                        guide_color,
                    );
                }
            }
            for (index, point) in points.iter().enumerate() {
                ui.painter().circle_filled(
                    *point,
                    if index == 0 { 5.0 } else { 3.5 },
                    stroke.color,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;

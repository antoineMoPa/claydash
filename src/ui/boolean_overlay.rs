#[cfg(test)]
use crate::model::{BooleanOperation, SdfObject, SdfParams};
use crate::{
    camera::Camera,
    model::{objects_ref, selected_ref, selection_scope, DataTree, SelectionScope},
};
use claydash_engine::wireframe::draw_guides;
pub(super) use claydash_engine::wireframe::Ghosts;
#[cfg(test)]
use glam::Vec2;
pub(super) fn draw(ui: &egui::Ui, tree: &DataTree, camera: &Camera) -> Ghosts {
    let selected = selected_ref(tree);
    let outline_mode = crate::commands::outline_mode(tree);
    draw_guides(
        ui,
        objects_ref(tree),
        selected,
        camera,
        outline_mode,
        selection_scope(tree) == SelectionScope::Group,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PrimitiveKind;

    #[test]
    fn a_fully_hidden_cutter_still_paints_translucent_viewport_guides() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let target = SdfObject::create_kind(PrimitiveKind::Box);
        let mut cutter = SdfObject::create_kind(PrimitiveKind::Sphere);
        cutter.params = SdfParams::SphereParams(crate::model::SphereParams { radius: 0.1 });
        cutter.boolean_parent = Some(target.uuid);
        cutter.operation = BooleanOperation::Subtract;
        let cutter_id = cutter.uuid;
        crate::model::set_selected(&mut tree, vec![target.uuid]);
        crate::model::set_objects(&mut tree, vec![target, cutter]);
        let mut camera = Camera::new();
        camera.viewport = glam::Vec2::new(400.0, 300.0);
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let mut ghosts = Ghosts::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_clip_rect(viewport);
            ghosts = draw(ui, &tree, &camera);
        });
        output.textures_delta.clear(); // Headless test has no GPU texture consumer.
        let mut lines = 0;
        for shape in output.shapes {
            if let egui::Shape::LineSegment { points, stroke } = shape.shape {
                lines += 1;
                assert!(points.iter().all(|point| viewport.contains(*point)));
                assert!(stroke.color.a() > 0 && stroke.color.a() < 255);
                assert_eq!(shape.clip_rect, viewport);
                assert_eq!(ghosts.pick(points[0].lerp(points[1], 0.5)), Some(cutter_id));
            }
        }
        assert_eq!(lines, 3 * 32);
        assert_eq!(ghosts.pick(viewport.left_top()), None);
    }

    #[test]
    fn outline_mode_draws_and_picks_unselected_roots_and_hidden_operands() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let target = SdfObject::create_kind(PrimitiveKind::Box);
        let mut cutter = SdfObject::create_kind(PrimitiveKind::Sphere);
        cutter.params = SdfParams::SphereParams(crate::model::SphereParams { radius: 0.1 });
        cutter.boolean_parent = Some(target.uuid);
        cutter.operation = BooleanOperation::Subtract;
        let target_id = target.uuid;
        let cutter_id = cutter.uuid;
        crate::model::set_objects(&mut tree, vec![target, cutter]);
        crate::commands::toggle_outline(&mut tree);
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(400.0, 300.0);
        let mut ghosts = Ghosts::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ghosts = draw(ui, &tree, &camera);
        });
        output.textures_delta.clear();
        assert!(ghosts.0.iter().any(|(id, _)| *id == target_id));
        let points = ghosts.0.iter().find(|(id, _)| *id == cutter_id).unwrap().1;
        assert_eq!(ghosts.pick(points[0].lerp(points[1], 0.5)), Some(cutter_id));
        assert_eq!(ghosts.0.len(), 12 + 3 * 32);

        crate::commands::toggle_outline(&mut tree);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ghosts = draw(ui, &tree, &camera);
        });
        output.textures_delta.clear();
        assert!(ghosts.0.is_empty());
    }
}

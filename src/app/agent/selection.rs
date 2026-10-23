use super::*;
use crate::cloud_edit::SelectionMode;

#[derive(Deserialize)]
pub struct SelectBoxArgs {
    min: [f32; 2],
    max: [f32; 2],
    #[serde(default)]
    mode: SelectionMode,
    cloud_object_id: Option<uuid::Uuid>,
}

#[derive(Deserialize)]
pub struct SelectCircleArgs {
    start: [f32; 2],
    end: Option<[f32; 2]>,
    radius: f32,
    #[serde(default)]
    mode: SelectionMode,
    cloud_object_id: Option<uuid::Uuid>,
}

fn screen_point(point: [f32; 2]) -> Result<egui::Pos2, String> {
    if !point.iter().all(|value| value.is_finite()) {
        return Err("selection coordinates must be finite viewport pixels".into());
    }
    Ok(egui::pos2(point[0], point[1]))
}

impl App {
    fn agent_prepare_cloud_selection(&mut self, id: Option<uuid::Uuid>) -> Result<bool, String> {
        let Some(id) = id else { return Ok(model::cloud_edit(&self.tree).is_some()); };
        if !model::objects_ref(&self.tree).iter().any(|object|
            object.uuid == id && object.imported_splats.is_some()) {
            return Err(format!("object {id} is not an imported Gaussian cloud"));
        }
        let indices = model::cloud_edit(&self.tree).filter(|edit| edit.object == id)
            .map_or_else(Vec::new, |edit| edit.indices);
        model::set_selected_exact(&mut self.tree, vec![id]);
        model::set_cloud_edit(&mut self.tree, Some(model::CloudEdit { object: id, indices }));
        Ok(true)
    }

    fn agent_selection_result(&self) -> Value {
        let cloud = model::cloud_edit(&self.tree);
        json!({
            "selected_ids": model::selected(&self.tree),
            "selected_variable_id": model::selected_variable(&self.tree),
            "cloud_object_id": cloud.as_ref().map(|edit| edit.object),
            "selected_splat_indices": cloud.as_ref().map(|edit| &edit.indices),
            "revision": self.scene_revision(),
        })
    }

    pub(super) fn agent_select_box(&mut self, args: SelectBoxArgs) -> AgentResult {
        let min = screen_point(args.min)?;
        let max = screen_point(args.max)?;
        if min.x > max.x || min.y > max.y {
            return Err("box min must be above and left of max".into());
        }
        let rect = egui::Rect::from_min_max(min, max);
        let camera = self.agent_view_camera();
        if self.agent_prepare_cloud_selection(args.cloud_object_id)? {
            crate::cloud_edit::select_rect(&mut self.tree, &camera, rect, 1.0, args.mode);
        } else {
            let scene = model::objects_ref(&self.tree);
            let hits = crate::ui::box_selection(scene, &camera, rect, 1.0, vec![]);
            let previous = model::selected(&self.tree);
            let next = crate::ui::region_selection_ids(scene, args.mode, previous.clone(), hits);
            if next != previous { model::set_selected(&mut self.tree, next); }
        }
        Ok(self.agent_selection_result())
    }

    pub(super) fn agent_select_circle(&mut self, args: SelectCircleArgs) -> AgentResult {
        let start = screen_point(args.start)?;
        let end = screen_point(args.end.unwrap_or(args.start))?;
        if !args.radius.is_finite() || !(4.0..=512.0).contains(&args.radius) {
            return Err("circle radius must be finite and between 4 and 512 viewport pixels".into());
        }
        let camera = self.agent_view_camera();
        if self.agent_prepare_cloud_selection(args.cloud_object_id)? {
            if args.mode == SelectionMode::Replace {
                if let Some(mut edit) = model::cloud_edit(&self.tree) {
                    edit.indices.clear();
                    model::set_cloud_edit(&mut self.tree, Some(edit));
                }
            }
            crate::cloud_edit::select_circle_stroke(&mut self.tree, &camera,
                start, end, args.radius, 1.0, args.mode);
        } else {
            let previous = model::selected(&self.tree);
            let previous_variable = model::selected_variable(&self.tree);
            let hits = crate::ui::circle_selection(model::objects_ref(&self.tree),
                &model::scene_cameras(&self.tree), &camera,
                start, end, args.radius, 1.0, vec![]);
            let next = crate::ui::region_selection_ids(model::objects_ref(&self.tree),
                args.mode, previous.clone(), hits);
            let variable = crate::ui::circle_variable_selection(&self.tree, &camera,
                start, end, args.radius, 1.0);
            if next != previous { model::set_selected(&mut self.tree, next); }
            match args.mode {
                SelectionMode::Subtract => {
                    if let Some(id) = previous_variable {
                        model::set_selected_variable(&mut self.tree, (variable != Some(id)).then_some(id));
                    }
                }
                SelectionMode::Replace | SelectionMode::Add => {
                    if let Some(id) = variable.or(previous_variable) {
                        model::set_selected_variable(&mut self.tree, Some(id));
                    }
                }
            }
        }
        Ok(self.agent_selection_result())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_region_tools_replace_add_and_subtract_object_selection() {
        let mut app = App::new();
        app.new_document();
        app.agent_camera.position = Vec3::new(0.0, 0.0, 8.0);
        app.agent_camera.target = Vec3::ZERO;
        app.agent_camera.viewport = Vec2::new(800.0, 600.0);
        let mut left = SdfObject::create_kind(PrimitiveKind::Box);
        left.transform.translation.x = -1.0;
        let mut right = SdfObject::create_kind(PrimitiveKind::Box);
        right.transform.translation.x = 1.0;
        model::set_objects(&mut app.tree, vec![left.clone(), right.clone()]);
        let rect = |center: egui::Pos2, mode| SelectBoxArgs {
            min: [center.x - 12.0, center.y - 12.0],
            max: [center.x + 12.0, center.y + 12.0], mode, cloud_object_id: None,
        };
        let left_point = app.agent_camera.project(left.transform.translation, 1.0).unwrap();
        let right_point = app.agent_camera.project(right.transform.translation, 1.0).unwrap();
        app.agent_select_box(rect(left_point, SelectionMode::Replace)).unwrap();
        assert_eq!(model::selected(&app.tree), vec![left.uuid]);
        app.agent_select_box(rect(right_point, SelectionMode::Add)).unwrap();
        assert_eq!(model::selected(&app.tree), vec![left.uuid, right.uuid]);
        app.agent_select_circle(SelectCircleArgs {
            start: [left_point.x, left_point.y], end: None, radius: 12.0,
            mode: SelectionMode::Subtract, cloud_object_id: None,
        }).unwrap();
        assert_eq!(model::selected(&app.tree), vec![right.uuid]);
        assert!(mcp::mcp_tools().iter().any(|tool| tool["name"] == "select_box"));
        assert!(mcp::mcp_tools().iter().any(|tool| tool["name"] == "select_circle"));
    }

    #[test]
    fn mcp_region_tools_select_imported_gaussian_points() {
        use claydash_engine::gaussian_ply::{Gaussian, GaussianCloud};
        let mut app = App::new();
        app.new_document();
        app.agent_camera.position = Vec3::new(0.0, 0.0, 8.0);
        app.agent_camera.target = Vec3::ZERO;
        app.agent_camera.viewport = Vec2::new(800.0, 600.0);
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.imported_splats = Some(std::sync::Arc::new(GaussianCloud {
            splats: [0.0, 2.0].map(|x| Gaussian {
                position: Vec3::new(x, 0.0, 0.0), scale: Vec3::splat(0.05),
                rotation: glam::Quat::IDENTITY, color: [1.0; 3], opacity: 1.0,
                sh_rest: Vec::new(),
            }).to_vec(),
        }));
        let id = object.uuid;
        model::set_objects(&mut app.tree, vec![object]);
        let point = app.agent_camera.project(Vec3::ZERO, 1.0).unwrap();
        app.agent_select_circle(SelectCircleArgs {
            start: [point.x, point.y], end: None, radius: 10.0,
            mode: SelectionMode::Replace, cloud_object_id: Some(id),
        }).unwrap();
        assert_eq!(model::cloud_edit(&app.tree).unwrap().indices, vec![0]);
        app.agent_select_circle(SelectCircleArgs {
            start: [point.x, point.y], end: None, radius: 10.0,
            mode: SelectionMode::Subtract, cloud_object_id: None,
        }).unwrap();
        assert!(model::cloud_edit(&app.tree).unwrap().indices.is_empty());
    }
}

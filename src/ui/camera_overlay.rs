use super::*;

pub(super) fn draw(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    viewport_camera: &Camera,
) -> Vec<egui::Rect> {
    let active = crate::model::active_camera_id(tree);
    let camera_view = matches!(
        tree.get_path("editor.camera_view"),
        crate::model::ClaydashValue::Bool(true)
    );
    let pixels_per_point = ui.ctx().pixels_per_point();
    let mut hit_regions = Vec::new();
    for camera in crate::model::scene_cameras(tree) {
        if camera_view && active == Some(camera.uuid) {
            continue;
        }
        let lines = camera_wireframe(&camera, viewport_camera.viewport);
        let projected: Vec<_> = lines
            .iter()
            .filter_map(|(start, end)| {
                Some((
                    viewport_camera.project(*start, pixels_per_point)?,
                    viewport_camera.project(*end, pixels_per_point)?,
                ))
            })
            .collect();
        if projected.is_empty() {
            continue;
        }
        let bounds = projected.iter().fold(egui::Rect::NOTHING, |bounds, line| {
            bounds.union(egui::Rect::from_two_pos(line.0, line.1))
        });
        let hit_region = bounds.expand(6.0);
        hit_regions.push(hit_region);
        let response = ui
            .interact(
                hit_region,
                egui::Id::new(("scene-camera", camera.uuid)),
                egui::Sense::click(),
            )
            .on_hover_text(format!("{} · click to select and make active", camera.name));
        if response.clicked() {
            tree.set_path(
                "scene.active_camera",
                crate::model::ClaydashValue::Uuid(camera.uuid),
            );
            crate::model::set_selected(tree, vec![camera.uuid]);
        }
        let color = if response.hovered() {
            Color32::WHITE
        } else if active == Some(camera.uuid) {
            Color32::from_rgb(255, 174, 57)
        } else {
            Color32::from_gray(180)
        };
        let stroke = Stroke::new(
            if active == Some(camera.uuid) {
                2.0
            } else {
                1.35
            },
            color,
        );
        for (start, end) in projected {
            ui.painter().line_segment([start, end], stroke);
        }
        ui.painter().text(
            bounds.center_bottom() + egui::vec2(0.0, 7.0),
            egui::Align2::CENTER_TOP,
            &camera.name,
            egui::FontId::proportional(11.0),
            color,
        );
    }
    hit_regions
}

pub(super) fn camera_wireframe(
    camera: &crate::camera::SceneCamera,
    viewport: Vec2,
) -> Vec<(Vec3, Vec3)> {
    let position = camera.transform.translation;
    let forward = camera.transform.rotation * Vec3::NEG_Z;
    let right = camera.transform.rotation * Vec3::X;
    let up = camera.transform.rotation * Vec3::Y;
    let scale = camera.transform.scale.abs();
    let size = (camera.focal_distance * 0.16).clamp(0.18, 0.8);
    let aspect = (viewport.x / viewport.y.max(1.0)).clamp(0.5, 2.0);
    let center = position + forward * size * 1.8 * scale.z;
    let half_height = size * 0.62 * scale.y;
    let half_width = size * 0.62 * aspect * scale.x;
    let corners = [
        center - right * half_width - up * half_height,
        center + right * half_width - up * half_height,
        center + right * half_width + up * half_height,
        center - right * half_width + up * half_height,
    ];
    let mut lines = Vec::with_capacity(11);
    for corner in corners {
        lines.push((position, corner));
    }
    for index in 0..4 {
        lines.push((corners[index], corners[(index + 1) % 4]));
    }
    // The roof triangle makes camera roll and the upward direction legible.
    let roof_left = center - right * half_width * 0.45 + up * half_height;
    let roof_right = center + right * half_width * 0.45 + up * half_height;
    let roof_peak = center + up * half_height * 1.55;
    lines.extend([
        (roof_left, roof_peak),
        (roof_peak, roof_right),
        (roof_right, roof_left),
    ]);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_object_wireframe_has_a_frustum_and_orientation_marker() {
        let view = Camera::new();
        let camera = crate::camera::SceneCamera::from_view("Camera", &view);
        let lines = camera_wireframe(&camera, Vec2::new(800.0, 600.0));
        assert_eq!(lines.len(), 11);
        assert!(lines
            .iter()
            .flat_map(|(start, end)| [start, end])
            .all(|point| point.is_finite()));
    }

    #[test]
    fn scaled_camera_still_produces_finite_geometry() {
        let view = Camera::new();
        let mut camera = crate::camera::SceneCamera::from_view("Camera", &view);
        camera.transform.scale = Vec3::new(0.5, 2.0, 0.01);
        assert!(camera_wireframe(&camera, Vec2::ONE)
            .iter()
            .flat_map(|(start, end)| [start, end])
            .all(|point| point.is_finite()));
    }

    #[test]
    fn camera_hit_region_blocks_the_scene_picker_on_the_first_click() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let mut viewport_camera = Camera::new();
        viewport_camera.viewport = Vec2::new(800.0, 600.0);
        let camera = crate::camera::SceneCamera::from_view("Camera", &viewport_camera);
        crate::model::set_scene_cameras(&mut tree, vec![camera]);
        let mut hit_regions = Vec::new();
        let mut viewport_rect = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_min_size(egui::vec2(800.0, 600.0));
            viewport_rect = ui.max_rect();
            hit_regions = draw(ui, &mut tree, &viewport_camera);
        });
        output.textures_delta.clear();

        assert_eq!(hit_regions.len(), 1);
        let mut state = UiState::default();
        state.viewport_rect = Some(viewport_rect);
        state.regions = hit_regions.clone();
        let center = hit_regions[0].center();
        assert!(state.contains_pointer(Vec2::new(center.x, center.y), 1.0));
    }
}

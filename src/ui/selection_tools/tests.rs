use super::*;
use crate::camera::ProjectionMode;

fn camera() -> Camera {
    let mut camera = Camera::new();
    camera.position = Vec3::new(0.0, 0.0, 8.0);
    camera.viewport = Vec2::new(800.0, 600.0);
    camera
}

fn draw(
    state: &mut UiState,
    ctx: &egui::Context,
    tree: &mut DataTree,
    camera: &Camera,
    events: Vec<egui::Event>,
) {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            focused: true,
            ..Default::default()
        },
        |ui| {
            state.regions.clear();
            state.draw_selection_tools(ui, tree, camera);
        },
    );
    output.textures_delta.clear();
}

mod selection;

mod transforms;

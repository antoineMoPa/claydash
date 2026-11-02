//! Construction-line presentation, deliberately separate from scene geometry.
use crate::{
    model::{SceneSeams, SdfObject},
    renderer::CapturedFrame,
    Camera,
};

pub fn projected_segments(
    seams: &SceneSeams,
    objects: &[SdfObject],
    camera: &Camera,
    scale: f32,
) -> Vec<[egui::Pos2; 2]> {
    let matrix = camera.projection() * camera.view();
    let mut lines = Vec::new();
    for seam in &seams.seams {
        if let Ok(points) = seam.positions(objects) {
            let count = points.len().saturating_sub(1) + usize::from(seam.closed);
            for i in 0..count {
                if let Some(line) = crate::wireframe::project_segment(
                    matrix,
                    points[i],
                    points[(i + 1) % points.len()],
                    camera,
                    scale,
                ) {
                    lines.push(line);
                }
            }
        }
    }
    lines
}
pub fn draw(ui: &egui::Ui, seams: &SceneSeams, objects: &[SdfObject], camera: &Camera) {
    for line in projected_segments(seams, objects, camera, ui.ctx().pixels_per_point()) {
        ui.painter()
            .line_segment(line, egui::Stroke::new(2.0, egui::Color32::RED));
    }
}
/// Explicit capture overlay. Ordinary renders never call this function.
pub fn annotate(
    frame: &mut CapturedFrame,
    seams: &SceneSeams,
    objects: &[SdfObject],
    camera: &Camera,
) {
    let mut camera = camera.clone();
    camera.viewport = glam::Vec2::new(frame.width as f32, frame.height as f32);
    camera.viewport_origin = glam::Vec2::ZERO;
    for [a, b] in projected_segments(seams, objects, &camera, 1.0) {
        let delta = b - a;
        let steps = delta.x.abs().max(delta.y.abs()).ceil().max(1.0) as usize;
        for i in 0..=steps {
            let p = a + delta * (i as f32 / steps as f32);
            for dy in 0..2 {
                for dx in 0..2 {
                    let x = p.x.floor() as i32 + dx;
                    let y = p.y.floor() as i32 + dy;
                    if x >= 0 && y >= 0 && x < frame.width as i32 && y < frame.height as i32 {
                        let at = (y as usize * frame.width as usize + x as usize) * 4;
                        frame.rgba[at..at + 4].copy_from_slice(&[255, 0, 0, 255]);
                    }
                }
            }
        }
    }
}

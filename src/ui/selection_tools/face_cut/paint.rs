use super::split::SplitRegions;
use super::*;

pub(super) fn paint_split_region(
    ui: &egui::Ui,
    camera: &Camera,
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    regions: &SplitRegions,
    selected: usize,
    scale: f32,
) {
    let color = Color32::from_rgb(94, 203, 255);
    let polygon = regions.polygon(selected);
    let screen: Vec<_> = polygon
        .iter()
        .filter_map(|point| {
            point_world(scene, face, *point).and_then(|world| camera.project(world, scale))
        })
        .collect();
    if screen.len() != polygon.len() {
        return;
    }
    if regions.hole(selected).is_none() {
        for triangle in crate::ui::object_gizmos::polygon_overlay_triangles(polygon) {
            ui.painter().add(egui::Shape::convex_polygon(
                triangle.map(|index| screen[index]).to_vec(),
                color.gamma_multiply(0.18),
                Stroke::NONE,
            ));
        }
    }
    ui.painter()
        .add(egui::Shape::closed_line(screen, Stroke::new(2.5, color)));
    if let Some(hole) = regions.hole(selected) {
        let screen: Vec<_> = hole
            .iter()
            .filter_map(|point| {
                point_world(scene, face, *point).and_then(|world| camera.project(world, scale))
            })
            .collect();
        if screen.len() == hole.len() {
            ui.painter()
                .add(egui::Shape::closed_line(screen, Stroke::new(2.5, color)));
        }
    }
}

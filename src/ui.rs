use egui::Color32;
use glam::{Vec2, Vec4};

use crate::{
    commands::{self, Commands},
    model::{objects, selected, set_objects, ClaydashValue, DataTree},
    undo_redo,
};

pub struct UiState {
    search: String,
    color_marker: Vec2,
    regions: Vec<egui::Rect>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            search: String::new(),
            color_marker: Vec2::from_angle((0.94 - 0.75) * std::f32::consts::TAU) * 0.85,
            regions: Vec::new(),
        }
    }
}

impl UiState {
    pub fn draw(
        &mut self,
        viewport_ui: &mut egui::Ui,
        tree: &mut DataTree,
        command_map: &mut Commands,
    ) {
        self.regions.clear();
        let top_panel = egui::Panel::top("menu").show(viewport_ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo").clicked() {
                        undo_redo::undo(tree);
                    }
                    if ui.button("Redo").clicked() {
                        undo_redo::redo(tree);
                    }
                });
            });
        });
        self.regions.push(top_panel.response.rect);

        let command_panel =
            egui::Panel::right("commands")
                .default_size(320.0)
                .show(viewport_ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search Commands..."),
                    );
                    if self.search.is_empty() {
                        return;
                    }
                    let results = command_map.search(&self.search, 5);
                    for (name, command) in results {
                        ui.separator();
                        ui.strong(command.title);
                        ui.label(command.docs);
                        if !command.shortcut.is_empty() {
                            ui.label(command.shortcut);
                        }
                        if ui.button("Run").clicked() {
                            commands::execute(command_map, &name, tree);
                            self.search.clear();
                        }
                    }
                });
        self.regions.push(command_panel.response.rect);

        let wheel_rect = egui::Rect::from_min_size(egui::pos2(10.0, 35.0), egui::vec2(66.0, 66.0));
        let wheel = viewport_ui.interact(
            wheel_rect,
            egui::Id::new("color_picker"),
            egui::Sense::click_and_drag(),
        );
        paint_color_wheel(viewport_ui.painter(), wheel_rect);
        if let Some(position) = wheel.interact_pointer_pos() {
            let delta = position - wheel_rect.center();
            let normalized = Vec2::new(delta.x, delta.y) / 29.0;
            if normalized.length() <= 1.0 {
                self.color_marker = normalized;
                self.set_color(tree, normalized);
            }
        }
        let marker = wheel_rect.center()
            + egui::vec2(self.color_marker.x * 29.0, self.color_marker.y * 29.0);
        viewport_ui
            .painter()
            .circle_stroke(marker, 5.0, egui::Stroke::new(2.0, Color32::BLACK));
        self.regions.push(wheel_rect);
    }

    pub fn contains_pointer(&self, physical_position: Vec2, pixels_per_point: f32) -> bool {
        let position = egui::pos2(
            physical_position.x / pixels_per_point,
            physical_position.y / pixels_per_point,
        );
        self.regions.iter().any(|region| region.contains(position))
    }

    fn set_color(&self, tree: &mut DataTree, normalized: Vec2) {
        let hue = (normalized.y.atan2(normalized.x) / std::f32::consts::TAU + 0.75).rem_euclid(1.0);
        let color32: Color32 = egui::ecolor::Hsva::new(hue, normalized.length(), 1.0, 1.0).into();
        let color = Vec4::new(
            color32.r() as f32 / 255.0,
            color32.g() as f32 / 255.0,
            color32.b() as f32 / 255.0,
            1.0,
        );
        tree.set_path("editor.color", ClaydashValue::Vec4(color));
        let selection = selected(tree);
        let mut scene = objects(tree);
        for object in &mut scene {
            if selection.contains(&object.uuid) {
                object.color = color;
            }
        }
        set_objects(tree, scene);
    }
}

fn paint_color_wheel(painter: &egui::Painter, rect: egui::Rect) {
    const SEGMENTS: u32 = 96;
    let center = rect.center();
    let radius = 29.0;
    let mut mesh = egui::Mesh::default();

    for index in 0..SEGMENTS {
        let angle_a = index as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let angle_b = (index + 1) as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let hue_a = (angle_a / std::f32::consts::TAU + 0.75).rem_euclid(1.0);
        let hue_b = (angle_b / std::f32::consts::TAU + 0.75).rem_euclid(1.0);
        let vertex = mesh.vertices.len() as u32;

        mesh.colored_vertex(center, Color32::WHITE);
        mesh.colored_vertex(
            center + egui::vec2(angle_a.cos(), angle_a.sin()) * radius,
            egui::ecolor::Hsva::new(hue_a, 1.0, 1.0, 1.0).into(),
        );
        mesh.colored_vertex(
            center + egui::vec2(angle_b.cos(), angle_b.sin()) * radius,
            egui::ecolor::Hsva::new(hue_b, 1.0, 1.0, 1.0).into(),
        );
        mesh.add_triangle(vertex, vertex + 1, vertex + 2);
    }

    painter.add(egui::Shape::mesh(mesh));
    painter.circle_stroke(center, radius + 1.5, egui::Stroke::new(3.0, Color32::WHITE));
}

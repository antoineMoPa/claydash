use super::*;

pub(in crate::ui) struct LatticeDrag {
    object: uuid::Uuid,
    index: usize,
    start_world: Vec3,
    start_offset: Vec3,
    start_pointer: egui::Pos2,
}

fn pointer_on_lattice_plane(
    camera: &Camera,
    pointer: egui::Pos2,
    pixels_per_point: f32,
    center: Vec3,
) -> Option<Vec3> {
    let (origin, direction) = camera.ray(Vec2::new(pointer.x, pointer.y) * pixels_per_point);
    let normal = (camera.target - camera.position).normalize_or_zero();
    let divisor = direction.dot(normal);
    (divisor.abs() > 0.0001).then(|| origin + direction * (center - origin).dot(normal) / divisor)
}

impl UiState {
    pub(in crate::ui) fn draw_selected_lattice_overlay(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
    ) {
        let selection = selected(tree);
        if selection.len() != 1 {
            return;
        }
        let mut scene = objects(tree);
        if scene
            .iter()
            .any(|object| object.uuid == selection[0] && object.lattice.is_some())
        {
            self.draw_lattice_gizmos(ui, tree, camera, &mut scene, selection[0], 0, false);
        }
    }

    pub(super) fn draw_lattice_gizmos(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
        scene: &mut Vec<SdfObject>,
        object_id: uuid::Uuid,
        blocker_count: usize,
        editable: bool,
    ) {
        let matrix = crate::model::lattice_world_matrix(scene, object_id);
        let Some(object) = scene.iter_mut().find(|object| object.uuid == object_id) else {
            return;
        };
        let Some(lattice) = &mut object.lattice else {
            return;
        };
        let n = lattice.resolution as usize;
        if !(2..=9).contains(&n) || lattice.offsets.len() != n * n * n {
            return;
        }
        let pixels_per_point = ui.ctx().pixels_per_point();
        let mut positions = vec![None; lattice.offsets.len()];
        let mut surface_points = Vec::new();
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    if !lattice.is_surface_point(x, y, z) {
                        continue;
                    }
                    let index = lattice.index(x, y, z);
                    let world =
                        matrix.transform_point3(lattice.position(x, y, z) + lattice.offsets[index]);
                    positions[index] = camera.project(world, pixels_per_point);
                    surface_points.push((x, y, z, world));
                }
            }
        }
        let outline = Stroke::new(4.0, Color32::from_black_alpha(185));
        let stroke = Stroke::new(2.0, Color32::from_rgb(90, 220, 255));
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    let index = lattice.index(x, y, z);
                    let Some(a) = positions[index] else {
                        continue;
                    };
                    for axis in 0..3 {
                        let (next_x, next_y, next_z) = match axis {
                            0 if x + 1 < n => (x + 1, y, z),
                            1 if y + 1 < n => (x, y + 1, z),
                            2 if z + 1 < n => (x, y, z + 1),
                            _ => continue,
                        };
                        if let Some(b) = positions[lattice.index(next_x, next_y, next_z)] {
                            ui.painter().line_segment([a, b], outline);
                            ui.painter().line_segment([a, b], stroke);
                        }
                    }
                }
            }
        }
        let mut changed = false;
        let mut finished = false;
        surface_points.sort_by(|a, b| {
            b.3.distance_squared(camera.position)
                .total_cmp(&a.3.distance_squared(camera.position))
        });
        let selected_id = egui::Id::new(("lattice-selected-point", object_id));
        for (x, y, z, world) in surface_points {
            let index = lattice.index(x, y, z);
            let Some(center) = positions[index] else {
                continue;
            };
            if !ui.clip_rect().contains(center) && !editable {
                continue;
            }
            if !editable {
                let selected = ui
                    .ctx()
                    .data_mut(|data| data.get_temp::<[usize; 3]>(selected_id))
                    == Some([x, y, z]);
                let color = if selected {
                    Color32::from_rgb(255, 205, 95)
                } else {
                    Color32::from_rgb(105, 225, 255)
                };
                ui.painter().circle_filled(
                    center,
                    if selected { 7.0 } else { 6.0 },
                    Color32::from_black_alpha(220),
                );
                ui.painter()
                    .circle_filled(center, if selected { 5.0 } else { 4.5 }, color);
                continue;
            }
            let id = egui::Id::new(("lattice", object_id, index));
            let captured = ui.ctx().is_being_dragged(id) || ui.ctx().drag_stopped_id() == Some(id);
            if !captured
                && (!ui.clip_rect().contains(center)
                    || self.regions[..blocker_count.min(self.regions.len())]
                        .iter()
                        .any(|rect| rect.contains(center)))
            {
                continue;
            }
            let hit = egui::Rect::from_center_size(center, egui::vec2(18.0, 18.0));
            self.regions.push(hit.intersect(ui.clip_rect()));
            let response = ui
                .interact(hit, id, egui::Sense::click_and_drag())
                .on_hover_text("Drag this lattice point in the view plane");
            if response.clicked() || response.drag_started() {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(selected_id, [x, y, z]));
            }
            if response.drag_started() {
                if let Some(pointer) = response.interact_pointer_pos() {
                    self.lattice_drag = Some(LatticeDrag {
                        object: object_id,
                        index,
                        start_world: world,
                        start_offset: lattice.offsets[index],
                        start_pointer: pointer - response.drag_delta(),
                    });
                }
            }
            if response.dragged() {
                if let (Some(drag), Some(pointer)) =
                    (&self.lattice_drag, response.interact_pointer_pos())
                {
                    if drag.object == object_id && drag.index == index {
                        if let (Some(start), Some(current)) = (
                            pointer_on_lattice_plane(
                                camera,
                                drag.start_pointer,
                                pixels_per_point,
                                drag.start_world,
                            ),
                            pointer_on_lattice_plane(
                                camera,
                                pointer,
                                pixels_per_point,
                                drag.start_world,
                            ),
                        ) {
                            lattice.offsets[index] = drag.start_offset
                                + matrix.inverse().transform_vector3(current - start);
                            changed = true;
                        }
                    }
                }
            }
            if response.drag_stopped() {
                self.lattice_drag = None;
                finished = true;
            }
            let selected = ui
                .ctx()
                .data_mut(|data| data.get_temp::<[usize; 3]>(selected_id))
                == Some([x, y, z]);
            let color = if response.hovered() || response.dragged() {
                Color32::WHITE
            } else if selected {
                Color32::from_rgb(255, 205, 95)
            } else {
                Color32::from_rgb(105, 225, 255)
            };
            ui.painter().circle_filled(
                center,
                if selected { 7.0 } else { 6.0 },
                Color32::from_black_alpha(220),
            );
            ui.painter()
                .circle_filled(center, if selected { 5.0 } else { 4.5 }, color);
        }
        if changed {
            set_objects(tree, scene.clone());
        }
        if finished {
            tree.make_undo_redo_snapshot();
        }
    }
}

use super::*;

impl UiState {
    pub(super) fn draw_object_gizmos(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
    ) {
        let selection = selected(tree);
        if selection.len() != 1 || commands::effective_selected_ids(tree).len() != 1 {
            return;
        }
        let mut scene = objects(tree);
        let matrix = crate::model::object_world_matrix(&scene, selection[0]);
        let Some(object) = scene.iter_mut().find(|object| object.uuid == selection[0]) else {
            return;
        };
        let handles = resize_handles_with_matrix(object, matrix, camera);
        let mut changed = false;
        let mut finished = false;
        for (index, handle) in handles.iter().enumerate() {
            let Some(center) = camera.project(handle.world, ui.ctx().pixels_per_point()) else {
                continue;
            };
            if !ui.clip_rect().contains(center)
                || self.regions.iter().any(|rect| rect.contains(center))
            {
                continue;
            }
            let Some(ahead) = camera.project(
                handle.world + handle.direction * 0.1,
                ui.ctx().pixels_per_point(),
            ) else {
                continue;
            };
            let projected_axis = (ahead - center) * 10.0;
            if projected_axis.length_sq() < 4.0 {
                continue;
            }
            let axis = projected_axis / projected_axis.length();
            let tip = center + axis * 28.0;
            let mut hit = egui::Rect::from_center_size(center, egui::vec2(22.0, 22.0))
                .union(egui::Rect::from_center_size(tip, egui::vec2(14.0, 14.0)));
            let patch: Vec<_> = handle
                .patch
                .iter()
                .filter_map(|point| camera.project(*point, ui.ctx().pixels_per_point()))
                .collect();
            if patch.len() == 4 {
                for point in &patch {
                    hit.extend_with(*point);
                }
            }
            hit = hit.intersect(ui.clip_rect());
            self.regions.push(hit);
            let response = ui
                .interact(
                    hit,
                    egui::Id::new(("resize", object.uuid, index)),
                    egui::Sense::drag(),
                )
                .on_hover_text(format!(
                    "Drag to resize {}. Release to finish.",
                    handle.label
                ));
            let stroke = Stroke::new(
                if response.hovered() || response.dragged() {
                    2.5
                } else {
                    1.5
                },
                handle.color,
            );
            if patch.len() == 4 {
                ui.painter().add(egui::Shape::convex_polygon(
                    patch,
                    handle.color.gamma_multiply(0.18),
                    stroke,
                ));
            } else if let Some(origin) =
                camera.project(handle.guide_origin, ui.ctx().pixels_per_point())
            {
                let delta = center - origin;
                for segment in 0..12 {
                    let t = segment as f32 / 12.0;
                    ui.painter()
                        .line_segment([origin + delta * t, origin + delta * (t + 0.045)], stroke);
                }
                ui.painter().circle_stroke(center, 5.0, stroke);
            }
            ui.painter().arrow(center, tip - center, stroke);
            if response.hovered() || response.dragged() {
                let text = format!("{} {:.2}", handle.label, handle.value);
                let galley = ui.painter().layout_no_wrap(
                    text,
                    egui::FontId::proportional(11.0),
                    Color32::WHITE,
                );
                if let Some(label_rect) =
                    gizmo_label_rect(ui.clip_rect(), tip, galley.size(), &self.regions)
                {
                    ui.painter()
                        .rect_filled(label_rect, 4.0, Color32::from_black_alpha(210));
                    ui.painter().galley(
                        label_rect.min + egui::vec2(4.0, 4.0),
                        galley,
                        Color32::WHITE,
                    );
                    self.regions.push(label_rect);
                }
            }
            if response.dragged() {
                let delta = ui.input(|input| input.pointer.delta());
                let amount = delta.dot(projected_axis) / projected_axis.length_sq();
                match &mut object.params {
                    SdfParams::BoxParams(params) => {
                        params.box_q[handle.parameter] =
                            (params.box_q[handle.parameter] + amount).max(0.01)
                    }
                    SdfParams::SphereParams(params) => {
                        // Keep the base radius and other axes unchanged. The
                        // existing local scale represents the ellipsoid radii.
                        let axis = handle.parameter;
                        let sign = if object.transform.scale[axis] < 0.0 {
                            -1.0
                        } else {
                            1.0
                        };
                        object.transform.scale[axis] =
                            sign * (handle.value + amount).max(0.01) / params.radius.max(0.0001);
                    }
                    SdfParams::CylinderParams {
                        radius,
                        half_height,
                    } => {
                        let value = if handle.parameter == 0 {
                            radius
                        } else {
                            half_height
                        };
                        *value = (*value + amount).max(0.01);
                    }
                    SdfParams::TorusParams {
                        major_radius,
                        minor_radius,
                    } => {
                        let value = if handle.parameter == 0 {
                            major_radius
                        } else {
                            minor_radius
                        };
                        *value = (*value + amount).max(0.01);
                    }
                }
                changed = true;
            }
            finished |= response.drag_stopped();
        }
        if changed {
            set_objects(tree, scene);
        }
        if finished {
            tree.make_undo_redo_snapshot();
        }
    }
}

pub(super) fn gizmo_label_rect(
    viewport: egui::Rect,
    tip: egui::Pos2,
    text_size: egui::Vec2,
    occupied: &[egui::Rect],
) -> Option<egui::Rect> {
    let size = text_size + egui::vec2(8.0, 8.0);
    let offsets = [
        egui::vec2(12.0, -size.y * 0.5),
        egui::vec2(-size.x - 12.0, -size.y * 0.5),
        egui::vec2(-size.x * 0.5, -size.y - 12.0),
        egui::vec2(-size.x * 0.5, 12.0),
    ];
    offsets
        .into_iter()
        .map(|offset| egui::Rect::from_min_size(tip + offset, size))
        .find(|rect| {
            viewport.contains_rect(*rect) && !occupied.iter().any(|other| rect.intersects(*other))
        })
}

#[cfg(test)]
pub(super) fn resize_handles(object: &SdfObject, camera: &Camera) -> Vec<ResizeHandle> {
    resize_handles_with_matrix(object, object.transform.matrix(), camera)
}

pub(super) fn resize_handles_with_matrix(
    object: &SdfObject,
    matrix: glam::Mat4,
    camera: &Camera,
) -> Vec<ResizeHandle> {
    let mut handles = Vec::new();
    let origin = matrix.transform_point3(Vec3::ZERO);
    match &object.params {
        SdfParams::BoxParams(params) => {
            for axis in 0..3 {
                for sign in [-1.0, 1.0] {
                    let mut local = Vec3::ZERO;
                    local[axis] = params.box_q[axis] * sign;
                    let mut direction = Vec3::ZERO;
                    direction[axis] = sign;
                    let world = matrix.transform_point3(local);
                    let direction = matrix.transform_vector3(direction);
                    if direction.dot(camera.position - world) <= 0.0 {
                        continue;
                    }
                    let u = (axis + 1) % 3;
                    let v = (axis + 2) % 3;
                    let patch = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                        .into_iter()
                        .map(|(a, b)| {
                            let mut point = local;
                            point[u] = params.box_q[u] * 0.3 * a;
                            point[v] = params.box_q[v] * 0.3 * b;
                            matrix.transform_point3(point)
                        })
                        .collect();
                    handles.push(ResizeHandle {
                        world,
                        guide_origin: origin,
                        direction,
                        patch,
                        label: format!("Resize {}", axis_label(axis)),
                        color: axis_color(axis),
                        parameter: axis,
                        value: params.box_q[axis] * 2.0,
                    });
                }
            }
        }
        SdfParams::SphereParams(params) => {
            for (axis, local_axis) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
                let sign = if object.transform.scale[axis] < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let axis_direction = matrix.transform_vector3(local_axis * sign).normalize();
                // Use the camera-facing end of each local axis so handles do
                // not bunch together on the far side of a small sphere.
                let side = if axis_direction.dot(camera.position - origin) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let direction = axis_direction * side;
                handles.push(ResizeHandle {
                    world: matrix.transform_point3(local_axis * params.radius * side),
                    guide_origin: origin,
                    direction,
                    patch: vec![],
                    label: format!("Radius {}", axis_label(axis)),
                    color: axis_color(axis),
                    parameter: axis,
                    value: params.radius * matrix.transform_vector3(local_axis).length(),
                });
            }
        }
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => {
            for (axis, value, label) in [(0, *radius, "Radius"), (1, *half_height, "Height")] {
                let direction = if axis == 0 { Vec3::X } else { Vec3::Y };
                handles.push(ResizeHandle {
                    world: matrix.transform_point3(direction * value),
                    guide_origin: origin,
                    direction: matrix.transform_vector3(direction),
                    patch: vec![],
                    label: label.into(),
                    color: axis_color(axis),
                    parameter: axis,
                    value: if axis == 1 { value * 2.0 } else { value },
                });
            }
        }
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => {
            for (parameter, local, direction, label, value) in [
                (
                    0,
                    Vec3::X * *major_radius,
                    Vec3::X,
                    "Ring radius",
                    *major_radius,
                ),
                (
                    1,
                    Vec3::X * *major_radius + Vec3::Y * *minor_radius,
                    Vec3::Y,
                    "Tube radius",
                    *minor_radius,
                ),
            ] {
                handles.push(ResizeHandle {
                    world: matrix.transform_point3(local),
                    guide_origin: matrix.transform_point3(if parameter == 1 {
                        Vec3::X * *major_radius
                    } else {
                        Vec3::ZERO
                    }),
                    direction: matrix.transform_vector3(direction),
                    patch: vec![],
                    label: label.into(),
                    color: axis_color(parameter),
                    parameter,
                    value,
                });
            }
        }
    }
    handles
}

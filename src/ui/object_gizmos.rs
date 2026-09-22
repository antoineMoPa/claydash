use super::*;

const EDGE_ON_FACE_DOT_LIMIT: f32 = 0.25;

impl UiState {
    #[cfg(test)]
    pub(super) fn draw_object_gizmos(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
    ) {
        let blocker_count = self.regions.len();
        self.draw_object_gizmos_avoiding(ui, tree, camera, blocker_count);
    }

    pub(super) fn draw_object_gizmos_avoiding(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
        blocker_count: usize,
    ) {
        let selection = selected(tree);
        if selection.len() != 1 || commands::effective_selected_ids(tree).len() != 1 {
            return;
        }
        let mut scene = objects(tree);
        let excluded = commands::effective_selected_ids(tree);
        let guide_candidates = crate::guides::face_center_guides(&scene, &excluded);
        let matrix = crate::model::object_world_matrix(&scene, selection[0]);
        let Some(object) = scene.iter_mut().find(|object| object.uuid == selection[0]) else {
            return;
        };
        // Keep back-facing box handles in the list while drawing. An egui drag
        // owns its handle id, so the captured handle must continue to exist if
        // resizing moves that face behind another face or past the camera.
        let handles = resize_handles_with_matrix_including_hidden(object, matrix, camera);
        let selected_face = crate::model::selected_box_face(tree);
        let mut changed = false;
        let mut finished = false;
        for handle in &handles {
            let id = egui::Id::new(("resize", object.uuid, handle.id));
            let captured = ui.ctx().is_being_dragged(id) || ui.ctx().drag_stopped_id() == Some(id);
            if !captured && !handle.camera_facing && !handle.edge_on {
                continue;
            }
            let Some(center) = camera.project(handle.world, ui.ctx().pixels_per_point()) else {
                continue;
            };
            if !captured
                && (!ui.clip_rect().contains(center)
                    || self.regions[..blocker_count.min(self.regions.len())]
                        .iter()
                        .any(|rect| rect.contains(center)))
            {
                continue;
            }
            let Some(ahead) = camera.project(
                handle.world + handle.direction * 0.1,
                ui.ctx().pixels_per_point(),
            ) else {
                continue;
            };
            let mut projected_axis = (ahead - center) * 10.0;
            if projected_axis.length_sq() < 4.0 {
                let screen_up = camera.view().inverse().y_axis.truncate();
                let Some(fallback) =
                    camera.project(handle.world + screen_up * 0.1, ui.ctx().pixels_per_point())
                else {
                    continue;
                };
                projected_axis = (fallback - center) * 10.0;
                if projected_axis.length_sq() < 0.0001 {
                    continue;
                }
            }
            let axis = projected_axis / projected_axis.length();
            let tip = center + axis * 28.0;
            let patch: Vec<_> = handle
                .patch
                .iter()
                .filter_map(|point| camera.project(*point, ui.ctx().pixels_per_point()))
                .collect();
            let full_patch: Vec<_> = handle
                .full_patch
                .iter()
                .filter_map(|point| camera.project(*point, ui.ctx().pixels_per_point()))
                .collect();
            let edge_segment = handle
                .edge_on
                .then(|| longest_projected_segment(&patch))
                .flatten();
            let mut hit = egui::Rect::from_center_size(center, egui::vec2(22.0, 22.0))
                .union(egui::Rect::from_center_size(tip, egui::vec2(14.0, 14.0)));
            if let Some([start, end]) = edge_segment {
                hit = hit.union(egui::Rect::from_two_pos(start, end).expand(7.0));
            } else if patch.len() == 4 {
                for point in &patch {
                    hit.extend_with(*point);
                }
            }
            hit = hit.intersect(ui.clip_rect());
            self.regions.push(hit);
            let response = ui.interact(hit, id, egui::Sense::drag());
            let response = if handle.edge_on {
                response.on_hover_text(format!(
                    "Drag this edge to resize {}. Hold Shift to resize from the center; Alt bypasses guides. Release to finish.",
                    axis_label(handle.parameter)
                ))
            } else if matches!(&object.params, SdfParams::BoxParams(_)) {
                response.on_hover_text(format!(
                    "Click to select this face. Drag to resize {}. Hold Shift to resize from the center; Alt bypasses guides.",
                    handle.label
                ))
            } else {
                response.on_hover_text(format!(
                    "Drag to resize {}. Release to finish.",
                    handle.label
                ))
            };
            let face_selected = match handle.id {
                ResizeHandleId::BoxFace { axis, positive } => selected_face.is_some_and(|face| {
                    face.object == object.uuid
                        && face.axis == vector_axis(axis)
                        && face.positive == positive
                }),
                _ => false,
            };
            if response.drag_started() {
                if let ResizeHandleId::BoxFace { .. } = handle.id {
                    if let SdfParams::BoxParams(params) = &object.params {
                        self.resize_guide_drag = Some(ResizeGuideDrag {
                            object: object.uuid,
                            handle: handle.id,
                            initial_transform: object.transform,
                            initial_half_extent: params.box_q[handle.parameter],
                            initial_face_center: handle.world,
                            world_direction_per_unit: handle.direction,
                            projected_axis,
                            raw_amount: 0.0,
                            guides: guide_candidates.clone(),
                            active_guide: None,
                        });
                    }
                }
            }
            let stroke = Stroke::new(
                if response.hovered() || response.dragged() || face_selected {
                    2.5
                } else {
                    1.5
                },
                handle.color,
            );
            if face_selected && full_patch.len() == 4 {
                ui.painter().add(egui::Shape::convex_polygon(
                    full_patch,
                    Color32::WHITE.gamma_multiply(0.14),
                    Stroke::new(3.0, Color32::WHITE),
                ));
            }
            if let Some([start, end]) = edge_segment {
                ui.painter().line_segment(
                    [start, end],
                    Stroke::new(
                        if response.hovered() || response.dragged() {
                            4.0
                        } else if face_selected {
                            3.5
                        } else {
                            2.5
                        },
                        if face_selected {
                            Color32::WHITE
                        } else {
                            handle.color
                        },
                    ),
                );
            } else if patch.len() == 4 {
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
                let resize_from_center = ui.input(|input| input.modifiers.shift);
                match &mut object.params {
                    SdfParams::BoxParams(params) => {
                        if let Some(session) = self.resize_guide_drag.as_mut().filter(|session| {
                            session.object == object.uuid && session.handle == handle.id
                        }) {
                            session.raw_amount += delta.dot(session.projected_axis)
                                / session.projected_axis.length_sq();
                            let raw_amount = session.raw_amount;
                            let raw_half_delta = if resize_from_center {
                                raw_amount
                            } else {
                                raw_amount * 0.5
                            };
                            let raw_half_extent =
                                (session.initial_half_extent + raw_half_delta).max(0.01);
                            let mut face_movement = if resize_from_center {
                                raw_half_extent - session.initial_half_extent
                            } else {
                                (raw_half_extent - session.initial_half_extent) * 2.0
                            };
                            let raw_face_center = session.initial_face_center
                                + session.world_direction_per_unit * face_movement;
                            let bypass_guides = ui.input(|input| input.modifiers.alt);
                            let snap = if bypass_guides {
                                None
                            } else {
                                crate::guides::snap_along_line(
                                    camera,
                                    &[raw_face_center],
                                    session.world_direction_per_unit,
                                    &session.guides,
                                    ui.ctx().pixels_per_point(),
                                    session.active_guide,
                                )
                            };
                            session.active_guide = snap.map(|snap| snap.active);
                            self.active_guide = session.active_guide;
                            if let Some(snap) = snap {
                                let direction_length =
                                    session.world_direction_per_unit.length().max(0.0001);
                                face_movement += snap
                                    .correction
                                    .dot(session.world_direction_per_unit / direction_length)
                                    / direction_length;
                            }
                            let half_delta = if resize_from_center {
                                face_movement
                            } else {
                                face_movement * 0.5
                            };
                            params.box_q[handle.parameter] =
                                (session.initial_half_extent + half_delta).max(0.01);
                            object.transform = session.initial_transform;
                            if !resize_from_center {
                                let applied_delta =
                                    params.box_q[handle.parameter] - session.initial_half_extent;
                                let local_offset = handle.local_direction * applied_delta;
                                object.transform.translation +=
                                    object.transform.matrix().transform_vector3(local_offset);
                            }
                        } else {
                            let half_size = &mut params.box_q[handle.parameter];
                            let old_half_size = *half_size;
                            let half_size_delta = if resize_from_center {
                                amount
                            } else {
                                amount * 0.5
                            };
                            *half_size = (*half_size + half_size_delta).max(0.01);
                            if !resize_from_center {
                                let local_offset =
                                    handle.local_direction * (*half_size - old_half_size);
                                object.transform.translation +=
                                    object.transform.matrix().transform_vector3(local_offset);
                            }
                        }
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
            if response.clicked() || response.drag_started() {
                if let ResizeHandleId::BoxFace { axis, positive } = handle.id {
                    crate::model::set_selected_box_face(
                        tree,
                        Some(crate::model::BoxFaceSelection {
                            object: object.uuid,
                            axis: vector_axis(axis),
                            positive,
                        }),
                    );
                }
            }
            finished |= response.drag_stopped();
        }
        if changed {
            set_objects(tree, scene);
        }
        if finished {
            self.resize_guide_drag = None;
            tree.make_undo_redo_snapshot();
        }
    }
}

fn longest_projected_segment(points: &[egui::Pos2]) -> Option<[egui::Pos2; 2]> {
    if points.len() < 2 {
        return None;
    }
    let mut longest = None;
    let mut longest_length = 0.0;
    for index in 0..points.len() {
        let start = points[index];
        let end = points[(index + 1) % points.len()];
        let length = start.distance_sq(end);
        if length > longest_length {
            longest = Some([start, end]);
            longest_length = length;
        }
    }
    longest
}

fn vector_axis(axis: usize) -> crate::model::VectorAxis {
    match axis {
        0 => crate::model::VectorAxis::X,
        1 => crate::model::VectorAxis::Y,
        _ => crate::model::VectorAxis::Z,
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

#[cfg(test)]
pub(super) fn resize_handles_with_matrix(
    object: &SdfObject,
    matrix: glam::Mat4,
    camera: &Camera,
) -> Vec<ResizeHandle> {
    resize_handles_with_matrix_impl(object, matrix, camera, false)
}

fn resize_handles_with_matrix_including_hidden(
    object: &SdfObject,
    matrix: glam::Mat4,
    camera: &Camera,
) -> Vec<ResizeHandle> {
    resize_handles_with_matrix_impl(object, matrix, camera, true)
}

fn resize_handles_with_matrix_impl(
    object: &SdfObject,
    matrix: glam::Mat4,
    camera: &Camera,
    include_hidden_box_faces: bool,
) -> Vec<ResizeHandle> {
    let mut handles = Vec::new();
    let origin = matrix.transform_point3(Vec3::ZERO);
    match &object.params {
        SdfParams::BoxParams(params) => {
            for axis in 0..3 {
                for sign in [-1.0, 1.0] {
                    let mut local = Vec3::ZERO;
                    local[axis] = params.box_q[axis] * sign;
                    let mut local_direction = Vec3::ZERO;
                    local_direction[axis] = sign;
                    let world = matrix.transform_point3(local);
                    let direction = matrix.transform_vector3(local_direction);
                    let camera_facing = direction.dot(camera.position - world) > 0.0;
                    let view_to_camera = (camera.position - camera.target).normalize_or_zero();
                    let edge_on = direction.normalize_or_zero().dot(view_to_camera).abs()
                        <= EDGE_ON_FACE_DOT_LIMIT;
                    if !camera_facing && !edge_on && !include_hidden_box_faces {
                        continue;
                    }
                    let u = (axis + 1) % 3;
                    let v = (axis + 2) % 3;
                    let patch_scale = if edge_on { 1.0 } else { 0.3 };
                    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
                    let patch = corners
                        .into_iter()
                        .map(|(a, b)| {
                            let mut point = local;
                            point[u] = params.box_q[u] * patch_scale * a;
                            point[v] = params.box_q[v] * patch_scale * b;
                            matrix.transform_point3(point)
                        })
                        .collect();
                    let full_patch = corners
                        .into_iter()
                        .map(|(a, b)| {
                            let mut point = local;
                            point[u] = params.box_q[u] * a;
                            point[v] = params.box_q[v] * b;
                            matrix.transform_point3(point)
                        })
                        .collect();
                    handles.push(ResizeHandle {
                        id: ResizeHandleId::BoxFace {
                            axis,
                            positive: sign > 0.0,
                        },
                        world,
                        guide_origin: origin,
                        direction,
                        local_direction,
                        patch,
                        full_patch,
                        label: format!("Resize {}", axis_label(axis)),
                        color: axis_color(axis),
                        parameter: axis,
                        value: params.box_q[axis] * 2.0,
                        camera_facing,
                        edge_on,
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
                    id: ResizeHandleId::SphereAxis(axis),
                    world: matrix.transform_point3(local_axis * params.radius * side),
                    guide_origin: origin,
                    direction,
                    local_direction: local_axis * side,
                    patch: vec![],
                    full_patch: vec![],
                    label: format!("Radius {}", axis_label(axis)),
                    color: axis_color(axis),
                    parameter: axis,
                    value: params.radius * matrix.transform_vector3(local_axis).length(),
                    camera_facing: true,
                    edge_on: false,
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
                    id: if axis == 0 {
                        ResizeHandleId::CylinderRadius
                    } else {
                        ResizeHandleId::CylinderHeight
                    },
                    world: matrix.transform_point3(direction * value),
                    guide_origin: origin,
                    direction: matrix.transform_vector3(direction),
                    local_direction: direction,
                    patch: vec![],
                    full_patch: vec![],
                    label: label.into(),
                    color: axis_color(axis),
                    parameter: axis,
                    value: if axis == 1 { value * 2.0 } else { value },
                    camera_facing: true,
                    edge_on: false,
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
                    id: if parameter == 0 {
                        ResizeHandleId::TorusMajorRadius
                    } else {
                        ResizeHandleId::TorusMinorRadius
                    },
                    world: matrix.transform_point3(local),
                    guide_origin: matrix.transform_point3(if parameter == 1 {
                        Vec3::X * *major_radius
                    } else {
                        Vec3::ZERO
                    }),
                    direction: matrix.transform_vector3(direction),
                    local_direction: direction,
                    patch: vec![],
                    full_patch: vec![],
                    label: label.into(),
                    color: axis_color(parameter),
                    parameter,
                    value,
                    camera_facing: true,
                    edge_on: false,
                });
            }
        }
    }
    handles
}

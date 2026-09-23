use super::*;

impl UiState {
    pub(super) fn draw_polygon_cap_handle(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
        face: crate::model::PolygonPrismFaceSelection,
        blocker_count: usize,
    ) {
        let crate::model::PolygonPrismFace::Cap { positive } = face.face else {
            return;
        };
        let mut scene = objects(tree);
        let matrix = crate::model::object_world_matrix(&scene, face.object);
        let split_void = scene.iter().find_map(|candidate| {
            if candidate.boolean_parent != Some(face.object) || candidate.name != "Face split void"
            {
                return None;
            }
            let SdfParams::PolygonPrismParams(params) = &candidate.params else {
                return None;
            };
            Some((candidate.uuid, candidate.transform, params.half_depth))
        });
        let Some(object) = scene.iter_mut().find(|object| object.uuid == face.object) else {
            return;
        };
        let SdfParams::PolygonPrismParams(params) = &object.params else {
            return;
        };
        let sign = if positive { 1.0 } else { -1.0 };
        let center_local =
            params.vertices.iter().copied().sum::<Vec2>() / params.vertices.len().max(1) as f32;
        let center_world = matrix.transform_point3(center_local.extend(sign * params.half_depth));
        let direction = matrix.transform_vector3(Vec3::Z * sign);
        let scale = ui.ctx().pixels_per_point();
        let (Some(center), Some(ahead)) = (
            camera.project(center_world, scale),
            camera.project(center_world + direction * 0.1, scale),
        ) else {
            return;
        };
        let mut projected_axis = (ahead - center) * 10.0;
        if projected_axis.length_sq() < 4.0 {
            let screen_up = camera.view().inverse().y_axis.truncate();
            if let Some(fallback) = camera.project(center_world + screen_up * 0.1, scale) {
                projected_axis = (fallback - center) * 10.0;
            }
        }
        if projected_axis.length_sq() < 0.0001 {
            return;
        }
        let tip = center + projected_axis / projected_axis.length() * 28.0;
        let id = egui::Id::new(("polygon-cap-depth", face.object, positive));
        let captured = ui.ctx().is_being_dragged(id) || ui.ctx().drag_stopped_id() == Some(id);
        let hit =
            egui::Rect::from_center_size(center, egui::vec2(24.0, 24.0)).intersect(ui.clip_rect());
        if !captured
            && (hit.is_negative()
                || self.regions[..blocker_count.min(self.regions.len())]
                    .iter()
                    .any(|rect| rect.contains(center)))
        {
            return;
        }
        self.regions.push(hit);
        let response = ui
            .interact(hit, id, egui::Sense::drag())
            .on_hover_text("Drag to slide this face and change its depth. Release to finish.");
        ui.painter()
            .arrow(center, tip - center, Stroke::new(2.0, Color32::WHITE));
        ui.painter().circle_filled(center, 5.0, Color32::WHITE);
        if response.drag_started() {
            self.polygon_cap_drag = Some(PolygonCapDrag {
                object: face.object,
                positive,
                initial_transform: object.transform,
                initial_half_depth: params.half_depth,
                split_void,
                projected_axis,
                raw_amount: 0.0,
            });
        }
        if response.dragged() {
            if let Some(session) = self
                .polygon_cap_drag
                .as_mut()
                .filter(|session| session.object == face.object && session.positive == positive)
            {
                let delta = ui.input(|input| input.pointer.delta());
                session.raw_amount +=
                    delta.dot(session.projected_axis) / session.projected_axis.length_sq();
                apply_polygon_cap_drag(&mut scene, session, sign);
                set_objects(tree, scene);
            }
        }
        if response.drag_stopped() {
            self.polygon_cap_drag = None;
            tree.make_undo_redo_snapshot();
        }
    }
}

pub(in crate::ui) fn apply_polygon_cap_drag(
    scene: &mut [SdfObject],
    session: &PolygonCapDrag,
    sign: f32,
) {
    let Some(object) = scene
        .iter_mut()
        .find(|object| object.uuid == session.object)
    else {
        return;
    };
    let SdfParams::PolygonPrismParams(params) = &mut object.params else {
        return;
    };
    params.half_depth = (session.initial_half_depth + session.raw_amount * 0.5).max(0.01);
    let depth_change = params.half_depth - session.initial_half_depth;
    object.transform = session.initial_transform;
    let translation_change = object
        .transform
        .matrix()
        .transform_vector3(Vec3::Z * sign * depth_change);
    object.transform.translation += translation_change;

    if let Some((id, initial_transform, initial_half_depth)) = session.split_void {
        if let Some(void) = scene.iter_mut().find(|candidate| candidate.uuid == id) {
            if let SdfParams::PolygonPrismParams(params) = &mut void.params {
                params.half_depth = (initial_half_depth + depth_change).max(0.01);
                void.transform = initial_transform;
                void.transform.translation += translation_change;
            }
        }
    }
}

pub(in crate::ui) fn selected_polygon_face_vertices(
    scene: &[SdfObject],
    selection: crate::model::PolygonPrismFaceSelection,
) -> Option<(Vec<Vec2>, Vec<Vec3>)> {
    let object = scene
        .iter()
        .find(|object| object.uuid == selection.object)?;
    let SdfParams::PolygonPrismParams(params) = &object.params else {
        return None;
    };
    let (planar, local) = match selection.face {
        crate::model::PolygonPrismFace::Cap { positive } => (
            params.vertices.clone(),
            params
                .vertices
                .iter()
                .map(|point| {
                    Vec3::new(
                        point.x,
                        point.y,
                        params.half_depth * if positive { 1.0 } else { -1.0 },
                    )
                })
                .collect(),
        ),
        crate::model::PolygonPrismFace::Side { edge } => {
            let a = params.vertices.get(edge).copied()?;
            let b = params
                .vertices
                .get((edge + 1) % params.vertices.len())
                .copied()?;
            (
                vec![
                    Vec2::new(-1.0, -1.0),
                    Vec2::new(1.0, -1.0),
                    Vec2::new(1.0, 1.0),
                    Vec2::new(-1.0, 1.0),
                ],
                vec![
                    Vec3::new(a.x, a.y, -params.half_depth),
                    Vec3::new(b.x, b.y, -params.half_depth),
                    Vec3::new(b.x, b.y, params.half_depth),
                    Vec3::new(a.x, a.y, params.half_depth),
                ],
            )
        }
    };
    Some((planar, local))
}

pub(in crate::ui) fn polygon_overlay_triangles(vertices: &[Vec2]) -> Vec<[usize; 3]> {
    if vertices.len() < 3 {
        return Vec::new();
    }
    let area = vertices
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let next = vertices[(index + 1) % vertices.len()];
            point.x * next.y - next.x * point.y
        })
        .sum::<f32>();
    let mut remaining: Vec<_> = if area >= 0.0 {
        (0..vertices.len()).collect()
    } else {
        (0..vertices.len()).rev().collect()
    };
    let mut triangles = Vec::with_capacity(vertices.len().saturating_sub(2));
    while remaining.len() > 3 {
        let mut clipped = false;
        for cursor in 0..remaining.len() {
            let previous = remaining[(cursor + remaining.len() - 1) % remaining.len()];
            let current = remaining[cursor];
            let next = remaining[(cursor + 1) % remaining.len()];
            let a = vertices[previous];
            let b = vertices[current];
            let c = vertices[next];
            let ab = b - a;
            let bc = c - b;
            if ab.x * bc.y - ab.y * bc.x <= 0.000_001 {
                continue;
            }
            let contains_point = remaining.iter().copied().any(|candidate| {
                if candidate == previous || candidate == current || candidate == next {
                    return false;
                }
                let point = vertices[candidate];
                let side = |start: Vec2, end: Vec2| {
                    let edge = end - start;
                    let relative = point - start;
                    edge.x * relative.y - edge.y * relative.x
                };
                side(a, b) >= -0.000_001 && side(b, c) >= -0.000_001 && side(c, a) >= -0.000_001
            });
            if contains_point {
                continue;
            }
            triangles.push([previous, current, next]);
            remaining.remove(cursor);
            clipped = true;
            break;
        }
        if !clipped {
            return Vec::new();
        }
    }
    if remaining.len() == 3 {
        triangles.push([remaining[0], remaining[1], remaining[2]]);
    }
    triangles
}

pub(super) fn draw_selected_polygon_face_overlay(ui: &egui::Ui, tree: &DataTree, camera: &Camera) {
    let Some(crate::model::ModelingFaceSelection::PolygonPrism(selection)) =
        crate::model::selected_modeling_face(tree)
    else {
        return;
    };
    let scene = objects(tree);
    let Some((planar, local)) = selected_polygon_face_vertices(&scene, selection) else {
        return;
    };
    let matrix = crate::model::object_world_matrix(&scene, selection.object);
    let screen: Vec<_> = local
        .iter()
        .filter_map(|point| {
            camera.project(matrix.transform_point3(*point), ui.ctx().pixels_per_point())
        })
        .collect();
    if screen.len() != local.len() || screen.len() < 3 {
        return;
    }
    let fill = Color32::from_rgb(255, 190, 72).gamma_multiply(0.22);
    for triangle in polygon_overlay_triangles(&planar) {
        ui.painter().add(egui::Shape::convex_polygon(
            triangle.map(|index| screen[index]).to_vec(),
            fill,
            Stroke::NONE,
        ));
    }
    ui.painter().add(egui::Shape::closed_line(
        screen,
        Stroke::new(3.0, Color32::WHITE),
    ));
}

pub(super) fn draw_selected_cylinder_cap_overlay(ui: &egui::Ui, tree: &DataTree, camera: &Camera) {
    let Some(crate::model::ModelingFaceSelection::CylinderCap(face)) =
        crate::model::selected_modeling_face(tree)
    else {
        return;
    };
    let scene = objects(tree);
    let Some(object) = scene.iter().find(|object| object.uuid == face.object) else {
        return;
    };
    let SdfParams::CylinderParams {
        radius,
        half_height,
    } = object.params
    else {
        return;
    };
    let matrix = crate::model::object_world_matrix(&scene, face.object);
    let y = half_height * if face.positive { 1.0 } else { -1.0 };
    let screen: Vec<_> = (0..48)
        .filter_map(|index| {
            let angle = index as f32 * std::f32::consts::TAU / 48.0;
            let local = Vec3::new(radius * angle.cos(), y, radius * angle.sin());
            camera.project(matrix.transform_point3(local), ui.ctx().pixels_per_point())
        })
        .collect();
    if screen.len() != 48 {
        return;
    }
    ui.painter().add(egui::Shape::convex_polygon(
        screen,
        Color32::from_rgb(255, 190, 72).gamma_multiply(0.22),
        Stroke::new(3.0, Color32::WHITE),
    ));
}

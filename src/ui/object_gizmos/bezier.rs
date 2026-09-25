use super::*;

pub(in crate::ui) struct BezierDrag {
    object: uuid::Uuid,
    index: usize,
    start_local: Vec3,
    start_world: Vec3,
    start_pointer: egui::Pos2,
}

pub(in crate::ui) fn draw_bezier_paths(
    ui: &egui::Ui,
    tree: &DataTree,
    camera: &Camera,
) -> Option<(uuid::Uuid, egui::Rect)> {
    let scene = crate::model::objects_ref(tree);
    let selected = crate::model::selected_ref(tree);
    let pixels_per_point = ui.ctx().pixels_per_point();
    let extending = matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(crate::model::EditorState::ExtendingCurve)
    );
    let moving_point = matches!(
        tree.get_path("editor.curve_grab_initial"),
        ClaydashValue::VecSDFObject(_)
    );
    let click = (!extending && !moving_point)
        .then(|| {
            ui.input(|input| {
                input
                    .pointer
                    .primary_clicked()
                    .then(|| input.pointer.interact_pos())
                    .flatten()
            })
        })
        .flatten();
    let click = click.filter(|pointer| {
        !scene.iter().any(|object| {
            if !selected.contains(&object.uuid) {
                return false;
            }
            let SdfParams::BezierCurveParams(curve) = &object.params else {
                return false;
            };
            let matrix = crate::model::object_world_matrix(scene, object.uuid);
            curve.points.iter().enumerate().any(|(index, point)| {
                !(curve.closed && index + 1 == curve.points.len())
                    && camera
                        .project(matrix.transform_point3(*point), pixels_per_point)
                        .is_some_and(|center| center.distance(*pointer) <= 9.0)
            })
        })
    });
    let mut nearest = 5.0_f32.powi(2);
    let mut picked = None;
    for object in scene {
        let SdfParams::BezierCurveParams(curve) = &object.params else {
            continue;
        };
        let matrix = crate::model::object_world_matrix(scene, object.uuid);
        let stroke = Stroke::new(
            if selected.contains(&object.uuid) {
                2.0
            } else {
                1.5
            },
            if selected.contains(&object.uuid) {
                Color32::from_rgb(105, 225, 255)
            } else {
                Color32::from_rgb(95, 160, 190)
            },
        );
        for segment in 0..curve
            .segment_count()
            .min(crate::model::BezierCurveParams::MAX_SEGMENTS)
        {
            let mut previous = camera.project(
                matrix.transform_point3(curve.point(segment, 0.0)),
                pixels_per_point,
            );
            for step in 1..=32 {
                let next = camera.project(
                    matrix.transform_point3(curve.point(segment, step as f32 / 32.0)),
                    pixels_per_point,
                );
                if let (Some(a), Some(b)) = (previous, next) {
                    ui.painter().line_segment(
                        [a, b],
                        Stroke::new(stroke.width + 2.0, Color32::from_black_alpha(175)),
                    );
                    ui.painter().line_segment([a, b], stroke);
                    if let Some(pointer) = click.filter(|pointer| ui.clip_rect().contains(*pointer))
                    {
                        let edge = b - a;
                        let along =
                            ((pointer - a).dot(edge) / edge.length_sq().max(1e-6)).clamp(0.0, 1.0);
                        let distance = pointer.distance_sq(a + edge * along);
                        if distance < nearest {
                            nearest = distance;
                            picked = Some((object.uuid, pointer));
                        }
                    }
                }
                previous = next;
            }
        }
        if !selected.contains(&object.uuid) {
            for &point in &curve.points {
                if let Some(center) =
                    camera.project(matrix.transform_point3(point), pixels_per_point)
                {
                    ui.painter()
                        .circle_filled(center, 3.0, Color32::from_rgb(95, 160, 190));
                }
            }
        } else if extending || moving_point {
            if let Some(point) =
                crate::model::selected_curve_point(tree).filter(|point| point.object == object.uuid)
            {
                if let Some(center) = curve.points.get(point.index).and_then(|point| {
                    camera.project(matrix.transform_point3(*point), pixels_per_point)
                }) {
                    ui.painter()
                        .circle_filled(center, 8.0, Color32::from_black_alpha(220));
                    ui.painter().circle_filled(center, 5.0, Color32::WHITE);
                }
            }
        }
    }
    picked.map(|(id, pointer)| {
        (
            id,
            egui::Rect::from_center_size(pointer, egui::vec2(10.0, 10.0)),
        )
    })
}

fn pointer_on_view_plane(
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

pub(super) fn draw_bezier_gizmo(
    state: &mut UiState,
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    camera: &Camera,
    blocker_count: usize,
) -> bool {
    let selection = selected(tree);
    let mut scene = objects(tree);
    let object_id = selection[0];
    let matrix = crate::model::object_world_matrix(&scene, object_id);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == object_id) else {
        return false;
    };
    let SdfParams::BezierCurveParams(curve) = &mut object.params else {
        return false;
    };
    let pixels_per_point = ui.ctx().pixels_per_point();
    let projected = |point: Vec3| camera.project(matrix.transform_point3(point), pixels_per_point);
    let handle_stroke = Stroke::new(1.0, Color32::from_rgb(105, 225, 255));
    for segment in 0..curve
        .segment_count()
        .min(crate::model::BezierCurveParams::MAX_SEGMENTS)
    {
        for (a, b) in [
            (segment * 3, segment * 3 + 1),
            (segment * 3 + 2, segment * 3 + 3),
        ] {
            if let (Some(start), Some(end)) =
                (projected(curve.points[a]), projected(curve.points[b]))
            {
                ui.painter().line_segment([start, end], handle_stroke);
            }
        }
    }
    let mut changed = false;
    let mut finished = false;
    let mut close_on_click = false;
    for index in 0..curve.points.len() {
        if curve.closed && index + 1 == curve.points.len() {
            continue;
        }
        let world = matrix.transform_point3(curve.points[index]);
        let Some(center) = camera.project(world, pixels_per_point) else {
            continue;
        };
        let id = egui::Id::new(("bezier-control", object_id, index));
        let captured = ui.ctx().is_being_dragged(id) || ui.ctx().drag_stopped_id() == Some(id);
        if !captured
            && (!ui.clip_rect().contains(center)
                || state.regions[..blocker_count.min(state.regions.len())]
                    .iter()
                    .any(|rect| rect.contains(center)))
        {
            continue;
        }
        let hit = egui::Rect::from_center_size(center, egui::vec2(18.0, 18.0));
        state.regions.push(hit.intersect(ui.clip_rect()));
        let response = ui
            .interact(hit, id, egui::Sense::click_and_drag())
            .on_hover_text(if index % 3 == 0 {
                "Drag anchor · G moves · Backspace removes · E extends · Enter closes from the end"
            } else {
                "Drag Bézier handle"
            });
        if response.clicked()
            && index == 0
            && crate::model::selected_curve_point(tree).is_some_and(|point| {
                point.object == object_id && point.index + 1 == curve.points.len()
            })
            && !curve.closed
        {
            close_on_click = true;
        } else if response.clicked() || response.drag_started() {
            crate::model::set_selected_curve_point(
                tree,
                Some(crate::model::CurvePointSelection {
                    object: object_id,
                    index,
                }),
            );
        }
        if response.drag_started() {
            if let Some(pointer) = response.interact_pointer_pos() {
                state.bezier_drag = Some(BezierDrag {
                    object: object_id,
                    index,
                    start_local: curve.points[index],
                    start_world: world,
                    start_pointer: pointer - response.drag_delta(),
                });
            }
        }
        if response.dragged() {
            if let Some(drag) = &state.bezier_drag {
                if drag.object == object_id && drag.index == index {
                    let start = pointer_on_view_plane(
                        camera,
                        drag.start_pointer,
                        pixels_per_point,
                        drag.start_world,
                    );
                    let current = response.interact_pointer_pos().and_then(|pointer| {
                        pointer_on_view_plane(camera, pointer, pixels_per_point, drag.start_world)
                    });
                    if let (Some(start), Some(current)) = (start, current) {
                        let local_delta = matrix.inverse().transform_vector3(current - start);
                        curve.points[index] = drag.start_local + local_delta;
                        if curve.closed && index == 0 {
                            let last = curve.points.len() - 1;
                            curve.points[last] = curve.points[0];
                        }
                        changed = true;
                    }
                }
            }
        }
        if response.drag_stopped() {
            state.bezier_drag = None;
            finished = true;
        }
        ui.painter().circle_filled(
            center,
            if index % 3 == 0 { 7.0 } else { 6.0 },
            Color32::from_black_alpha(220),
        );
        ui.painter().circle_filled(
            center,
            if index % 3 == 0 { 5.0 } else { 4.0 },
            if response.hovered()
                || response.dragged()
                || crate::model::selected_curve_point(tree)
                    .is_some_and(|point| point.object == object_id && point.index == index)
            {
                Color32::WHITE
            } else {
                Color32::from_rgb(105, 225, 255)
            },
        );
    }
    if changed {
        set_objects(tree, scene);
    }
    if close_on_click {
        crate::commands::close_selected_curve(tree);
    }
    if finished {
        tree.make_undo_redo_snapshot();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicking_an_anchor_then_backspace_keeps_the_curve_object() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::BezierCurve);
        let id = object.uuid;
        let SdfParams::BezierCurveParams(curve) = &mut object.params else {
            unreachable!()
        };
        curve.extend_from_end();
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800.0, 600.0);
        let pointer = camera.project(curve.points[3], 1.0).unwrap();
        set_objects(&mut tree, vec![object]);
        set_selected(&mut tree, vec![id]);
        let mut state = UiState::default();
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    state.regions.clear();
                    if let Some((curve, hit)) = draw_bezier_paths(ui, &tree, &camera) {
                        set_selected(&mut tree, vec![curve]);
                        state.regions.push(hit);
                    }
                    let blockers = state.regions.len();
                    draw_bezier_gizmo(&mut state, ui, &mut tree, &camera, blockers);
                },
            );
            output.textures_delta.clear();
        };
        frame(vec![]);
        frame(vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        frame(vec![egui::Event::PointerButton {
            pos: pointer,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(crate::model::selected_curve_point(&tree).unwrap().index, 3);
        let mut commands = crate::commands::Commands::new();
        crate::commands::register_all(&mut commands);
        crate::commands::execute(&commands, "delete", &mut tree);
        let scene = objects(&tree);
        assert_eq!(scene.len(), 1);
        let SdfParams::BezierCurveParams(curve) = &scene[0].params else {
            unreachable!()
        };
        assert_eq!(curve.segment_count(), 1);
    }

    #[test]
    fn an_unextruded_curve_can_be_picked_from_its_viewport_path() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(crate::model::PrimitiveKind::BezierCurve);
        let id = object.uuid;
        let SdfParams::BezierCurveParams(curve) = &object.params else {
            unreachable!()
        };
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800.0, 600.0);
        let pointer = camera.project(curve.point(0, 0.5), 1.0).unwrap();
        set_objects(&mut tree, vec![object]);
        let frame = |events| {
            let mut picked = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    picked = draw_bezier_paths(ui, &tree, &camera);
                },
            );
            output.textures_delta.clear();
            picked
        };
        frame(vec![egui::Event::PointerMoved(pointer)]);
        frame(vec![egui::Event::PointerButton {
            pos: pointer,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        let picked = frame(vec![egui::Event::PointerButton {
            pos: pointer,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(picked.map(|(candidate, _)| candidate), Some(id));
    }
}

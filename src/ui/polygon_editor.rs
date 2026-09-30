use super::*;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct PointSelection {
    pub active: Option<usize>,
    pub previous: Option<usize>,
}

#[derive(Clone, Copy)]
struct PointDrag {
    index: usize,
    offset: Vec2,
}

fn selection_id(object: uuid::Uuid) -> egui::Id {
    egui::Id::new(("polygon-point-selection", object))
}

fn drag_id(object: uuid::Uuid) -> egui::Id {
    egui::Id::new(("polygon-point-drag", object))
}

fn selection(ctx: &egui::Context, object: uuid::Uuid, count: usize) -> PointSelection {
    let mut value = ctx
        .data(|data| data.get_temp::<PointSelection>(selection_id(object)))
        .unwrap_or_default();
    if value.active.is_some_and(|index| index >= count) {
        value.active = None;
    }
    if value.previous.is_some_and(|index| index >= count) {
        value.previous = None;
    }
    value
}

fn set_selection(ctx: &egui::Context, object: uuid::Uuid, value: PointSelection) {
    ctx.data_mut(|data| data.insert_temp(selection_id(object), value));
}

pub(super) fn insertion_index(selection: PointSelection, count: usize) -> Option<usize> {
    let (Some(active), Some(previous)) = (selection.active, selection.previous) else {
        return None;
    };
    if count < 3 || active >= count || previous >= count || active == previous {
        return None;
    }
    if (previous + 1) % count == active {
        Some(active)
    } else if (active + 1) % count == previous {
        Some(previous)
    } else {
        None
    }
}

pub(super) fn active(tree: &DataTree) -> Option<uuid::Uuid> {
    let ClaydashValue::Uuid(id) = tree.get_path("editor.polygon_edit") else {
        return None;
    };
    (selected(tree) == [id]
        && crate::model::selection_scope(tree) == crate::model::SelectionScope::Exact
        && crate::model::objects_ref(tree).iter().any(|object| {
            object.uuid == id && matches!(object.params, SdfParams::PolygonPrismParams(_))
        }))
    .then_some(id)
}

pub(super) fn begin(ctx: &egui::Context, tree: &mut DataTree, object: uuid::Uuid) {
    if !matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(crate::model::EditorState::Start)
    ) {
        commands::finish(tree);
    }
    tree.set_path("editor.spawn_at_cursor", ClaydashValue::None);
    ctx.data_mut(|data| {
        data.remove::<PointSelection>(selection_id(object));
        data.remove::<PointDrag>(drag_id(object));
    });
    crate::model::set_selected_exact(tree, vec![object]);
    tree.set_transient_path("editor.polygon_edit", ClaydashValue::Uuid(object));
}

pub(super) fn finish(tree: &mut DataTree) {
    if let Some(object) = active(tree) {
        crate::model::set_selected(tree, vec![object]);
    }
    tree.set_transient_path("editor.polygon_edit", ClaydashValue::None);
}

pub(super) fn control_availability(ctx: &egui::Context, tree: &DataTree) -> (bool, bool) {
    let Some(id) = active(tree) else {
        return (false, false);
    };
    let Some(object) = crate::model::objects_ref(tree)
        .iter()
        .find(|object| object.uuid == id)
    else {
        return (false, false);
    };
    let SdfParams::PolygonPrismParams(params) = &object.params else {
        return (false, false);
    };
    let selection = selection(ctx, id, params.vertices.len());
    let add = params.vertices.len() < crate::model::MAX_POLYGON_PRISM_VERTICES
        && insertion_index(selection, params.vertices.len()).is_some();
    let remove = selection.active.is_some_and(|index| {
        if params.vertices.len() <= 3 {
            return false;
        }
        let mut candidate = params.vertices.clone();
        candidate.remove(index);
        selection_tools::polygon_is_valid(&candidate)
    });
    (add, remove)
}

pub(super) fn add_point(ctx: &egui::Context, tree: &mut DataTree) {
    let Some(id) = active(tree) else { return };
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == id) else {
        return;
    };
    let SdfParams::PolygonPrismParams(params) = &mut object.params else {
        return;
    };
    if params.vertices.len() >= crate::model::MAX_POLYGON_PRISM_VERTICES {
        return;
    }
    let mut selection = selection(ctx, id, params.vertices.len());
    let Some(index) = insertion_index(selection, params.vertices.len()) else {
        return;
    };
    let active = selection.active.unwrap();
    let previous = selection.previous.unwrap();
    let point = (params.vertices[active] + params.vertices[previous]) * 0.5;
    params.vertices.insert(index, point);
    selection.previous = Some(if index <= active { active + 1 } else { active });
    selection.active = Some(index);
    set_selection(ctx, id, selection);
    set_objects(tree, scene);
    tree.make_undo_redo_snapshot();
}

pub(super) fn remove_point(ctx: &egui::Context, tree: &mut DataTree) {
    let Some(id) = active(tree) else { return };
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == id) else {
        return;
    };
    let SdfParams::PolygonPrismParams(params) = &mut object.params else {
        return;
    };
    let mut selection = selection(ctx, id, params.vertices.len());
    let Some(index) = selection.active else {
        return;
    };
    if params.vertices.len() <= 3 {
        return;
    }
    let mut candidate = params.vertices.clone();
    candidate.remove(index);
    if !selection_tools::polygon_is_valid(&candidate) {
        return;
    }
    params.vertices = candidate;
    selection.active = Some(if index == 0 {
        params.vertices.len() - 1
    } else {
        index - 1
    });
    selection.previous = None;
    set_selection(ctx, id, selection);
    set_objects(tree, scene);
    tree.make_undo_redo_snapshot();
}

fn point_on_cap(
    camera: &Camera,
    pointer: egui::Pos2,
    pixels_per_point: f32,
    inverse: glam::Mat4,
    cap_z: f32,
) -> Option<Vec2> {
    let (origin, direction) = camera.ray(Vec2::new(pointer.x, pointer.y) * pixels_per_point);
    let local_origin = inverse.transform_point3(origin);
    let local_direction = inverse.transform_vector3(direction);
    if local_direction.z.abs() < 0.0001 {
        return None;
    }
    let distance = (cap_z - local_origin.z) / local_direction.z;
    (distance >= 0.0).then_some((local_origin + local_direction * distance).truncate())
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    camera: &Camera,
    regions: &mut Vec<egui::Rect>,
    blocker_count: usize,
) {
    let Some(id) = active(tree) else { return };
    let mut scene = objects(tree);
    let matrix = crate::model::object_world_matrix(&scene, id);
    let inverse = matrix.inverse();
    let cap_sign = if inverse.transform_point3(camera.position).z >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let Some(object) = scene.iter_mut().find(|object| object.uuid == id) else {
        return;
    };
    let SdfParams::PolygonPrismParams(params) = &mut object.params else {
        return;
    };
    let pixels_per_point = ui.ctx().pixels_per_point();
    let cap_z = params.half_depth * cap_sign;
    let positions: Vec<_> = params
        .vertices
        .iter()
        .map(|point| {
            camera.project(
                matrix.transform_point3(point.extend(cap_z)),
                pixels_per_point,
            )
        })
        .collect();
    if positions.iter().any(Option::is_none) {
        return;
    }
    let positions: Vec<_> = positions.into_iter().map(Option::unwrap).collect();
    let mut selection = selection(ui.ctx(), id, params.vertices.len());
    let accent = Color32::from_rgb(255, 166, 66);
    for index in 0..positions.len() {
        let segment = [positions[index], positions[(index + 1) % positions.len()]];
        ui.painter()
            .line_segment(segment, Stroke::new(4.0, Color32::from_black_alpha(180)));
        ui.painter()
            .line_segment(segment, Stroke::new(2.0, Color32::WHITE));
    }
    if insertion_index(selection, params.vertices.len()).is_some() {
        ui.painter().line_segment(
            [
                positions[selection.active.unwrap()],
                positions[selection.previous.unwrap()],
            ],
            Stroke::new(2.5, accent),
        );
    }
    let mut changed = false;
    let mut finished = false;
    for (index, center) in positions.iter().copied().enumerate() {
        let handle_id = egui::Id::new(("polygon-viewport-point", id, index));
        let captured =
            ui.ctx().is_being_dragged(handle_id) || ui.ctx().drag_stopped_id() == Some(handle_id);
        if !captured
            && (!ui.clip_rect().contains(center)
                || regions[..blocker_count.min(regions.len())]
                    .iter()
                    .any(|rect| rect.contains(center)))
        {
            continue;
        }
        let hit = egui::Rect::from_center_size(center, egui::vec2(20.0, 20.0));
        regions.push(hit.intersect(ui.clip_rect()));
        let response = ui
            .interact(hit, handle_id, egui::Sense::click_and_drag())
            .on_hover_text(format!("Polygon point {}", index + 1));
        if (response.clicked() || response.drag_started()) && selection.active != Some(index) {
            selection.previous = selection.active;
            selection.active = Some(index);
        }
        if response.drag_started() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let start = pointer - response.drag_delta();
                if let Some(local) = point_on_cap(camera, start, pixels_per_point, inverse, cap_z) {
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            drag_id(id),
                            PointDrag {
                                index,
                                offset: params.vertices[index] - local,
                            },
                        )
                    });
                }
            }
        }
        if response.dragged() {
            let drag = ui
                .ctx()
                .data(|data| data.get_temp::<PointDrag>(drag_id(id)));
            if let (Some(drag), Some(pointer)) = (drag, response.interact_pointer_pos()) {
                if drag.index == index {
                    if let Some(local) =
                        point_on_cap(camera, pointer, pixels_per_point, inverse, cap_z)
                    {
                        let mut candidate = params.vertices.clone();
                        candidate[index] = local + drag.offset;
                        if selection_tools::polygon_is_valid(&candidate) {
                            params.vertices = candidate;
                            changed = true;
                        }
                    }
                }
            }
        }
        if response.drag_stopped() {
            ui.ctx()
                .data_mut(|data| data.remove::<PointDrag>(drag_id(id)));
            finished = true;
        }
        ui.painter()
            .circle_filled(center, 6.0, Color32::from_black_alpha(220));
        ui.painter()
            .circle_stroke(center, 5.0, Stroke::new(1.5, Color32::WHITE));
        if selection.active == Some(index) {
            ui.painter()
                .circle_stroke(center, 10.0, Stroke::new(2.5, accent));
        } else if selection.previous == Some(index) {
            for segment in (0..12).step_by(2) {
                let start = std::f32::consts::TAU * segment as f32 / 12.0;
                let end = std::f32::consts::TAU * (segment + 1) as f32 / 12.0;
                let at = |angle: f32| center + egui::vec2(angle.cos() * 10.0, angle.sin() * 10.0);
                ui.painter()
                    .line_segment([at(start), at(end)], Stroke::new(2.0, Color32::WHITE));
            }
        }
    }
    set_selection(ui.ctx(), id, selection);
    if changed {
        set_objects(tree, scene);
    }
    if finished {
        tree.make_undo_redo_snapshot();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_shape_mode_finishes_polygon_placement() {
        let mut tree = DataTree::default();
        commands::spawn(&mut tree, PrimitiveKind::PolygonPrism.object_type());
        let id = selected(&tree)[0];
        begin(&egui::Context::default(), &mut tree, id);
        assert_eq!(active(&tree), Some(id));
        assert!(matches!(
            tree.get_path("editor.spawn_at_cursor"),
            ClaydashValue::None
        ));
        assert!(matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(crate::model::EditorState::Start)
        ));
    }

    #[test]
    fn insertion_requires_two_neighboring_selected_points() {
        assert_eq!(
            insertion_index(
                PointSelection {
                    active: Some(1),
                    previous: Some(0)
                },
                4
            ),
            Some(1)
        );
        assert_eq!(
            insertion_index(
                PointSelection {
                    active: Some(0),
                    previous: Some(3)
                },
                4
            ),
            Some(0)
        );
        assert_eq!(
            insertion_index(
                PointSelection {
                    active: Some(2),
                    previous: Some(0)
                },
                4
            ),
            None
        );
        assert_eq!(
            insertion_index(
                PointSelection {
                    active: Some(2),
                    previous: None
                },
                4
            ),
            None
        );
    }

    #[test]
    fn projected_polygon_points_map_back_to_the_cap_plane() {
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(120.0, 45.0);
        camera.viewport = Vec2::new(720.0, 540.0);
        let matrix = glam::Mat4::from_scale_rotation_translation(
            Vec3::new(1.2, 0.8, 1.5),
            glam::Quat::from_rotation_y(0.3),
            Vec3::new(0.3, -0.2, 0.1),
        );
        let local = Vec2::new(0.2, -0.15);
        let cap_z = 0.4;
        let pointer = camera
            .project(matrix.transform_point3(local.extend(cap_z)), 1.0)
            .unwrap();
        let recovered = point_on_cap(&camera, pointer, 1.0, matrix.inverse(), cap_z).unwrap();
        assert!(recovered.distance(local) < 0.0001);
    }

    #[test]
    fn dragging_viewport_point_edits_polygon_and_undoes() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
        let id = object.uuid;
        set_objects(&mut tree, vec![object]);
        begin(&ctx, &mut tree, id);
        tree.make_undo_redo_snapshot();
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::ZERO;
        camera.viewport = Vec2::new(800.0, 600.0);
        let scene = objects(&tree);
        let matrix = crate::model::object_world_matrix(&scene, id);
        let SdfParams::PolygonPrismParams(params) = &scene[0].params else {
            unreachable!()
        };
        let original = params.vertices[0];
        let cap_z = params.half_depth
            * if matrix.inverse().transform_point3(camera.position).z >= 0.0 {
                1.0
            } else {
                -1.0
            };
        let start = camera
            .project(matrix.transform_point3(original.extend(cap_z)), 1.0)
            .unwrap();
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
                    let mut regions = Vec::new();
                    draw(ui, &mut tree, &camera, &mut regions, 0);
                },
            );
            output.textures_delta.clear();
        };
        frame(vec![]);
        frame(vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        frame(vec![egui::Event::PointerMoved(
            start + egui::vec2(8.0, 4.0),
        )]);
        frame(vec![egui::Event::PointerMoved(
            start + egui::vec2(16.0, 8.0),
        )]);
        frame(vec![egui::Event::PointerButton {
            pos: start + egui::vec2(16.0, 8.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        let SdfParams::PolygonPrismParams(params) = &objects(&tree)[0].params else {
            unreachable!()
        };
        assert!(params.vertices[0].distance(original) > 0.001);
        undo_redo::undo(&mut tree);
        let SdfParams::PolygonPrismParams(params) = &objects(&tree)[0].params else {
            unreachable!()
        };
        assert_eq!(params.vertices[0], original);
    }

    #[test]
    fn viewport_selection_adds_and_removes_polygon_points_with_undo() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
        let id = object.uuid;
        set_objects(&mut tree, vec![object]);
        begin(&ctx, &mut tree, id);
        tree.make_undo_redo_snapshot();
        let ui_state = UiState::default();
        assert!(ui_state.delete_active_polygon_point(&ctx, &mut tree));
        assert_eq!(objects(&tree)[0].uuid, id);
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::ZERO;
        camera.viewport = Vec2::new(800.0, 600.0);
        let scene = objects(&tree);
        let matrix = crate::model::object_world_matrix(&scene, id);
        let SdfParams::PolygonPrismParams(params) = &scene[0].params else {
            unreachable!()
        };
        let cap_sign = if matrix.inverse().transform_point3(camera.position).z >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let points: Vec<_> = params
            .vertices
            .iter()
            .take(2)
            .map(|point| {
                camera
                    .project(
                        matrix.transform_point3(point.extend(params.half_depth * cap_sign)),
                        1.0,
                    )
                    .unwrap()
            })
            .collect();
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
                    let mut regions = Vec::new();
                    draw(ui, &mut tree, &camera, &mut regions, 0);
                },
            );
            output.textures_delta.clear();
        };
        frame(vec![]);
        for point in points {
            for pressed in [true, false] {
                frame(vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
        }
        let selection = selection(&ctx, id, 3);
        assert_eq!(
            selection,
            PointSelection {
                active: Some(1),
                previous: Some(0)
            }
        );
        assert_eq!(control_availability(&ctx, &tree), (true, false));
        add_point(&ctx, &mut tree);
        let scene = objects(&tree);
        let SdfParams::PolygonPrismParams(params) = &scene[0].params else {
            unreachable!()
        };
        assert_eq!(params.vertices.len(), 4);
        assert!(selection_tools::polygon_is_valid(&params.vertices));
        assert_eq!(control_availability(&ctx, &tree), (true, true));
        assert!(ui_state.delete_active_polygon_point(&ctx, &mut tree));
        let SdfParams::PolygonPrismParams(params) = &objects(&tree)[0].params else {
            unreachable!()
        };
        assert_eq!(params.vertices.len(), 3);
        undo_redo::undo(&mut tree);
        let SdfParams::PolygonPrismParams(params) = &objects(&tree)[0].params else {
            unreachable!()
        };
        assert_eq!(params.vertices.len(), 4);
        finish(&mut tree);
        assert!(!ui_state.delete_active_polygon_point(&ctx, &mut tree));
    }
}

use super::*;

const FACE_CUT_OVERLAP: f32 = 0.003;
const FACE_CUT_INITIAL_DEPTH: f32 = 0.04;
const CLOSE_DISTANCE: f32 = 10.0;

struct FaceFrame {
    u: Vec3,
    v: Vec3,
    normal: Vec3,
    origin: Vec3,
    domain: FaceDomain,
}

enum FaceDomain {
    Rectangle { extent_u: f32, extent_v: f32 },
    Polygon(Vec<Vec2>),
}

fn box_face_frame(face: crate::model::BoxFaceSelection, half_extents: Vec3) -> FaceFrame {
    match (face.axis, face.positive) {
        (VectorAxis::X, true) => FaceFrame {
            u: Vec3::Y,
            v: Vec3::Z,
            normal: Vec3::X,
            origin: Vec3::X * half_extents.x,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.y,
                extent_v: half_extents.z,
            },
        },
        (VectorAxis::X, false) => FaceFrame {
            u: Vec3::Y,
            v: Vec3::NEG_Z,
            normal: Vec3::NEG_X,
            origin: Vec3::NEG_X * half_extents.x,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.y,
                extent_v: half_extents.z,
            },
        },
        (VectorAxis::Y, true) => FaceFrame {
            u: Vec3::Z,
            v: Vec3::X,
            normal: Vec3::Y,
            origin: Vec3::Y * half_extents.y,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.z,
                extent_v: half_extents.x,
            },
        },
        (VectorAxis::Y, false) => FaceFrame {
            u: Vec3::Z,
            v: Vec3::NEG_X,
            normal: Vec3::NEG_Y,
            origin: Vec3::NEG_Y * half_extents.y,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.z,
                extent_v: half_extents.x,
            },
        },
        (VectorAxis::Z, true) => FaceFrame {
            u: Vec3::X,
            v: Vec3::Y,
            normal: Vec3::Z,
            origin: Vec3::Z * half_extents.z,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.x,
                extent_v: half_extents.y,
            },
        },
        (VectorAxis::Z, false) => FaceFrame {
            u: Vec3::X,
            v: Vec3::NEG_Y,
            normal: Vec3::NEG_Z,
            origin: Vec3::NEG_Z * half_extents.z,
            domain: FaceDomain::Rectangle {
                extent_u: half_extents.x,
                extent_v: half_extents.y,
            },
        },
    }
}

fn source_and_frame(
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
) -> Option<(&SdfObject, FaceFrame)> {
    let source = scene.iter().find(|object| object.uuid == face.object())?;
    let frame = match (face, &source.params) {
        (crate::model::ModelingFaceSelection::Box(face), SdfParams::BoxParams(params)) => {
            box_face_frame(face, params.box_q)
        }
        (
            crate::model::ModelingFaceSelection::PolygonPrism(selection),
            SdfParams::PolygonPrismParams(params),
        ) => match selection.face {
            crate::model::PolygonPrismFace::Cap { positive } => {
                let (v, normal, z) = if positive {
                    (Vec3::Y, Vec3::Z, params.half_depth)
                } else {
                    (Vec3::NEG_Y, Vec3::NEG_Z, -params.half_depth)
                };
                let vertices = params
                    .vertices
                    .iter()
                    .map(|point| Vec2::new(point.x, if positive { point.y } else { -point.y }))
                    .collect();
                FaceFrame {
                    u: Vec3::X,
                    v,
                    normal,
                    origin: Vec3::Z * z,
                    domain: FaceDomain::Polygon(vertices),
                }
            }
            crate::model::PolygonPrismFace::Side { edge } => {
                let a = params.vertices.get(edge).copied()?;
                let b = params
                    .vertices
                    .get((edge + 1) % params.vertices.len())
                    .copied()?;
                let delta = b - a;
                let length = delta.length();
                if length <= 0.000_01 {
                    return None;
                }
                let winding = signed_area(&params.vertices);
                let mut u = Vec3::new(delta.x / length, delta.y / length, 0.0);
                let mut normal = Vec3::new(u.y, -u.x, 0.0);
                if winding < 0.0 {
                    u = -u;
                    normal = -normal;
                }
                FaceFrame {
                    u,
                    v: normal.cross(u),
                    normal,
                    origin: Vec3::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5, 0.0),
                    domain: FaceDomain::Rectangle {
                        extent_u: length * 0.5,
                        extent_v: params.half_depth,
                    },
                }
            }
        },
        _ => return None,
    };
    Some((source, frame))
}

fn point_on_face(
    camera: &Camera,
    physical_pointer: Vec2,
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
) -> Option<Vec2> {
    let (_, frame) = source_and_frame(scene, face)?;
    let inverse = crate::model::object_world_matrix(scene, face.object()).inverse();
    let (world_origin, world_direction) = camera.ray(physical_pointer);
    let origin = inverse.transform_point3(world_origin);
    let direction = inverse.transform_vector3(world_direction);
    let denominator = direction.dot(frame.normal);
    if denominator.abs() < 0.000_01 {
        return None;
    }
    let local = origin + direction * ((frame.origin - origin).dot(frame.normal) / denominator);
    let relative = local - frame.origin;
    let point = Vec2::new(relative.dot(frame.u), relative.dot(frame.v));
    match frame.domain {
        FaceDomain::Rectangle { extent_u, extent_v } => Some(Vec2::new(
            point.x.clamp(-extent_u, extent_u),
            point.y.clamp(-extent_v, extent_v),
        )),
        FaceDomain::Polygon(vertices) => {
            (crate::model::polygon_distance(point, &vertices) <= 0.001).then_some(point)
        }
    }
}

fn point_world(
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    point: Vec2,
) -> Option<Vec3> {
    let (_, frame) = source_and_frame(scene, face)?;
    Some(
        crate::model::object_world_matrix(scene, face.object())
            .transform_point3(frame.origin + frame.u * point.x + frame.v * point.y),
    )
}

fn signed_area(vertices: &[Vec2]) -> f32 {
    vertices
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let next = vertices[(index + 1) % vertices.len()];
            point.x * next.y - next.x * point.y
        })
        .sum::<f32>()
        * 0.5
}

fn edge_side(a: Vec2, b: Vec2, point: Vec2) -> f32 {
    let edge = b - a;
    let relative = point - a;
    edge.x * relative.y - edge.y * relative.x
}

fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab_c = edge_side(a, b, c);
    let ab_d = edge_side(a, b, d);
    let cd_a = edge_side(c, d, a);
    let cd_b = edge_side(c, d, b);
    if ab_c * ab_d < 0.0 && cd_a * cd_b < 0.0 {
        return true;
    }
    let on_segment = |start: Vec2, end: Vec2, point: Vec2, side: f32| {
        side.abs() <= 0.000_001
            && point.x >= start.x.min(end.x) - 0.000_001
            && point.x <= start.x.max(end.x) + 0.000_001
            && point.y >= start.y.min(end.y) - 0.000_001
            && point.y <= start.y.max(end.y) + 0.000_001
    };
    on_segment(a, b, c, ab_c)
        || on_segment(a, b, d, ab_d)
        || on_segment(c, d, a, cd_a)
        || on_segment(c, d, b, cd_b)
}

fn polygon_is_valid(vertices: &[Vec2]) -> bool {
    if vertices.len() < 3
        || signed_area(vertices).abs() < 0.000_01
        || vertices.iter().enumerate().any(|(index, point)| {
            point.distance_squared(vertices[(index + 1) % vertices.len()]) < 0.000_001
        })
    {
        return false;
    }
    for first in 0..vertices.len() {
        let first_next = (first + 1) % vertices.len();
        for second in (first + 1)..vertices.len() {
            let second_next = (second + 1) % vertices.len();
            if first == second
                || first_next == second
                || second_next == first
                || (first == 0 && second_next == 0)
            {
                continue;
            }
            if segments_intersect(
                vertices[first],
                vertices[first_next],
                vertices[second],
                vertices[second_next],
            ) {
                return false;
            }
        }
    }
    true
}

fn create_face_shape(
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    vertices: &[Vec2],
    depth: f32,
    id: uuid::Uuid,
) -> Option<SdfObject> {
    let (source, frame) = source_and_frame(scene, face)?;
    if !polygon_is_valid(vertices) {
        return None;
    }
    let minimum = vertices
        .iter()
        .copied()
        .fold(Vec2::splat(f32::INFINITY), Vec2::min);
    let maximum = vertices
        .iter()
        .copied()
        .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
    let center = (minimum + maximum) * 0.5;
    let centered: Vec<_> = vertices.iter().map(|point| *point - center).collect();
    let signed_depth = if depth.abs() < 0.01 {
        0.01 * if depth < 0.0 { -1.0 } else { 1.0 }
    } else {
        depth
    };
    let local_origin = frame.origin
        + frame.u * center.x
        + frame.v * center.y
        + frame.normal * (signed_depth * 0.5);
    let basis = glam::Mat4::from_cols(
        frame.u.extend(0.0),
        frame.v.extend(0.0),
        frame.normal.extend(0.0),
        local_origin.extend(1.0),
    );
    let matrix = source.transform.matrix() * basis;
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    let mut shape = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    shape.uuid = id;
    shape.name = if signed_depth < 0.0 {
        "Face cut".into()
    } else {
        "Face extrusion".into()
    };
    shape.transform = crate::model::Transform {
        translation,
        rotation,
        scale,
    };
    shape.params = SdfParams::PolygonPrismParams(crate::model::PolygonPrismParams {
        vertices: centered,
        half_depth: signed_depth.abs() * 0.5 + FACE_CUT_OVERLAP,
    });
    shape.color = source.color;
    shape.material = source.material;
    shape.material_id = source.material_id;
    shape.boolean_parent = Some(source.uuid);
    shape.operation = if signed_depth < 0.0 {
        BooleanOperation::Subtract
    } else {
        BooleanOperation::Union
    };
    Some(shape)
}

impl UiState {
    pub(super) fn cancel_face_cut(&mut self, tree: &mut DataTree) {
        if let Some(FaceCutDraft {
            phase: FaceCutPhase::Depth { object, .. },
            ..
        }) = self.selection_tools.face_cut.take()
        {
            let mut scene = objects(tree);
            scene.retain(|candidate| candidate.uuid != object);
            crate::model::set_objects_transient(tree, scene);
        }
        self.selection_tools.face_cut = None;
    }

    fn finish_face_cut(&mut self, tree: &mut DataTree) {
        let Some(FaceCutDraft {
            phase: FaceCutPhase::Depth { object, .. },
            ..
        }) = self.selection_tools.face_cut.take()
        else {
            return;
        };
        set_objects(tree, objects(tree));
        crate::model::set_selected_exact(tree, vec![object]);
        tree.make_undo_redo_snapshot();
        self.selection_tools.tool = SelectionTool::Select;
    }

    pub(super) fn draw_face_cut(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
    ) {
        let scale = ui.ctx().pixels_per_point();
        let (pointer, pressed, enter, backspace, escape) = ui.input(|input| {
            (
                input.pointer.interact_pos(),
                input.pointer.primary_pressed(),
                input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Backspace),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if escape {
            self.cancel_face_cut(tree);
            self.selection_tools.tool = SelectionTool::Select;
            return;
        }
        let scene = objects(tree);
        let selected_face = crate::model::selected_modeling_face(tree);
        if self.selection_tools.face_cut.is_none() {
            let Some(face) = selected_face else {
                self.selection_tools.tool = SelectionTool::Select;
                return;
            };
            self.selection_tools.face_cut = Some(FaceCutDraft {
                face,
                vertices: Vec::new(),
                phase: FaceCutPhase::Outline,
            });
        }
        let Some(draft) = self.selection_tools.face_cut.as_mut() else {
            return;
        };
        if !scene
            .iter()
            .any(|object| object.uuid == draft.face.object())
        {
            self.cancel_face_cut(tree);
            self.selection_tools.tool = SelectionTool::Select;
            return;
        }

        let mut close_outline = enter && draft.vertices.len() >= 3;
        if backspace && matches!(draft.phase, FaceCutPhase::Outline) {
            draft.vertices.pop();
        }
        let pointer_in_view = pointer.filter(|point| {
            ui.clip_rect().contains(*point)
                && !self.regions.iter().any(|rect| rect.contains(*point))
        });
        if matches!(draft.phase, FaceCutPhase::Outline) && pressed {
            if let Some(point) = pointer_in_view {
                let first_screen = draft.vertices.first().and_then(|first| {
                    point_world(&scene, draft.face, *first)
                        .and_then(|world| camera.project(world, scale))
                });
                if draft.vertices.len() >= 3
                    && first_screen.is_some_and(|first| first.distance(point) <= CLOSE_DISTANCE)
                {
                    close_outline = true;
                } else if draft.vertices.len() < crate::model::MAX_POLYGON_PRISM_VERTICES {
                    if let Some(local) = point_on_face(
                        camera,
                        Vec2::new(point.x, point.y) * scale,
                        &scene,
                        draft.face,
                    ) {
                        draft.vertices.push(local);
                    }
                }
            }
        }

        let mut just_closed = false;
        if close_outline && matches!(draft.phase, FaceCutPhase::Outline) {
            let start_pointer = pointer.unwrap_or(ui.clip_rect().center());
            if let Some((_, frame)) = source_and_frame(&scene, draft.face) {
                let matrix = crate::model::object_world_matrix(&scene, draft.face.object());
                let center = matrix.transform_point3(frame.origin);
                let direction = matrix.transform_vector3(frame.normal);
                if let (Some(at), Some(ahead)) = (
                    camera.project(center, scale),
                    camera.project(center + direction * 0.1, scale),
                ) {
                    let mut projected_axis = (ahead - at) * 10.0;
                    if projected_axis.length_sq() < 4.0 {
                        let screen_up = camera.view().inverse().y_axis.truncate();
                        if let Some(fallback) = camera.project(center + screen_up * 0.1, scale) {
                            projected_axis = (fallback - at) * 10.0;
                        }
                    }
                    if projected_axis.length_sq() > 0.0001 {
                        let id = uuid::Uuid::new_v4();
                        if let Some(shape) = create_face_shape(
                            &scene,
                            draft.face,
                            &draft.vertices,
                            FACE_CUT_INITIAL_DEPTH,
                            id,
                        ) {
                            let mut preview = scene.clone();
                            preview.push(shape);
                            crate::model::set_objects_transient(tree, preview);
                            draft.phase = FaceCutPhase::Depth {
                                object: id,
                                start_pointer,
                                projected_axis,
                            };
                            just_closed = true;
                        }
                    }
                }
            }
        }

        let mut depth_label = None;
        if let FaceCutPhase::Depth {
            object,
            start_pointer,
            projected_axis,
        } = draft.phase
        {
            if let Some(point) = pointer {
                let depth = FACE_CUT_INITIAL_DEPTH
                    + (point - start_pointer).dot(projected_axis) / projected_axis.length_sq();
                let mut preview = objects(tree);
                preview.retain(|candidate| candidate.uuid != object);
                if let Some(shape) =
                    create_face_shape(&preview, draft.face, &draft.vertices, depth, object)
                {
                    preview.push(shape);
                    crate::model::set_objects_transient(tree, preview);
                    depth_label = Some(depth);
                }
            }
            if !just_closed && (enter || (pressed && pointer_in_view.is_some())) {
                self.finish_face_cut(tree);
                return;
            }
        }

        let scene = objects(tree);
        let points: Vec<_> = draft
            .vertices
            .iter()
            .filter_map(|point| point_world(&scene, draft.face, *point))
            .filter_map(|world| camera.project(world, scale))
            .collect();
        let stroke = Stroke::new(2.0, Color32::from_rgb(255, 190, 72));
        for segment in points.windows(2) {
            ui.painter().line_segment([segment[0], segment[1]], stroke);
        }
        if matches!(draft.phase, FaceCutPhase::Outline) {
            if let (Some(last), Some(point)) = (points.last(), pointer_in_view) {
                ui.painter().line_segment([*last, point], stroke);
            }
            for (index, point) in points.iter().enumerate() {
                ui.painter().circle_filled(
                    *point,
                    if index == 0 { 5.0 } else { 3.5 },
                    stroke.color,
                );
            }
        }
        let outline_invalid = draft.vertices.len() >= 3 && !polygon_is_valid(&draft.vertices);
        let instruction = if let Some(depth) = depth_label {
            format!(
                "{} {:.2} · click or Enter confirms · Esc cancels",
                if depth < 0.0 {
                    "Cut depth"
                } else {
                    "Extrusion"
                },
                depth.abs()
            )
        } else if outline_invalid {
            "Outline cannot cross itself · Backspace removes a point · Esc cancels".into()
        } else {
            "Click polygon points · click the first point or Enter to close · Backspace removes · Esc cancels".into()
        };
        ui.painter().text(
            ui.clip_rect().center_bottom() - egui::vec2(0.0, 16.0),
            egui::Align2::CENTER_BOTTOM,
            instruction,
            egui::FontId::proportional(13.0),
            Color32::WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_validation_accepts_concave_shapes_and_rejects_crossings() {
        let concave = [
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::ZERO,
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ];
        let crossed = [
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
            Vec2::new(1.0, -1.0),
        ];
        assert!(polygon_is_valid(&concave));
        assert!(!polygon_is_valid(&crossed));
    }

    #[test]
    fn face_shape_depth_maps_explicitly_to_union_or_subtraction() {
        let source = SdfObject::create_kind(PrimitiveKind::Box);
        let face = crate::model::BoxFaceSelection {
            object: source.uuid,
            axis: VectorAxis::Z,
            positive: true,
        };
        let modeling_face = crate::model::ModelingFaceSelection::Box(face);
        let vertices = [
            Vec2::new(-0.2, -0.2),
            Vec2::new(0.2, -0.2),
            Vec2::new(0.0, 0.2),
        ];
        let scene = [source];
        let raised =
            create_face_shape(&scene, modeling_face, &vertices, 0.2, uuid::Uuid::new_v4()).unwrap();
        let cut = create_face_shape(&scene, modeling_face, &vertices, -0.2, uuid::Uuid::new_v4())
            .unwrap();
        assert_eq!(raised.operation, BooleanOperation::Union);
        assert_eq!(cut.operation, BooleanOperation::Subtract);
        assert_eq!(raised.boolean_parent, Some(face.object));
        assert_eq!(cut.boolean_parent, Some(face.object));
    }

    #[test]
    fn polygon_caps_and_sides_can_spawn_nested_face_shapes() {
        let source = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
        let scene = [source.clone()];
        let outline = [
            Vec2::new(-0.05, -0.05),
            Vec2::new(0.05, -0.05),
            Vec2::new(0.0, 0.05),
        ];
        for face in [
            crate::model::PolygonPrismFace::Cap { positive: true },
            crate::model::PolygonPrismFace::Side { edge: 0 },
        ] {
            let selection = crate::model::ModelingFaceSelection::PolygonPrism(
                crate::model::PolygonPrismFaceSelection {
                    object: source.uuid,
                    face,
                },
            );
            let child = create_face_shape(&scene, selection, &outline, -0.05, uuid::Uuid::new_v4())
                .unwrap();
            assert_eq!(child.boolean_parent, Some(source.uuid));
            assert_eq!(child.operation, BooleanOperation::Subtract);
        }
    }
}

use super::*;
use crate::model::{
    scene_seams, CageAxis, LatticeEdge, Seam, SeamBinding, SeamPoint, SeamTarget, SheetEdge,
};

pub(super) fn selected_seam(tree: &DataTree) -> Option<uuid::Uuid> {
    let selection = selected(tree);
    (selection.len() == 1)
        .then(|| selection[0])
        .filter(|id| scene_seams(tree).seams.iter().any(|s| s.id == *id))
}
fn error_id() -> egui::Id {
    egui::Id::new("seam-edit-error")
}
fn commit(ui: &egui::Ui, tree: &mut DataTree, seams: crate::model::SceneSeams) -> bool {
    match crate::model::try_set_seams(tree, seams) {
        Ok(()) => {
            tree.make_undo_redo_snapshot();
            ui.ctx().data_mut(|d| d.remove::<String>(error_id()));
            true
        }
        Err(e) => {
            ui.ctx().data_mut(|d| d.insert_temp(error_id(), e));
            false
        }
    }
}
/// Ordinary geometry edits remain fallible when an edge connection is active.
/// Validate before touching animation tracks or taking an undo snapshot.
pub(super) fn commit_objects(
    ui: &egui::Ui,
    tree: &mut DataTree,
    mut scene: Vec<SdfObject>,
) -> bool {
    let mut variables = crate::model::scene_variables(tree);
    crate::model::evaluate_derived_variables(&mut variables);
    crate::model::apply_vector_bindings(&variables, &mut scene);
    let validation = scene
        .iter()
        .try_for_each(|o| {
            if let SdfParams::SheetPanelParams(p) = &o.params {
                p.validate()
            } else {
                Ok(())
            }
        })
        .and_then(|_| crate::model::resolve_sheet_seams(&mut scene))
        .and_then(|_| crate::model::resolve_seam_bindings(&scene_seams(tree), &mut scene));
    match validation {
        Ok(()) => {
            set_objects(tree, scene);
            ui.ctx().data_mut(|d| d.remove::<String>(error_id()));
            true
        }
        Err(e) => {
            ui.ctx().data_mut(|d| d.insert_temp(error_id(), e));
            false
        }
    }
}
fn error(ui: &mut egui::Ui) {
    if let Some(e) = ui.ctx().data(|d| d.get_temp::<String>(error_id())) {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
}
pub(super) fn list(ui: &mut egui::Ui, tree: &mut DataTree) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Seams").strong());
        if ui
            .small_button("+")
            .on_hover_text("Add a construction seam")
            .clicked()
        {
            let id = uuid::Uuid::new_v4();
            let mut seams = scene_seams(tree);
            seams.seams.push(Seam {
                id,
                name: format!("Seam {}", seams.seams.len() + 1),
                points: Vec::new(),
                closed: false,
            });
            if commit(ui, tree, seams) {
                crate::model::set_selected_exact(tree, vec![id]);
            }
        }
    });
    for seam in scene_seams(tree).seams {
        if ui
            .selectable_label(
                selected_seam(tree) == Some(seam.id),
                format!("{} · {} points", seam.name, seam.points.len()),
            )
            .clicked()
        {
            crate::model::set_selected_exact(tree, vec![seam.id]);
        }
    }
}
#[derive(Clone, Default)]
struct SurfaceTool {
    source: Option<uuid::Uuid>,
    append: bool,
}
fn tool_id(seam: uuid::Uuid) -> egui::Id {
    egui::Id::new(("seam-source", seam))
}
pub(super) fn inspector(ui: &mut egui::Ui, tree: &mut DataTree) -> bool {
    let Some(id) = selected_seam(tree) else {
        return false;
    };
    let mut seams = scene_seams(tree);
    let seam = seams.seams.iter_mut().find(|s| s.id == id).unwrap();
    ui.label(RichText::new("Seam").strong());
    let mut changed = ui.text_edit_singleline(&mut seam.name).changed();
    ui.weak("Construction line; hidden in final renders. Use --show-seams to include it.");
    ui.add_enabled_ui(seam.points.len() >= 3, |ui| {
        changed |= ui.checkbox(&mut seam.closed, "Closed loop").changed();
    });
    let mut tool = ui
        .ctx()
        .data(|d| d.get_temp::<SurfaceTool>(tool_id(id)))
        .unwrap_or_default();
    egui::ComboBox::from_id_salt(("seam-source-mesh", id))
        .selected_text(
            tool.source
                .and_then(|id| {
                    objects(tree)
                        .iter()
                        .find(|o| o.uuid == id)
                        .map(|o| o.name.clone())
                })
                .unwrap_or_else(|| "Choose source mesh".into()),
        )
        .show_ui(ui, |ui| {
            for source in objects(tree).iter().filter(|o| o.imported_mesh.is_some()) {
                ui.selectable_value(&mut tool.source, Some(source.uuid), source.display_name());
            }
        });
    ui.add_enabled_ui(tool.source.is_some(), |ui| {
        ui.checkbox(&mut tool.append, "Place surface points in viewport");
    });
    ui.weak("Drag a point to move it. Surface points stay attached to their source mesh.");
    if ui.button("Add world point").clicked() {
        let mut p = crate::model::cursor_position(tree);
        if let Some(last) = seam
            .points
            .last()
            .and_then(|p| crate::model::seam_point_position(p, &objects(tree)).ok())
        {
            if p.distance_squared(last) < 1e-12 {
                p = last + Vec3::X * 0.1;
            }
        }
        seam.points.push(SeamPoint::World(p));
        changed = true;
    }
    let mut remove = None;
    for (i, point) in seam.points.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("{}", i + 1));
            match point {
                SeamPoint::World(p) => {
                    for a in 0..3 {
                        changed |= ui
                            .add(egui::DragValue::new(&mut p[a]).speed(0.01))
                            .changed();
                    }
                }
                SeamPoint::Surface { .. } => {
                    ui.label("On mesh");
                    if ui.small_button("Detach").clicked() {
                        if let Ok(p) = crate::model::seam_point_position(point, &objects(tree)) {
                            *point = SeamPoint::World(p);
                            changed = true;
                        }
                    }
                }
            }
            if ui.small_button("×").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        seam.points.remove(i);
        if seam.points.len() < 3 {
            seam.closed = false;
        }
        changed = true;
    }
    ui.ctx().data_mut(|d| d.insert_temp(tool_id(id), tool));
    if changed {
        commit(ui, tree, seams);
    }
    if ui.button("Delete seam").clicked() {
        let mut seams = scene_seams(tree);
        seams.seams.retain(|s| s.id != id);
        seams.bindings.retain(|b| b.seam != id);
        if commit(ui, tree, seams) {
            set_selected(tree, Vec::new());
        }
    }
    if ui.button("Finish editing seam").clicked() {
        set_selected(tree, Vec::new());
    }
    error(ui);
    true
}
#[derive(Clone)]
struct BindingDraft {
    seam: Option<uuid::Uuid>,
    target: SeamTarget,
    reverse: bool,
    span: bool,
    start: usize,
    end: usize,
    tolerance: f32,
}
impl Default for BindingDraft {
    fn default() -> Self {
        Self {
            seam: None,
            target: SeamTarget::LatticeEdge(LatticeEdge {
                axis: CageAxis::X,
                sides: [false, false],
            }),
            reverse: false,
            span: false,
            start: 0,
            end: 1,
            tolerance: 0.001,
        }
    }
}
pub(super) fn connections(ui: &mut egui::Ui, tree: &mut DataTree, object: uuid::Uuid) {
    let mut seams = scene_seams(tree);
    if seams.seams.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Seam connections").strong());
    let mut remove = None;
    for (i, b) in seams
        .bindings
        .iter()
        .enumerate()
        .filter(|(_, b)| b.object == object)
    {
        ui.horizontal(|ui| {
            ui.label(format!(
                "{:?} → {}",
                b.target,
                seams
                    .seams
                    .iter()
                    .find(|s| s.id == b.seam)
                    .map(|s| s.name.as_str())
                    .unwrap_or("missing")
            ));
            if ui.small_button("Unbind").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        seams.bindings.remove(i);
        commit(ui, tree, seams.clone());
    }
    let key = egui::Id::new(("seam-binding-draft", object));
    let mut draft = ui
        .ctx()
        .data(|d| d.get_temp::<BindingDraft>(key))
        .unwrap_or_default();
    egui::ComboBox::from_id_salt(("bind-seam", object))
        .selected_text(
            draft
                .seam
                .and_then(|id| {
                    seams
                        .seams
                        .iter()
                        .find(|s| s.id == id)
                        .map(|s| s.name.as_str())
                })
                .unwrap_or("Choose seam"),
        )
        .show_ui(ui, |ui| {
            for seam in &seams.seams {
                if ui
                    .selectable_value(&mut draft.seam, Some(seam.id), &seam.name)
                    .changed()
                {
                    draft.span = seam.closed;
                    draft.start = 0;
                    draft.end = seam.points.len().saturating_sub(1);
                }
            }
        });
    let scene = objects(tree);
    let Some(owner) = scene.iter().find(|o| o.uuid == object) else {
        return;
    };
    egui::ComboBox::from_id_salt(("bind-target", object))
        .selected_text(format!("{:?}", draft.target))
        .show_ui(ui, |ui| {
            if owner.lattice.is_some() {
                for axis in [CageAxis::X, CageAxis::Y, CageAxis::Z] {
                    for a in [false, true] {
                        for b in [false, true] {
                            let edge = LatticeEdge {
                                axis,
                                sides: [a, b],
                            };
                            ui.selectable_value(
                                &mut draft.target,
                                SeamTarget::LatticeEdge(edge),
                                format!(
                                    "Lattice {:?}, sides {} / {}",
                                    axis,
                                    if a { "max" } else { "min" },
                                    if b { "max" } else { "min" }
                                ),
                            );
                        }
                    }
                }
            }
            if matches!(owner.params, SdfParams::SheetPanelParams(_)) {
                for edge in [
                    SheetEdge::MinX,
                    SheetEdge::MaxX,
                    SheetEdge::MinY,
                    SheetEdge::MaxY,
                ] {
                    ui.selectable_value(
                        &mut draft.target,
                        SeamTarget::SheetBoundary(edge),
                        format!("Sheet {:?}", edge),
                    );
                }
            }
        });
    ui.checkbox(&mut draft.reverse, "Reverse direction");
    ui.checkbox(&mut draft.span, "Use point span");
    if draft.span {
        ui.horizontal(|ui| {
            ui.label("Point indices (zero-based)");
            ui.add(egui::DragValue::new(&mut draft.start));
            ui.add(egui::DragValue::new(&mut draft.end));
        });
    }
    ui.horizontal(|ui| {
        ui.label("Tolerance");
        ui.add(
            egui::DragValue::new(&mut draft.tolerance)
                .speed(0.0001)
                .range(0.000001..=1.0),
        );
    });
    if ui
        .add_enabled(
            draft.seam.is_some(),
            egui::Button::new("Connect edge to seam"),
        )
        .clicked()
    {
        seams
            .bindings
            .retain(|b| b.object != object || b.target != draft.target);
        seams.bindings.push(SeamBinding {
            object,
            seam: draft.seam.unwrap(),
            target: draft.target,
            span: if draft.span {
                Some([draft.start, draft.end])
            } else {
                None
            },
            reverse: draft.reverse,
            tolerance: draft.tolerance,
        });
        commit(ui, tree, seams);
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, draft));
    error(ui);
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    camera: &Camera,
    regions: &mut Vec<egui::Rect>,
) {
    let mut seams = scene_seams(tree);
    let scene = objects(tree);
    claydash_engine::seam_guides::draw(ui, &seams, &scene, camera);
    let Some(id) = selected_seam(tree) else {
        return;
    };
    let tool = ui
        .ctx()
        .data(|d| d.get_temp::<SurfaceTool>(tool_id(id)))
        .unwrap_or_default();
    let blocked = ui
        .input(|i| i.pointer.hover_pos())
        .is_none_or(|p| !ui.clip_rect().contains(p) || regions.iter().any(|r| r.contains(p)));
    let seam = seams.seams.iter_mut().find(|s| s.id == id).unwrap();
    let mut changed = false;
    let mut handled = false;
    let ppp = ui.ctx().pixels_per_point();
    for i in 0..seam.points.len() {
        let Ok(world) = crate::model::seam_point_position(&seam.points[i], &scene) else {
            continue;
        };
        let Some(center) = camera.project(world, ppp) else {
            continue;
        };
        let rect = egui::Rect::from_center_size(center, egui::vec2(18.0, 18.0));
        if !ui.clip_rect().contains(center) {
            continue;
        }
        let response = ui
            .interact(
                rect,
                egui::Id::new(("seam-point", id, i)),
                egui::Sense::drag(),
            )
            .on_hover_text(format!("Seam point {}", i + 1));
        ui.painter().circle_filled(center, 4.0, Color32::RED);
        handled |= response.hovered() || response.dragged();
        if response.dragged() && !blocked {
            if let Some(pointer) = response.interact_pointer_pos() {
                let (o, d) = camera.ray(Vec2::new(pointer.x, pointer.y) * ppp);
                match seam.points[i] {
                    SeamPoint::Surface { object, .. } => {
                        if let Some(point) = crate::model::seam_surface_hit(&scene, object, o, d) {
                            seam.points[i] = point;
                            changed = true;
                        }
                    }
                    SeamPoint::World(_) => {
                        let normal = camera.target - camera.position;
                        let denominator = d.dot(normal);
                        if denominator.abs() > 1e-8 {
                            seam.points[i] =
                                SeamPoint::World(o + d * (world - o).dot(normal) / denominator);
                            changed = true;
                        }
                    }
                }
            }
        }
    }
    if tool.append && !blocked && !handled && ui.input(|i| i.pointer.primary_clicked()) {
        if let (Some(source), Some(pointer)) = (tool.source, ui.input(|i| i.pointer.interact_pos()))
        {
            let (o, d) = camera.ray(Vec2::new(pointer.x, pointer.y) * ppp);
            if let Some(point) = crate::model::seam_surface_hit(&scene, source, o, d) {
                seam.points.push(point);
                changed = true;
            }
        }
    }
    if changed {
        match crate::model::try_set_seams(tree, seams) {
            Ok(()) => {
                ui.ctx().data_mut(|d| d.remove::<String>(error_id()));
                if !ui.input(|i| i.pointer.primary_down()) {
                    tree.make_undo_redo_snapshot();
                }
            }
            Err(e) => {
                ui.ctx().data_mut(|d| d.insert_temp(error_id(), e));
            }
        }
    }
    if ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
        tree.make_undo_redo_snapshot();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        tree: &mut DataTree,
        camera: &Camera,
        events: Vec<egui::Event>,
    ) -> bool {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 700.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ui.set_clip_rect(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 700.),
                ));
                draw(ui, tree, camera, &mut Vec::new());
            },
        );
        let red_line=output.shapes.iter().any(|shape|matches!(&shape.shape,egui::epaint::Shape::LineSegment{stroke,..}if stroke.color==Color32::RED));
        output.textures_delta.clear();
        red_line
    }
    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn surface_points_form_a_red_polyline_and_drag_without_creating_geometry() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        let source = object.uuid;
        object.imported_mesh = Some(std::sync::Arc::new(
            crate::renderer::poisson_mesh::geometry::Mesh::from_parts(
                vec![
                    Vec3::new(-1., -1., 0.),
                    Vec3::new(1., -1., 0.),
                    Vec3::new(1., 1., 0.),
                    Vec3::new(-1., 1., 0.),
                ],
                vec![[0, 1, 2], [0, 2, 3]],
                vec![source; 2],
            ),
        ));
        set_objects(&mut tree, vec![object]);
        let original = serde_json::to_value(objects(&tree)).unwrap();
        let id = uuid::Uuid::new_v4();
        crate::model::try_set_seams(
            &mut tree,
            crate::model::SceneSeams {
                seams: vec![Seam {
                    id,
                    name: "Guide".into(),
                    points: Vec::new(),
                    closed: false,
                }],
                bindings: Vec::new(),
            },
        )
        .unwrap();
        crate::model::set_selected_exact(&mut tree, vec![id]);
        ctx.data_mut(|d| {
            d.insert_temp(
                tool_id(id),
                SurfaceTool {
                    source: Some(source),
                    append: true,
                },
            )
        });
        let mut camera = Camera::new();
        camera.position = Vec3::new(0., 0., 3.);
        camera.target = Vec3::ZERO;
        camera.viewport = Vec2::new(800., 700.);
        camera.viewport_origin = Vec2::ZERO;
        frame(&ctx, &mut tree, &camera, Vec::new());
        for point in [Vec3::new(-0.5, 0., 0.), Vec3::new(0.5, 0., 0.)] {
            let p = camera.project(point, 1.).unwrap();
            frame(
                &ctx,
                &mut tree,
                &camera,
                vec![egui::Event::PointerMoved(p), button(p, true)],
            );
            frame(&ctx, &mut tree, &camera, vec![button(p, false)]);
        }
        assert_eq!(scene_seams(&tree).seams[0].points.len(), 2);
        assert!(frame(&ctx, &mut tree, &camera, Vec::new()));
        let a = camera.project(Vec3::new(-0.5, 0., 0.), 1.).unwrap();
        let b = camera.project(Vec3::new(-0.4, 0.2, 0.), 1.).unwrap();
        frame(
            &ctx,
            &mut tree,
            &camera,
            vec![egui::Event::PointerMoved(a), button(a, true)],
        );
        frame(&ctx, &mut tree, &camera, vec![egui::Event::PointerMoved(b)]);
        frame(&ctx, &mut tree, &camera, vec![button(b, false)]);
        let point = scene_seams(&tree).seams[0]
            .positions(&objects(&tree))
            .unwrap()[0];
        assert!(point.distance(Vec3::new(-0.4, 0.2, 0.)) < 1e-4);
        assert_eq!(serde_json::to_value(objects(&tree)).unwrap(), original);
    }
}

#[cfg(test)]
#[test]
fn removing_a_connected_lattice_preserves_geometry_and_animation_on_failure() {
    use crate::model::{
        AnimatableProperty, AnimationBinding, AnimationData, AnimationTrack, Lattice, SceneSeams,
    };
    let ctx = egui::Context::default();
    let mut tree = DataTree::default();
    let mut runtime = AnimationRuntime::default();
    let mut object = SdfObject::create_kind(PrimitiveKind::Box);
    let id = object.uuid;
    object.lattice = Some(Lattice::new(Vec3::splat(-1.), Vec3::ONE, 3));
    set_objects(&mut tree, vec![object]);
    crate::model::set_selected_exact(&mut tree, vec![id]);
    let seam = Seam {
        id: uuid::Uuid::new_v4(),
        name: "Edge".into(),
        points: vec![
            SeamPoint::World(Vec3::new(-1., -1., -1.)),
            SeamPoint::World(Vec3::new(-1., 1., -1.)),
        ],
        closed: false,
    };
    crate::model::try_set_seams(
        &mut tree,
        SceneSeams {
            bindings: vec![SeamBinding {
                object: id,
                seam: seam.id,
                target: SeamTarget::LatticeEdge(LatticeEdge {
                    axis: CageAxis::Y,
                    sides: [false, false],
                }),
                span: None,
                reverse: false,
                tolerance: 0.001,
            }],
            seams: vec![seam],
        },
    )
    .unwrap();
    tree.set_path(
        "scene.animation",
        ClaydashValue::Animation(AnimationData {
            tracks: vec![AnimationTrack {
                binding: AnimationBinding {
                    object: id,
                    property: AnimatableProperty::LatticeShape,
                },
                keyframes: Vec::new(),
            }],
            ..Default::default()
        }),
    );
    let mut frame = |tree: &mut DataTree, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 800.),
                )),
                events,
                ..Default::default()
            },
            |ui| super::modifiers_panel(ui, tree, &mut runtime),
        );
        output.textures_delta.clear();
        output.shapes
    };
    let shapes = frame(&mut tree, Vec::new());
    let before = crate::document::serialize_scene(&tree).unwrap();
    let position = shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Remove" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap();
    for pressed in [true, false] {
        frame(
            &mut tree,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    assert!(ctx.data(|d| d.get_temp::<String>(error_id())).is_some());
    assert_eq!(crate::document::serialize_scene(&tree).unwrap(), before);
}

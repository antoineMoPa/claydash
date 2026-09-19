use super::*;
use crate::model::{ClaydashValue, EditorState, Transform};

#[derive(Default, PartialEq, Eq)]
enum SelectionTool {
    #[default]
    Select,
    Box,
}

struct MoveGesture {
    start: Vec3,
    center: Vec3,
    objects: Vec<(uuid::Uuid, Transform)>,
}

enum Gesture {
    Move(MoveGesture),
    Box {
        start: egui::Pos2,
        end: egui::Pos2,
        additive: bool,
    },
}

#[derive(Default)]
pub(super) struct SelectionTools {
    tool: SelectionTool,
    gesture: Option<Gesture>,
}

impl SelectionTools {
    pub fn active(&self) -> bool {
        self.gesture.is_some()
    }
    pub fn box_mode(&self) -> bool {
        self.tool == SelectionTool::Box
    }
}

fn box_selection(
    scene: &[SdfObject],
    camera: &Camera,
    rect: egui::Rect,
    scale: f32,
    mut ids: Vec<uuid::Uuid>,
) -> Vec<uuid::Uuid> {
    for object in scene {
        if (object.transform.translation - camera.position).dot(camera.target - camera.position)
            <= 0.0
        {
            continue;
        }
        if camera
            .project(object.transform.translation, scale)
            .is_some_and(|point| rect.contains(point))
        {
            let id = scene_actions::viewport_group_root(scene, object.uuid);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

impl UiState {
    pub fn selection_gesture_active(&self) -> bool {
        self.selection_tools.active()
    }

    pub fn box_selection_enabled(&self, tree: &DataTree) -> bool {
        self.selection_tools.box_mode()
            && scene_actions::pending_boolean(tree).is_none()
            && matches!(
                tree.get_path("editor.state"),
                ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
            )
    }

    pub fn enter_box_selection_mode(&mut self, tree: &DataTree) -> bool {
        if self.selection_tools.active()
            || scene_actions::pending_boolean(tree).is_some()
            || !matches!(
                tree.get_path("editor.state"),
                ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
            )
        {
            return false;
        }
        self.selection_tools.tool = SelectionTool::Box;
        true
    }

    pub(super) fn draw_selection_toolbar(&mut self, ctx: &egui::Context, viewport: egui::Rect) {
        let area = egui::Area::new("selection-tools".into())
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(viewport.right_bottom() - egui::vec2(6.0, 6.0))
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport);
                ui.add_enabled_ui(!self.selection_tools.active(), |ui| {
                    ui.horizontal(|ui| {
                        ui.set_height(26.0);
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (tool, source, tooltip) in [
                            (
                                SelectionTool::Select,
                                egui::include_image!(
                                    "../../assets/icons/lucide/mouse-pointer-2.svg"
                                ),
                                "Select objects",
                            ),
                            (
                                SelectionTool::Box,
                                egui::include_image!("../../assets/icons/lucide/scan.svg"),
                                "Box select (B) · drag a rectangle · Shift adds",
                            ),
                        ] {
                            if selectable_view_button(
                                ui,
                                source,
                                tooltip,
                                self.selection_tools.tool == tool,
                            )
                            .clicked()
                            {
                                self.selection_tools.tool = tool;
                            }
                        }
                    });
                });
            });
        self.regions.push(area.response.rect);
    }

    pub(super) fn draw_selection_tools(
        &mut self,
        ui: &mut egui::Ui,
        tree: &mut DataTree,
        camera: &Camera,
    ) {
        let scale = ui.ctx().pixels_per_point();
        let (pointer, pressed, released, cancel, additive, navigating) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.key_pressed(egui::Key::Escape) || !i.focused,
                i.modifiers.shift,
                i.modifiers.ctrl || i.pointer.secondary_down(),
            )
        });
        if cancel {
            if let Some(Gesture::Move(session)) = self.selection_tools.gesture.take() {
                let mut scene = objects(tree);
                for object in &mut scene {
                    if let Some((_, initial)) =
                        session.objects.iter().find(|(id, _)| *id == object.uuid)
                    {
                        object.transform = *initial;
                    }
                }
                set_objects(tree, scene);
            }
            self.selection_tools.gesture = None;
            return;
        }
        if !matches!(
            tree.get_path("editor.state"),
            ClaydashValue::None | ClaydashValue::EditorState(EditorState::Start)
        ) {
            return;
        }
        let scene = objects(tree);
        let selection = selected(tree);
        let ids = commands::effective_selected_ids(tree);
        let selected_objects: Vec<_> = scene.iter().filter(|o| ids.contains(&o.uuid)).collect();
        if !self.selection_tools.box_mode() && !selected_objects.is_empty() {
            let center = selected_objects
                .iter()
                .map(|o| o.transform.translation)
                .sum::<Vec3>()
                / selected_objects.len() as f32;
            let points: Vec<_> = selected_objects
                .iter()
                .flat_map(|o| {
                    std::iter::once(o.transform.translation)
                        .chain(resize_handles(o, camera).into_iter().map(|h| h.world))
                })
                .filter_map(|p| camera.project(p, scale))
                .collect();
            if let Some(first) = points.first() {
                let mut bounds = egui::Rect::from_min_max(*first, *first);
                for point in &points {
                    bounds.extend_with(*point);
                }
                // Equal screen-space offsets keep the handle on a 45° diagonal,
                // beyond the top-right extent of the projected selection.
                let offset = bounds.width().max(bounds.height()) * 0.5 + 24.0;
                let position = bounds.center() + egui::vec2(offset, -offset);
                let hit = egui::Rect::from_center_size(position, egui::vec2(28.0, 28.0));
                if ui.clip_rect().contains_rect(hit)
                    && !self.regions.iter().any(|r| r.intersects(hit))
                {
                    let response = ui
                        .interact(hit, egui::Id::new("move-selection"), egui::Sense::hover())
                        .on_hover_text("Drag to move · Esc cancels");
                    let stroke = Stroke::new(
                        if response.hovered() || self.selection_tools.active() {
                            2.5
                        } else {
                            1.5
                        },
                        axis_color(2),
                    );
                    for axis in [egui::vec2(9.0, 0.0), egui::vec2(0.0, 9.0)] {
                        ui.painter()
                            .line_segment([position - axis, position + axis], stroke);
                        for sign in [-1.0, 1.0] {
                            let tip = position + axis * sign;
                            let side = egui::vec2(-axis.y, axis.x) / 3.0;
                            ui.painter()
                                .line_segment([tip, tip - axis * sign / 3.0 + side], stroke);
                            ui.painter()
                                .line_segment([tip, tip - axis * sign / 3.0 - side], stroke);
                        }
                    }
                    if pressed
                        && !navigating
                        && pointer.is_some_and(|p| hit.contains(p))
                        && !self.selection_tools.active()
                    {
                        let p = pointer.unwrap();
                        self.selection_tools.gesture = Some(Gesture::Move(MoveGesture {
                            start: camera.cursor_on_plane(Vec2::new(p.x, p.y) * scale, center),
                            center,
                            objects: selected_objects
                                .iter()
                                .map(|o| (o.uuid, o.transform))
                                .collect(),
                        }));
                    }
                    self.regions.push(hit);
                }
            }
        }
        if self.selection_tools.box_mode()
            && !egui::Popup::is_any_open(ui.ctx())
            && pressed
            && !navigating
            && !self.selection_tools.active()
        {
            if let Some(p) = pointer.filter(|p| {
                ui.clip_rect().contains(*p) && !self.regions.iter().any(|r| r.contains(*p))
            }) {
                self.selection_tools.gesture = Some(Gesture::Box {
                    start: p,
                    end: p,
                    additive,
                });
            }
        }
        match &mut self.selection_tools.gesture {
            Some(Gesture::Move(session)) => {
                if let Some(p) = pointer {
                    let delta = camera.cursor_on_plane(Vec2::new(p.x, p.y) * scale, session.center)
                        - session.start;
                    let mut scene = scene.clone();
                    for object in &mut scene {
                        if let Some((_, initial)) =
                            session.objects.iter().find(|(id, _)| *id == object.uuid)
                        {
                            object.transform.translation = initial.translation + delta;
                        }
                    }
                    set_objects(tree, scene);
                }
                if released {
                    tree.make_undo_redo_snapshot();
                }
            }
            Some(Gesture::Box {
                start,
                end,
                additive,
            }) => {
                if let Some(p) = pointer {
                    *end = ui.clip_rect().clamp(p);
                }
                let rect = egui::Rect::from_two_pos(*start, *end);
                let color = ui.visuals().selection.bg_fill;
                ui.painter()
                    .rect_filled(rect, 0.0, color.gamma_multiply(0.15));
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
                if released {
                    // Match UI coordinates so the click tolerance stays consistent
                    // across display scales. Exactly 3 px remains a box gesture.
                    if start.distance(*end) < 3.0 {
                        self.selection_tools.tool = SelectionTool::Select;
                        crate::interactions::InteractionState::select_at(
                            camera,
                            tree,
                            Vec2::new(end.x, end.y) * scale,
                            self.ghosts.pick(*end),
                            *additive,
                        );
                    } else {
                        set_selected(
                            tree,
                            box_selection(
                                &scene,
                                camera,
                                rect,
                                scale,
                                if *additive { selection } else { vec![] },
                            ),
                        );
                    }
                }
            }
            None => {}
        }
        if released {
            self.selection_tools.gesture = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::ProjectionMode;

    fn camera() -> Camera {
        let mut camera = Camera::new();
        camera.position = Vec3::new(0.0, 0.0, 8.0);
        camera.viewport = Vec2::new(800.0, 600.0);
        camera
    }

    #[test]
    fn box_selection_shortcut_enters_mode_only_while_idle() {
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        tree.set_path(
            "editor.state",
            ClaydashValue::EditorState(EditorState::Start),
        );

        assert!(state.enter_box_selection_mode(&tree));
        assert!(state.selection_tools.box_mode());

        state.selection_tools.tool = SelectionTool::Select;
        tree.set_path(
            "editor.state",
            ClaydashValue::EditorState(EditorState::Grabbing),
        );
        assert!(!state.enter_box_selection_mode(&tree));
        assert!(!state.selection_tools.box_mode());
    }

    #[test]
    fn box_select_handles_scale_direction_groups_and_hidden_origins() {
        let mut camera = camera();
        let root = SdfObject::create(sdf_consts::TYPE_BOX);
        let mut child = SdfObject::create(sdf_consts::TYPE_SPHERE);
        child.boolean_parent = Some(root.uuid);
        let other = SdfObject::create(sdf_consts::TYPE_BOX);
        let mut behind = SdfObject::create(sdf_consts::TYPE_BOX);
        behind.transform.translation = Vec3::new(0.0, 0.0, 10.0);
        let scene = vec![root.clone(), child, other.clone(), behind];
        for mode in [ProjectionMode::Perspective, ProjectionMode::Orthographic] {
            camera.projection_mode = mode;
            for scale in [1.0, 2.0] {
                for (a, b) in [
                    (egui::pos2(350.0, 250.0), egui::pos2(450.0, 350.0)),
                    (egui::pos2(450.0, 250.0), egui::pos2(350.0, 350.0)),
                ] {
                    for (start, end) in [(a, b), (b, a)] {
                        let rect = egui::Rect::from_two_pos(start / scale, end / scale);
                        assert_eq!(
                            box_selection(&scene, &camera, rect, scale, vec![]),
                            vec![root.uuid, other.uuid]
                        );
                        assert_eq!(
                            box_selection(&scene, &camera, rect, scale, vec![other.uuid]),
                            vec![other.uuid, root.uuid]
                        );
                    }
                }
            }
        }
        let empty = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(10.0, 10.0));
        assert!(box_selection(&scene, &camera, empty, 1.0, vec![]).is_empty());
        assert_eq!(
            box_selection(&scene, &camera, empty, 1.0, vec![root.uuid]),
            vec![root.uuid]
        );
    }

    #[test]
    fn movement_stays_in_camera_plane_in_both_projections() {
        let mut camera = camera();
        for mode in [ProjectionMode::Perspective, ProjectionMode::Orthographic] {
            camera.projection_mode = mode;
            let start = camera.cursor_on_plane(Vec2::new(400.0, 300.0), Vec3::ZERO);
            let end = camera.cursor_on_plane(Vec2::new(530.0, 240.0), Vec3::ZERO);
            assert!((end - start).dot(camera.target - camera.position).abs() < 0.0001);
            assert!(end.x > start.x && end.y > start.y);
        }
    }

    fn draw(
        state: &mut UiState,
        ctx: &egui::Context,
        tree: &mut DataTree,
        camera: &Camera,
        events: Vec<egui::Event>,
    ) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| {
                state.regions.clear();
                state.draw_selection_tools(ui, tree, camera);
            },
        );
        output.textures_delta.clear();
    }

    #[test]
    fn box_gesture_commits_on_release_and_escape_keeps_selection() {
        let ctx = egui::Context::default();
        let camera = camera();
        let mut state = UiState::default();
        state.selection_tools.tool = SelectionTool::Box;
        let mut tree = DataTree::default();
        let object = SdfObject::create(sdf_consts::TYPE_BOX);
        set_objects(&mut tree, vec![object.clone()]);
        let start = egui::pos2(300.0, 200.0);
        let end = egui::pos2(500.0, 400.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::default(),
        };
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        assert!(state.selection_gesture_active());
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![egui::Event::PointerMoved(end)],
        );
        assert!(selected(&tree).is_empty());
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![button(end, false)],
        );
        assert_eq!(selected(&tree), vec![object.uuid]);
        assert!(state.selection_tools.box_mode());
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
        );
        draw(
            &mut state,
            &ctx,
            &mut tree,
            &camera,
            vec![button(start, false)],
        );
        assert_eq!(selected(&tree), vec![object.uuid]);
        assert!(!state.selection_gesture_active());
    }

    #[test]
    fn tiny_box_returns_to_select_and_background_click_clears_selection() {
        for (delta, tiny) in [
            (egui::vec2(0.0, 0.0), true),
            (egui::vec2(2.9, 0.0), true),
            (egui::vec2(-2.0, -2.0), true),
            (egui::vec2(3.0, 0.0), false),
            (egui::vec2(0.0, 3.1), false),
        ] {
            let ctx = egui::Context::default();
            let camera = camera();
            let mut state = UiState::default();
            state.selection_tools.tool = SelectionTool::Box;
            let mut tree = DataTree::default();
            let object = SdfObject::create(sdf_consts::TYPE_BOX);
            set_objects(&mut tree, vec![object.clone()]);
            set_selected(&mut tree, vec![object.uuid]);
            let start = egui::pos2(100.0, 100.0);
            let end = start + delta;
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            };
            draw(
                &mut state,
                &ctx,
                &mut tree,
                &camera,
                vec![egui::Event::PointerMoved(start), button(start, true)],
            );
            assert!(state.selection_tools.box_mode());
            draw(
                &mut state,
                &ctx,
                &mut tree,
                &camera,
                vec![egui::Event::PointerMoved(end), button(end, false)],
            );
            assert_eq!(state.selection_tools.box_mode(), !tiny);
            assert!(!state.selection_gesture_active());
            assert!(selected(&tree).is_empty());
        }
    }

    #[test]
    fn tiny_box_picks_object_like_regular_click_with_shift() {
        for shift in [false, true] {
            let ctx = egui::Context::default();
            let camera = camera();
            let mut state = UiState::default();
            state.selection_tools.tool = SelectionTool::Box;
            let mut tree = DataTree::default();
            let target = SdfObject::create(sdf_consts::TYPE_BOX);
            let mut other = SdfObject::create(sdf_consts::TYPE_BOX);
            other.transform.translation = Vec3::X * 3.0;
            set_objects(&mut tree, vec![target.clone(), other.clone()]);
            set_selected(&mut tree, vec![other.uuid]);
            let start = egui::pos2(400.0, 300.0);
            state.selection_tools.gesture = Some(Gesture::Box {
                start,
                end: start,
                additive: shift,
            });
            draw(
                &mut state,
                &ctx,
                &mut tree,
                &camera,
                vec![
                    egui::Event::PointerMoved(start + egui::vec2(1.0, 0.0)),
                    egui::Event::PointerButton {
                        pos: start + egui::vec2(1.0, 0.0),
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            assert!(!state.selection_tools.box_mode());
            assert_eq!(
                selected(&tree),
                if shift {
                    vec![other.uuid, target.uuid]
                } else {
                    vec![target.uuid]
                }
            );
        }
    }

    #[test]
    fn move_gesture_cancel_and_commit_preserve_children_and_undo() {
        let ctx = egui::Context::default();
        let camera = camera();
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let root = SdfObject::create(sdf_consts::TYPE_BOX);
        let mut child = SdfObject::create(sdf_consts::TYPE_SPHERE);
        child.boolean_parent = Some(root.uuid);
        child.transform.translation = Vec3::X;
        let originals = vec![root.clone(), child.clone()];
        set_objects(&mut tree, originals.clone());
        set_selected(&mut tree, vec![root.uuid, child.uuid]);
        tree.make_undo_redo_snapshot();
        for cancel in [true, false] {
            draw(&mut state, &ctx, &mut tree, &camera, vec![]);
            let handle = state.regions.last().expect("move handle").center();
            draw(
                &mut state,
                &ctx,
                &mut tree,
                &camera,
                vec![
                    egui::Event::PointerMoved(handle),
                    egui::Event::PointerButton {
                        pos: handle,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
            assert!(state.selection_gesture_active());
            let destination = handle + egui::vec2(80.0, 0.0);
            draw(
                &mut state,
                &ctx,
                &mut tree,
                &camera,
                vec![egui::Event::PointerMoved(destination)],
            );
            let moved = objects(&tree);
            assert!(moved[0].transform.translation.x > 0.0);
            assert!(
                (moved[1].transform.translation - moved[0].transform.translation - Vec3::X)
                    .length()
                    < 0.0001
            );
            let event = if cancel {
                egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::default(),
                }
            } else {
                egui::Event::PointerButton {
                    pos: destination,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }
            };
            draw(&mut state, &ctx, &mut tree, &camera, vec![event]);
            assert!(!state.selection_gesture_active());
            if !cancel {
                tree.undo();
            } else {
                draw(
                    &mut state,
                    &ctx,
                    &mut tree,
                    &camera,
                    vec![
                        egui::Event::PointerButton {
                            pos: destination,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::default(),
                        },
                        egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                );
            }
            assert_eq!(
                objects(&tree)[0].transform.translation,
                root.transform.translation
            );
            assert_eq!(
                objects(&tree)[1].transform.translation,
                child.transform.translation
            );
            if !cancel {
                tree.redo();
                assert_eq!(
                    objects(&tree)[0].transform.translation,
                    moved[0].transform.translation
                );
            }
        }
    }
}

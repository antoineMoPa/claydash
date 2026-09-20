use super::*;

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
        let root_world =
            crate::model::object_world_matrix(&moved, root.uuid).transform_point3(Vec3::ZERO);
        let child_world =
            crate::model::object_world_matrix(&moved, child.uuid).transform_point3(Vec3::ZERO);
        assert!(root_world.x > 0.0);
        assert!((child_world - root_world - Vec3::X).length() < 0.0001);
        assert_eq!(moved[0].transform, root.transform);
        assert_eq!(moved[1].transform, child.transform);
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
        assert_eq!(objects(&tree)[0].group_transform, root.group_transform);
        if !cancel {
            tree.redo();
            assert_eq!(
                objects(&tree)[0].transform.translation,
                moved[0].transform.translation
            );
        }
    }
}

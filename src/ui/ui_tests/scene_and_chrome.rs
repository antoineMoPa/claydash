    fn scene_frame(
        ctx: &egui::Context,
        tree: &mut DataTree,
        mut events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> Vec<egui::epaint::ClippedShape> {
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                ui.set_width(350.0);
                scene_panel(ui, tree);
            },
        );
        output.textures_delta.clear();
        output.shapes
    }

    fn click_scene_text(
        ctx: &egui::Context,
        tree: &mut DataTree,
        label: &str,
        modifiers: egui::Modifiers,
    ) {
        let shapes = scene_frame(ctx, tree, vec![], modifiers);
        let position = shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing scene control: {label}"));
        for pressed in [true, false] {
            scene_frame(
                ctx,
                tree,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers,
                    },
                ],
                modifiers,
            );
        }
    }

    #[test]
    fn scene_clicks_apply_subtraction_to_first_selected_target_and_can_detach() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let mut target = SdfObject::create(sdf_consts::TYPE_BOX);
        target.name = "Target shape".into();
        let mut cutter = SdfObject::create(sdf_consts::TYPE_SPHERE);
        cutter.name = "Cutter shape".into();
        let target_id = target.uuid;
        let cutter_id = cutter.uuid;
        // Reverse storage order proves the selection order determines the target.
        set_objects(&mut tree, vec![cutter, target]);
        tree.make_undo_redo_snapshot();
        click_scene_text(&ctx, &mut tree, "Target shape", egui::Modifiers::NONE);
        click_scene_text(&ctx, &mut tree, "Cutter shape", egui::Modifiers::SHIFT);
        assert_eq!(selected(&tree), vec![target_id, cutter_id]);
        click_scene_text(&ctx, &mut tree, "Subtract", egui::Modifiers::NONE);
        let scene = objects(&tree);
        let cutter = scene
            .iter()
            .find(|object| object.uuid == cutter_id)
            .unwrap();
        assert_eq!(cutter.boolean_parent, Some(target_id));
        assert_eq!(cutter.operation, BooleanOperation::Subtract);
        assert!(crate::model::scene_sample(Vec3::ZERO, &scene).unwrap().0 > 0.0);
        edit_boolean_operand(&mut tree, cutter_id, None);
        assert!(objects(&tree)
            .iter()
            .all(|object| object.boolean_parent.is_none()));
        undo_redo::undo(&mut tree);
        assert_eq!(
            objects(&tree)
                .iter()
                .find(|object| object.uuid == cutter_id)
                .unwrap()
                .boolean_parent,
            Some(target_id)
        );
    }

    #[test]
    fn tree_drag_opens_operation_picker_and_subtracts_on_click() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let mut target = SdfObject::create_kind(PrimitiveKind::Box);
        target.name = "Drop target".into();
        let mut source = SdfObject::create_kind(PrimitiveKind::Sphere);
        source.name = "Dragged cutter".into();
        let target_id = target.uuid;
        let source_id = source.uuid;
        set_objects(&mut tree, vec![target, source]);
        tree.make_undo_redo_snapshot();
        scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        let shapes = scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        let position = |label: &str| {
            shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .unwrap()
        };
        let start = position("Dragged cutter");
        let end = position("Drop target");
        scene_frame(
            &ctx,
            &mut tree,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
        );
        scene_frame(
            &ctx,
            &mut tree,
            vec![egui::Event::PointerMoved(start + egui::vec2(15.0, 0.0))],
            egui::Modifiers::NONE,
        );
        scene_frame(
            &ctx,
            &mut tree,
            vec![egui::Event::PointerMoved(end)],
            egui::Modifiers::NONE,
        );
        scene_frame(
            &ctx,
            &mut tree,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
        );
        scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        assert_eq!(
            objects(&tree)[1].boolean_parent,
            None,
            "dropping awaits an explicit operation"
        );
        click_scene_text(&ctx, &mut tree, "−  Subtract", egui::Modifiers::NONE);
        let scene = objects(&tree);
        let cutter = scene
            .iter()
            .find(|object| object.uuid == source_id)
            .unwrap();
        assert_eq!(cutter.boolean_parent, Some(target_id));
        assert_eq!(cutter.operation, BooleanOperation::Subtract);
    }

    #[test]
    fn minus_menu_creates_a_cutter_with_left_clicks() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let target = SdfObject::create_kind(PrimitiveKind::Box);
        let target_id = target.uuid;
        set_objects(&mut tree, vec![target]);
        tree.make_undo_redo_snapshot();
        scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        click_scene_text(&ctx, &mut tree, "−", egui::Modifiers::NONE);
        scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        click_scene_text(&ctx, &mut tree, "Sphere", egui::Modifiers::NONE);
        let scene = objects(&tree);
        assert_eq!(scene.len(), 2);
        assert_eq!(scene[1].boolean_parent, Some(target_id));
        assert_eq!(scene[1].operation, BooleanOperation::Subtract);
        assert_eq!(selected(&tree), vec![scene[1].uuid]);
    }

    #[test]
    fn labels_avoid_overlays_and_viewport_edges() {
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 200.0));
        let occupied = [egui::Rect::from_min_max(
            egui::pos2(150.0, 50.0),
            egui::pos2(300.0, 150.0),
        )];
        let label = gizmo_label_rect(
            viewport,
            egui::pos2(140.0, 100.0),
            egui::vec2(90.0, 20.0),
            &occupied,
        )
        .unwrap();
        assert!(viewport.contains_rect(label));
        assert!(!label.intersects(occupied[0]));
    }

    #[test]
    fn viewport_toolbars_do_not_overlap_when_narrow() {
        let ctx = egui::Context::default();
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let mut camera = Camera::new();
        for width in [500.0, 300.0, 200.0, 140.0] {
            state.viewport_rect = Some(egui::Rect::from_min_size(
                egui::pos2(100.0, 40.0),
                egui::vec2(width, 500.0),
            ));
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        state.regions.clear();
                        state.draw_top_controls(ui.ctx(), &mut tree, &mut camera);
                    },
                );
                output.textures_delta.clear();
                assert_eq!(state.regions.len(), 3);
                assert!(
                    !state.regions[0].intersects(state.regions[1])
                        && !state.regions[0].intersects(state.regions[2])
                        && !state.regions[1].intersects(state.regions[2]),
                    "overlapping toolbars at width {width}: {:?}",
                    state.regions
                );
            }
        }
    }

    #[test]
    fn toolbar_button_size_stays_fixed_on_hover_press_and_selection() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let mut rect = egui::Rect::NOTHING;
        for pass in 0..10 {
            let events = match pass {
                2 | 3 => vec![egui::Event::PointerMoved(rect.center())],
                4 => vec![egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                5 => vec![egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                8 => vec![egui::Event::PointerMoved(egui::pos2(500.0, 500.0))],
                _ => vec![],
            };
            let previous = rect;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    rect = selectable_view_button(
                        ui,
                        primitive_icon_source(PrimitiveKind::Box),
                        "Box",
                        pass >= 6,
                    )
                    .rect;
                },
            );
            output.textures_delta.clear();
            assert_eq!(rect.size(), egui::vec2(26.0, 26.0), "pass {pass}");
            if pass > 0 {
                assert_eq!(rect, previous, "pass {pass}");
            }
        }
    }

    #[test]
    fn compact_toolbar_buttons_and_top_right_isometric_control() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let response = view_button(ui, primitive_icon_source(PrimitiveKind::Box), "Add Box");
            assert_eq!(response.rect.width(), 26.0);
            assert_eq!(response.rect.width(), response.rect.height());
        });
        output.textures_delta.clear();
        let mut state = UiState::default();
        state.viewport_rect = Some(egui::Rect::from_min_size(
            egui::pos2(100.0, 40.0),
            egui::vec2(600.0, 500.0),
        ));
        let mut tree = DataTree::default();
        let mut camera = Camera::new();
        for pass in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(pass as f64),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    state.regions.clear();
                    state.draw_top_controls(ui.ctx(), &mut tree, &mut camera);
                    state.draw_view_gizmo(ui.ctx(), &mut camera);
                },
            );
            output.textures_delta.clear();
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if ["Isometric", "Perspective", "Orthographic"]
                            .contains(&text.galley.text()) =>
                    {
                        Some(text.pos)
                    }
                    _ => None,
                })
                .collect();
            if pass == 2 {
                assert!(labels.is_empty(), "view controls should be icon-only");
                let circles: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill == Color32::from_black_alpha(165)
                                && state.regions[1].contains_rect(rect.rect) =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(circles.len(), 2);
                for circle in &circles {
                    assert_eq!(circle.width(), circle.height());
                    assert!(state.regions[1].contains_rect(*circle));
                }
                assert!(
                    (circles[0].center().y - circles[1].center().y).abs() < 2.0,
                    "top-right controls should be side by side"
                );
                assert!(circles[0].right() < circles[1].left());
                assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Rect(rect) if state.regions[2].contains_rect(rect.rect) && rect.rect.width() > 80.0 && rect.fill != Color32::TRANSPARENT
                )), "bottom-left gizmo should have no background");
            }
        }
    }


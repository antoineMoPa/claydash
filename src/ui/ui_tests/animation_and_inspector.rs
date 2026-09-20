    #[test]
    fn animation_timeline_is_optional_and_starts_closed() {
        let mut ui = UiState::default();
        assert!(!ui.animation_timeline_open());
        ui.set_animation_timeline_open(true);
        assert!(ui.animation_timeline_open());
        ui.set_animation_timeline_open(false);
        assert!(!ui.animation_timeline_open());
    }

    #[test]
    fn bezier_handles_do_not_change_timeline_value_bounds() {
        let object = SdfObject::create_kind(PrimitiveKind::Sphere);
        let track = AnimationTrack {
            binding: AnimationBinding {
                object: object.uuid,
                property: AnimatableProperty::Position(VectorAxis::Y),
            },
            keyframes: vec![
                crate::model::Keyframe {
                    frame: 0,
                    value: 0.0,
                    interpolation: KeyframeInterpolation::Bezier,
                    incoming_handle: None,
                    outgoing_handle: Some(crate::model::BezierHandle {
                        frame_offset: 3.0,
                        value_offset: 10_000.0,
                    }),
                },
                crate::model::Keyframe {
                    frame: 10,
                    value: 10.0,
                    interpolation: KeyframeInterpolation::Bezier,
                    incoming_handle: Some(crate::model::BezierHandle {
                        frame_offset: -3.0,
                        value_offset: -10_000.0,
                    }),
                    outgoing_handle: None,
                },
            ],
        };

        let (minimum, maximum) = timeline_value_bounds(&track);
        assert!((minimum + 1.2).abs() < 0.0001);
        assert!((maximum - 11.2).abs() < 0.0001);
    }

    #[test]
    fn animation_lanes_share_one_origin_with_truncated_labels() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.name = "A deliberately very long animated object name".into();
        let object_id = object.uuid;
        set_objects(&mut tree, vec![object]);
        set_selected(&mut tree, vec![object_id]);
        for property in [
            AnimatableProperty::Position(VectorAxis::X),
            AnimatableProperty::MaterialReflectivity,
        ] {
            animation::insert_keyframe(
                &mut tree,
                AnimationBinding {
                    object: object_id,
                    property,
                },
                0,
                0.0,
            );
        }
        let mut runtime = AnimationRuntime::default();

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(760.0);
            animation_panel(ui, &mut tree, &mut runtime);
        });
        output.textures_delta.clear();
        let lanes: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill == Color32::from_rgb(34, 35, 39) => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect();

        assert_eq!(lanes.len(), 2);
        assert!((lanes[0].left() - lanes[1].left()).abs() < 0.01);
        assert!((lanes[0].width() - lanes[1].width()).abs() < 0.01);
    }

    #[test]
    fn viewport_animation_shortcuts_toggle_and_step_playback() {
        let ctx = egui::Context::default();
        let mut ui_state = UiState::default();
        let mut tree = DataTree::default();
        ui_state.animation.current_frame = 5.0;
        fn press(
            ctx: &egui::Context,
            ui_state: &mut UiState,
            tree: &mut DataTree,
            key: egui::Key,
            repeat: bool,
            modifiers: egui::Modifiers,
        ) {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat,
                        modifiers,
                    }],
                    ..Default::default()
                },
                |_ui| ui_state.handle_animation_shortcuts(ctx, tree),
            );
            output.textures_delta.clear();
        }
        press(
            &ctx,
            &mut ui_state,
            &mut tree,
            egui::Key::Space,
            false,
            egui::Modifiers::NONE,
        );
        assert!(ui_state.animation.playing);
        press(
            &ctx,
            &mut ui_state,
            &mut tree,
            egui::Key::Space,
            true,
            egui::Modifiers::NONE,
        );
        assert!(ui_state.animation.playing);
        press(
            &ctx,
            &mut ui_state,
            &mut tree,
            egui::Key::ArrowLeft,
            false,
            egui::Modifiers::NONE,
        );
        assert!(!ui_state.animation.playing);
        assert_eq!(ui_state.animation.current_frame, 4.0);
        press(
            &ctx,
            &mut ui_state,
            &mut tree,
            egui::Key::ArrowRight,
            true,
            egui::Modifiers::NONE,
        );
        assert_eq!(ui_state.animation.current_frame, 5.0);
        press(
            &ctx,
            &mut ui_state,
            &mut tree,
            egui::Key::ArrowRight,
            false,
            egui::Modifiers::SHIFT,
        );
        assert_eq!(ui_state.animation.current_frame, 15.0);
        press(
            &ctx,
            &mut ui_state,
            &mut tree,
            egui::Key::ArrowLeft,
            true,
            egui::Modifiers::SHIFT,
        );
        assert_eq!(ui_state.animation.current_frame, 5.0);
    }

    #[test]
    fn insert_keyframe_menu_is_compact_and_cursor_anchored() {
        let ctx = egui::Context::default();
        let mut ui_state = UiState::default();
        let mut tree = DataTree::default();
        let cursor = egui::pos2(240.0, 180.0);
        ui_state.insert_keyframe_menu_position = Some(cursor);

        let mut output = ctx.run_ui(egui::RawInput::default(), |_ui| {
            ui_state.draw_insert_keyframe_menu(&ctx, &mut tree);
        });
        output.textures_delta.clear();

        let popup = *ui_state.regions.last().expect("keyframe popup region");
        assert!(popup.left() >= cursor.x);
        assert!(popup.top() >= cursor.y);
        assert!(popup.left() - cursor.x <= 12.0);
        assert!(popup.top() - cursor.y <= 12.0);
        assert!(popup.height() < 135.0);
    }

    #[test]
    fn hovering_a_transform_input_and_pressing_i_inserts_a_keyframe() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Box);
        let object_id = object.uuid;
        set_selected(&mut tree, vec![object_id]);
        set_objects(&mut tree, vec![object]);
        tree.make_undo_redo_snapshot();
        let mut runtime = AnimationRuntime::default();
        runtime.current_frame = 18.0;
        let frame = |tree: &mut DataTree, runtime: &mut AnimationRuntime, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(350.0);
                    object_panel(ui, tree, runtime);
                },
            );
            output.textures_delta.clear();
            output.shapes
        };
        let shapes = frame(&mut tree, &mut runtime, vec![]);
        let position = shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().starts_with("X ") => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("position X input");
        frame(
            &mut tree,
            &mut runtime,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::Key {
                    key: egui::Key::I,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
            ],
        );
        assert!(animation::animation_data(&tree).tracks.is_empty());
        frame(
            &mut tree,
            &mut runtime,
            vec![egui::Event::Key {
                key: egui::Key::I,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        frame(
            &mut tree,
            &mut runtime,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::Key {
                    key: egui::Key::I,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );

        let data = animation::animation_data(&tree);
        assert_eq!(data.tracks.len(), 1);
        assert_eq!(data.tracks[0].binding.object, object_id);
        assert_eq!(
            data.tracks[0].binding.property,
            AnimatableProperty::Position(VectorAxis::X)
        );
        assert_eq!(data.tracks[0].keyframes[0].frame, 18);
    }

    #[test]
    fn transform_keying_menu_inserts_blender_style_location_rotation_scale_sets() {
        let mut tree = DataTree::default();
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.transform.translation = Vec3::new(1.0, 2.0, 3.0);
        object.transform.rotation = glam::Quat::from_euler(EulerRot::XYZ, 0.1, 0.2, 0.3);
        object.transform.scale = Vec3::new(2.0, 3.0, 4.0);
        let object_id = object.uuid;
        set_objects(&mut tree, vec![object]);
        set_selected(&mut tree, vec![object_id]);
        let mut runtime = AnimationRuntime::default();
        runtime.current_frame = 14.0;

        insert_transform_keyframes(
            &mut tree,
            &mut runtime,
            TransformKeySet::LocationRotationScale,
        );

        let data = animation::animation_data(&tree);
        assert_eq!(data.tracks.len(), 9);
        assert!(data.tracks.iter().all(|track| {
            track.binding.object == object_id && track.keyframes[0].frame == 14
        }));
    }

    #[test]
    fn location_keyframes_animate_the_selected_boolean_group_transform() {
        let mut tree = DataTree::default();
        let root = SdfObject::create_kind(PrimitiveKind::Box);
        let root_id = root.uuid;
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.boolean_parent = Some(root_id);
        set_objects(&mut tree, vec![root, child]);
        set_selected(&mut tree, vec![root_id]);
        let mut runtime = AnimationRuntime::default();

        insert_transform_keyframes(
            &mut tree,
            &mut runtime,
            TransformKeySet::Location,
        );
        let mut scene = objects(&tree);
        scene[0].group_transform.translation.y = 3.0;
        set_objects(&mut tree, scene);
        runtime.current_frame = 10.0;
        insert_transform_keyframes(
            &mut tree,
            &mut runtime,
            TransformKeySet::Location,
        );

        let data = animation::animation_data(&tree);
        assert_eq!(data.tracks.len(), 3);
        assert!(data.tracks.iter().all(|track| matches!(
            track.binding.property,
            AnimatableProperty::GroupPosition(_)
        )));
        assert!(animation::evaluate(&mut tree, 0.0));
        assert_eq!(objects(&tree)[0].group_transform.translation.y, 0.0);
        assert!(animation::evaluate(&mut tree, 10.0));
        assert_eq!(objects(&tree)[0].group_transform.translation.y, 3.0);
    }

    #[test]
    fn applying_material_to_a_group_links_every_descendant() {
        let mut tree = DataTree::default();
        let root = SdfObject::create_kind(PrimitiveKind::Box);
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        set_selected(&mut tree, vec![root.uuid]);
        set_objects(&mut tree, vec![root, child]);

        let material = Material::preset(MaterialKind::Metallic);
        apply_material(&mut tree, material);

        let scene = objects(&tree);
        let shared = scene[0].material_id.expect("linked material");
        assert!(scene.iter().all(|object| {
            object.material_id == Some(shared) && object.material.kind == MaterialKind::Metallic
        }));
        assert_eq!(crate::model::material_assets(&tree).len(), 1);
    }

    #[test]
    fn keyboard_then_tree_click_combines_whole_groups_and_undoes() {
        use winit::keyboard::KeyCode;
        for (key, operation) in [
            (KeyCode::Equal, BooleanOperation::Union),
            (KeyCode::Minus, BooleanOperation::Subtract),
            (KeyCode::NumpadMultiply, BooleanOperation::Intersect),
        ] {
            let ctx = egui::Context::default();
            let mut tree = DataTree::default();
            let mut target = SdfObject::create_kind(PrimitiveKind::Box);
            target.name = "First target".into();
            let mut group = SdfObject::create_kind(PrimitiveKind::Sphere);
            group.name = "Other group".into();
            let mut child = SdfObject::create_kind(PrimitiveKind::Box);
            child.boolean_parent = Some(group.uuid);
            child.operation = BooleanOperation::Subtract;
            set_objects(
                &mut tree,
                vec![target.clone(), group.clone(), child.clone()],
            );
            tree.make_undo_redo_snapshot();
            click_scene_text(&ctx, &mut tree, "First target", egui::Modifiers::NONE);
            let mut interactions = crate::interactions::InteractionState::default();
            interactions.key_pressed(
                key,
                ctx.egui_wants_keyboard_input(),
                &Commands::new(),
                &mut tree,
            );
            assert_eq!(
                scene_actions::pending_boolean(&tree).unwrap().target,
                target.uuid
            );
            click_scene_text(&ctx, &mut tree, "Other group", egui::Modifiers::NONE);
            let scene = objects(&tree);
            assert_eq!(scene[1].boolean_parent, Some(target.uuid));
            assert_eq!(scene[1].operation, operation);
            assert_eq!(scene[2].boolean_parent, Some(group.uuid));
            assert_eq!(scene[2].operation, BooleanOperation::Subtract);
            assert!(scene_actions::pending_boolean(&tree).is_none());
            undo_redo::undo(&mut tree);
            assert!(objects(&tree)[1].boolean_parent.is_none());
            assert!(scene_actions::pending_boolean(&tree).is_none());
        }
    }

    #[test]
    fn inspector_resets_only_requested_transform_property_and_undoes() {
        for reset_index in 0..2 {
            let ctx = egui::Context::default();
            let mut tree = DataTree::default();
            let mut object = SdfObject::create_kind(PrimitiveKind::Box);
            object.transform.translation = Vec3::new(1.0, 2.0, 3.0);
            object.transform.rotation = glam::Quat::from_rotation_y(0.7);
            object.transform.scale = Vec3::new(2.0, 3.0, 4.0);
            set_selected(&mut tree, vec![object.uuid]);
            set_objects(&mut tree, vec![object.clone()]);
            tree.make_undo_redo_snapshot();
            let mut animation = AnimationRuntime::default();
            let mut frame = |events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| object_panel(ui, &mut tree, &mut animation),
                );
                output.textures_delta.clear();
                output.shapes
            };
            let shapes = frame(vec![]);
            let positions: Vec<_> = shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Reset" => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .collect();
            let position = positions[reset_index];
            for pressed in [true, false] {
                frame(vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
            let result = objects(&tree)[0].transform;
            assert_eq!(
                result.translation,
                if reset_index == 0 {
                    Vec3::ZERO
                } else {
                    object.transform.translation
                }
            );
            assert_eq!(
                result.rotation,
                if reset_index == 1 {
                    glam::Quat::IDENTITY
                } else {
                    object.transform.rotation
                }
            );
            assert_eq!(result.scale, object.transform.scale);
            undo_redo::undo(&mut tree);
            let restored = objects(&tree)[0].transform;
            assert_eq!(restored.translation, object.transform.translation);
            assert_eq!(restored.rotation, object.transform.rotation);
        }
    }

    #[test]
    fn group_selection_shows_only_group_transform_until_drilling_into_a_primitive() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let root = SdfObject::create_kind(PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(PrimitiveKind::Box);
        child.boolean_parent = Some(root.uuid);
        set_objects(&mut tree, vec![root.clone(), child]);
        set_selected(&mut tree, vec![root.uuid]);
        let mut runtime = AnimationRuntime::default();
        let labels = |tree: &mut DataTree, runtime: &mut AnimationRuntime| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                object_panel(ui, tree, runtime);
            });
            output.textures_delta.clear();
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let group_labels = labels(&mut tree, &mut runtime);
        assert!(group_labels.iter().any(|label| label == "Group transform"));
        assert!(!group_labels.iter().any(|label| label == "Object settings"));
        assert!(!group_labels.iter().any(|label| label.contains("Radius")));

        crate::model::set_selected_exact(&mut tree, vec![root.uuid]);
        let object_labels = labels(&mut tree, &mut runtime);
        assert!(object_labels.iter().any(|label| label == "Object settings"));
        assert!(object_labels.iter().any(|label| label.contains("Radius")));
    }

    #[test]
    fn picking_wood_without_selection_sets_material_for_new_objects_and_cutters() {
        let mut tree = DataTree::default();
        let material = Material::preset(MaterialKind::Wood);
        apply_material(&mut tree, material);
        commands::spawn(&mut tree, sdf_consts::TYPE_BOX);
        let target = objects(&tree)[0].uuid;
        scene_actions::add_operand(
            &mut tree,
            target,
            PrimitiveKind::Sphere,
            BooleanOperation::Subtract,
        );
        for object in objects(&tree) {
            assert_eq!(object.material.kind, MaterialKind::Wood);
            assert_eq!(object.color, material.color);
        }
    }

    #[test]
    fn operand_softness_slider_changes_geometry_and_undoes() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let target = SdfObject::create_kind(PrimitiveKind::Box);
        let mut operand = SdfObject::create_kind(PrimitiveKind::Sphere);
        operand.boolean_parent = Some(target.uuid);
        set_selected(&mut tree, vec![operand.uuid]);
        set_objects(&mut tree, vec![target, operand]);
        tree.make_undo_redo_snapshot();
        let mut animation = AnimationRuntime::default();
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(350.0);
                    operand_panel(ui, &mut tree, &mut animation);
                },
            );
            output.textures_delta.clear();
            output.shapes
        };
        let shapes = frame(vec![]);
        let label = shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Softness" => Some(text.pos),
                _ => None,
            })
            .expect("softness control");
        // Slider track occupies the left side of the same row.
        let position = egui::pos2(65.0, label.y + 7.0);
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        assert!(objects(&tree)[1].softness > 0.1);
        undo_redo::undo(&mut tree);
        assert_eq!(objects(&tree)[1].softness, 0.05);
    }

    #[test]
    fn inline_rename_updates_the_tree_and_undoes() {
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Box);
        let id = object.uuid;
        set_objects(&mut tree, vec![object]);
        tree.make_undo_redo_snapshot();

        rename_object(&mut tree, id, "Workbench".into());

        assert_eq!(objects(&tree)[0].display_name(), "Workbench");
        undo_redo::undo(&mut tree);
        assert_eq!(objects(&tree)[0].display_name(), "Box");
        undo_redo::redo(&mut tree);
        assert_eq!(objects(&tree)[0].display_name(), "Workbench");
    }

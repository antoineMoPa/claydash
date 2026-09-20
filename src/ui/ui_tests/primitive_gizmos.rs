    #[test]
    fn dragging_each_primitive_handle_outward_increases_its_dimension() {
        for (kind, handle_index) in PrimitiveKind::ALL.into_iter().flat_map(|kind| {
            let count = if kind == PrimitiveKind::Sphere { 3 } else { 1 };
            (0..count).map(move |index| (kind, index))
        }) {
            let ctx = egui::Context::default();
            let viewport =
                egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
            let mut camera = Camera::new();
            camera.viewport_origin = Vec2::new(100.0, 40.0);
            camera.viewport = Vec2::new(600.0, 500.0);
            let mut state = UiState::default();
            let mut tree = DataTree::default();
            let object = SdfObject::create_kind(kind);
            let handles = resize_handles(&object, &camera);
            let handle = &handles[handle_index];
            let start = camera.project(handle.world, 1.0).unwrap();
            let end = camera
                .project(handle.world + handle.direction * 0.2, 1.0)
                .unwrap();
            let before = handle.value;
            let original = object.clone();
            set_selected(&mut tree, vec![object.uuid]);
            set_objects(&mut tree, vec![object]);
            let mut frame = |events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(800.0, 700.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        state.regions.clear();
                        ui.set_clip_rect(viewport);
                        state.draw_object_gizmos(ui, &mut tree, &camera);
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
            frame(vec![egui::Event::PointerMoved(start.lerp(end, 0.5))]);
            frame(vec![egui::Event::PointerMoved(end)]);
            frame(vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            let resized = &objects(&tree)[0];
            let after = resize_handles(resized, &camera)[handle_index].value;
            if kind == PrimitiveKind::Sphere {
                for axis in 0..3 {
                    if axis != handle_index {
                        assert_eq!(
                            resized.transform.scale[axis],
                            original.transform.scale[axis]
                        );
                    }
                }
                let (SdfParams::SphereParams(before), SdfParams::SphereParams(after)) =
                    (&original.params, &resized.params)
                else {
                    unreachable!()
                };
                assert_eq!(
                    before.radius, after.radius,
                    "axis resize must not change the shared base radius"
                );
                assert_eq!(resized.transform.rotation, original.transform.rotation);
                assert_eq!(
                    resized.transform.translation,
                    original.transform.translation
                );
            }
            assert!(
                after > before,
                "{kind:?}: outward drag should increase dimension ({before} -> {after})"
            );
        }
    }

    #[test]
    fn resize_labels_only_appear_while_hovering_a_handle() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(100.0, 40.0);
        camera.viewport = Vec2::new(600.0, 500.0);
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Sphere);
        let handle = camera
            .project(resize_handles(&object, &camera)[0].world, 1.0)
            .unwrap();
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let mut state = UiState::default();
        for (position, visible) in [
            (egui::pos2(10.0, 10.0), false),
            (handle, true),
            (egui::pos2(10.0, 10.0), false),
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 700.0),
                    )),
                    events: vec![egui::Event::PointerMoved(position)],
                    ..Default::default()
                },
                |ui| {
                    state.regions.clear();
                    ui.set_clip_rect(viewport);
                    state.draw_object_gizmos(ui, &mut tree, &camera);
                },
            );
            output.textures_delta.clear();
            let has_label = output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.text().starts_with("Radius ")
                )
            });
            assert_eq!(has_label, visible);
        }
    }

    #[test]
    fn sphere_has_three_local_axis_radii_in_every_camera_view() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.transform.scale = Vec3::new(1.5, 0.8, 1.1);
        object.transform.rotation = glam::Quat::from_rotation_z(0.4);
        let inverse = object.transform.matrix().inverse();
        let SdfParams::SphereParams(params) = &object.params else {
            unreachable!()
        };
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800.0, 600.0);
        for angle in [
            ViewAngle::Front,
            ViewAngle::Back,
            ViewAngle::Left,
            ViewAngle::Right,
            ViewAngle::Top,
            ViewAngle::Bottom,
            ViewAngle::Isometric,
        ] {
            camera.snap(angle);
            for mode in [
                crate::camera::ProjectionMode::Perspective,
                crate::camera::ProjectionMode::Orthographic,
            ] {
                camera.projection_mode = mode;
                let handles = resize_handles(&object, &camera);
                assert_eq!(handles.len(), 3);
                for (axis, handle) in handles.into_iter().enumerate() {
                    let local_axis = [Vec3::X, Vec3::Y, Vec3::Z][axis];
                    assert_eq!(handle.parameter, axis);
                    let local = inverse.transform_point3(handle.world);
                    assert!((local[axis].abs() - params.radius).abs() < 0.0001);
                    assert!((local - local_axis * local[axis]).length() < 0.0001);
                    assert!(
                        (handle
                            .direction
                            .dot(object.transform.rotation * local_axis)
                            .abs()
                            - 1.0)
                            .abs()
                            < 0.0001
                    );
                    assert!(
                        handle
                            .direction
                            .dot(camera.position - object.transform.translation)
                            >= -0.0001
                    );
                    assert!(
                        (handle.value - params.radius * object.transform.scale[axis].abs()).abs()
                            < 0.00001
                    );
                }
            }
        }
    }

    fn assert_no_id_warnings(shape: &egui::Shape) {
        match shape {
            egui::Shape::Text(text) => {
                let text = text.galley.text();
                for warning in ["First use of", "Second use of", "Double use of"] {
                    assert!(!text.contains(warning), "egui ID warning: {text}");
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    assert_no_id_warnings(shape);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn duplicate_inspectors_and_narrow_viewports_have_no_id_warnings() {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.warn_on_id_clash = true);
        let mut state = UiState::default();
        state
            .layout
            .add_pane_against_edge(DropSide::Bottom, 0.25, EditorPane::Object);
        let mut tree = DataTree::default();
        let object = SdfObject::create(sdf_consts::TYPE_BOX);
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let mut commands = Commands::new();
        let mut camera = Camera::new();
        let mut document = DocumentState::default();
        for width in [1200.0, 800.0, 480.0] {
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        state.draw(ui, &mut tree, &mut commands, &mut camera, &mut document);
                    },
                );
                output.textures_delta.clear(); // Headless test has no GPU texture consumer.
                for shape in output.shapes {
                    assert_no_id_warnings(&shape.shape);
                }
            }
        }
    }

    #[test]
    fn resize_gizmo_shapes_are_clipped_to_the_viewport() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 80.0), egui::vec2(300.0, 250.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(viewport.left(), viewport.top());
        camera.viewport = Vec2::new(viewport.width(), viewport.height());
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create(sdf_consts::TYPE_BOX);
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_clip_rect(viewport);
            state.draw_object_gizmos(ui, &mut tree, &camera);
        });
        output.textures_delta.clear();
        let mut painted = 0;
        for shape in output.shapes {
            if !matches!(shape.shape, egui::Shape::Noop) {
                assert!(viewport.contains_rect(shape.clip_rect));
                painted += 1;
            }
        }
        assert!(painted > 0, "test must render actual handles");
    }

    #[test]
    fn group_selection_hides_primitive_resize_gizmos() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 80.0), egui::vec2(500.0, 400.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(viewport.left(), viewport.top());
        camera.viewport = Vec2::new(viewport.width(), viewport.height());
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let root = SdfObject::create(sdf_consts::TYPE_BOX);
        let mut child = SdfObject::create(sdf_consts::TYPE_SPHERE);
        child.boolean_parent = Some(root.uuid);
        set_objects(&mut tree, vec![root.clone(), child]);
        set_selected(&mut tree, vec![root.uuid]);

        let draw = |state: &mut UiState, tree: &mut DataTree| {
            state.regions.clear();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_clip_rect(viewport);
                state.draw_object_gizmos(ui, tree, &camera);
            });
            output.textures_delta.clear();
        };
        draw(&mut state, &mut tree);
        assert!(state.regions.is_empty());

        crate::model::set_selected_exact(&mut tree, vec![root.uuid]);
        draw(&mut state, &mut tree);
        assert!(!state.regions.is_empty());
    }

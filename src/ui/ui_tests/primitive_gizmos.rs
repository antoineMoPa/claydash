    fn primitive_gizmo_frame(
        ctx: &egui::Context,
        state: &mut UiState,
        tree: &mut DataTree,
        camera: &Camera,
        viewport: egui::Rect,
        mut events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) {
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
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
                state.draw_object_gizmos(ui, tree, camera);
            },
        );
        output.textures_delta.clear();
    }

    fn drag_primitive_handle(
        object: SdfObject,
        handle_index: usize,
        modifiers: egui::Modifiers,
    ) -> SdfObject {
        let ctx = egui::Context::default();
        let viewport =
            egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(100.0, 40.0);
        camera.viewport = Vec2::new(600.0, 500.0);
        let handle = &resize_handles(&object, &camera)[handle_index];
        let start = camera.project(handle.world, 1.0).unwrap();
        let end = camera
            .project(handle.world + handle.direction * 0.2, 1.0)
            .unwrap();
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let mut frame = |events| {
            primitive_gizmo_frame(
                &ctx, &mut state, &mut tree, &camera, viewport, events, modifiers,
            );
        };
        frame(vec![]);
        frame(vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        frame(vec![egui::Event::PointerMoved(start.lerp(end, 0.5))]);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        objects(&tree).remove(0)
    }

    #[test]
    fn dragging_bezier_handle_changes_the_rendered_curve() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(100.0, 40.0);
        camera.viewport = Vec2::new(600.0, 500.0);
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::BezierCurve);
        let SdfParams::BezierCurveParams(curve) = &object.params else { unreachable!() };
        let before = curve.point(0, 0.5);
        let start = camera.project(curve.points[1], 1.0).unwrap();
        let end = start + egui::vec2(0.0, -35.0);
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let mut frame = |events| primitive_gizmo_frame(&ctx, &mut state, &mut tree, &camera, viewport, events, egui::Modifiers::NONE);
        frame(vec![]);
        frame(vec![egui::Event::PointerMoved(start), egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE }]);
        frame(vec![egui::Event::PointerMoved(start.lerp(end, 0.5))]);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }]);
        let scene = objects(&tree);
        let SdfParams::BezierCurveParams(curve) = &scene[0].params else { unreachable!() };
        assert!(curve.point(0, 0.5).distance(before) > 0.01);
    }

    #[test]
    fn dragging_each_primitive_handle_outward_increases_its_dimension() {
        for (kind, handle_index) in [PrimitiveKind::Sphere, PrimitiveKind::Box, PrimitiveKind::Cylinder, PrimitiveKind::Torus].into_iter().flat_map(|kind| {
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
    fn dragging_a_box_face_keeps_the_opposite_face_fixed() {
        let mut original = SdfObject::create_kind(PrimitiveKind::Box);
        original.transform.translation = Vec3::new(0.1, -0.2, 0.05);
        original.transform.rotation = glam::Quat::from_rotation_z(0.35);
        original.transform.scale = Vec3::new(1.4, 0.8, 1.1);
        let camera = Camera::new();
        let handle = &resize_handles(&original, &camera)[0];
        let axis = handle.parameter;
        let sign = handle.local_direction[axis];
        let SdfParams::BoxParams(params) = &original.params else {
            unreachable!()
        };
        let mut opposite_local = Vec3::ZERO;
        opposite_local[axis] = -sign * params.box_q[axis];
        let opposite_before = original
            .transform
            .matrix()
            .transform_point3(opposite_local);
        let center_before = original.transform.translation;

        let resized = drag_primitive_handle(original, 0, egui::Modifiers::NONE);
        let SdfParams::BoxParams(params) = &resized.params else {
            unreachable!()
        };
        opposite_local[axis] = -sign * params.box_q[axis];
        let opposite_after = resized
            .transform
            .matrix()
            .transform_point3(opposite_local);

        assert!(opposite_after.distance(opposite_before) < 0.0001);
        assert!(resized.transform.translation.distance(center_before) > 0.01);
    }

    #[test]
    fn shift_dragging_a_box_face_resizes_around_its_center() {
        let mut original = SdfObject::create_kind(PrimitiveKind::Box);
        original.transform.translation = Vec3::new(0.1, -0.2, 0.05);
        original.transform.rotation = glam::Quat::from_rotation_z(0.35);
        original.transform.scale = Vec3::new(1.4, 0.8, 1.1);
        let center_before = original.transform.translation;
        let SdfParams::BoxParams(params) = &original.params else {
            unreachable!()
        };
        let size_before = params.box_q;

        let resized = drag_primitive_handle(original, 0, egui::Modifiers::SHIFT);
        let SdfParams::BoxParams(params) = &resized.params else {
            unreachable!()
        };

        assert_eq!(resized.transform.translation, center_before);
        assert!(params.box_q.distance(size_before) > 0.01);
    }

    #[test]
    fn view_aligned_box_face_exposes_its_four_silhouette_edges() {
        let ctx = egui::Context::default();
        let viewport =
            egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
        let mut camera = Camera::new();
        camera.snap(ViewAngle::Front);
        camera.projection_mode = crate::camera::ProjectionMode::Orthographic;
        camera.viewport_origin = Vec2::new(viewport.left(), viewport.top());
        camera.viewport = Vec2::new(viewport.width(), viewport.height());
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Box);
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);

        primitive_gizmo_frame(
            &ctx,
            &mut state,
            &mut tree,
            &camera,
            viewport,
            vec![],
            egui::Modifiers::NONE,
        );

        let handles = resize_handles(&objects(&tree)[0], &camera);
        assert_eq!(handles.iter().filter(|handle| handle.edge_on).count(), 4);
        assert_eq!(handles.iter().filter(|handle| handle.camera_facing).count(), 1);
        assert_eq!(state.regions.len(), 5);
    }

    #[test]
    fn top_view_silhouette_edge_resizes_the_box_without_moving_the_camera() {
        let ctx = egui::Context::default();
        let viewport =
            egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
        let mut camera = Camera::new();
        camera.snap(ViewAngle::Top);
        camera.projection_mode = crate::camera::ProjectionMode::Orthographic;
        camera.viewport_origin = Vec2::new(viewport.left(), viewport.top());
        camera.viewport = Vec2::new(viewport.width(), viewport.height());
        let camera_position = camera.position;
        let object = SdfObject::create_kind(PrimitiveKind::Box);
        let object_id = object.uuid;
        let handle = resize_handles(&object, &camera)
            .into_iter()
            .find(|handle| {
                handle.id
                    == ResizeHandleId::BoxFace {
                        axis: 0,
                        positive: true,
                    }
            })
            .expect("right silhouette edge");
        assert!(handle.edge_on);
        let initial_half_width = match &object.params {
            SdfParams::BoxParams(params) => params.box_q.x,
            _ => unreachable!(),
        };
        let start = camera.project(handle.world, 1.0).unwrap();
        let end = camera
            .project(handle.world + handle.direction * 0.25, 1.0)
            .unwrap();
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut frame = |tree: &mut DataTree, events| {
            primitive_gizmo_frame(
                &ctx,
                &mut state,
                tree,
                &camera,
                viewport,
                events,
                egui::Modifiers::NONE,
            );
        };
        frame(&mut tree, vec![]);
        frame(
            &mut tree,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        frame(
            &mut tree,
            vec![egui::Event::PointerMoved(start.lerp(end, 0.5))],
        );
        assert!(ctx.is_being_dragged(egui::Id::new((
            "resize",
            object_id,
            handle.id,
        ))));
        assert_eq!(
            crate::model::selected_box_face(&tree),
            Some(crate::model::BoxFaceSelection {
                object: object_id,
                axis: crate::model::VectorAxis::X,
                positive: true,
            })
        );
        frame(&mut tree, vec![egui::Event::PointerMoved(end)]);
        frame(&mut tree, vec![button(end, false)]);
        let SdfParams::BoxParams(params) = &objects(&tree)[0].params else {
            unreachable!()
        };
        assert!(
            params.box_q.x > initial_half_width,
            "resized half-width: {} (started at {initial_half_width})",
            params.box_q.x
        );
        assert_eq!(camera.position, camera_position);
    }

    #[test]
    fn transform_gizmo_regions_do_not_hide_oblique_face_gizmos() {
        let ctx = egui::Context::default();
        let viewport =
            egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(viewport.left(), viewport.top());
        camera.viewport = Vec2::new(viewport.width(), viewport.height());
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Box);
        let transform_regions: Vec<_> = resize_handles(&object, &camera)
            .into_iter()
            .map(|handle| {
                egui::Rect::from_center_size(
                    camera.project(handle.world, 1.0).unwrap(),
                    egui::Vec2::splat(30.0),
                )
            })
            .collect();
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let mut state = UiState::default();
        state.regions = transform_regions;
        let transform_region_count = state.regions.len();

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_clip_rect(viewport);
            state.draw_object_gizmos_avoiding(ui, &mut tree, &camera, 0);
        });
        output.textures_delta.clear();

        assert!(state.regions.len() > transform_region_count);
    }

    #[test]
    fn active_box_face_drag_survives_its_handle_moving_outside_the_viewport() {
        let ctx = egui::Context::default();
        let viewport =
            egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(300.0, 250.0));
        let mut camera = Camera::new();
        camera.snap(ViewAngle::Front);
        camera.projection_mode = crate::camera::ProjectionMode::Orthographic;
        camera.viewport_origin = Vec2::new(viewport.left(), viewport.top());
        camera.viewport = Vec2::new(viewport.width(), viewport.height());
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Box);
        let handle = resize_handles(&object, &camera)
            .into_iter()
            .find(|handle| handle.camera_facing && !handle.edge_on)
            .expect("front face handle");
        let handle_id = egui::Id::new(("resize", object.uuid, handle.id));
        let start = camera.project(handle.world, 1.0).unwrap();
        let screen_up = camera.view().inverse().y_axis.truncate();
        let drag_direction = (camera
            .project(handle.world + screen_up * 0.1, 1.0)
            .unwrap()
            - start)
            .normalized();
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);

        let mut frame = |tree: &mut DataTree, events| {
            primitive_gizmo_frame(
                &ctx,
                &mut state,
                tree,
                &camera,
                viewport,
                events,
                egui::Modifiers::NONE,
            );
        };
        frame(&mut tree, vec![]);
        frame(
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
        );
        let first = start + drag_direction * 12.0;
        frame(&mut tree, vec![egui::Event::PointerMoved(first)]);
        assert!(ctx.is_being_dragged(handle_id));

        let mut scene = objects(&tree);
        scene[0].transform.translation.x = 10.0;
        set_objects(&mut tree, scene);
        let SdfParams::BoxParams(params) = &objects(&tree)[0].params else {
            unreachable!()
        };
        let size_before_second_move = params.box_q;

        frame(
            &mut tree,
            vec![egui::Event::PointerMoved(first + drag_direction * 12.0)],
        );
        let SdfParams::BoxParams(params) = &objects(&tree)[0].params else {
            unreachable!()
        };
        assert!(params.box_q.distance(size_before_second_move) > 0.001);
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
                        state.draw(
                            ui,
                            &mut tree,
                            &mut commands,
                            &mut camera,
                            &mut document,
                            None,
                            None,
                        );
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
    fn selected_boolean_group_shows_its_lattice_handles() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800.0, 600.0);
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(PrimitiveKind::Box);
        child.boolean_parent = Some(root.uuid);
        let scene = vec![root.clone(), child];
        let (min, max) = crate::model::lattice_bounds(&scene, root.uuid).unwrap();
        root.lattice = Some(crate::model::Lattice::new(min, max, 2));
        set_objects(&mut tree, vec![root.clone(), scene[1].clone()]);
        set_selected(&mut tree, vec![root.uuid]);
        assert_eq!(commands::effective_selected_ids(&tree).len(), 2);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_clip_rect(viewport);
            state.draw_object_gizmos(ui, &mut tree, &camera);
        });
        output.textures_delta.clear();
        assert!(
            state.regions.len() >= 8,
            "the selected group must expose its cage handles"
        );
    }

    #[test]
    fn dragging_a_group_lattice_handle_moves_a_control_point() {
        let ctx = egui::Context::default();
        let viewport =
            egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(viewport.left(), viewport.top());
        camera.viewport = Vec2::new(viewport.width(), viewport.height());
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(PrimitiveKind::Box);
        child.boolean_parent = Some(root.uuid);
        child.transform.translation.x = 0.7;
        let scene = vec![root.clone(), child.clone()];
        let (min, max) = crate::model::lattice_bounds(&scene, root.uuid).unwrap();
        let lattice = crate::model::Lattice::new(min, max, 2);
        let nearest = (0..2)
            .flat_map(|z| (0..2).flat_map(move |y| (0..2).map(move |x| (x, y, z))))
            .min_by(|a, b| {
                lattice.position(a.0, a.1, a.2).distance_squared(camera.position)
                    .total_cmp(&lattice.position(b.0, b.1, b.2).distance_squared(camera.position))
            })
            .unwrap();
        let start = camera
            .project(lattice.position(nearest.0, nearest.1, nearest.2), 1.0)
            .unwrap();
        let end = start + egui::vec2(36.0, 0.0);
        root.lattice = Some(lattice);
        set_objects(&mut tree, vec![root.clone(), child]);
        set_selected(&mut tree, vec![root.uuid]);
        let mut frame = |events| {
            primitive_gizmo_frame(
                &ctx,
                &mut state,
                &mut tree,
                &camera,
                viewport,
                events,
                egui::Modifiers::NONE,
            );
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
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        let lattice = objects(&tree)[0].lattice.clone().unwrap();
        assert!(lattice.offsets.iter().any(|offset| offset.length() > 0.01));
        assert_eq!(lattice.current_shape_key, Some(1));
        assert_eq!(lattice.shape_keys[0].offsets, lattice.offsets);
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
    fn polygon_face_overlay_covers_caps_sides_and_concave_outlines() {
        let mut object = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
        object.params = SdfParams::PolygonPrismParams(crate::model::PolygonPrismParams {
            vertices: vec![
                Vec2::new(-1.0, -1.0),
                Vec2::new(1.0, -1.0),
                Vec2::ZERO,
                Vec2::new(1.0, 1.0),
                Vec2::new(-1.0, 1.0),
            ],
            half_depth: 0.25,
        });
        let scene = [object.clone()];
        let cap = selected_polygon_face_vertices(
            &scene,
            crate::model::PolygonPrismFaceSelection {
                object: object.uuid,
                face: crate::model::PolygonPrismFace::Cap { positive: true },
            },
        )
        .unwrap();
        let side = selected_polygon_face_vertices(
            &scene,
            crate::model::PolygonPrismFaceSelection {
                object: object.uuid,
                face: crate::model::PolygonPrismFace::Side { edge: 0 },
            },
        )
        .unwrap();

        assert_eq!(cap.1.len(), 5);
        assert_eq!(polygon_overlay_triangles(&cap.0).len(), 3);
        assert_eq!(side.1.len(), 4);
        assert_eq!(polygon_overlay_triangles(&side.0).len(), 2);
    }

    #[test]
    fn reselected_polygon_cap_slides_while_opposite_cap_stays_fixed() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(100.0, 40.0);
        camera.viewport = Vec2::new(600.0, 500.0);
        let mut root = SdfObject::create_kind(PrimitiveKind::Box);
        root.transform.translation = Vec3::X * 10.0;
        let mut prism = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
        prism.boolean_parent = Some(root.uuid);
        let half_depth = match &prism.params {
            SdfParams::PolygonPrismParams(params) => params.half_depth,
            _ => unreachable!(),
        };
        let original_back = prism.transform.matrix().transform_point3(Vec3::Z * -half_depth);
        let center = camera.project(Vec3::Z * half_depth, 1.0).unwrap();
        let ahead = camera.project(Vec3::Z * (half_depth + 0.1), 1.0).unwrap();
        let axis = (ahead - center) * 10.0;
        let start = center;
        let end = start + axis * 0.2;
        let mut tree = DataTree::default();
        set_objects(&mut tree, vec![root.clone(), prism.clone()]);
        set_selected(&mut tree, vec![root.uuid]);
        crate::model::set_selected_modeling_face(&mut tree, Some(
            crate::model::ModelingFaceSelection::PolygonPrism(
                crate::model::PolygonPrismFaceSelection {
                    object: prism.uuid,
                    face: crate::model::PolygonPrismFace::Cap { positive: true },
                },
            ),
        ));
        let mut state = UiState::default();
        let button = |pos, pressed| egui::Event::PointerButton {
            pos, button: egui::PointerButton::Primary, pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut frame = |events| primitive_gizmo_frame(
            &ctx, &mut state, &mut tree, &camera, viewport, events, egui::Modifiers::NONE,
        );
        frame(vec![]);
        frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![button(end, false)]);
        prism = objects(&tree).into_iter().find(|object| object.uuid == prism.uuid).unwrap();
        let SdfParams::PolygonPrismParams(params) = &prism.params else { unreachable!() };
        assert!(params.half_depth > half_depth);
        let new_back = prism.transform.matrix().transform_point3(Vec3::Z * -params.half_depth);
        assert!(new_back.distance(original_back) < 0.0001);
    }

    #[test]
    fn split_void_tracks_the_selected_cap_depth() {
        let prism = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
        let mut split_void = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
        split_void.boolean_parent = Some(prism.uuid);
        split_void.name = "Face split void".into();
        let SdfParams::PolygonPrismParams(root_params) = &prism.params else {
            unreachable!();
        };
        let initial_half_depth = root_params.half_depth;
        let SdfParams::PolygonPrismParams(void_params) = &mut split_void.params else {
            unreachable!();
        };
        void_params.half_depth = initial_half_depth + 0.006;
        let void_half_depth = void_params.half_depth;
        let session = PolygonCapDrag {
            object: prism.uuid,
            positive: true,
            initial_transform: prism.transform,
            initial_half_depth,
            split_void: Some((split_void.uuid, split_void.transform, void_half_depth)),
            projected_axis: egui::Vec2::Y,
            raw_amount: 0.2,
        };
        let mut scene = vec![prism, split_void];

        apply_polygon_cap_drag(&mut scene, &session, 1.0);

        let SdfParams::PolygonPrismParams(root_params) = &scene[0].params else {
            unreachable!();
        };
        let SdfParams::PolygonPrismParams(void_params) = &scene[1].params else {
            unreachable!();
        };
        assert!((root_params.half_depth - initial_half_depth - 0.1).abs() < 0.0001);
        assert!((void_params.half_depth - root_params.half_depth - 0.006).abs() < 0.0001);
        assert!(scene[0]
            .transform
            .translation
            .distance(scene[1].transform.translation)
            < 0.0001);
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

use super::*;

#[cfg(not(target_arch = "wasm32"))]
struct SurfaceJob {
    cloud: std::sync::Arc<claydash_engine::gaussian_ply::GaussianCloud>,
    result: std::sync::Arc<
        std::sync::Mutex<
            Option<Result<claydash_engine::renderer::poisson_mesh::geometry::Mesh, String>>,
        >,
    >,
}

fn cloud_mesh_color(cloud: &claydash_engine::gaussian_ply::GaussianCloud) -> glam::Vec4 {
    let step = cloud.splats.len().div_ceil(4096).max(1);
    let (sum, weight) = cloud.splats.iter().step_by(step).fold(
        (glam::Vec3::ZERO, 0.0_f32),
        |(sum, weight), splat| {
            (
                sum + glam::Vec3::from_array(splat.color) * splat.opacity,
                weight + splat.opacity,
            )
        },
    );
    (sum / weight.max(1e-6)).extend(1.0)
}

pub(super) fn object_panel(ui: &mut egui::Ui, tree: &mut DataTree, runtime: &mut AnimationRuntime) {
    let selection = selected(tree);
    if selection.len() == 1
        && crate::model::scene_cameras(tree)
            .iter()
            .any(|camera| camera.uuid == selection[0])
    {
        camera_object_panel(ui, tree, runtime, selection[0]);
        return;
    }
    if let Some(group) = commands::selected_group_id(tree) {
        group_transform_panel(ui, tree, runtime, group);
        if objects(tree).iter().any(|object| {
            object.uuid == group && matches!(object.params, SdfParams::PolygonPrismParams(_))
        }) && ui.button("Edit shape in viewport").clicked()
        {
            polygon_editor::begin(ui.ctx(), tree, group);
            return;
        }
        ui.separator();
        modifiers_panel(ui, tree, runtime);
        group_optimizations_panel(ui, tree);
        ui.separator();
        super::variables::variable_bindings_editor(ui, tree);
        return;
    }
    if selection.len() != 1 {
        if selection.is_empty() {
            ui.label("Select one object to edit it.");
        } else {
            group_optimizations_panel(ui, tree);
        }
        return;
    }
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == selection[0]) else {
        return;
    };
    let object_id = object.uuid;
    let mut keyframes = Vec::new();
    let mut changed = false;
    let position_linked =
        crate::model::vector_binding(tree, object_id, crate::model::VectorBindingTarget::Position)
            .is_some();
    ui.label(RichText::new("Object settings").strong());
    changed |= ui.text_edit_singleline(&mut object.name).changed();
    ui.separator();
    let variables = crate::model::scene_variables(tree);
    let rigid_bound = crate::model::rigid_binding(
        &variables,
        object_id,
        crate::model::RigidBindingTarget::Object,
    )
    .is_some();
    let mut reset_pose = false;
    ui.add_enabled_ui(!rigid_bound, |ui| {
        let reset_position = ui
            .horizontal(|ui| {
                ui.label("Position");
                ui.add_enabled(
                    !position_linked && object.transform.translation != Vec3::ZERO,
                    egui::Button::new("Reset"),
                )
                .on_hover_text("Reset position to the origin")
                .clicked()
            })
            .inner;
        if reset_position {
            tree.make_undo_redo_snapshot();
            object.transform.translation = Vec3::ZERO;
            changed = true;
        }
        changed |= ui
            .add_enabled_ui(!position_linked, |ui| {
                animatable_vec3_editor(
                    ui,
                    tree,
                    runtime,
                    &mut object.transform.translation,
                    0.01,
                    object_id,
                    [
                        AnimatableProperty::Position(VectorAxis::X),
                        AnimatableProperty::Position(VectorAxis::Y),
                        AnimatableProperty::Position(VectorAxis::Z),
                    ],
                    &mut keyframes,
                )
            })
            .inner;
        let reset_rotation = ui
            .horizontal(|ui| {
                ui.label("Rotation");
                ui.add_enabled(
                    object.transform.rotation != glam::Quat::IDENTITY,
                    egui::Button::new("Reset"),
                )
                .on_hover_text("Reset rotation to zero on all axes")
                .clicked()
            })
            .inner;
        if reset_rotation {
            tree.make_undo_redo_snapshot();
            object.transform.rotation = glam::Quat::IDENTITY;
            changed = true;
        }
        let (x, y, z) = object.transform.rotation.to_euler(EulerRot::XYZ);
        let mut degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
        if animatable_vec3_editor(
            ui,
            tree,
            runtime,
            &mut degrees,
            1.0,
            object_id,
            [
                AnimatableProperty::Rotation(VectorAxis::X),
                AnimatableProperty::Rotation(VectorAxis::Y),
                AnimatableProperty::Rotation(VectorAxis::Z),
            ],
            &mut keyframes,
        ) {
            object.transform.rotation = glam::Quat::from_euler(
                EulerRot::XYZ,
                degrees.x.to_radians(),
                degrees.y.to_radians(),
                degrees.z.to_radians(),
            );
            changed = true;
        }
        reset_pose = reset_position || reset_rotation;
        ui.label("Scale");
        changed |= animatable_vec3_editor(
            ui,
            tree,
            runtime,
            &mut object.transform.scale,
            0.01,
            object_id,
            [
                AnimatableProperty::Scale(VectorAxis::X),
                AnimatableProperty::Scale(VectorAxis::Y),
                AnimatableProperty::Scale(VectorAxis::Z),
            ],
            &mut keyframes,
        );
    })
    .response
    .on_disabled_hover_text(
        "Transform driven by shared points; unlink the rigid binding to edit it",
    );
    ui.separator();
    if let Some(cloud) = &object.imported_splats {
        ui.label(RichText::new("Gaussian splat asset").strong());
        ui.label(format!("{} Gaussians", cloud.splats.len()));
        #[cfg(not(target_arch = "wasm32"))]
        let job_id = egui::Id::new(("gaussian-surface-job", object_id));
        let error_id = egui::Id::new(("gaussian-surface-error", object_id));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let job = ui
                .ctx()
                .data(|data| data.get_temp::<std::sync::Arc<SurfaceJob>>(job_id));
            if let Some(job) = job {
                let completed = job.result.lock().ok().and_then(|mut result| result.take());
                if let Some(result) = completed {
                    ui.ctx().data_mut(|data| data.remove::<std::sync::Arc<SurfaceJob>>(job_id));
                    if std::sync::Arc::ptr_eq(cloud, &job.cloud) {
                        match result {
                            Ok(mesh) => {
                                tree.make_undo_redo_snapshot();
                                if object.material_id.is_none() {
                                    let color = cloud_mesh_color(cloud);
                                    object.material.color = color;
                                    object.color = color;
                                }
                                object.imported_mesh = Some(std::sync::Arc::new(mesh));
                                object.imported_splats = None;
                                object.render_representation = crate::model::GroupRenderRepresentation::DirectMesh;
                                changed = true;
                                ui.ctx().request_repaint();
                            }
                            Err(error) => { ui.ctx().data_mut(|data| data.insert_temp(error_id, error)); }
                        }
                    }
                } else {
                    ui.horizontal(|ui| { ui.spinner(); ui.label("Surfacing Gaussians…"); });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                }
            } else if ui.button("Convert to mesh").on_hover_text("Reconstruct a textured triangle surface. This replaces the splat asset and can be undone.").clicked() {
                let source = cloud.clone();
                let result = std::sync::Arc::new(std::sync::Mutex::new(None));
                let output = result.clone();
                std::thread::spawn(move || {
                    let mesh = claydash_engine::gaussian_surface::reconstruct(&source, object_id);
                    if let Ok(mut slot) = output.lock() { *slot = Some(mesh); }
                });
                ui.ctx().data_mut(|data| {
                    data.insert_temp(job_id, std::sync::Arc::new(SurfaceJob { cloud: cloud.clone(), result }));
                    data.remove::<String>(error_id);
                });
            }
        }
        #[cfg(target_arch = "wasm32")]
        if ui.button("Convert to mesh").clicked() {
            let result = claydash_engine::gaussian_surface::reconstruct(cloud, object_id);
            match result {
                Ok(mesh) => {
                    tree.make_undo_redo_snapshot();
                    if object.material_id.is_none() {
                        let color = cloud_mesh_color(cloud);
                        object.material.color = color;
                        object.color = color;
                    }
                    object.imported_mesh = Some(std::sync::Arc::new(mesh));
                    object.imported_splats = None;
                    object.render_representation =
                        crate::model::GroupRenderRepresentation::DirectMesh;
                    changed = true;
                    ui.ctx().request_repaint();
                }
                Err(error) => {
                    ui.ctx().data_mut(|data| data.insert_temp(error_id, error));
                }
            }
        }
        if let Some(error) = ui.ctx().data(|data| data.get_temp::<String>(error_id)) {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if changed {
            set_objects(tree, scene);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        apply_keyframe_requests(tree, runtime, keyframes);
        return;
    }
    if let Some(mesh) = &mut object.imported_mesh {
        ui.label(RichText::new("Triangle mesh asset").strong());
        ui.label(format!(
            "{} vertices, {} triangles",
            mesh.positions.len(),
            mesh.triangles.len()
        ));
        let mut shading = mesh.shading;
        egui::ComboBox::from_label("Shading")
            .selected_text(match shading {
                crate::renderer::poisson_mesh::geometry::MeshShading::Auto => "Auto",
                crate::renderer::poisson_mesh::geometry::MeshShading::Flat => "Flat",
                crate::renderer::poisson_mesh::geometry::MeshShading::Smooth => "Smooth",
            })
            .show_ui(ui, |ui| {
                use crate::renderer::poisson_mesh::geometry::MeshShading;
                ui.selectable_value(&mut shading, MeshShading::Auto, "Auto (60° crease)");
                ui.selectable_value(&mut shading, MeshShading::Flat, "Flat");
                ui.selectable_value(&mut shading, MeshShading::Smooth, "Smooth");
            });
        if shading != mesh.shading {
            std::sync::Arc::make_mut(mesh).set_shading(shading);
            changed = true;
        }
        if changed {
            set_objects(tree, scene);
        }
        apply_keyframe_requests(tree, runtime, keyframes);
        return;
    }
    if matches!(object.params, SdfParams::PolygonPrismParams(_)) {
        let editing = polygon_editor::active(tree) == Some(object_id);
        if ui
            .button(if editing {
                "Finish shape editing"
            } else {
                "Edit shape in viewport"
            })
            .clicked()
        {
            if editing {
                polygon_editor::finish(tree);
            } else {
                polygon_editor::begin(ui.ctx(), tree, object_id);
            }
        }
    }
    changed |= params_editor(
        ui,
        tree,
        runtime,
        &mut object.params,
        object_id,
        &mut keyframes,
    );
    ui.separator();
    ui.label(RichText::new("Image stencil").strong());
    if ui
        .button(if object.image_stencil.is_some() {
            "Replace image…"
        } else {
            "Add image…"
        })
        .clicked()
    {
        if let Some((name, bytes)) = request_image(ui.ctx(), object_id) {
            let mut replacement = crate::model::ImageStencil::new(name, bytes, false);
            if let Some(previous) = &object.image_stencil {
                replacement.offset = previous.offset;
                if !previous.image.is_empty() {
                    replacement.size = previous.size;
                }
                replacement.rotation = previous.rotation;
                replacement.both_sides = previous.both_sides;
                replacement.image_plane = previous.image_plane;
            }
            object.image_stencil = Some(replacement);
            changed = true;
        }
    }
    if let Some(stencil) = &mut object.image_stencil {
        ui.label(&stencil.name);
        changed |= ui
            .add(
                egui::DragValue::new(&mut stencil.offset.x)
                    .speed(0.01)
                    .prefix("X "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut stencil.offset.y)
                    .speed(0.01)
                    .prefix("Y "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut stencil.size.x)
                    .range(0.01..=100.0)
                    .speed(0.01)
                    .prefix("Width "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut stencil.size.y)
                    .range(0.01..=100.0)
                    .speed(0.01)
                    .prefix("Height "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut stencil.rotation)
                    .speed(1.0)
                    .suffix("°"),
            )
            .changed();
        changed |= ui
            .checkbox(&mut stencil.both_sides, "Show on both sides")
            .changed();
        if ui.button("Remove image").clicked() {
            object.image_stencil = None;
            changed = true;
        }
    }
    if changed {
        if let Some(stencil) = &object.image_stencil {
            if stencil.image_plane {
                if let SdfParams::BoxParams(box_params) = &mut object.params {
                    box_params.box_q.x = stencil.size.x * 0.5;
                    box_params.box_q.y = stencil.size.y * 0.5;
                }
            }
        }
        set_objects(tree, scene);
        if reset_pose {
            tree.make_undo_redo_snapshot();
        }
    }
    apply_keyframe_requests(tree, runtime, keyframes);
    ui.separator();
    modifiers_panel(ui, tree, runtime);
    group_optimizations_panel(ui, tree);
    ui.separator();
    super::variables::variable_bindings_editor(ui, tree);
}

fn camera_object_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
    camera_id: uuid::Uuid,
) {
    let mut cameras = crate::model::scene_cameras(tree);
    let Some(camera) = cameras.iter_mut().find(|camera| camera.uuid == camera_id) else {
        return;
    };
    let mut changed = false;
    let mut keyframes = Vec::new();
    ui.label(RichText::new("Camera settings").strong());
    changed |= ui.text_edit_singleline(&mut camera.name).changed();
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Position");
        if ui
            .add_enabled(
                camera.transform.translation != Vec3::ZERO,
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            camera.transform.translation = Vec3::ZERO;
            changed = true;
        }
    });
    changed |= animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut camera.transform.translation,
        0.01,
        camera_id,
        [
            AnimatableProperty::Position(VectorAxis::X),
            AnimatableProperty::Position(VectorAxis::Y),
            AnimatableProperty::Position(VectorAxis::Z),
        ],
        &mut keyframes,
    );

    ui.horizontal(|ui| {
        ui.label("Rotation");
        if ui
            .add_enabled(
                camera.transform.rotation != glam::Quat::IDENTITY,
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            camera.transform.rotation = glam::Quat::IDENTITY;
            changed = true;
        }
    });
    let (x, y, z) = camera.transform.rotation.to_euler(EulerRot::XYZ);
    let mut degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
    if animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut degrees,
        1.0,
        camera_id,
        [
            AnimatableProperty::Rotation(VectorAxis::X),
            AnimatableProperty::Rotation(VectorAxis::Y),
            AnimatableProperty::Rotation(VectorAxis::Z),
        ],
        &mut keyframes,
    ) {
        camera.transform.rotation = glam::Quat::from_euler(
            EulerRot::XYZ,
            degrees.x.to_radians(),
            degrees.y.to_radians(),
            degrees.z.to_radians(),
        );
        changed = true;
    }

    ui.horizontal(|ui| {
        ui.label("Scale");
        if ui
            .add_enabled(
                camera.transform.scale != Vec3::ONE,
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            camera.transform.scale = Vec3::ONE;
            changed = true;
        }
    });
    changed |= animatable_vec3_editor(
        ui,
        tree,
        runtime,
        &mut camera.transform.scale,
        0.01,
        camera_id,
        [
            AnimatableProperty::Scale(VectorAxis::X),
            AnimatableProperty::Scale(VectorAxis::Y),
            AnimatableProperty::Scale(VectorAxis::Z),
        ],
        &mut keyframes,
    );

    ui.separator();
    let focal_binding = AnimationBinding {
        object: camera_id,
        property: AnimatableProperty::CameraFocalDistance,
    };
    let focal_response = animatable_widget(ui, tree, runtime, focal_binding, |ui| {
        ui.add(
            egui::DragValue::new(&mut camera.focal_distance)
                .speed(0.02)
                .range(0.01..=100.0)
                .prefix("Focal distance  "),
        )
    });
    changed |= focal_response.changed();
    animatable_response(
        ui,
        &focal_response,
        focal_binding,
        camera.focal_distance,
        &mut keyframes,
    );

    let mut projection = camera.projection_mode;
    let projection_binding = AnimationBinding {
        object: camera_id,
        property: AnimatableProperty::CameraProjection,
    };
    let projection_response = animatable_widget(ui, tree, runtime, projection_binding, |ui| {
        egui::ComboBox::from_label("Projection")
            .selected_text(projection.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut projection,
                    crate::camera::ProjectionMode::Perspective,
                    "Perspective",
                );
                ui.selectable_value(
                    &mut projection,
                    crate::camera::ProjectionMode::Orthographic,
                    "Orthographic",
                );
            })
            .response
    });
    changed |= projection_response.changed();
    camera.projection_mode = projection;
    animatable_response(
        ui,
        &projection_response,
        projection_binding,
        if projection == crate::camera::ProjectionMode::Orthographic {
            1.0
        } else {
            0.0
        },
        &mut keyframes,
    );

    let mut camera_view = matches!(
        tree.get_path("editor.camera_view"),
        crate::model::ClaydashValue::Bool(true)
    );
    if ui
        .checkbox(&mut camera_view, "View through camera")
        .changed()
    {
        tree.set_path(
            "scene.active_camera",
            crate::model::ClaydashValue::Uuid(camera_id),
        );
        tree.set_transient_path(
            "editor.camera_view",
            crate::model::ClaydashValue::Bool(camera_view),
        );
    }

    if changed {
        crate::model::set_scene_cameras(tree, cameras);
        tree.make_undo_redo_snapshot();
    }
    apply_keyframe_requests(tree, runtime, keyframes);
}

fn group_transform_panel(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
    group: uuid::Uuid,
) {
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == group) else {
        return;
    };
    let position_linked = crate::model::vector_binding(
        tree,
        group,
        crate::model::VectorBindingTarget::GroupPosition,
    )
    .is_some();
    ui.label(RichText::new("Group transform").strong());
    let mut changed = false;
    let mut keyframes = Vec::new();
    let variables = crate::model::scene_variables(tree);
    let rigid_bound =
        crate::model::rigid_binding(&variables, group, crate::model::RigidBindingTarget::Group)
            .is_some();
    ui.add_enabled_ui(!rigid_bound, |ui| {
        ui.horizontal(|ui| {
            ui.label("Position");
            if ui
                .add_enabled(
                    !position_linked && object.group_transform.translation != Vec3::ZERO,
                    egui::Button::new("Reset"),
                )
                .clicked()
            {
                object.group_transform.translation = Vec3::ZERO;
                changed = true;
            }
        });
        changed |= ui
            .add_enabled_ui(!position_linked, |ui| {
                animatable_vec3_editor(
                    ui,
                    tree,
                    runtime,
                    &mut object.group_transform.translation,
                    0.01,
                    group,
                    [
                        AnimatableProperty::GroupPosition(VectorAxis::X),
                        AnimatableProperty::GroupPosition(VectorAxis::Y),
                        AnimatableProperty::GroupPosition(VectorAxis::Z),
                    ],
                    &mut keyframes,
                )
            })
            .inner;
        ui.horizontal(|ui| {
            ui.label("Rotation");
            if ui
                .add_enabled(
                    object.group_transform.rotation != glam::Quat::IDENTITY,
                    egui::Button::new("Reset"),
                )
                .clicked()
            {
                object.group_transform.rotation = glam::Quat::IDENTITY;
                changed = true;
            }
        });
        let (x, y, z) = object.group_transform.rotation.to_euler(EulerRot::XYZ);
        let mut degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
        if animatable_vec3_editor(
            ui,
            tree,
            runtime,
            &mut degrees,
            1.0,
            group,
            [
                AnimatableProperty::GroupRotation(VectorAxis::X),
                AnimatableProperty::GroupRotation(VectorAxis::Y),
                AnimatableProperty::GroupRotation(VectorAxis::Z),
            ],
            &mut keyframes,
        ) {
            object.group_transform.rotation = glam::Quat::from_euler(
                EulerRot::XYZ,
                degrees.x.to_radians(),
                degrees.y.to_radians(),
                degrees.z.to_radians(),
            );
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.label("Scale");
            if ui
                .add_enabled(
                    object.group_transform.scale != Vec3::ONE,
                    egui::Button::new("Reset"),
                )
                .clicked()
            {
                object.group_transform.scale = Vec3::ONE;
                changed = true;
            }
        });
        changed |= animatable_vec3_editor(
            ui,
            tree,
            runtime,
            &mut object.group_transform.scale,
            0.01,
            group,
            [
                AnimatableProperty::GroupScale(VectorAxis::X),
                AnimatableProperty::GroupScale(VectorAxis::Y),
                AnimatableProperty::GroupScale(VectorAxis::Z),
            ],
            &mut keyframes,
        );
    })
    .response
    .on_disabled_hover_text(
        "Transform driven by shared points; unlink the rigid binding to edit it",
    );
    if changed {
        set_objects(tree, scene);
    }
    apply_keyframe_requests(tree, runtime, keyframes);
}

pub(super) fn params_editor(
    ui: &mut egui::Ui,
    tree: &DataTree,
    runtime: &AnimationRuntime,
    params: &mut SdfParams,
    object: uuid::Uuid,
    keyframes: &mut Vec<KeyframeRequest>,
) -> bool {
    match params {
        SdfParams::SphereParams(value) => {
            let binding = AnimationBinding {
                object,
                property: AnimatableProperty::SphereRadius,
            };
            let response = animatable_widget(ui, tree, runtime, binding, |ui| {
                ui.add(egui::Slider::new(&mut value.radius, 0.01..=4.0).text("Radius"))
            });
            animatable_response(ui, &response, binding, value.radius, keyframes);
            response.changed()
        }
        SdfParams::BoxParams(value) => {
            ui.label("Half extents");
            let extent_changed = animatable_vec3_editor(
                ui,
                tree,
                runtime,
                &mut value.box_q,
                0.01,
                object,
                [
                    AnimatableProperty::BoxHalfExtent(VectorAxis::X),
                    AnimatableProperty::BoxHalfExtent(VectorAxis::Y),
                    AnimatableProperty::BoxHalfExtent(VectorAxis::Z),
                ],
                keyframes,
            );
            let radius_changed = ui
                .add(
                    egui::Slider::new(
                        &mut value.corner_radius,
                        0.0..=value.box_q.min_element().max(0.0),
                    )
                    .text("Corner radius"),
                )
                .changed();
            extent_changed | radius_changed
        }
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => {
            let radius_binding = AnimationBinding {
                object,
                property: AnimatableProperty::CylinderRadius,
            };
            let radius_response = animatable_widget(ui, tree, runtime, radius_binding, |ui| {
                ui.add(egui::Slider::new(radius, 0.01..=4.0).text("Radius"))
            });
            animatable_response(ui, &radius_response, radius_binding, *radius, keyframes);
            let height_binding = AnimationBinding {
                object,
                property: AnimatableProperty::CylinderHalfHeight,
            };
            let height_response = animatable_widget(ui, tree, runtime, height_binding, |ui| {
                ui.add(egui::Slider::new(half_height, 0.01..=4.0).text("Half height"))
            });
            animatable_response(
                ui,
                &height_response,
                height_binding,
                *half_height,
                keyframes,
            );
            radius_response.changed() | height_response.changed()
        }
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => {
            let major_binding = AnimationBinding {
                object,
                property: AnimatableProperty::TorusMajorRadius,
            };
            let major_response = animatable_widget(ui, tree, runtime, major_binding, |ui| {
                ui.add(egui::Slider::new(major_radius, 0.02..=4.0).text("Major radius"))
            });
            animatable_response(ui, &major_response, major_binding, *major_radius, keyframes);
            let minor_binding = AnimationBinding {
                object,
                property: AnimatableProperty::TorusMinorRadius,
            };
            let minor_response = animatable_widget(ui, tree, runtime, minor_binding, |ui| {
                ui.add(egui::Slider::new(minor_radius, 0.01..=2.0).text("Tube radius"))
            });
            animatable_response(ui, &minor_response, minor_binding, *minor_radius, keyframes);
            major_response.changed() | minor_response.changed()
        }
        SdfParams::PolygonPrismParams(value) => {
            let depth_changed = ui
                .add(egui::Slider::new(&mut value.half_depth, 0.005..=4.0).text("Half depth"))
                .changed();
            let softness_changed = ui
                .add(
                    egui::Slider::new(&mut value.edge_softness, 0.0..=value.half_depth.min(0.5))
                        .text("Edge softness"),
                )
                .on_hover_text("Round the outline and its front and back edges")
                .changed();
            depth_changed | softness_changed
        }
        SdfParams::SheetPanelParams(_) => false,
        SdfParams::BezierCurveParams(value) => {
            ui.label(format!(
                "{} Bézier segment{}",
                value.segment_count(),
                if value.segment_count() == 1 { "" } else { "s" }
            ));
            ui.weak("G moves a point · Backspace removes an anchor · E extends · Enter closes the loop.");
            false
        }
        SdfParams::TextParams(value) => {
            let mut changed = ui
                .add(egui::TextEdit::multiline(&mut value.text).desired_rows(3))
                .changed();
            ui.horizontal(|ui| {
                ui.label("Size");
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut value.size)
                            .speed(0.01)
                            .range(0.001..=100.0),
                    )
                    .changed();
                ui.label("Half-depth");
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut value.half_depth)
                            .speed(0.005)
                            .range(0.001..=100.0),
                    )
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.label("Tracking");
                changed |= ui
                    .add(egui::DragValue::new(&mut value.tracking).speed(0.005))
                    .changed();
                ui.label("Line spacing");
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut value.line_spacing)
                            .speed(0.01)
                            .range(0.01..=10.0),
                    )
                    .changed();
            });
            let mut scene = objects(tree);
            let path_name = value
                .path
                .and_then(|id| scene.iter().find(|candidate| candidate.uuid == id))
                .map_or("Straight".to_string(), |path| path.display_name());
            egui::ComboBox::from_label("Placement")
                .selected_text(path_name)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut value.path, None, "Straight")
                        .changed();
                    for candidate in &scene {
                        if candidate.uuid != object
                            && matches!(candidate.params, SdfParams::BezierCurveParams(_))
                        {
                            changed |= ui
                                .selectable_value(
                                    &mut value.path,
                                    Some(candidate.uuid),
                                    candidate.display_name(),
                                )
                                .changed();
                        }
                    }
                });
            if let Some(preview) = scene.iter_mut().find(|candidate| candidate.uuid == object) {
                preview.params = SdfParams::TextParams(value.clone());
            }
            if let Err(error) = crate::model::validate_scene_text_budget(&scene) {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            changed
        }
    }
}

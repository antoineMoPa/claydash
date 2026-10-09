use super::*;

fn scene_render_versions(tree: &DataTree, capture_render: bool) -> [i32; 2] {
    let export_version = if capture_render { i32::MIN } else { 0 };
    // Material source and parameters both affect the cached viewport image.
    [
        tree.path_version("scene.sdf_objects")
            .wrapping_add(tree.path_version("scene.materials"))
            .wrapping_add(tree.path_version("scene.selected_uuids")),
        tree.path_version("scene.world")
            .wrapping_add(export_version),
    ]
}

pub(super) fn mesh_source_revision(objects: &[crate::model::SdfObject]) -> i32 {
    use std::hash::{Hash, Hasher};
    #[derive(serde::Serialize)]
    struct Geometry<'a> {
        id: uuid::Uuid,
        transform: Option<&'a crate::model::Transform>,
        group_transform: Option<&'a crate::model::Transform>,
        object_type: i32,
        params: &'a crate::model::SdfParams,
        operation: crate::model::BooleanOperation,
        boolean_parent: Option<uuid::Uuid>,
        softness: f32,
        repetition: &'a crate::model::Repetition,
        mirror: &'a Option<crate::model::Mirror>,
        lattice: &'a Option<crate::model::Lattice>,
        path_extrusion: &'a Option<crate::model::PathExtrusion>,
        surface_inlay: &'a Option<crate::model::SurfaceInlay>,
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let independent: std::collections::HashSet<_> = objects.iter()
        .filter(|object| object.boolean_parent.is_none()
            && crate::renderer::poisson_mesh::geometry::mesh_pose_is_independent(objects, object.uuid))
        .map(|object| object.uuid).collect();
    for object in objects {
        let independent_pose = independent.contains(&object.uuid);
        let geometry = Geometry {
            id: object.uuid, transform: (!independent_pose
                || !crate::renderer::poisson_mesh::geometry::mesh_uses_object_pose(objects, object.uuid)).then_some(&object.transform),
            group_transform: (!independent_pose).then_some(&object.group_transform),
            object_type: object.object_type, params: &object.params,
            operation: object.operation, boolean_parent: object.boolean_parent,
            softness: object.softness, repetition: &object.repetition,
            mirror: &object.mirror, lattice: &object.lattice,
            path_extrusion: &object.path_extrusion, surface_inlay: &object.surface_inlay,
        };
        serde_json::to_vec(&geometry).expect("serialize mesh geometry")
            .hash(&mut hasher);
    }
    hasher.finish() as i32
}

#[cfg(test)]
#[test]
fn selection_change_invalidates_rendered_proxy_highlight() {
    let mut tree = DataTree::default();
    let object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    let id = object.uuid;
    crate::model::set_objects(&mut tree, vec![object]);
    let before = scene_render_versions(&tree, false);
    crate::model::set_selected_exact(&mut tree, vec![id]);
    assert_ne!(scene_render_versions(&tree, false), before);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
fn selection_and_camera_changes_keep_mesh_source_revision() {
    let mut tree = DataTree::default();
    let object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    let id = object.uuid;
    crate::model::set_objects(&mut tree, vec![object]);
    let mesh_revision = mesh_source_revision(crate::model::objects_ref(&tree));
    let viewport_revision = scene_render_versions(&tree, false)[0];
    crate::model::set_selected_exact(&mut tree, vec![id]);
    assert_eq!(mesh_source_revision(crate::model::objects_ref(&tree)), mesh_revision);
    assert_ne!(scene_render_versions(&tree, false)[0], viewport_revision);
    // Editor view state is outside the source object path.
    tree.set_path("scene.cursor_position", ClaydashValue::Vec3(glam::Vec3::new(2.0, 3.0, 4.0)));
    assert_eq!(mesh_source_revision(crate::model::objects_ref(&tree)), mesh_revision);
    let mut appearance = crate::model::objects(&tree);
    appearance[0].color = glam::Vec4::new(0.8, 0.2, 0.1, 1.0);
    appearance[0].material.opacity = 0.5;
    crate::model::set_objects(&mut tree, appearance);
    assert_eq!(mesh_source_revision(crate::model::objects_ref(&tree)), mesh_revision);
    crate::model::set_objects(&mut tree, vec![]);
    assert_ne!(mesh_source_revision(crate::model::objects_ref(&tree)), mesh_revision);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
fn fusca_global_pose_keeps_mesh_but_part_edits_invalidate_it() {
    let scene = crate::document::deserialize_scene(include_bytes!("../../examples/fusca.claydash")).unwrap();
    let mut objects = match scene.get_path("sdf_objects") {
        ClaydashValue::VecSDFObject(objects) => objects,
        _ => panic!("Fusca objects missing"),
    };
    let root = objects.iter().position(|object| object.boolean_parent.is_none()).unwrap();
    let revision = mesh_source_revision(&objects);
    objects[root].group_transform.translation = glam::Vec3::new(2.0, -3.0, 4.0);
    objects[root].group_transform.rotation = glam::Quat::from_rotation_y(0.8);
    objects[root].group_transform.scale = glam::Vec3::new(1.2, 0.7, 2.0);
    assert_eq!(mesh_source_revision(&objects), revision, "global pose must reuse the mesh, including internal inlays");
    let child = objects.iter().position(|object| object.boolean_parent.is_some()).unwrap();
    objects[child].transform.translation.x += 0.1;
    assert_ne!(mesh_source_revision(&objects), revision, "editing a part changes the shape");
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
fn standalone_mesh_pose_reuses_shape_but_external_references_do_not() {
    let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    let original = mesh_source_revision(&[object.clone()]);
    object.transform.translation.x += 3.0;
    object.transform.rotation = glam::Quat::from_rotation_z(0.7);
    object.transform.scale = glam::Vec3::new(1.0, 2.0, 3.0);
    assert_eq!(mesh_source_revision(&[object.clone()]), original);
    let mut patch = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    patch.surface_inlay = Some(crate::model::SurfaceInlay { host: object.uuid, offset: 0.0, thickness: 0.01 });
    let mut scene = vec![object, patch];
    let original = mesh_source_revision(&scene);
    scene[0].transform.translation.x += 1.0;
    assert_ne!(mesh_source_revision(&scene), original, "external inlay geometry depends on the relative pose");
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
fn mesh_resolution_settings_do_not_invalidate_displayed_geometry() {
    let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    let revision = mesh_source_revision(&[object.clone()]);
    object.gaussian_splats.resolution = 96;
    object.poisson_mesh.resolution = 96;
    assert_eq!(mesh_source_revision(&[object]), revision);
}

impl App {
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn advance_background_computations(&mut self) -> bool {
        // Explicit image/video exports keep rendering offscreen. Ordinary
        // viewport refinement remains paused while the window is inactive.
        if self.pending_render.is_some() {
            self.redraw();
            return true;
        }
        self.poll_native_encoding();
        self.poll_mesh_export();
        let mut busy = self.encoding.is_some() || self.mesh_export.is_some();
        if let Some(renderer) = &mut self.renderer {
            renderer.poll_background_gpu();
            if renderer.has_pending_computations(&self.egui) {
                let source = crate::model::objects_ref(&self.tree);
                renderer.advance_computations(&self.camera, source,
                    &commands::effective_selected_ids(&self.tree),
                    scene_render_versions(&self.tree, false), mesh_source_revision(source),
                    crate::model::world(&self.tree), &self.egui, true);
                busy |= renderer.has_pending_computations(&self.egui);
            }
            let previews = self.egui.data(|data|
                data.get_temp::<crate::renderer::MaterialPreviewRequests>(
                    crate::renderer::MaterialPreviewRequests::egui_id()).unwrap_or_default());
            if renderer.has_pending_material_previews(&previews.0) {
                renderer.sync_visible_material_previews(&previews.0, &self.egui);
                busy |= renderer.has_pending_material_previews(&previews.0);
            }
        }
        busy
    }

    pub(super) fn render_progress(&self) -> Option<crate::ui::RenderProgress> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(pending) = &self.pending_render {
                return Some(match pending {
                    PendingRender::Still { .. } => crate::ui::RenderProgress::Image,
                    PendingRender::Video {
                        start_frame,
                        next_frame,
                        end_frame,
                        ..
                    } => crate::ui::RenderProgress::Video {
                        completed: next_frame - start_frame,
                        total: end_frame - start_frame + 1,
                    },
                });
            }
            self.encoding.as_ref().map(|job| {
                if job.cancelling {
                    crate::ui::RenderProgress::Cancelling
                } else if job.format == crate::document::RenderFormat::WebP {
                    crate::ui::RenderProgress::Finalizing
                } else {
                    crate::ui::RenderProgress::Encoding {
                        completed: job.control.completed.load(Ordering::Relaxed),
                        total: job.total,
                    }
                }
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.pending_render
                .as_ref()
                .map(|pending| match pending {
                    WebPendingRender::Still { .. } => crate::ui::RenderProgress::Image,
                    WebPendingRender::Video {
                        start_frame,
                        next_frame,
                        end_frame,
                        ..
                    } => crate::ui::RenderProgress::Video {
                        completed: next_frame - start_frame,
                        total: end_frame - start_frame + 1,
                    },
                })
                .or_else(|| {
                    self.encoding
                        .as_ref()
                        .map(|_| crate::ui::RenderProgress::Finalizing)
                })
        }
    }

    pub(super) fn cancel_render(&mut self) {
        if let Some(renderer) = &mut self.renderer {
            self.discard_capture |= renderer.capture_pending();
            renderer.cancel_pending_capture();
        }
        #[cfg(all(not(target_arch = "wasm32"), unix))]
        if let Some(capture) = self.agent_capture.take() {
            let _ = capture.reply.send(Err("Capture cancelled".into()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(pending) = self.pending_render.take() {
                if let PendingRender::Video {
                    frames_directory,
                    restore_frame,
                    ..
                } = pending
                {
                    let _ = std::fs::remove_dir_all(frames_directory);
                    self.ui.set_animation_frame(&mut self.tree, restore_frame);
                }
            }
            if let Some(job) = &mut self.encoding {
                job.control.cancelled.store(true, Ordering::Relaxed);
                job.cancelling = true;
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(WebPendingRender::Video { restore_frame, .. }) = self.pending_render.take()
            {
                self.ui.set_animation_frame(&mut self.tree, restore_frame);
            }
            if let Some(job) = self.encoding.take() {
                job.cancel.cancel();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn poll_native_encoding(&mut self) {
        let Some(job) = &self.encoding else {
            return;
        };
        match job.completed.try_recv() {
            Ok(result) => {
                let cancelled = job.cancelling;
                let format = job.format;
                self.encoding = None;
                if !cancelled {
                    if let Err(error) = result {
                        self.document.set_error(
                            if format == crate::document::RenderFormat::WebP {
                                "render the scene"
                            } else {
                                "encode the animation"
                            },
                            error,
                        );
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let format = job.format;
                let cancelled = job.cancelling;
                self.encoding = None;
                if !cancelled {
                    self.document.set_error(
                        if format == crate::document::RenderFormat::WebP {
                            "render the scene"
                        } else {
                            "encode the animation"
                        },
                        "the encoder stopped unexpectedly",
                    );
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    pub(super) fn redraw(&mut self) {
        #[cfg(target_arch = "wasm32")]
        self.process_web_document_messages();
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_native_encoding();
        self.poll_mesh_export();
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        let material_assets = crate::model::material_assets(&self.tree);
        let shader_error = renderer.sync_custom_materials(&material_assets).err();
        self.egui.data_mut(|data| {
            data.insert_temp(egui::Id::new("custom-material-render-error"), shader_error)
        });
        if self.camera.viewport == Vec2::ONE {
            self.camera.viewport = renderer.size();
        }
        if !self.ui.selection_gesture_active() {
            if self.interactions.update(&mut self.camera, &mut self.tree)
                && !self.interactions.view_rotation_active()
            {
                crate::ui::exit_camera_view(&mut self.tree);
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        let input = match &mut self.egui_state {
            Some(egui_state) => egui_state.take_egui_input(&window),
            None => return,
        };
        #[cfg(target_arch = "wasm32")]
        let input = self.egui_state.take(&window);
        let egui = self.egui.clone();
        egui.data_mut(|data| {
            data.insert_temp(
                crate::renderer::MaterialPreviewIds::egui_id(),
                renderer.material_preview_ids(&material_assets),
            );
            data.insert_temp(
                crate::renderer::MaterialPreviewRequests::egui_id(),
                crate::renderer::MaterialPreviewRequests::default(),
            );
        });
        let mut file_action = None;
        let mut cancel_render = false;
        let material_version_before_ui = self.tree.path_version("scene.materials");
        let render_progress = self.render_progress();
        let interaction_guide = self.interactions.active_guide();
        let mut output = egui.run_ui(input, |ui| {
            (file_action, cancel_render) = self.ui.draw(
                ui,
                &mut self.tree,
                &mut self.commands,
                &mut self.camera,
                &mut self.document,
                interaction_guide,
                render_progress,
            );
            self.mesh_export_panel(ui);
        });
        let preview_requests = egui.data(|data| {
            data.get_temp::<crate::renderer::MaterialPreviewRequests>(
                crate::renderer::MaterialPreviewRequests::egui_id(),
            )
            .unwrap_or_default()
        });
        if let Some(renderer) = &mut self.renderer {
            renderer.sync_visible_material_previews(&preview_requests.0, &egui);
        }
        // UI edits happen after shader synchronization at the start of this frame.
        // Schedule one more frame so a newly applied WGSL body is compiled promptly.
        if self.tree.path_version("scene.materials") != material_version_before_ui {
            window.request_redraw();
        }
        if cancel_render {
            self.cancel_render();
        }
        if let Some(action) = file_action {
            self.handle_file_action(action);
        }
        self.document.refresh_dirty(&self.tree);
        let project_name = self
            .document
            .current_path()
            .and_then(std::path::Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        window.set_title(&format!(
            "{}{} - Claydash",
            project_name,
            if self.document.is_dirty() { "*" } else { "" }
        ));
        self.document
            .set_animation_timeline_open(self.ui.animation_timeline_open());
        // Toolbar/palette commands run inside the UI pass. Place their new
        // objects before rendering, using the just-updated viewport geometry.
        self.interactions
            .place_pending_spawn(&self.camera, &mut self.tree);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(egui_state) = &mut self.egui_state {
            egui_state.handle_platform_output(&window, std::mem::take(&mut output.platform_output));
        }

        let effective_selection = if self.pending_render.is_some() {
            Vec::new()
        } else {
            commands::effective_selected_ids(&self.tree)
        };
        #[cfg(not(target_arch = "wasm32"))]
        let capture_render = self.pending_render.is_some() || self.guide_screenshot.is_some() || {
            #[cfg(unix)]
            {
                self.agent_capture.is_some()
            }
            #[cfg(not(unix))]
            {
                false
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        let capture_ui = self.guide_screenshot.is_some();
        #[cfg(target_arch = "wasm32")]
        let capture_render = self.pending_render.is_some();
        #[cfg(target_arch = "wasm32")]
        let capture_ui = false;
        #[cfg(all(not(target_arch = "wasm32"), unix))]
        let offscreen_capture = self.agent_capture.is_some()
            || (self.pending_render.is_some() && (!self.window_focused || self.window_occluded));
        #[cfg(all(not(target_arch = "wasm32"), not(unix)))]
        let offscreen_capture = self.pending_render.is_some()
            && (!self.window_focused || self.window_occluded);
        #[cfg(target_arch = "wasm32")]
        let offscreen_capture = false;
        #[cfg(all(not(target_arch = "wasm32"), unix))]
        let render_camera = self.agent_capture.as_ref().map_or_else(
            || self.camera.clone(),
            |capture| capture.camera(&self.camera),
        );
        #[cfg(any(target_arch = "wasm32", not(unix)))]
        let render_camera = self.camera.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let refine = if self.pending_render.is_some() || self.guide_screenshot.is_some() {
            true
        } else {
            #[cfg(unix)]
            {
                self.agent_capture
                    .as_ref()
                    .map_or(commands::full_material_rendering(&self.tree), |capture| {
                        capture.mode == agent::CaptureMode::FullMaterial
                    })
            }
            #[cfg(not(unix))]
            {
                commands::full_material_rendering(&self.tree)
            }
        };
        #[cfg(target_arch = "wasm32")]
        let refine = self.pending_render.is_some() || commands::full_material_rendering(&self.tree);
        #[cfg(not(target_arch = "wasm32"))]
        let animation_playback = self.ui.animation_playing()
            || self
                .ui_benchmark
                .as_ref()
                .is_some_and(|benchmark| benchmark.animation);
        #[cfg(target_arch = "wasm32")]
        let animation_playback = self.ui.animation_playing();
        #[cfg(all(not(target_arch = "wasm32"), unix))]
        let outline_capture = self
            .agent_capture
            .as_ref()
            .is_some_and(|capture| capture.mode == agent::CaptureMode::Outline);
        #[cfg(any(target_arch = "wasm32", not(unix)))]
        let outline_capture = false;
        // Pointer gizmos and modal G/R/S transforms can pause between motion
        // events. Do not queue expensive refinement ahead of their next edit.
        let interacting = !capture_render && (self.egui.input(|input| input.pointer.any_down())
            || matches!(self.tree.get_path("editor.state"),
                ClaydashValue::EditorState(state) if state != EditorState::Start));
        if let Some(renderer) = &mut self.renderer {
            // Selection is drawn by the editor gizmos. Keep the cached scene
            // image when only selection changes, avoiding a full refinement.
            let scene_versions = scene_render_versions(&self.tree, capture_render);
            #[cfg(all(not(target_arch = "wasm32"), unix))]
            let scene_objects = self
                .agent_capture
                .as_ref()
                .and_then(|capture| capture.objects.as_deref())
                .unwrap_or_else(|| objects_ref(&self.tree));
            #[cfg(any(target_arch = "wasm32", not(unix)))]
            let scene_objects = objects_ref(&self.tree);
            let presented = renderer.render(
                &render_camera,
                scene_objects,
                &effective_selection,
                scene_versions,
                mesh_source_revision(scene_objects),
                crate::model::world(&self.tree),
                crate::model::post_processing_ref(&self.tree),
                &self.egui,
                &mut output,
                capture_render,
                capture_ui,
                offscreen_capture,
                refine,
                animation_playback,
                interacting,
                outline_capture,
                commands::outline_mode(&self.tree) && !capture_render,
            );
            #[cfg(target_arch = "wasm32")]
            if presented && !self.web_loading_complete {
                if let Some(loader) = web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| document.get_element_by_id("loading-message"))
                {
                    loader.remove();
                }
                self.web_loading_complete = true;
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = presented;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(result) = renderer.take_capture() {
                #[cfg(unix)]
                let agent_handled = if let Some(mut capture) = self.agent_capture.take() {
                    let encoded = result
                        .as_ref()
                        .map_err(Clone::clone)
                        .and_then(|frame| capture.accept_frame(frame.clone(), &render_camera));
                    if matches!(encoded, Ok(serde_json::Value::Null)) {
                        self.agent_capture = Some(capture);
                        renderer.invalidate_scene();
                    } else {
                        let _ = capture.reply.send(encoded);
                    }
                    true
                } else {
                    false
                };
                #[cfg(not(unix))]
                let agent_handled = false;
                if !agent_handled {
                    if self.discard_capture {
                        self.discard_capture = false;
                    } else if let Err(error) = result {
                        if let Some(PendingRender::Video {
                            frames_directory,
                            restore_frame,
                            ..
                        }) = self.pending_render.take()
                        {
                            let _ = std::fs::remove_dir_all(frames_directory);
                            self.ui.set_animation_frame(&mut self.tree, restore_frame);
                        }
                        if self.guide_screenshot.is_some() {
                            panic!("Could not capture guide screenshot: {error}");
                        }
                        self.document.set_error("render the scene", error);
                    } else if let Ok(frame) = result {
                        if let Some(path) = self.guide_screenshot.take() {
                            let result = path
                                .parent()
                                .filter(|parent| !parent.as_os_str().is_empty())
                                .map(std::fs::create_dir_all)
                                .transpose()
                                .and_then(|_| {
                                    crate::render_export::png_bytes(&frame)
                                        .map_err(std::io::Error::other)
                                })
                                .and_then(|bytes| std::fs::write(&path, bytes));
                            result.unwrap_or_else(|error| {
                                panic!(
                                    "Could not write guide screenshot {}: {error}",
                                    path.display()
                                )
                            });
                            eprintln!("Guide screenshot: {}", path.display());
                            self.guide_capture_done = true;
                        }
                        if let Some(pending) = self.pending_render.take() {
                            let frame = crate::render_export::crop_to_viewport(frame, &self.camera);
                            match pending {
                                PendingRender::Still { path } => {
                                    let control =
                                        Arc::new(crate::render_export::EncodingControl::default());
                                    let worker_control = control.clone();
                                    let (tx, completed) = channel();
                                    std::thread::spawn(move || {
                                        let staging_path = path.with_extension(format!(
                                            "webp.partial-{}",
                                            uuid::Uuid::new_v4()
                                        ));
                                        let result =
                                            crate::render_export::write_render_with_control(
                                                &staging_path,
                                                crate::document::RenderFormat::WebP,
                                                &frame,
                                                &worker_control,
                                            )
                                            .and_then(
                                                |_| {
                                                    if worker_control
                                                        .cancelled
                                                        .load(Ordering::Relaxed)
                                                    {
                                                        return Err("render cancelled".into());
                                                    }
                                                    std::fs::rename(&staging_path, &path)
                                                        .map_err(|error| error.to_string())
                                                },
                                            );
                                        let _ = std::fs::remove_file(&staging_path);
                                        let _ = tx.send(result);
                                    });
                                    self.encoding = Some(NativeEncoding {
                                        control,
                                        completed,
                                        total: 0,
                                        cancelling: false,
                                        format: crate::document::RenderFormat::WebP,
                                    });
                                }
                                PendingRender::Video {
                                    path,
                                    frames_directory,
                                    start_frame,
                                    next_frame,
                                    end_frame,
                                    output_index,
                                    fps,
                                    restore_frame,
                                } => {
                                    let write = crate::render_export::write_video_frame(
                                        &frames_directory,
                                        output_index,
                                        &frame,
                                    );
                                    if let Err(error) = write {
                                        self.document.set_error("render the animation", error);
                                        let _ = std::fs::remove_dir_all(&frames_directory);
                                        self.ui.set_animation_frame(&mut self.tree, restore_frame);
                                    } else if next_frame < end_frame {
                                        let next_frame = next_frame + 1;
                                        self.ui
                                            .set_animation_frame(&mut self.tree, next_frame as f32);
                                        self.pending_render = Some(PendingRender::Video {
                                            path,
                                            frames_directory,
                                            start_frame,
                                            next_frame,
                                            end_frame,
                                            output_index: output_index + 1,
                                            fps,
                                            restore_frame,
                                        });
                                    } else {
                                        self.ui.set_animation_frame(&mut self.tree, restore_frame);
                                        let control = Arc::new(
                                            crate::render_export::EncodingControl::default(),
                                        );
                                        let worker_control = control.clone();
                                        let (tx, completed) = channel();
                                        std::thread::spawn(move || {
                                            let staging_path = path.with_extension(format!(
                                                "mp4.partial-{}",
                                                uuid::Uuid::new_v4()
                                            ));
                                            let result =
                                                crate::render_export::write_mp4_with_control(
                                                    &staging_path,
                                                    &frames_directory,
                                                    fps,
                                                    &worker_control,
                                                )
                                                .and_then(|_| {
                                                    if worker_control
                                                        .cancelled
                                                        .load(Ordering::Relaxed)
                                                    {
                                                        return Err("render cancelled".into());
                                                    }
                                                    std::fs::rename(&staging_path, &path)
                                                        .map_err(|error| error.to_string())
                                                });
                                            let _ = std::fs::remove_dir_all(&frames_directory);
                                            let _ = std::fs::remove_file(&staging_path);
                                            let _ = tx.send(result);
                                        });
                                        self.encoding = Some(NativeEncoding {
                                            control,
                                            completed,
                                            total: end_frame - start_frame + 1,
                                            cancelling: false,
                                            format: crate::document::RenderFormat::Mp4,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            {
                if let Some(result) = renderer.take_capture() {
                    if self.discard_capture {
                        self.discard_capture = false;
                    } else if let Some(pending) = self.pending_render.take() {
                        match result {
                            Ok(frame) => {
                                let frame =
                                    crate::render_export::crop_to_viewport(frame, &self.camera);
                                match pending {
                                    WebPendingRender::Still { name } => {
                                        let cancel = crate::render_export::WebCancel::image();
                                        let worker_cancel = cancel.clone();
                                        self.next_render_id = self.next_render_id.wrapping_add(1);
                                        let id = self.next_render_id;
                                        self.encoding = Some(WebEncoding { id, cancel });
                                        let tx = self.document_tx.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let result = crate::render_export::download_webp(
                                                &name,
                                                &frame,
                                                &worker_cancel,
                                            )
                                            .await;
                                            if !worker_cancel.is_cancelled() {
                                                let message = match result {
                                                    Ok(()) => {
                                                        WebDocumentMessage::RenderFinished(id)
                                                    }
                                                    Err(message) => {
                                                        WebDocumentMessage::RenderError {
                                                            id,
                                                            video: false,
                                                            message,
                                                        }
                                                    }
                                                };
                                                let _ = tx.send(message);
                                            }
                                        });
                                    }
                                    WebPendingRender::Video {
                                        name,
                                        mut encoder,
                                        start_frame,
                                        next_frame,
                                        end_frame,
                                        fps,
                                        restore_frame,
                                    } => {
                                        let encode = (|| {
                                            if encoder.is_none() {
                                                encoder =
                                                    Some(crate::render_export::WebVideo::new(
                                                        &frame, fps,
                                                    )?);
                                            }
                                            encoder.as_mut().unwrap().push(&frame)
                                        })();
                                        if let Err(error) = encode {
                                            self.ui
                                                .set_animation_frame(&mut self.tree, restore_frame);
                                            self.document.set_error("render the animation", error);
                                        } else if next_frame < end_frame {
                                            let next_frame = next_frame + 1;
                                            self.ui.set_animation_frame(
                                                &mut self.tree,
                                                next_frame as f32,
                                            );
                                            self.pending_render = Some(WebPendingRender::Video {
                                                name,
                                                encoder,
                                                start_frame,
                                                next_frame,
                                                end_frame,
                                                fps,
                                                restore_frame,
                                            });
                                        } else {
                                            self.ui
                                                .set_animation_frame(&mut self.tree, restore_frame);
                                            let tx = self.document_tx.clone();
                                            let video = encoder
                                                .expect("captured frame initialized encoder");
                                            let cancel =
                                                crate::render_export::WebCancel::video(&video);
                                            let worker_cancel = cancel.clone();
                                            self.next_render_id =
                                                self.next_render_id.wrapping_add(1);
                                            let id = self.next_render_id;
                                            self.encoding = Some(WebEncoding { id, cancel });
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let result =
                                                    video.finish(&name, fps, &worker_cancel).await;
                                                if !worker_cancel.is_cancelled() {
                                                    let message = match result {
                                                        Ok(()) => {
                                                            WebDocumentMessage::RenderFinished(id)
                                                        }
                                                        Err(message) => {
                                                            WebDocumentMessage::RenderError {
                                                                id,
                                                                video: true,
                                                                message,
                                                            }
                                                        }
                                                    };
                                                    let _ = tx.send(message);
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                if let WebPendingRender::Video { restore_frame, .. } = pending {
                                    self.ui.set_animation_frame(&mut self.tree, restore_frame);
                                    self.document.set_error("render the animation", error);
                                } else {
                                    self.document.set_error("render the scene", error);
                                }
                            }
                        }
                    }
                }
            }
        }
        self.tree.reset_update_cycle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_only_wgsl_invalidates_the_viewport_key() {
        let mut tree = DataTree::default();
        let mut asset = crate::model::MaterialAsset::custom("Bands".into());
        crate::model::set_material_assets(&mut tree, vec![asset.clone()]);
        let before = scene_render_versions(&tree, false);
        asset.wgsl = Some("return base;".into());
        crate::model::set_material_assets(&mut tree, vec![asset]);
        assert_ne!(scene_render_versions(&tree, false), before);
        assert_ne!(scene_render_versions(&tree, true), before);
    }
}

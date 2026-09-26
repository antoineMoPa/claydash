use super::*;

impl App {
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
        if let Some(renderer) = &self.renderer {
            self.discard_capture |= renderer.capture_pending();
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
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        renderer.sync_material_asset_previews(&crate::model::material_assets(&self.tree));
        if self.camera.viewport == Vec2::ONE {
            self.camera.viewport = renderer.size();
        }
        if !self.ui.selection_gesture_active() {
            if self.interactions.update(&mut self.camera, &mut self.tree) {
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
        if let Some(ids) = renderer.material_preview_ids() {
            egui.data_mut(|data| {
                data.insert_temp(crate::renderer::MaterialPreviewIds::egui_id(), ids)
            });
        }
        let mut file_action = None;
        let mut cancel_render = false;
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
        });
        if cancel_render {
            self.cancel_render();
        }
        if let Some(action) = file_action {
            self.handle_file_action(action);
        }
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
        let offscreen_capture = self.agent_capture.is_some();
        #[cfg(any(target_arch = "wasm32", not(unix)))]
        let offscreen_capture = false;
        if let Some(renderer) = &mut self.renderer {
            let export_version = if capture_render { i32::MIN } else { 0 };
            // Selection is drawn by the editor gizmos. Keep the cached scene
            // image when only selection changes, avoiding a full refinement.
            let scene_versions = [
                self.tree.path_version("scene.sdf_objects"),
                self.tree
                    .path_version("scene.world")
                    .wrapping_add(export_version),
            ];
            renderer.render(
                &self.camera,
                objects_ref(&self.tree),
                &effective_selection,
                scene_versions,
                crate::model::world(&self.tree),
                &self.egui,
                &mut output,
                capture_render,
                capture_ui,
                offscreen_capture,
            );
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(result) = renderer.take_capture() {
                #[cfg(unix)]
                let agent_handled = if let Some(reply) = self.agent_capture.take() {
                    use base64::Engine;
                    let encoded = result.as_ref().map_err(Clone::clone).and_then(|frame| {
                        let cropped =
                            crate::render_export::crop_to_viewport(frame.clone(), &self.camera);
                        crate::render_export::png_bytes(&cropped).map(|bytes| {
                            serde_json::json!({"width": cropped.width, "height": cropped.height,
                                "data": base64::engine::general_purpose::STANDARD.encode(bytes)})
                        })
                    });
                    let _ = reply.send(encoded);
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

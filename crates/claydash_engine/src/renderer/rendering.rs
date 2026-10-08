use super::*;

impl Renderer {
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    pub fn viewport_diagnostics(&self) -> serde_json::Value {
        self.viewport.diagnostics()
    }
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    pub fn viewport_refined(&self) -> bool {
        self.viewport.is_refined()
    }

    pub fn render(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
        mesh_source_revision: i32,
        world: World,
        post_processing: &[crate::model::PostProcessPass],
        egui: &egui::Context,
        output: &mut egui::FullOutput,
        capture: bool,
        capture_ui: bool,
        offscreen_capture: bool,
        refine: bool,
        animation_playback: bool,
        outline_capture: bool,
        outline_viewport: bool,
    ) -> bool {
        let offscreen_capture = offscreen_capture || self.surface.is_none();
        self.advance_computations(camera, objects, selected, scene_versions,
            mesh_source_revision, world, egui, !outline_viewport);
        let clipped = egui.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        for (id, image_deltas) in output.textures_delta.set.drain() {
            for image_delta in image_deltas {
                self.egui_renderer
                    .update_texture(&self.device, &self.queue, id, &image_delta);
            }
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let callback_commands = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &clipped,
            &screen,
        );
        let offscreen = (capture_ui
            || offscreen_capture
            || (cfg!(target_arch = "wasm32") && capture))
            .then(|| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("guide screenshot target"),
                    size: wgpu::Extent3d {
                        width: self.config.width,
                        height: self.config.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.config.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &self.config.view_formats,
                })
            });
        let frame = if capture_ui || offscreen_capture {
            None
        } else {
            Some(match self.surface.as_ref().expect("window surface").get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    self.surface.as_ref().expect("window surface").configure(&self.device, &self.config);
                    self.free_textures(output);
                    return false;
                }
                wgpu::CurrentSurfaceTexture::Timeout
                | wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Validation => {
                    self.free_textures(output);
                    return false;
                }
            })
        };
        let target = offscreen
            .as_ref()
            .unwrap_or_else(|| &frame.as_ref().expect("surface frame").texture);
        let view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_format),
            ..Default::default()
        });
        if outline_viewport {
            // The editor has already painted the viewport background and its
            // wireframes into egui. Shading this scene would be fully hidden.
            let _ = self.device.poll(wgpu::PollType::Poll);
            {
                let mut pass = encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("outline editor"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(match egui.theme() {
                                    egui::Theme::Dark => wgpu::Color::BLACK,
                                    egui::Theme::Light => wgpu::Color::WHITE,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    })
                    .forget_lifetime();
                self.egui_renderer.render(&mut pass, &clipped, &screen);
            }
            self.queue
                .submit(callback_commands.into_iter().chain([encoder.finish()]));
            self.free_textures(output);
            if let Some(frame) = frame {
                self.queue.present(frame);
            }
            return true;
        }
        let display_view = offscreen
            .as_ref()
            .and_then(|_| frame.as_ref())
            .map(|frame| {
                frame.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(self.render_format),
                    ..Default::default()
                })
            });
        let mesh_only = self.mesh_vertex_count > 0 && self.uploaded_object_count == 0;
        let view_key = crate::viewport::ViewKey {
            matrix: (camera.projection() * camera.view()).to_cols_array_2d(),
            position: camera.position.to_array(),
            projection: u32::from(camera.projection_mode == ProjectionMode::Orthographic),
            versions: scene_versions,
            size: [
                camera.viewport.x.max(1.0) as u32,
                camera.viewport.y.max(1.0) as u32,
            ],
            refine: refine && !mesh_only,
        };
        let deferred = !mesh_only && world.render_pipeline == crate::model::RenderPipelineMode::Deferred
            && self.deferred_supported;
        let interleaved = refine && !deferred && !self.hybrid_enabled && self.mesh_vertex_count == 0;
        let viewport_refine = refine && !mesh_only;
        self.viewport.set_playback_budget(animation_playback);
        let work = self.viewport.prepare(
            &self.device,
            view_key.clone(),
            viewport_refine,
            deferred,
            interleaved,
        );
        if work != crate::viewport::Work::Cached {
            self.upload_splat_camera(camera, world);
        }
        let quick_preview =
            !interleaved && (matches!(work, crate::viewport::Work::Preview) || !refine);
        let scene_pipeline = if mesh_only {
            &self.mesh_background_pipeline
        } else if self.hybrid_enabled {
            if !quick_preview {
                self.hybrid_pipeline
                    .as_ref()
                    .expect("hybrid scene pipeline")
            } else {
                self.hybrid_fast_pipeline
                    .as_ref()
                    .expect("hybrid preview pipeline")
            }
        } else {
            match (!quick_preview, self.has_booleans) {
                (true, true) => &self.boolean_pipeline.as_ref().expect("boolean pipeline").1,
                (true, false) => &self.pipeline,
                (false, true) => {
                    &self
                        .fast_boolean_pipeline
                        .as_ref()
                        .expect("fast boolean pipeline")
                        .1
                }
                (false, false) => &self.fast_pipeline,
            }
        };
        let mesh_vertex_count = if self.mesh_is_visible() { self.mesh_vertex_count } else { 0 };
        let readback = self.viewport.encode(
            &self.device,
            &self.queue,
            &mut encoder,
            work,
            scene_pipeline,
            &self.bind_group,
            (self.hybrid_depth_pipeline.is_some() || mesh_vertex_count > 0).then_some(crate::viewport::HybridPass {
                depth_pipeline: if mesh_only { None } else { self.hybrid_depth_pipeline.as_ref() },
                splat_pipeline: &self.splat_pipeline,
                splat_bind_group: &self.splat_camera_bind_group,
                splat_buffer: &self.splat_buffer,
                splat_count: if self.hybrid_enabled { self.splat_instances.len() as u32 } else { 0 },
                mesh_pipeline: &self.mesh_pipeline,
                mesh_bind_group: &self.bind_group,
                mesh_buffer: &self.mesh_buffer,
                mesh_vertex_count,
            }),
            deferred.then(|| crate::viewport::DeferredPass {
                geometry_pipeline: &self
                    .deferred_geometry_pipeline
                    .as_ref()
                    .expect("deferred geometry pipeline")
                    .2,
            }),
        );
        // Export only after the adaptive viewport has rendered every native-
        // resolution tile for this exact camera and scene state. If the final
        // tile is part of this submission, the later texture copy observes it.
        let capture = capture
            && self.viewport.matches(&view_key)
            && (!viewport_refine || self.viewport.is_refined())
            && !self.capture_pending;
        let workspace_background = match egui.theme() {
            egui::Theme::Dark => wgpu::Color::BLACK,
            egui::Theme::Light => wgpu::Color::WHITE,
        };
        // Documents can be opened outside the atomic editor/MCP path. Keep a
        // malformed source from reaching wgpu's pipeline creation.
        let post_enabled = post_processing.iter().any(|pass| pass.enabled)
            && self.post_processing.is_valid(post_processing);
        if post_enabled {
            egui.request_repaint();
            self.post_processing.prepare(
                &self.device,
                post_processing,
                [self.config.width, self.config.height],
            );
        }
        let scene_view = if post_enabled {
            self.post_processing.source_view()
        } else {
            &view
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: scene_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(workspace_background),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(
                camera.viewport_origin.x,
                camera.viewport_origin.y,
                camera.viewport.x,
                camera.viewport.y,
                0.0,
                1.0,
            );
            self.viewport.composite(&mut pass);
        }
        // Browser exports need a separate copyable texture for readback. Draw
        // the same scene to the surface too, so progress and Cancel repaint.
        if !post_enabled {
            if let Some(display_view) = &display_view {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("display scene"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: display_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(workspace_background),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(
                    camera.viewport_origin.x,
                    camera.viewport_origin.y,
                    camera.viewport.x,
                    camera.viewport.y,
                    0.0,
                    1.0,
                );
                self.viewport.composite(&mut pass);
            }
        }
        if post_enabled {
            self.post_processing.encode(
                &self.device,
                &self.queue,
                &mut encoder,
                post_processing,
                [
                    camera.viewport_origin.x,
                    camera.viewport_origin.y,
                    camera.viewport.x,
                    camera.viewport.y,
                ],
                &view,
                display_view.as_ref(),
            );
        }
        if outline_capture {
            let renderer = self.outline_capture.get_or_insert_with(|| {
                super::outline_capture::OutlineCaptureRenderer::new(
                    &self.device,
                    self.render_format,
                )
            });
            renderer.encode(
                &self.device,
                &self.queue,
                &mut encoder,
                &view,
                [self.config.width, self.config.height],
                camera,
                objects,
            );
        }
        // Copy the scene before egui is composited so exports never contain
        // editor chrome, transform gizmos, camera wireframes, or labels.
        let mut capture_buffer = capture.then(|| {
            let unpadded = self.config.width * 4;
            let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("render export readback"),
                size: (padded * self.config.height) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            if !capture_ui {
                encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: target,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(padded),
                            rows_per_image: Some(self.config.height),
                        },
                    },
                    wgpu::Extent3d {
                        width: self.config.width,
                        height: self.config.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            (buffer, padded)
        });
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: display_view.as_ref().unwrap_or(&view),
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut pass, &clipped, &screen);
        }
        if capture_ui {
            if let Some((buffer, padded)) = &mut capture_buffer {
                encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: target,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(*padded),
                            rows_per_image: Some(self.config.height),
                        },
                    },
                    wgpu::Extent3d {
                        width: self.config.width,
                        height: self.config.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        self.queue
            .submit(callback_commands.into_iter().chain([encoder.finish()]));
        self.viewport.submitted(&self.queue, work, readback);
        self.free_textures(output);
        let presented = frame.is_some();
        if let Some(frame) = frame {
            self.queue.present(frame);
        }
        if let Some((buffer, padded)) = capture_buffer {
            self.capture_pending = true;
            self.capture_cancel
                .store(false, std::sync::atomic::Ordering::Relaxed);
            let cancelled = self.capture_cancel.clone();
            let result_slot = self.capture_result.clone();
            let mapped_buffer = buffer.clone();
            let width = self.config.width;
            let height = self.config.height;
            let swap_channels = matches!(
                self.config.format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            );
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let frame = result.map_err(|error| error.to_string()).and_then(|_| {
                        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                            return Err("Capture cancelled".to_owned());
                        }
                        let mapped = mapped_buffer
                            .slice(..)
                            .get_mapped_range()
                            .map_err(|error| error.to_string())?;
                        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                        for row in mapped.chunks_exact(padded as usize) {
                            rgba.extend_from_slice(&row[..(width * 4) as usize]);
                        }
                        drop(mapped);
                        if swap_channels {
                            for pixel in rgba.chunks_exact_mut(4) {
                                pixel.swap(0, 2);
                            }
                        }
                        Ok(CapturedFrame {
                            width,
                            height,
                            rgba,
                        })
                    });
                    mapped_buffer.unmap();
                    *result_slot.lock().unwrap() = Some(frame);
                });
        }
        presented
    }

    pub fn capture_pending(&self) -> bool {
        self.capture_pending
    }

    pub fn cancel_pending_capture(&mut self) {
        self.capture_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn take_capture(&mut self) -> Option<Result<CapturedFrame, String>> {
        let frame = self.capture_result.lock().unwrap().take();
        if frame.is_some() {
            self.capture_pending = false;
        }
        frame
    }
}

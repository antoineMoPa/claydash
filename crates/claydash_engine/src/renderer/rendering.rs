use super::*;

impl Renderer {
    pub fn gpu_fault(&self) -> Option<&'static str> { self.viewport.gpu_fault() }
    pub fn set_viewport_repaint_callback(&mut self, repaint: impl Fn() + Send + Sync + 'static) {
        self.viewport.set_repaint_callback(repaint);
    }
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    pub fn viewport_diagnostics(&self) -> serde_json::Value {
        self.viewport.diagnostics()
    }
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
        interacting: bool,
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
                self.ui_textures.update(id, &image_delta);
                self.egui_renderer
                    .update_texture(&self.device, &self.queue, id, &image_delta);
            }
        }

        // A surface has only a few drawable textures. Submitting cached UI
        // frames behind slow scene work exhausts them and makes acquisition
        // wait on the UI thread. Keep processing input, but submit no more
        // presentation work until the existing scene fence has completed.
        if self.viewport.gpu_work_pending(&self.device) {
            self.free_textures(output);
            return false;
        }
        if self.viewport.gpu_fault().is_some() {
            self.free_textures(output);
            return false;
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
        let computing_optimization = self.has_pending_computations(egui);
        // Interaction pauses a stable plan; it must not change the plan's
        // representation and discard all progress on press/release events.
        let interleaved = refine && !deferred && !self.hybrid_enabled && self.mesh_vertex_count == 0;
        let rect = if interleaved && self.use_bvh {
            refinement_region::projected_region(camera, self.scene_bounds, view_key.size)
        } else { [0, 0, view_key.size[0], view_key.size[1]] };
        self.viewport.set_refinement_region(if interleaved { rect } else { [0; 4] },
            interleaved.then(|| self.mesh_background_pipeline.clone()));
        self.queue.write_buffer(&self.camera_buffer, std::mem::offset_of!(GpuCamera, refinement_rect) as u64,
            bytemuck::cast_slice(&rect));
        // Simple shading still traces the source field. It needs the same
        // bounded previews/native refinement as full materials on complex
        // scenes; shading quality alone cannot make a full viewport cheap.
        let viewport_refine = !mesh_only;
        self.viewport.set_playback_budget(animation_playback);
        self.viewport.set_interacting(interacting);
        self.viewport.set_refinement_paused(computing_optimization);
        self.viewport.set_rendering_path(if mesh_only {
            crate::viewport::RenderingPath::Mesh
        } else if deferred {
            crate::viewport::RenderingPath::Deferred
        } else if self.hybrid_enabled {
            crate::viewport::RenderingPath::Hybrid
        } else {
            crate::viewport::RenderingPath::Exact
        });
        self.viewport.bloom_settings(&self.queue, world);
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
        let quick_preview = matches!(work, crate::viewport::Work::Preview)
            || matches!(work, crate::viewport::Work::Interleave { first, .. }
                if first < self.viewport.phase_samples())
            || !refine;
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
        self.viewport.set_contact_active(!quick_preview && !deferred && !mesh_only);
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
            && !computing_optimization
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

    /// Cancel and detach the readback so a timed-out client cannot block a retry.
    pub fn discard_pending_capture(&mut self) {
        self.cancel_pending_capture();
        self.capture_result = Arc::new(std::sync::Mutex::new(None));
        self.capture_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.capture_pending = false;
    }

    pub fn take_capture(&mut self) -> Option<Result<CapturedFrame, String>> {
        let frame = self.capture_result.lock().unwrap().take();
        if frame.is_some() {
            self.capture_pending = false;
            if let Some(fault) = self.gpu_fault() { return Some(Err(fault.to_owned())); }
        }
        frame
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod scheduling_tests {
    use super::*;

    fn sampled_field(renderer: &Renderer, capacity: u32, features: SceneShaderFeatures) -> Vec<[f32; 4]> {
        let source = specialized_shader_source(&renderer.shader_source, capacity);
        let source = format!("{source}\n{}", r#"
@fragment fn fs_navigation_field(input: VertexOutput) -> @location(0) vec4<f32> {
    let index = u32(input.position.x) + 32u * u32(input.position.y);
    let point = vec3(f32(index % 13u) * 0.4 - 2.4,
        f32((index / 13u) % 13u) * 0.4 - 2.4, f32(index / 169u) * 0.5 - 1.2);
    let field = scene_distance(point);
    return vec4(field, 0.0, 1.0);
}
"#);
        let shader = renderer.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("navigation field regression"), source: wgpu::ShaderSource::Wgsl(source.clone().into()),
        });
        let mut constants = scene_feature_constants(&source, features);
        constants.extend_from_slice(&[("USE_BVH", 1.0), ("HAS_BOOLEANS", f64::from(capacity > 1))]);
        let pipeline = renderer.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("navigation field regression"), layout: Some(&renderer.pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"),
                buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_navigation_field"),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba32Float,
                    blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() } }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
            multiview_mask: None, cache: None,
        });
        let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("navigation field grid"),
            size: wgpu::Extent3d { width: 32, height: 32, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("navigation field readback"), size: 32 * 32 * 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
        });
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view,
                    resolve_target: None, depth_slice: None, ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &renderer.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(target.as_image_copy(), wgpu::TexelCopyBufferInfo {
            buffer: &buffer, layout: wgpu::TexelCopyBufferLayout {
                offset: 0, bytes_per_row: Some(32 * 16), rows_per_image: None }
        }, target.size());
        renderer.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| { sender.send(result).unwrap(); });
        renderer.device.poll(wgpu::PollType::Wait { submission_index: None,
            timeout: Some(std::time::Duration::from_secs(15)) }).unwrap();
        receiver.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        let values = bytemuck::cast_slice(&mapped).to_vec();
        drop(mapped);
        buffer.unmap();
        values
    }

    fn native_simple_pixels(renderer: &Renderer) -> Vec<u8> {
        renderer.direct_reference_pixels(true)
    }

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn deformed_splat_fallback_preserves_distance_field_and_source() {
        use crate::model::{GroupRenderRepresentation, Lattice, PrimitiveKind};
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.softness = 0.0;
        root.render_representation = GroupRenderRepresentation::GaussianSplats;
        let mut cage = Lattice::new(-Vec3::splat(2.5), Vec3::splat(2.5), 7);
        for z in 0..7 { for y in 0..7 { for x in 0..7 {
            let index = cage.index(x, y, z);
            cage.offsets[index] = Vec3::new(0.12 * y as f32 / 6.0,
                0.07 * (x as f32 / 6.0).sin(), 0.03 * z as f32 / 6.0);
        } } }
        root.lattice = Some(cage);
        let mut scene = vec![root.clone()];
        for index in 0..8 {
            let mut part = SdfObject::create_kind(PrimitiveKind::Box);
            part.boolean_parent = Some(root.uuid);
            part.transform.translation = Vec3::new((index % 3) as f32 * 0.65 - 0.65,
                (index / 3) as f32 * 0.6 - 0.6, 0.0);
            if index == 0 { part.transform.scale = Vec3::new(0.7, 1.2, 0.85); }
            scene.push(part);
        }
        let saved = serde_json::to_vec(&scene).unwrap();
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 65, height: 49, ..Default::default()
        })).unwrap();
        let mut camera = Camera::new();
        camera.viewport = renderer.size();
        let world = World::default();
        // The material-preview upload retains the unsplit source component.
        renderer.upload_scene_with_world_exposure(&camera, &scene, &[], [1, 1],
            1.0, world, scene_upload::ScenePipelinePreparation::MaterialPreview);
        assert!(renderer.has_booleans);
        let mut features = SceneShaderFeatures::for_scene(&[root.material], &[]);
        features.spatial.lattice = true;
        features.flat_unions = true;
        renderer.prepare_scene_pipelines(world, 2, features);
        renderer.upload_splat_camera(&camera, world);
        let reference = native_simple_pixels(&renderer);
        let reference_field = sampled_field(&renderer, 2, features);
        renderer.upload_scene_with_world(&camera, &scene, &[], [2, 2], world);
        assert!(!renderer.has_booleans, "uncomputed splats must keep the exact fallback accelerated");
        assert_eq!(renderer.node_count, 17);
        renderer.upload_splat_camera(&camera, world);
        let accelerated = native_simple_pixels(&renderer);
        let header = b"P6\n65 49\n255\n".len();
        let changed = reference[header..].chunks_exact(3).zip(accelerated[header..].chunks_exact(3))
            .filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 3)).count();
        // Different ray starts and finite-difference normals can affect a few
        // contour/seam pixels. Check the underlying GPU field independently.
        assert!(changed <= 16, "deformed union changed {changed}/3185 pixels");
        features.flat_unions = false;
        let accelerated_field = sampled_field(&renderer, 1, features);
        for (expected, actual) in reference_field.iter().zip(&accelerated_field) {
            assert!((expected[0] - actual[0]).abs() < 0.00002,
                "deformed field changed: {expected:?} vs {actual:?}");
        }
        assert_eq!(serde_json::to_vec(&scene).unwrap(), saved);
    }

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn simple_shading_moves_with_a_preview_and_finishes_at_native_resolution() {
        use std::time::{Duration, Instant};
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 65, height: 49, ..Default::default()
        })).unwrap();
        let scene = vec![SdfObject::create_kind(crate::model::PrimitiveKind::Sphere)];
        let world = World::default();
        let mut camera = Camera::new();
        camera.viewport = renderer.size();
        renderer.upload_scene_with_world(&camera, &scene, &[], [1, 1], world);
        renderer.upload_splat_camera(&camera, world);
        let reference = native_simple_pixels(&renderer);
        renderer.viewport.set_initial_budget(64);
        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |_| {});
        renderer.render(&camera, &scene, &[], [1, 1], 0, world, &[], &context,
            &mut output, true, false, true, false, false, true, false, false);
        assert!(renderer.preview_pixels() <= 256, "simple shading must honor the minimum useful motion budget");
        assert!(!renderer.capture_pending, "exports must wait for native tiles");
        let deadline = Instant::now() + Duration::from_secs(15);
        let image = loop {
            assert!(Instant::now() < deadline, "simple shading did not converge");
            renderer.device.poll(wgpu::PollType::Wait {
                submission_index: None, timeout: Some(Duration::from_secs(5))
            }).unwrap();
            if let Some(image) = renderer.take_capture() { break image.unwrap(); }
            let mut output = context.run_ui(egui::RawInput::default(), |_| {});
            renderer.render(&camera, &scene, &[], [1, 1], 0, world, &[], &context,
                &mut output, true, false, true, false, false, false, false, false);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(renderer.viewport_refined());
        let mut actual = b"P6\n65 49\n255\n".to_vec();
        for pixel in image.rgba.chunks_exact(4) { actual.extend_from_slice(&pixel[..3]); }
        assert_eq!(actual, reference, "native simple shading must retain its exact pixels");
    }

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn partial_first_sparse_phase_keeps_preview_and_publishes_samples() {
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 65, height: 49, ..Default::default()
        })).unwrap();
        let mut camera = Camera::new();
        camera.viewport = renderer.size();
        let scene = vec![SdfObject::create_kind(crate::model::PrimitiveKind::Sphere)];
        renderer.upload_scene_with_world(&camera, &scene, &[], [1, 1], World::default());
        renderer.viewport.set_initial_budget(64);
        let work = renderer.viewport.prepare(&renderer.device, crate::viewport::ViewKey {
            matrix: (camera.projection() * camera.view()).to_cols_array_2d(),
            position: camera.position.to_array(), projection: 0,
            versions: [1, 1], size: [65, 49], refine: true,
        }, true, false, true);
        assert_eq!(work, crate::viewport::Work::Preview);
        let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("partial phase composite"),
            size: wgpu::Extent3d { width: 65, height: 49, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format: renderer.render_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut draw = |work| {
            let mut encoder = renderer.device.create_command_encoder(&Default::default());
            let quick = matches!(work, crate::viewport::Work::Preview)
                || matches!(work, crate::viewport::Work::Interleave { first, .. } if first < 20);
            let pipeline = if quick { &renderer.fast_pipeline } else { &renderer.pipeline };
            let readback = renderer.viewport.encode(&renderer.device, &renderer.queue, &mut encoder,
                work, pipeline, &renderer.bind_group, None, None);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view, resolve_target: None, depth_slice: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })], ..Default::default()
                });
                renderer.viewport.composite(&mut pass);
            }
            renderer.queue.submit([encoder.finish()]);
            renderer.viewport.submitted(&renderer.queue, work, readback);
            renderer.benchmark_pixels(&target)
        };
        let coarse = draw(work);
        let preview = draw(crate::viewport::Work::Interleave { first: 0, end: 20 });
        let header = b"P6\n65 49\n255\n".len();
        let reconstructed = coarse[header..].chunks_exact(3)
            .zip(preview[header..].chunks_exact(3)).filter(|(a, b)| a != b).count();
        assert!(reconstructed > 20 * 16,
            "coverage must improve the background between samples, not leave the tiny navigation preview");
        let partial = draw(crate::viewport::Work::Interleave { first: 40, end: 40 + 13 });
        let header = b"P6\n65 49\n255\n".len();
        let mut changed = 0;
        for (i, (before, after)) in preview[header..].chunks_exact(3)
            .zip(partial[header..].chunks_exact(3)).enumerate() {
            let (x, y) = (i % 65, i / 65);
            let written = x % 16 == 8 && y % 16 == 8 && (y / 16) * 5 + x / 16 < 13;
            if written { changed += usize::from(before != after); }
            else { assert_eq!(before, after, "unwritten sample at {x},{y} must retain its preview"); }
        }
        assert!(changed > 0, "partial phases must publish completed native samples immediately");
    }

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn repeated_navigation_pauses_keep_one_plan_and_finish_exactly() {
        use std::time::{Duration, Instant};
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 65, height: 49, ..Default::default()
        })).unwrap();
        let mut camera = Camera::new();
        camera.viewport = renderer.size();
        let scene = vec![SdfObject::create_kind(crate::model::PrimitiveKind::Sphere)];
        let context = egui::Context::default();
        let mut generation = None;
        for frame in 0..18 {
            if frame < 6 { camera.pan(Vec2::new(2.0, 1.0)); }
            let interacting = frame < 6 || frame % 2 == 1;
            let mut output = context.run_ui(egui::RawInput::default(), |_| {});
            renderer.render(&camera, &scene, &[], [17, 19], 0, World::default(), &[],
                &context, &mut output, false, false, true, true, false, interacting, false, false);
            renderer.device.poll(wgpu::PollType::Wait {
                submission_index: None, timeout: Some(Duration::from_secs(5))
            }).unwrap();
            #[cfg(unix)]
            if frame >= 6 {
                let diagnostics = renderer.viewport.diagnostics();
                assert_eq!(diagnostics["interleaved"], true);
                let current = diagnostics["render_generation"].as_u64().unwrap();
                if let Some(previous) = generation { assert_eq!(previous, current,
                    "press/release must not replace a stable refinement plan"); }
                generation = Some(current);
            }
            std::thread::sleep(Duration::from_millis(17));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let actual = loop {
            assert!(Instant::now() < deadline, "stable view must finish after navigation pauses");
            let mut output = context.run_ui(egui::RawInput::default(), |_| {});
            renderer.render(&camera, &scene, &[], [17, 19], 0, World::default(), &[],
                &context, &mut output, true, false, true, true, false, false, false, false);
            renderer.device.poll(wgpu::PollType::Wait {
                submission_index: None, timeout: Some(Duration::from_secs(5))
            }).unwrap();
            if let Some(capture) = renderer.take_capture() { break capture.unwrap(); }
            std::thread::sleep(Duration::from_millis(17));
        };
        assert!(renderer.viewport_refined());
        let reference = renderer.render_offscreen(&camera, &scene, World::default(), &[], Duration::from_secs(10)).unwrap();
        assert_eq!(actual.rgba, reference.rgba);
        let mut actual_ppm = b"P6\n65 49\n255\n".to_vec();
        for pixel in actual.rgba.chunks_exact(4) { actual_ppm.extend_from_slice(&pixel[..3]); }
        assert_eq!(actual_ppm, renderer.direct_reference_pixels(false),
            "finished native output must match a direct full-screen draw that bypasses the scheduler");
    }

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn split_sparse_batches_match_the_completed_image() {
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 65, height: 49, ..Default::default()
        })).expect("headless renderer");
        let scene = vec![SdfObject::create_kind(crate::model::PrimitiveKind::Sphere)];
        let camera = Camera::new();
        let timeout = std::time::Duration::from_secs(30);
        let reference = renderer.render_offscreen(&camera, &scene, World::default(), &[], timeout)
            .expect("reference image");
        // A phase covers 5 * 4 blocks; a three-pixel budget must stop inside it.
        renderer.viewport.set_initial_budget(3);
        let split = renderer.render_offscreen(&camera, &scene, World::default(), &[], timeout)
            .expect("split sparse image");
        assert_eq!(reference.rgba, split.rgba);

        // A dropped encoder has no GPU fence. Its cursor must be rolled back
        // and re-encoded, rather than leaving a phantom pending submission.
        let mut camera = camera;
        camera.viewport = renderer.size();
        renderer.upload_scene_with_world(&camera, &scene, &[], [7, 7], World::default());
        renderer.viewport.prepare(&renderer.device, crate::viewport::ViewKey {
            matrix: (camera.projection() * camera.view()).to_cols_array_2d(),
            position: camera.position.to_array(), projection: 0,
            versions: [7, 7], size: [65, 49], refine: true,
        }, true, false, true);
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        renderer.viewport.encode(&renderer.device, &renderer.queue, &mut encoder,
            crate::viewport::Work::Interleave { first: 0, end: 257 * 5 * 4 },
            &renderer.pipeline, &renderer.bind_group, None, None);
        drop(encoder);
        let context = egui::Context::default();
        for _ in 0..3 {
            let mut output = context.run_ui(egui::RawInput::default(), |_| {});
            assert!(!renderer.render(&camera, &scene, &[], [7, 7], 0, World::default(), &[],
                &context, &mut output, true, false, true, true, false, false, false, false));
            renderer.device.poll(wgpu::PollType::Wait {
                submission_index: None, timeout: Some(std::time::Duration::from_secs(5))
            }).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(17));
        }
        #[cfg(unix)]
        assert!(renderer.viewport.diagnostics()["completed"].as_u64().unwrap() > 0,
            "dropping an encoder must not prevent subsequent refinement");
    }
}

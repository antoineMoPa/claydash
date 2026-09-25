use super::*;

impl Renderer {
    pub fn render(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
        egui: &egui::Context,
        output: &mut egui::FullOutput,
        capture: bool,
        capture_ui: bool,
    ) -> Option<CapturedFrame> {
        self.upload_scene(camera, objects, selected, scene_versions);
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
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                self.free_textures(output);
                return None;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                self.free_textures(output);
                return None;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_format),
            ..Default::default()
        });
        let work = self.viewport.prepare(
            &self.device,
            crate::viewport::ViewKey {
                matrix: (camera.projection() * camera.view()).to_cols_array_2d(),
                position: camera.position.to_array(),
                projection: u32::from(camera.projection_mode == ProjectionMode::Orthographic),
                versions: scene_versions,
                size: [
                    camera.viewport.x.max(1.0) as u32,
                    camera.viewport.y.max(1.0) as u32,
                ],
            },
        );
        let scene_pipeline = if self.has_booleans {
            &self.boolean_pipeline.as_ref().expect("boolean pipeline").1
        } else {
            &self.pipeline
        };
        let readback = self.viewport.encode(
            &self.device,
            &self.queue,
            &mut encoder,
            work,
            scene_pipeline,
            &self.bind_group,
        );
        // Export only after the adaptive viewport has rendered every native-
        // resolution tile for this exact camera and scene state. If the final
        // tile is part of this submission, the later texture copy observes it.
        let capture = capture && self.viewport.is_refined();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
                        texture: &frame.texture,
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
                        view: &view,
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
                        texture: &frame.texture,
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
        let submission = self
            .queue
            .submit(callback_commands.into_iter().chain([encoder.finish()]));
        self.viewport.submitted(&self.queue, work, readback);
        self.free_textures(output);
        self.queue.present(frame);
        let (buffer, padded) = capture_buffer?;
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(15)),
            })
            .ok()?;
        rx.recv_timeout(std::time::Duration::from_secs(15))
            .ok()?
            .ok()?;
        let mapped = slice.get_mapped_range().ok()?;
        let mut rgba = Vec::with_capacity((self.config.width * self.config.height * 4) as usize);
        for row in mapped.chunks_exact(padded as usize) {
            rgba.extend_from_slice(&row[..(self.config.width * 4) as usize]);
        }
        drop(mapped);
        buffer.unmap();
        if matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Some(CapturedFrame {
            width: self.config.width,
            height: self.config.height,
            rgba,
        })
    }
}

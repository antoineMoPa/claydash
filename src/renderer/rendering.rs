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
    ) {
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
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                self.free_textures(output);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
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
        self.queue
            .submit(callback_commands.into_iter().chain([encoder.finish()]));
        self.viewport.submitted(&self.queue, work, readback);
        self.free_textures(output);
        self.queue.present(frame);
    }
}

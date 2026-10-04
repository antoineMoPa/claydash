use crate::{camera::Camera, model::SdfObject};

/// A separate egui renderer keeps capture geometry and textures independent of editor UI.
pub(super) struct OutlineCaptureRenderer {
    context: egui::Context,
    renderer: egui_wgpu::Renderer,
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::model::{BooleanOperation, PrimitiveKind, SdfParams, SphereParams};
    use glam::{Vec2, Vec3};

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn outline_png_pixels_include_hidden_cutters_from_each_axis() {
        pollster::block_on(async {
            let adapter = wgpu::Instance::default()
                .request_adapter(&Default::default())
                .await
                .unwrap();
            let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let size = 128;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("outline capture test"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (size * size * 4) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let root = SdfObject::create_kind(PrimitiveKind::Box);
            let mut cutter = SdfObject::create_kind(PrimitiveKind::Sphere);
            cutter.boolean_parent = Some(root.uuid);
            cutter.operation = BooleanOperation::Subtract;
            cutter.params = SdfParams::SphereParams(SphereParams { radius: 0.2 });
            let scene = [root, cutter];
            let mut renderer = OutlineCaptureRenderer::new(&device, format);
            for (direction, up) in [
                (Vec3::X, Vec3::Y),
                (Vec3::Y, Vec3::NEG_Z),
                (Vec3::Z, Vec3::Y),
            ] {
                let mut camera = Camera::new();
                camera.projection_mode = crate::camera::ProjectionMode::Orthographic;
                camera.viewport = Vec2::splat(size as f32);
                camera.viewport_origin = Vec2::ZERO;
                camera.position = direction * 4.0;
                camera.target = Vec3::ZERO;
                camera.up = up;
                // Also exercise the empty scene to ensure captures clear stale wires.
                for objects in [&scene[..], &[][..]] {
                    let mut encoder = device.create_command_encoder(&Default::default());
                    renderer.encode(
                        &device,
                        &queue,
                        &mut encoder,
                        &view,
                        [size, size],
                        &camera,
                        objects,
                    );
                    encoder.copy_texture_to_buffer(
                        wgpu::TexelCopyTextureInfo {
                            texture: &texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyBufferInfo {
                            buffer: &buffer,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(size * 4),
                                rows_per_image: Some(size),
                            },
                        },
                        wgpu::Extent3d {
                            width: size,
                            height: size,
                            depth_or_array_layers: 1,
                        },
                    );
                    queue.submit([encoder.finish()]);
                    let (send, receive) = std::sync::mpsc::channel();
                    buffer
                        .slice(..)
                        .map_async(wgpu::MapMode::Read, move |result| {
                            send.send(result).unwrap();
                        });
                    device
                        .poll(wgpu::PollType::Wait {
                            submission_index: None,
                            timeout: None,
                        })
                        .unwrap();
                    receive.recv().unwrap().unwrap();
                    let pixels = buffer.slice(..).get_mapped_range().unwrap();
                    let background = &pixels[..4];
                    let visible = pixels
                        .chunks_exact(4)
                        .filter(|pixel| *pixel != background)
                        .count();
                    let amber = pixels
                        .chunks_exact(4)
                        .filter(|pixel| {
                            u16::from(pixel[0]) > u16::from(pixel[1]) + 10
                                && u16::from(pixel[1]) > u16::from(pixel[2]) + 10
                        })
                        .count();
                    if objects.is_empty() {
                        assert_eq!(visible, 0, "empty captures must have no stale wires");
                    } else {
                        assert!(visible > 50, "expected visible wires from {direction:?}");
                        assert!(
                            amber > 10,
                            "hidden cutter must remain visible from {direction:?}"
                        );
                    }
                    drop(pixels);
                    buffer.unmap();
                }
            }
        });
    }
}

impl OutlineCaptureRenderer {
    pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let context = egui::Context::default();
        context.set_theme(egui::Theme::Dark);
        Self {
            context,
            renderer: egui_wgpu::Renderer::new(
                device,
                format,
                egui_wgpu::RendererOptions::default(),
            ),
        }
    }

    pub(super) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        camera: &Camera,
        objects: &[SdfObject],
    ) {
        let screen_rect =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0] as f32, size[1] as f32));
        let viewport = egui::Rect::from_min_size(
            egui::pos2(camera.viewport_origin.x, camera.viewport_origin.y),
            egui::vec2(camera.viewport.x, camera.viewport.y),
        );
        let mut output = self.context.run_ui(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ui| {
                ui.set_clip_rect(viewport);
                crate::ui::boolean_overlay::draw_capture(ui, objects, camera);
            },
        );
        for (id, deltas) in output.textures_delta.set.drain() {
            for delta in deltas {
                self.renderer.update_texture(device, queue, id, &delta);
            }
        }
        let clipped = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point: output.pixels_per_point,
        };
        // The shared wire painter produces geometry only, with no GPU callbacks.
        let callbacks = self
            .renderer
            .update_buffers(device, queue, encoder, &clipped, &screen);
        debug_assert!(callbacks.is_empty());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("agent outline capture"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.02,
                                g: 0.02,
                                b: 0.02,
                                a: 1.0,
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
            self.renderer.render(&mut pass, &clipped, &screen);
        }
        for id in output.textures_delta.free.drain() {
            self.renderer.free_texture(&id);
        }
    }
}

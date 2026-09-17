//! Bounded native GPU and viewport regression benchmarks.
use super::*;

impl Renderer {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn benchmark_scene(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
        case: &str,
    ) {
        use std::time::{Duration, Instant};

        self.uploaded_scene_versions = [i32::MIN; 2];
        self.upload_scene(camera, objects, selected, scene_versions);
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("SDF stress benchmark target"),
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
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let draw_batch = |pass_count: usize| {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            for _ in 0..pass_count {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("SDF stress benchmark draw"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(if self.has_booleans {
                    &self.boolean_pipeline.as_ref().expect("boolean pipeline").1
                } else {
                    &self.pipeline
                });
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            let start = Instant::now();
            let submission = self.queue.submit([encoder.finish()]);
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(Duration::from_secs(15)),
                })
                .expect("SDF stress benchmark exceeded its 15 second GPU timeout");
            start.elapsed()
        };

        draw_batch(1);
        let passes = 3;
        let mut samples = [0.0_f64; 5];
        for sample in &mut samples {
            *sample = draw_batch(passes).as_secs_f64() * 1000.0 / passes as f64;
        }
        samples.sort_by(f64::total_cmp);
        let milliseconds = samples[2];
        eprintln!(
            "SDF benchmark: {milliseconds:.3} ms/frame ({:.1} FPS), {} objects, {}x{}",
            1000.0 / milliseconds,
            objects.len().min(MAX_OBJECTS),
            self.config.width,
            self.config.height,
        );
        let full_image = self.benchmark_pixels(&target);
        assert!(
            full_image.iter().skip(32).any(|&value| value > 0),
            "GPU returned a blank image"
        );
        save_benchmark_image(case, &full_image);
        if std::env::args().any(|arg| arg == "--benchmark-progressive") {
            self.benchmark_viewport(camera, objects, selected, scene_versions, &target);
            let refined_image = self.benchmark_pixels(&target);
            let max_error = full_image
                .iter()
                .zip(&refined_image)
                .map(|(&a, &b)| a.abs_diff(b))
                .max()
                .unwrap_or(0);
            assert!(
                max_error <= 1,
                "refined viewport differs from full-resolution reference: max error {max_error}"
            );
            eprintln!("Refined image matches full-resolution reference (max channel error {max_error}/255)");
            save_benchmark_image(&format!("{case}-refined"), &refined_image);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn benchmark_pixels(&self, target: &wgpu::Texture) -> Vec<u8> {
        use std::time::Duration;
        let row_bytes = self.config.width * 4;
        let padded_row_bytes = row_bytes.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("benchmark image readback"),
            size: u64::from(padded_row_bytes) * u64::from(self.config.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: None,
                },
            },
            target.size(),
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap();
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(15)),
            })
            .expect("image readback timeout");
        rx.recv().unwrap().expect("map benchmark image");
        let pixels = buffer
            .slice(..)
            .get_mapped_range()
            .expect("mapped pixel range");
        let mut ppm =
            format!("P6\n{} {}\n255\n", self.config.width, self.config.height).into_bytes();
        for row in pixels.chunks(padded_row_bytes as usize) {
            for pixel in row[..row_bytes as usize].chunks(4) {
                if matches!(
                    self.config.format,
                    wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
                ) {
                    ppm.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
                } else {
                    ppm.extend_from_slice(&pixel[..3]);
                }
            }
        }
        ppm
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn benchmark_viewport(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        versions: [i32; 2],
        target: &wgpu::Texture,
    ) {
        use std::time::{Duration, Instant};
        self.viewport = crate::viewport::Viewport::new(
            &self.device,
            self.config.format,
            self.device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY),
        );
        self.viewport.set_initial_budget(self.initial_pixel_budget);
        let view = target.create_view(&Default::default());
        let mut moving = Vec::new();
        let mut refining = Vec::new();
        let mut editing = Vec::new();
        let mut idle = Vec::new();
        let mut settled = false;
        for frame in 0..(140 + self.config.width.div_ceil(32) * self.config.height.div_ceil(32)) {
            let start = Instant::now();
            let angle = if frame < 60 {
                (frame as f32 * 0.11).sin() * 0.3
            } else {
                0.0
            };
            let current = Camera {
                position: camera.target
                    + glam::Quat::from_rotation_y(angle) * (camera.position - camera.target),
                target: camera.target,
                viewport: camera.viewport,
                viewport_origin: Vec2::ZERO,
                projection_mode: camera.projection_mode,
            };
            // Stationary edits exercise geometry/material and selection cache
            // invalidation independently of camera movement. Restore the exact
            // reference scene before checking convergence pixel for pixel.
            let phase = if frame < 60 {
                0
            } else if frame < 80 {
                1
            } else if frame < 100 {
                2
            } else {
                3
            };
            let mut edited = Vec::new();
            let mut selection = Vec::new();
            if phase == 1 {
                edited = objects.to_vec();
                for object in &mut edited {
                    object.transform.translation.x += 0.15;
                    object.material =
                        crate::model::Material::preset(crate::model::MaterialKind::Solid);
                    object.color = glam::Vec4::new(0.05, 0.9, 0.1, 1.0);
                }
            }
            if phase == 2 {
                selection = objects.iter().map(|object| object.uuid).collect();
            }
            let frame_versions = [
                versions[0].wrapping_add(phase),
                versions[1].wrapping_add(phase),
            ];
            self.upload_scene(
                &current,
                if phase == 1 { &edited } else { objects },
                if phase == 2 { &selection } else { selected },
                frame_versions,
            );
            let work = self.viewport.prepare(
                &self.device,
                crate::viewport::ViewKey {
                    matrix: (current.projection() * current.view()).to_cols_array_2d(),
                    position: current.position.to_array(),
                    projection: u32::from(current.projection_mode == ProjectionMode::Orthographic),
                    versions: frame_versions,
                    size: [self.config.width, self.config.height],
                },
            );
            let mut encoder = self.device.create_command_encoder(&Default::default());
            let pipeline = if self.has_booleans {
                &self.boolean_pipeline.as_ref().unwrap().1
            } else {
                &self.pipeline
            };
            let readback = self.viewport.encode(
                &self.device,
                &self.queue,
                &mut encoder,
                work,
                pipeline,
                &self.bind_group,
            );
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("benchmark viewport composite"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                self.viewport.composite(&mut pass);
            }
            self.queue.submit([encoder.finish()]);
            self.viewport.submitted(&self.queue, work, readback);
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(15)),
                })
                .expect("viewport benchmark timeout");
            let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
            if frame < 60 {
                moving.push(milliseconds);
            } else if frame < 100 {
                editing.push(milliseconds);
            } else if settled {
                idle.push(milliseconds);
            } else {
                refining.push(milliseconds);
            }
            if frame >= 100 && self.viewport.is_refined() {
                settled = true;
            }
            if idle.len() == 10 {
                break;
            }
        }
        assert!(settled, "viewport never converged to full resolution");
        for (label, mut samples) in [
            ("motion", moving),
            ("editing", editing),
            ("refinement", refining),
            ("cached", idle),
        ] {
            samples.sort_by(f64::total_cmp);
            eprintln!(
                "Viewport {label}: p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms, {} frames",
                samples[samples.len() / 2],
                samples[(samples.len() - 1) * 95 / 100],
                samples[samples.len() - 1],
                samples.len()
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn save_benchmark_image(case: &str, pixels: &[u8]) {
    if let Some(directory) =
        std::env::args().find_map(|arg| arg.strip_prefix("--benchmark-images=").map(str::to_owned))
    {
        std::fs::create_dir_all(&directory).expect("create image directory");
        std::fs::write(
            std::path::Path::new(&directory).join(format!("{case}.ppm")),
            pixels,
        )
        .expect("write benchmark image");
    }
}

//! Encode viewport passes and collect GPU completion timings.
use super::*;

impl Viewport {
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        work: Work,
        scene: &wgpu::RenderPipeline,
        scene_bind_group: &wgpu::BindGroup,
        hybrid: Option<HybridPass<'_>>,
        deferred: Option<DeferredPass<'_>>,
    ) -> Option<wgpu::Buffer> {
        // A material pipeline change can invalidate the view while a previous
        // GPU timing query is still pending. prepare() then returns Cached
        // before installing a new key; there is no frame to encode yet.
        let Some(targets) = self.targets.as_ref() else {
            return None;
        };
        let Some(size) = self.key.as_ref().map(|key| key.size) else {
            return None;
        };
        if work != Work::Cached {
            let view = if matches!(work, Work::Preview | Work::Relight { native: false }) {
                &targets.preview
            } else {
                &targets.refined
            };
            let depth_view = if matches!(work, Work::Preview | Work::Relight { native: false }) {
                &targets.preview_depth
            } else {
                &targets.refined_depth
            };
            if let Some(deferred) = deferred
                .as_ref()
                .filter(|_| !matches!(work, Work::Relight { .. }))
            {
                let deferred_targets = targets
                    .deferred
                    .as_ref()
                    .expect("deferred targets")
                    .for_work(work);
                let clear = matches!(work, Work::Preview | Work::Refine { first: 0, .. });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("deferred SDF geometry"),
                    color_attachments: &deferred_targets
                        .buffers
                        .iter()
                        .map(|buffer| {
                            Some(wgpu::RenderPassColorAttachment {
                                view: buffer,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: if clear {
                                        wgpu::LoadOp::Clear(wgpu::Color {
                                            r: 0.0,
                                            g: 0.0,
                                            b: 0.0,
                                            a: -1.0,
                                        })
                                    } else {
                                        wgpu::LoadOp::Load
                                    },
                                    store: wgpu::StoreOp::Store,
                                },
                            })
                        })
                        .collect::<Vec<_>>(),
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: if clear {
                                wgpu::LoadOp::Clear(1.0)
                            } else {
                                wgpu::LoadOp::Load
                            },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: self.query_set.as_ref().map(|query_set| {
                        wgpu::RenderPassTimestampWrites {
                            query_set,
                            beginning_of_pass_write_index: Some(0),
                            end_of_pass_write_index: None,
                        }
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(deferred.geometry_pipeline);
                pass.set_bind_group(0, scene_bind_group, &[]);
                draw_work(&mut pass, work, size);
            }
            if deferred.is_none() {
                if let Some(hybrid) = &hybrid {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("opaque SDF depth batch"),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: self.query_set.as_ref().map(|query_set| {
                            wgpu::RenderPassTimestampWrites {
                                query_set,
                                beginning_of_pass_write_index: Some(0),
                                end_of_pass_write_index: None,
                            }
                        }),
                        ..Default::default()
                    });
                    if let Some(depth_pipeline) = hybrid.depth_pipeline {
                        pass.set_pipeline(depth_pipeline);
                        pass.set_bind_group(0, scene_bind_group, &[]);
                        draw_work(&mut pass, work, size);
                    }
                }
            }
            {
                let shading_work = if deferred.is_some() {
                    deferred_shading_work(work, size)
                } else {
                    work
                };
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("budgeted scene batch"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        timestamp_writes: self
                            .query_set
                            .as_ref()
                            .filter(|_| hybrid.is_none() || matches!(work, Work::Relight { .. }))
                            .map(|query_set| wgpu::RenderPassTimestampWrites {
                                query_set,
                                beginning_of_pass_write_index: (deferred.is_none()
                                    || matches!(work, Work::Relight { .. }))
                                .then_some(0),
                                end_of_pass_write_index: hybrid.is_none().then_some(1),
                            }),
                        ..Default::default()
                    });
                    pass.set_pipeline(if deferred.is_some() {
                        &self.deferred_pipeline
                    } else {
                        scene
                    });
                    pass.set_bind_group(0, scene_bind_group, &[]);
                    if deferred.is_some() {
                        pass.set_bind_group(
                            1,
                            &targets
                                .deferred
                                .as_ref()
                                .expect("deferred targets")
                                .for_work(work)
                                .bind_group,
                            &[],
                        );
                    }
                    draw_work(&mut pass, shading_work, size);
                }
                if let Some(hybrid) = &hybrid {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Gaussian splat mesh batch"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: self.query_set.as_ref().map(|query_set| {
                            wgpu::RenderPassTimestampWrites {
                                query_set,
                                beginning_of_pass_write_index: None,
                                end_of_pass_write_index: Some(1),
                            }
                        }),
                        ..Default::default()
                    });
                    pass.set_pipeline(hybrid.splat_pipeline);
                    pass.set_bind_group(0, hybrid.splat_bind_group, &[]);
                    pass.set_vertex_buffer(0, hybrid.splat_buffer.slice(..));
                    draw_splat_work(&mut pass, shading_work, size, hybrid.splat_count);
                    if hybrid.mesh_vertex_count > 0 {
                        pass.set_pipeline(hybrid.mesh_pipeline);
                        pass.set_bind_group(0, hybrid.mesh_bind_group, &[]);
                        pass.set_vertex_buffer(0, hybrid.mesh_buffer.slice(..));
                        draw_mesh_work(&mut pass, shading_work, size, hybrid.mesh_vertex_count);
                    }
                }
            }
            match work {
                Work::Refine { end, .. } | Work::Interleave { end, .. } => self.completed = end,
                _ => {}
            }
        }
        queue.write_buffer(
            &self.progress,
            0,
            bytemuck::cast_slice(&[
                size[0],
                size[1],
                if self.interleaved { 0 } else { TILE },
                self.completed,
            ]),
        );
        if work == Work::Cached {
            return None;
        }
        self.submitted_pixels = match work {
            Work::Preview => targets.preview_size[0] * targets.preview_size[1],
            Work::Refine { first, end } => (first..end)
                .map(|tile| {
                    let rect = tile_rect(size, tile);
                    rect[2] * rect[3]
                })
                .sum(),
            Work::Interleave { first, end } => {
                // Edge blocks have fewer than 256 native pixels; the
                // padded estimate keeps the adaptive budget conservative.
                (end - first) * size[0].div_ceil(16) * size[1].div_ceil(16)
            }
            Work::Relight { .. } | Work::Cached => 0,
        };
        self.submitted_budget = match work {
            Work::Refine { .. } | Work::Interleave { .. } => self.pixel_budget,
            Work::Preview => self.pixel_budget,
            Work::Relight { .. } | Work::Cached => 0,
        };
        self.pending = true;
        if let (Some(query_set), Some(resolve)) = (&self.query_set, &self.query_resolve) {
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("viewport timestamp readback"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.resolve_query_set(query_set, 0..2, resolve, 0);
            encoder.copy_buffer_to_buffer(resolve, 0, &readback, 0, 16);
            Some(readback)
        } else {
            None
        }
    }

    pub fn submitted(&self, queue: &wgpu::Queue, work: Work, readback: Option<wgpu::Buffer>) {
        if work == Work::Cached {
            return;
        }
        let timing = self.timing.clone();
        if let Some(buffer) = readback {
            let period = queue.get_timestamp_period() as f64;
            let mapped = buffer.clone();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let milliseconds = result.ok().and_then(|_| {
                        let view = mapped.slice(..).get_mapped_range().ok()?;
                        let values: &[u64] = bytemuck::cast_slice(&view);
                        Some(values[1].saturating_sub(values[0]) as f64 * period / 1_000_000.0)
                    });
                    mapped.unmap();
                    *timing.lock().unwrap() = Some(milliseconds);
                });
        } else {
            queue.on_submitted_work_done(move || *timing.lock().unwrap() = Some(None));
        }
    }

}

fn draw_work(pass: &mut wgpu::RenderPass<'_>, work: Work, size: [u32; 2]) {
    match work {
        Work::Preview => pass.draw(0..3, 0..1),
        Work::Refine { first, end } => {
            let columns = size[0].div_ceil(TILE);
            let mut tile = first;
            while tile < end {
                let row_end = ((tile / columns + 1) * columns).min(end);
                let [x, y, _, height] = tile_rect(size, tile);
                let last = tile_rect(size, row_end - 1);
                pass.set_scissor_rect(x, y, last[0] + last[2] - x, height);
                pass.draw(0..3, 0..1);
                tile = row_end;
            }
        }
        Work::Interleave { first, end } => {
            let samples = size[0].div_ceil(16) * size[1].div_ceil(16);
            for phase in first..end {
                let start = 0x8000_0000 | (phase << 23);
                pass.draw(0..6, start..start + samples);
            }
        }
        Work::Relight { .. } | Work::Cached => unreachable!(),
    }
}

fn draw_splat_work(pass: &mut wgpu::RenderPass<'_>, work: Work, size: [u32; 2], count: u32) {
    match work {
        Work::Preview => pass.draw(0..6, 0..count),
        Work::Refine { first, end } => {
            let columns = size[0].div_ceil(TILE);
            let mut tile = first;
            while tile < end {
                let row_end = ((tile / columns + 1) * columns).min(end);
                let [x, y, _, height] = tile_rect(size, tile);
                let last = tile_rect(size, row_end - 1);
                pass.set_scissor_rect(x, y, last[0] + last[2] - x, height);
                pass.draw(0..6, 0..count);
                tile = row_end;
            }
        }
        Work::Interleave { .. } => unreachable!("hybrid splats use tiled refinement"),
        Work::Relight { .. } | Work::Cached => unreachable!(),
    }
}

fn draw_mesh_work(pass: &mut wgpu::RenderPass<'_>, work: Work, size: [u32; 2], count: u32) {
    match work {
        Work::Preview => pass.draw(0..count, 0..1),
        Work::Refine { first, end } => {
            let columns = size[0].div_ceil(TILE);
            let mut tile = first;
            while tile < end {
                let row_end = ((tile / columns + 1) * columns).min(end);
                let [x, y, _, height] = tile_rect(size, tile);
                let last = tile_rect(size, row_end - 1);
                pass.set_scissor_rect(x, y, last[0] + last[2] - x, height);
                pass.draw(0..count, 0..1);
                tile = row_end;
            }
        }
        Work::Interleave { .. } => unreachable!("hybrid mesh uses tiled refinement"),
        Work::Relight { .. } | Work::Cached => unreachable!(),
    }
}

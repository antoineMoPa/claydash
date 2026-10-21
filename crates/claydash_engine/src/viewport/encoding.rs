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
        let sparse_size = self.sparse_size(size);
        let cells = sparse_phase_pixels(sparse_size);
        if work == Work::Preview {
            if let Some(background) = &self.native_background {
                let emission = targets.bloom.as_ref().map(|bloom| &bloom.refined);
                let attachment = |view| Some(wgpu::RenderPassColorAttachment {
                    view, resolve_target: None, depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                });
                let attachments = [attachment(&targets.refined), emission.and_then(attachment)];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("native empty-space background"),
                    color_attachments: &attachments[..if emission.is_some() { 2 } else { 1 }],
                    ..Default::default()
                });
                pass.set_pipeline(background);
                pass.set_bind_group(0, scene_bind_group, &[]);
                pass.draw(0..3,0..1);
            }
        }
        if work != Work::Cached {
            let contact = targets.contact.as_ref().zip(self.contact_pipeline.as_ref())
                .filter(|_| self.contact_active && deferred.is_none() && matches!(work, Work::Refine { .. } | Work::Interleave { .. }))
                .filter(|_| !matches!(work, Work::Interleave { first, .. } if first < cells));
            let timing = BatchTiming::new(
                contact.is_some(), deferred.is_some(), hybrid.is_some(),
                matches!(work, Work::Relight { .. }), targets.bloom.is_some(),
            );
            if let Some((target, pipeline)) = contact {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("bounded primary contact probes"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    timestamp_writes: timing.writes(self.query_set.as_ref(), TimedPass::Contact),
                    ..Default::default()
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, scene_bind_group, &[]);
                draw_work(&mut pass, work, size, cells);
            }
            let view = if matches!(work, Work::Preview | Work::Relight { native: false }) {
                &targets.preview
            } else {
                &targets.refined
            };
            let emission_view = targets.bloom.as_ref().map(|bloom| {
                if matches!(work, Work::Preview | Work::Relight { native: false }) {
                    &bloom.preview
                } else {
                    &bloom.refined
                }
            });
            let attachment = |view| wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            };
            let attachments = [Some(attachment(view)), emission_view.map(attachment)];
            let attachments = &attachments[..if emission_view.is_some() { 2 } else { 1 }];
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
                    timestamp_writes: timing.writes(self.query_set.as_ref(), TimedPass::Geometry),
                    ..Default::default()
                });
                pass.set_pipeline(deferred.geometry_pipeline);
                pass.set_bind_group(0, scene_bind_group, &[]);
                draw_work(&mut pass, work, size, cells);
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
                        timestamp_writes: timing.writes(self.query_set.as_ref(), TimedPass::Depth),
                        ..Default::default()
                    });
                    if let Some(depth_pipeline) = hybrid.depth_pipeline {
                        pass.set_pipeline(depth_pipeline);
                        pass.set_bind_group(0, scene_bind_group, &[]);
                        draw_work(&mut pass, work, size, cells);
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
                        color_attachments: attachments,
                        timestamp_writes: timing.writes(self.query_set.as_ref(), TimedPass::Shading),
                        ..Default::default()
                    });
                    pass.set_pipeline(if deferred.is_some() {
                        if self.bloom.enabled {
                            self.deferred_bloom_pipeline
                                .as_ref()
                                .expect("deferred bloom pipeline")
                        } else if self.bloom.emission {
                            self.deferred_emission_pipeline
                                .as_ref()
                                .expect("deferred emission pipeline")
                        } else {
                            &self.deferred_pipeline
                        }
                    } else {
                        scene
                    });
                    pass.set_bind_group(0, scene_bind_group, &[]);
                    if let Some((target, _)) = contact {
                        pass.set_bind_group(1, &target.bind_group, &[]);
                    }
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
                    draw_work(&mut pass, shading_work, size, cells);
                }
                if let Some(hybrid) = &hybrid {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Gaussian splat mesh batch"),
                        // Radiance assets use the color target; preserve the
                        // separately accumulated scene emission for bloom.
                        color_attachments: &attachments[..1],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: timing.writes(self.query_set.as_ref(), TimedPass::Hybrid),
                        ..Default::default()
                    });
                    if hybrid.splat_count > 0 {
                        pass.set_pipeline(hybrid.splat_pipeline);
                        pass.set_bind_group(0, hybrid.splat_bind_group, &[]);
                        pass.set_vertex_buffer(0, hybrid.splat_buffer.slice(..));
                        draw_splat_work(&mut pass, shading_work, size, hybrid.splat_first_count);
                        for (buffer, count) in hybrid.splat_extra_buffers {
                            pass.set_vertex_buffer(0, buffer.slice(..));
                            draw_splat_work(&mut pass, shading_work, size, *count);
                        }
                    }
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
                self.refinement_rect[0], self.refinement_rect[1],
                sparse_size[0], sparse_size[1],
            ]),
        );
        if work == Work::Cached {
            return None;
        }
        if let Some(targets) = &targets.bloom {
            self.bloom.encode(encoder, targets, self.query_set.as_ref());
        }
        let stage = match work {
            Work::Preview => Stage::Preview,
            Work::Interleave { first, .. } if first < cells => Stage::Coverage,
            Work::Interleave { .. } => Stage::Sparse,
            Work::Refine { .. } => Stage::Tiles,
            Work::Relight { .. } | Work::Cached => Stage::Relight,
        };
        let pixels = match work {
            Work::Preview => targets.preview_size[0] * targets.preview_size[1],
            Work::Refine { first, end } => (first..end)
                .map(|tile| {
                    let rect = tile_rect(size, tile);
                    rect[2] * rect[3]
                })
                .sum(),
            Work::Interleave { first, end } => end - first,
            Work::Relight { .. } | Work::Cached => 0,
        };
        self.scheduler
            .encoded(work, stage, pixels, self.pixel_budget, self.target_ms);
        if work == Work::Preview {
            self.preview_ready = true;
        }
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene receipt and optional GPU timing"),
            size: 24,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if let (Some(query_set), Some(resolve)) = (&self.query_set, &self.query_resolve) {
            encoder.resolve_query_set(query_set, 0..2, resolve, 0);
            encoder.copy_buffer_to_buffer(resolve, 0, &readback, 0, 16);
        }
        // This tail command must execute in the same command buffer as the
        // draw. A later queue callback alone can hide a failed Metal buffer.
        encoder.copy_buffer_to_buffer(&self.receipt_source, 0, &readback, 16, 8);
        Some(readback)
    }

    pub fn submitted(&mut self, queue: &wgpu::Queue, work: Work, readback: Option<wgpu::Buffer>) {
        if work == Work::Cached {
            return;
        }
        let completion = self.scheduler.submitted(work);
        // Submission, not encoding or callback polling, starts the pacing clock.
        self.refinement_resume_at = Some(web_time::Instant::now() + REFINEMENT_PAUSE);
        if let Some(buffer) = readback {
            let period = queue.get_timestamp_period() as f64;
            let timestamps = self.query_set.is_some();
            let mapped = buffer.clone();
            let observation = completion.clone();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let milliseconds = result.ok().and_then(|_| {
                        let view = mapped.slice(..).get_mapped_range().ok()?;
                        let values: &[u64] = bytemuck::cast_slice(&view);
                        observation.verify_receipt(values[2] == SUBMISSION_RECEIPT);
                        timestamps.then(|| {
                            values[1].saturating_sub(values[0]) as f64 * period / 1_000_000.0
                        })
                    });
                    mapped.unmap();
                    *observation.milliseconds.lock().unwrap() = milliseconds;
                });
        }
        let repaint = self.repaint.clone();
        // A missing/failed/late timing map cannot block this independent fence.
        queue.on_submitted_work_done(move || {
            completion.finish_fence();
            if let Some(repaint) = repaint {
                repaint();
            }
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimedPass { Contact, Geometry, Depth, Shading, Hybrid, Bloom }

struct BatchTiming { first: TimedPass, last: TimedPass }
impl BatchTiming {
    fn new(contact: bool, deferred: bool, hybrid: bool, relight: bool, bloom: bool) -> Self {
        let first = if contact { TimedPass::Contact }
            else if deferred && !relight { TimedPass::Geometry }
            else if hybrid && !deferred { TimedPass::Depth }
            else { TimedPass::Shading };
        // Bloom owns only the end. In particular mesh+bloom must still record
        // the start in its depth pass instead of reusing a previous timestamp.
        let last = if bloom { TimedPass::Bloom }
            else if hybrid { TimedPass::Hybrid }
            else { TimedPass::Shading };
        Self { first, last }
    }

    fn writes<'a>(&self, query_set: Option<&'a wgpu::QuerySet>, pass: TimedPass)
        -> Option<wgpu::RenderPassTimestampWrites<'a>> {
        // wgpu rejects a descriptor with neither index. Intermediate passes
        // must omit it when earlier/later passes own both timestamps.
        query_set.filter(|_| pass == self.first || pass == self.last)
            .map(|query_set| wgpu::RenderPassTimestampWrites {
                query_set,
                beginning_of_pass_write_index: (pass == self.first).then_some(0),
                end_of_pass_write_index: (pass == self.last).then_some(1),
            })
    }
}

#[cfg(test)]
#[test]
fn every_render_path_times_its_first_and_last_executed_pass() {
    for deferred in [false, true] { for hybrid in [false, true] {
        for relight in [false, true] { for bloom in [false, true] {
            for contact in [false, true] {
                if contact && (deferred || relight) { continue; }
                if relight && !deferred { continue; }
                let mut passes = Vec::new();
                if contact { passes.push(TimedPass::Contact); }
                if deferred && !relight { passes.push(TimedPass::Geometry); }
                if !deferred && hybrid { passes.push(TimedPass::Depth); }
                passes.push(TimedPass::Shading);
                if hybrid { passes.push(TimedPass::Hybrid); }
                if bloom { passes.push(TimedPass::Bloom); }
                let timing = BatchTiming::new(contact, deferred, hybrid, relight, bloom);
                assert_eq!(timing.first, passes[0], "{passes:?}");
                assert_eq!(timing.last, *passes.last().unwrap(), "{passes:?}");
            }
        } }
    } }
}

fn draw_work(pass: &mut wgpu::RenderPass<'_>, work: Work, size: [u32; 2], cells: u32) {
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
            let offset = if first < cells { 0 } else { cells };
            let first = first - offset;
            let end = end - offset;
            pass.draw(3..9, first..end);
        }
        Work::Relight { .. } | Work::Cached => unreachable!(),
    }
}

fn draw_splat_work(pass: &mut wgpu::RenderPass<'_>, work: Work, size: [u32; 2], count: u32) {
    match work {
        Work::Preview => pass.draw(0..6, 0..count),
        Work::Refine { first, end } => {
            // A batch can span many rows. Drawing all splats once per row
            // repeats expensive covariance projection for every instance.
            // At most three disjoint rectangles cover the exact tile range.
            for [x, y, width, height] in splat_refine_scissors(size, first, end) {
                pass.set_scissor_rect(x, y, width, height);
                pass.draw(0..6, 0..count);
            }
        }
        Work::Interleave { .. } => unreachable!("hybrid splats use tiled refinement"),
        Work::Relight { .. } | Work::Cached => unreachable!(),
    }
}

pub(super) fn splat_refine_scissors(size: [u32; 2], first: u32, end: u32) -> Vec<[u32; 4]> {
    if first >= end { return Vec::new(); }
    let columns = size[0].div_ceil(TILE);
    let [first_x, first_y, _, first_height] = tile_rect(size, first);
    let [last_x, last_y, last_width, last_height] = tile_rect(size, end - 1);
    if first_y == last_y {
        return vec![[first_x, first_y, last_x + last_width - first_x, first_height]];
    }
    let mut strips = vec![[first_x, first_y, size[0] - first_x, first_height]];
    let middle_y = first_y + first_height;
    if middle_y < last_y {
        strips.push([0, middle_y, size[0], last_y - middle_y]);
    }
    strips.push([0, last_y, last_x + last_width, last_height]);
    debug_assert!(first / columns < (end - 1) / columns);
    strips
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

// Phase/state lives on the CPU. Every logical native pixel can be split out;
// each native draw is one pixel to bound the cost of divergent sheet lanes.
#[cfg(test)]
pub(super) fn sparse_draw_rects(
    size: [u32; 2],
    first: u32,
    end: u32,
) -> impl Iterator<Item = [u32; 4]> {
    let cells = sparse_phase_pixels(size);
    let columns = size[0].div_ceil(16);
    let mut next = first;
    std::iter::from_fn(move || loop {
        if next >= end {
            return None;
        }
        let native = next.saturating_sub(cells);
        let phase = native / cells;
        let cell = if next < cells { next } else { native % cells };
        let mut offset = [0, 0];
        for bit in 0..4 {
            let digit = (phase >> (6 - 2 * bit)) & 3;
            offset[0] |= u32::from(digit == 1 || digit == 2) << bit;
            offset[1] |= u32::from(digit == 1 || digit == 3) << bit;
        }
        let x = cell % columns * 16 + offset[0];
        let y = cell / columns * 16 + offset[1];
        next += 1;
        if x < size[0] && y < size[1] { return Some([x, y, 1, 1]); }
    })
}

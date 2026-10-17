use super::*;

#[test]
fn splat_refinement_draws_each_tile_pixel_once_with_bounded_draws() {
    for size in [[97, 73], [160, 100], [1165, 1448]] {
        let total = tile_count(size);
        for budget in [1, 7, 53, total] {
            let mut visits = vec![0u8; (size[0] * size[1]) as usize];
            for first in (0..total).step_by(budget as usize) {
                let strips = encoding::splat_refine_scissors(size, first, (first + budget).min(total));
                assert!(strips.len() <= 3);
                for [x, y, width, height] in strips {
                    for row in y..y + height {
                        for column in x..x + width {
                            visits[(row * size[0] + column) as usize] += 1;
                        }
                    }
                }
            }
            assert!(visits.iter().all(|visits| *visits == 1), "size={size:?}, budget={budget}");
        }
    }
}

#[test]
fn sparse_refinement_visits_distributed_single_pixels_before_neighbours() {
    let size = [64, 48];
    let cells = sparse_phase_pixels(size);
    for (phase, offset) in [(0, [0, 0]), (1, [8, 8]), (2, [8, 0]), (3, [0, 8])] {
        let first = cells + phase * cells;
        let rectangles: Vec<_> = encoding::sparse_draw_rects(size, first, first + cells).collect();
        assert_eq!(rectangles.len(), cells as usize);
        for (cell, [x, y, width, height]) in rectangles.into_iter().enumerate() {
            assert_eq!([width, height], [1, 1]);
            assert_eq!([x, y], [cell as u32 % 4 * 16 + offset[0], cell as u32 / 4 * 16 + offset[1]]);
        }
    }
}

#[test]
fn sparse_batches_cover_every_native_pixel_once() {
    for size in [[17, 31], [160, 100], [1165, 1448]] {
        let samples = sparse_phase_pixels(size);
        let total = sparse_sample_count(size);
        for budget in [1, 37, 1024, 16 * samples] {
            let mut visits = vec![0u8; (size[0] * size[1]) as usize];
            for first in (samples..total).step_by(budget as usize) {
                for [x, y, width, height] in encoding::sparse_draw_rects(size, first, (first + budget).min(total)) {
                    for row in y..y + height { for column in x..x + width {
                        visits[(row * size[0] + column) as usize] += 1;
                    } }
                }
            }
            assert!(visits.iter().all(|count| *count == 1));
            assert_eq!(encoding::sparse_draw_rects(size, 0, samples).count(), samples as usize);
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn point_drag_previews_changes_without_queuing_native_refinement() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default().request_adapter(&Default::default())
            .await.expect("GPU adapter");
        let (device, _) = adapter.request_device(&Default::default()).await.expect("GPU device");
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("interactive refinement test"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None,
                }, count: None,
            }],
        });
        for deferred in [false, true] {
            let mut viewport = Viewport::new(&device, wgpu::TextureFormat::Rgba8Unorm,
                false, &layout);
            let mut key = ViewKey {
                matrix: glam::Mat4::IDENTITY.to_cols_array_2d(), position: [0.0; 3],
                projection: 0, versions: [1, 1], size: [1024, 1024], refine: true,
            };
            viewport.set_interacting(true);
            viewport.set_rendering_path(RenderingPath::Mesh);
            viewport.pixel_budget = 1024 * 1024;
            // Unchanged initial budgets used to retain learned fast mesh
            // throughput after a point edit discarded the cached mesh.
            viewport.set_initial_budget(INITIAL_PIXELS);
            assert_eq!(viewport.pixel_budget, 1024 * 1024);
            viewport.scheduler.encoded(Work::Preview, Stage::Preview, 1024 * 1024, 1024 * 1024, EDIT_TARGET_MS);
            let completion = viewport.scheduler.submitted(Work::Preview);
            viewport.set_rendering_path(if deferred { RenderingPath::Deferred } else { RenderingPath::Exact });
            assert_eq!(viewport.pixel_budget, INITIAL_PIXELS);
            assert!(viewport.scheduler.busy(), "switching paths must preserve the GPU fence");
            assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Cached);
            *completion.milliseconds.lock().unwrap() = Some(0.1);
            completion.finish();
            assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Preview);
            assert_eq!(viewport.pixel_budget, INITIAL_PIXELS, "old mesh timings cannot inflate fallback work");
            viewport.preview_ready = true;
            for _ in 0..3 {
                assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Cached);
                assert_eq!(viewport.completed, 0);
                if deferred {
                    assert!(viewport.targets.as_ref().unwrap().deferred.as_ref().unwrap().native.is_none());
                }
                key.versions[0] += 1;
                assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Preview);
                viewport.preview_ready = true;
            }
            let learned_budget = viewport.scheduler.budget(Stage::Preview, INITIAL_PIXELS);
            viewport.set_interacting(false);
            assert!(matches!(viewport.prepare(&device, key, true, deferred, false), Work::Refine { .. }));
            assert_eq!(viewport.scheduler.budget(Stage::Preview, INITIAL_PIXELS), learned_budget);
        }
    });
}

#[test]
#[ignore = "requires a GPU adapter"]
fn mesh_only_view_renders_at_native_resolution_then_uses_cache() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default().request_adapter(&Default::default())
            .await.expect("GPU adapter");
        let (device, _) = adapter.request_device(&Default::default()).await.expect("GPU device");
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh scheduling test scene layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None,
                }, count: None,
            }],
        });
        let mut viewport = Viewport::new(&device, wgpu::TextureFormat::Rgba8Unorm,
            false, &scene_layout);
        let key = ViewKey {
            matrix: glam::Mat4::IDENTITY.to_cols_array_2d(), position: [0.0; 3],
            projection: 0, versions: [1, 1], size: [333, 217], refine: false,
        };
        assert_eq!(viewport.prepare(&device, key.clone(), false, false, false), Work::Preview);
        assert_eq!(viewport.targets.as_ref().unwrap().preview_size, key.size);
        assert!(!viewport.is_refined(), "allocating targets does not render an image");
        viewport.preview_ready = true; // Model the native preview encoding.
        assert_eq!(viewport.prepare(&device, key.clone(), false, false, false), Work::Cached);

        // A different document can reuse every version and camera value.
        // It must still start fresh, even with a late previous GPU callback.
        viewport.scheduler.encoded(Work::Preview, Stage::Preview, 1024, 1024, EDIT_TARGET_MS);
        let old = viewport.scheduler.submitted(Work::Preview);
        viewport.pixel_budget = TILE * TILE;
        viewport.completed = 7;
        viewport.reset_for_scene();
        assert!(viewport.scheduler.busy(), "document replacement must preserve the real GPU fence");
        assert!(viewport.targets.is_none());
        assert_eq!(viewport.completed, 0);
        assert_eq!(viewport.pixel_budget, INITIAL_PIXELS);
        assert_eq!(viewport.prepare(&device, key.clone(), false, false, false), Work::Cached);
        old.finish();
        assert_eq!(viewport.prepare(&device, key, false, false, false), Work::Preview);
        viewport.scheduler.encoded(Work::Preview, Stage::Preview, 64, 64, EDIT_TARGET_MS);
        let current = viewport.scheduler.submitted(Work::Preview);
        *old.milliseconds.lock().unwrap() = Some(1000.0);
        viewport.scheduler.retire(EDIT_TARGET_MS);
        assert!(viewport.scheduler.busy(), "old document callbacks cannot release new work");
        current.finish();

    });
}
#[test]
#[ignore = "requires a GPU adapter"]
fn refinement_yields_but_camera_changes_do_not_wait() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default().request_adapter(&Default::default())
            .await.expect("GPU adapter");
        let (device, _) = adapter.request_device(&Default::default()).await.expect("GPU device");
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("refinement scheduling test"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None,
                }, count: None,
            }],
        });
        for (size, interleaved) in [([1024, 1024], false), ([1024, 1024], true), ([2048, 1024], true)] {
            let mut viewport = Viewport::new(&device, wgpu::TextureFormat::Rgba8Unorm,
                false, &layout);
            viewport.set_initial_budget(4096);
            let mut key = ViewKey {
                matrix: glam::Mat4::IDENTITY.to_cols_array_2d(), position: [0.0; 3],
                projection: 0, versions: [1, 1], size, refine: true,
            };
            assert_ne!(viewport.prepare(&device, key.clone(), true, false, interleaved), Work::Cached);
            assert_eq!(viewport.interleaved, interleaved);
            viewport.preview_ready = true;
            // Inject a generous deadline so the test does not depend on
            // wall-clock scheduling, and simulate its expiry explicitly.
            viewport.refinement_resume_at = Some(web_time::Instant::now() + std::time::Duration::from_secs(60));
            assert_eq!(viewport.prepare(&device, key.clone(), true, false, interleaved), Work::Cached);
            // A cheap old angle must not inflate the next camera preview.
            viewport.pixel_budget = 1024 * 1024;
            key.position[0] = 1.0;
            assert_ne!(viewport.prepare(&device, key.clone(), true, false, interleaved), Work::Cached);
            assert_eq!(viewport.pixel_budget, 4096);
            viewport.preview_ready = true;
            viewport.refinement_resume_at = Some(web_time::Instant::now() - REFINEMENT_PAUSE);
            // Sparse work can be smaller than one whole viewport phase.
            viewport.scheduler.set_budget(Stage::Coverage, 37);
            let work = viewport.prepare(&device, key.clone(), true, false, interleaved);
            let pixels = match work {
                Work::Refine { first, end } => (end - first) * TILE * TILE,
                Work::Interleave { first, end } => end - first,
                other => panic!("expected refinement, got {other:?}"),
            };
            assert!(pixels <= viewport.pixel_budget.max(if interleaved { 64 } else { TILE * TILE }));
            viewport.scheduler.encoded(work, if interleaved { Stage::Coverage } else { Stage::Tiles }, pixels, viewport.pixel_budget, EDIT_TARGET_MS);
            let completion = viewport.scheduler.submitted(work);
            assert!(viewport.gpu_work_pending(&device));
            *completion.milliseconds.lock().unwrap() = Some(1.0);
            completion.finish();
            assert!(!viewport.gpu_work_pending(&device), "completed fence must unblock presentation");
            let deadline = viewport.refinement_resume_at;
            assert_ne!(viewport.prepare(&device, key, true, false, interleaved), Work::Cached,
                "observing completion after the pacing interval must not add an idle frame");
            assert_eq!(viewport.refinement_resume_at, deadline);
            viewport.pixel_budget = 1;
            let mut changed = viewport.key.clone().unwrap();
            changed.position[0] += 1.0;
            assert_eq!(viewport.prepare(&device, changed, true, false, interleaved), Work::Preview);
            assert!(viewport.pixel_budget >= MIN_PREVIEW_PIXELS,
                "native refinement must not leave the next camera preview at one pixel");
        }
    });
}

#[test]
#[ignore = "requires a GPU adapter"]
fn hybrid_refinement_improves_the_complete_preview_before_native_tiles() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default().request_adapter(&Default::default())
            .await.expect("GPU adapter");
        let (device, _) = adapter.request_device(&Default::default()).await.expect("GPU device");
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hybrid preview scheduling test"), entries: &[],
        });
        let mut viewport = Viewport::new(&device, wgpu::TextureFormat::Rgba8Unorm,
            false, &layout);
        viewport.set_rendering_path(RenderingPath::Hybrid);
        viewport.set_progressive_hybrid_preview(true);
        let key = ViewKey {
            matrix: glam::Mat4::IDENTITY.to_cols_array_2d(), position: [0.0; 3],
            projection: 0, versions: [1, 1], size: [1024, 1024], refine: true,
        };
        assert_eq!(viewport.prepare(&device, key.clone(), true, false, false), Work::Preview);
        let first = viewport.targets.as_ref().unwrap().preview_size;
        viewport.preview_ready = true;
        viewport.set_refinement_paused(true);
        assert_eq!(viewport.prepare(&device, key.clone(), true, false, false), Work::Cached);
        viewport.set_refinement_paused(false);
        assert_eq!(viewport.prepare(&device, key.clone(), true, false, false), Work::Preview);
        let second = viewport.targets.as_ref().unwrap().preview_size;
        assert!(second[0] > first[0] && second[1] > first[1]);
        viewport.preview_ready = true;
        assert_eq!(viewport.prepare(&device, key.clone(), true, false, false), Work::Preview);
        let third = viewport.targets.as_ref().unwrap().preview_size;
        assert!(third[0] > second[0] && third[1] > second[1]);
        viewport.preview_ready = true;
        assert!(matches!(viewport.prepare(&device, key.clone(), true, false, false), Work::Refine { .. }));
        let mut changed = key;
        changed.position[0] = 1.0;
        assert_eq!(viewport.prepare(&device, changed, true, false, false), Work::Preview);
        assert_eq!(viewport.preview_step, 0);
    });
}
#[test]
fn deferred_lighting_revision_reuses_geometry_but_edits_do_not() {
    let key = ViewKey {
        matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
        position: [0.0; 3],
        projection: 0,
        versions: [1, 1],
        size: [333, 217],
        refine: true,
    };
    let mut changed = key.clone();
    changed.versions[1] += 1;
    assert!(same_geometry(&key, &changed));
    changed.versions[0] += 1;
    assert!(!same_geometry(&key, &changed));
    for change in 0..5 {
        let mut changed = key.clone();
        match change {
            0 => changed.matrix[0][0] += 0.1,
            1 => changed.position[0] += 0.1,
            2 => changed.projection = 1,
            3 => changed.size[0] += 1,
            _ => changed.refine = false,
        }
        assert!(!same_geometry(&key, &changed));
    }
}

#[test]
fn deferred_tiles_display_immediately_and_final_batch_refreshes_effects() {
    let size = [333, 217];
    let total = tile_count(size);
    assert_eq!(deferred_shading_work(Work::Preview, size), Work::Preview);
    assert_eq!(deferred_shading_work(Work::Cached, size), Work::Cached);
    for end in 1..=total {
        let batch = Work::Refine {
            first: end - 1,
            end,
        };
        assert_eq!(deferred_shading_work(batch, size),
            if end == total { Work::Preview } else { batch });
    }
}

#[test]
fn tiles_cover_odd_sized_viewports_exactly_once() {
    for size in [[1, 1], [63, 65], [1280, 720], [333, 217]] {
        let mut coverage = vec![0u8; (size[0] * size[1]) as usize];
        for tile in 0..tile_count(size) {
            let [x, y, w, h] = tile_rect(size, tile);
            for row in y..y + h {
                for column in x..x + w {
                    coverage[(row * size[0] + column) as usize] += 1;
                }
            }
        }
        assert!(coverage.iter().all(|&count| count == 1));
    }
}
#[test]
fn preview_respects_budget_and_never_exceeds_native_size() {
    for size in [[1, 2000], [2000, 1], [1280, 720], [333, 217]] {
        for budget in [4096, INITIAL_PIXELS, 4 * 1024 * 1024] {
            let preview = preview_size(size, budget);
            assert!(preview[0] > 0 && preview[1] > 0);
            assert!(preview[0] <= size[0] && preview[1] <= size[1]);
            assert!(preview[0] * preview[1] <= budget);
        }
    }
}
#[test]
fn gpu_budget_recovers_from_expensive_views_without_vsync_feedback() {
    assert!(adjusted_budget(INITIAL_PIXELS, 35.0, EDIT_TARGET_MS, TILE * TILE) < INITIAL_PIXELS);
    assert!(adjusted_budget(INITIAL_PIXELS, 2.0, EDIT_TARGET_MS, TILE * TILE) > INITIAL_PIXELS);
    assert_eq!(
        adjusted_budget(INITIAL_PIXELS, f64::NAN, EDIT_TARGET_MS, TILE * TILE),
        INITIAL_PIXELS
    );
    assert_eq!(
        adjusted_budget(TILE * TILE, 100.0, EDIT_TARGET_MS, TILE * TILE),
        TILE * TILE
    );
    assert!(
        adjusted_budget(INITIAL_PIXELS, 10.0, PLAYBACK_TARGET_MS, TILE * TILE)
            > adjusted_budget(INITIAL_PIXELS, 10.0, EDIT_TARGET_MS, TILE * TILE)
    );
}

#[test]
fn sparse_budget_can_shrink_below_a_tile_for_expensive_small_views() {
    assert_eq!(budget_after_sample(1, 1, 1, 60.0, EDIT_TARGET_MS, 16),
        16, "growth limiting must honor a changed stage's minimum");
    let phase_pixels = 160_u32.div_ceil(16) * 100_u32.div_ceil(16);
    let mut budget = budget_after_sample(INITIAL_PIXELS, 16 * phase_pixels,
        16 * phase_pixels, 60.0, EDIT_TARGET_MS, 1);
    assert!(budget < TILE * TILE, "a capped phase batch must teach the budget");
    for _ in 0..4 {
        budget = budget_after_sample(budget, budget, budget, 60.0, EDIT_TARGET_MS, 1);
    }
    assert!(budget < phase_pixels, "a single sparse phase must not set the minimum batch size");
    assert_eq!(adjusted_budget(1, 60.0, EDIT_TARGET_MS, 1), 1);
    for budget in 1..5 {
        assert!(budget_after_sample(budget, budget, budget, 0.01, EDIT_TARGET_MS, 1) > budget,
            "rounding must not trap tiny sparse batches at their minimum");
    }
}

#[test]
fn final_partial_tile_keeps_the_budget_for_the_next_view() {
    let budget = 16 * 1024;
    assert_eq!(
        budget_after_sample(budget, budget * 2, 288, 1.0, EDIT_TARGET_MS, TILE * TILE),
        budget
    );
    assert!(budget_after_sample(budget, budget * 2, budget * 2, 20.0, EDIT_TARGET_MS, TILE * TILE) < budget);
}

#[test]
fn deferred_shader_modules_validate() {
    let source = deferred_shader_source();
    let module = wgpu::naga::front::wgsl::parse_str(&source)
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate deferred lighting and transmission");
}

#[test]
#[ignore = "requires a GPU adapter"]
fn optimization_work_pauses_refinement_but_keeps_previews_and_resumes() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default().request_adapter(&Default::default())
            .await.expect("GPU adapter");
        let (device, _) = adapter.request_device(&Default::default()).await.expect("GPU device");
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("optimization pause test"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None,
                }, count: None,
            }],
        });
        for deferred in [false, true] {
            let mut viewport = Viewport::new(&device, wgpu::TextureFormat::Rgba8Unorm, false, &layout);
            let mut key = ViewKey {
                matrix: glam::Mat4::IDENTITY.to_cols_array_2d(), position: [0.0; 3],
                projection: 0, versions: [1, 1], size: [1024, 1024], refine: true,
            };
            viewport.set_refinement_paused(true);
            assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Preview);
            viewport.preview_ready = true;
            for _ in 0..3 {
                assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Cached);
                assert_eq!(viewport.completed, 0);
                key.versions[0] += 1;
                assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Preview);
                viewport.preview_ready = true;
            }
            viewport.set_refinement_paused(false);
            assert!(matches!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Refine { .. }));
            let completed = viewport.completed;
            viewport.set_refinement_paused(true);
            assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Cached);
            assert_eq!(viewport.completed, completed);
            viewport.set_refinement_paused(false);
            assert!(matches!(viewport.prepare(&device, key, true, deferred, false), Work::Refine { .. }));
            viewport.set_refinement_paused(true);
            viewport.reset_for_scene();
            assert!(!viewport.refinement_paused);
        }
    });
}

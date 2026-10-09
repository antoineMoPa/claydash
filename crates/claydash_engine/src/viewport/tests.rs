use super::*;
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
            viewport.pending = true;
            viewport.submitted_pixels = 1024 * 1024;
            viewport.submitted_budget = 1024 * 1024;
            viewport.set_rendering_path(if deferred { RenderingPath::Deferred } else { RenderingPath::Exact });
            assert_eq!(viewport.pixel_budget, INITIAL_PIXELS);
            assert!(viewport.pending, "switching paths must preserve the GPU fence");
            assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Cached);
            *viewport.timing.lock().unwrap() = Some(Some(0.1));
            assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Preview);
            assert_eq!(viewport.pixel_budget, INITIAL_PIXELS, "old mesh timings cannot inflate fallback work");
            for _ in 0..3 {
                assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Cached);
                assert_eq!(viewport.completed, 0);
                if deferred {
                    assert!(viewport.targets.as_ref().unwrap().deferred.as_ref().unwrap().native.is_none());
                }
                key.versions[0] += 1;
                assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Preview);
            }
            let learned_budget = viewport.pixel_budget;
            viewport.set_interacting(false);
            assert!(matches!(viewport.prepare(&device, key, true, deferred, false), Work::Refine { .. }));
            assert_eq!(viewport.pixel_budget, learned_budget);
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
        assert_eq!(viewport.prepare(&device, key.clone(), false, false, false), Work::Cached);

        // A different document can reuse every version and camera value.
        // It must still start fresh, even with a late previous GPU callback.
        let old_timing = viewport.timing.clone();
        viewport.pending = true;
        viewport.pixel_budget = TILE * TILE;
        viewport.completed = 7;
        viewport.reset_for_scene();
        assert!(!viewport.pending);
        assert!(viewport.targets.is_none());
        assert_eq!(viewport.completed, 0);
        assert_eq!(viewport.pixel_budget, INITIAL_PIXELS);
        assert_eq!(viewport.prepare(&device, key, false, false, false), Work::Preview);
        viewport.pending = true;
        *old_timing.lock().unwrap() = Some(Some(1000.0));
        assert!(viewport.timing.lock().unwrap().is_none(),
            "late completions from the old document cannot affect the new one");
        assert!(viewport.pending);
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
        for interleaved in [false, true] {
            let mut viewport = Viewport::new(&device, wgpu::TextureFormat::Rgba8Unorm,
                false, &layout);
            let mut key = ViewKey {
                matrix: glam::Mat4::IDENTITY.to_cols_array_2d(), position: [0.0; 3],
                projection: 0, versions: [1, 1], size: [1024, 1024], refine: true,
            };
            assert_ne!(viewport.prepare(&device, key.clone(), true, false, interleaved), Work::Cached);
            // Inject a generous deadline so the test does not depend on
            // wall-clock scheduling, and simulate its expiry explicitly.
            viewport.refinement_resume_at = Some(web_time::Instant::now() + std::time::Duration::from_secs(60));
            assert_eq!(viewport.prepare(&device, key.clone(), true, false, interleaved), Work::Cached);
            key.position[0] = 1.0;
            assert_ne!(viewport.prepare(&device, key.clone(), true, false, interleaved), Work::Cached);
            viewport.refinement_resume_at = Some(web_time::Instant::now() - REFINEMENT_PAUSE);
            let work = viewport.prepare(&device, key, true, false, interleaved);
            let pixels = match work {
                Work::Refine { first, end } => (end - first) * TILE * TILE,
                Work::Interleave { first, end } => (end - first) * 64 * 64,
                other => panic!("expected refinement, got {other:?}"),
            };
            assert!(pixels <= viewport.pixel_budget);
        }
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
    assert!(adjusted_budget(INITIAL_PIXELS, 35.0, EDIT_TARGET_MS) < INITIAL_PIXELS);
    assert!(adjusted_budget(INITIAL_PIXELS, 2.0, EDIT_TARGET_MS) > INITIAL_PIXELS);
    assert_eq!(
        adjusted_budget(INITIAL_PIXELS, f64::NAN, EDIT_TARGET_MS),
        INITIAL_PIXELS
    );
    assert_eq!(
        adjusted_budget(TILE * TILE, 100.0, EDIT_TARGET_MS),
        TILE * TILE
    );
    assert!(
        adjusted_budget(INITIAL_PIXELS, 10.0, PLAYBACK_TARGET_MS)
            > adjusted_budget(INITIAL_PIXELS, 10.0, EDIT_TARGET_MS)
    );
}

#[test]
fn final_partial_tile_keeps_the_budget_for_the_next_view() {
    let budget = 16 * 1024;
    assert_eq!(
        budget_after_sample(budget, budget * 2, 288, 1.0, EDIT_TARGET_MS),
        budget
    );
    assert!(budget_after_sample(budget, budget * 2, budget * 2, 20.0, EDIT_TARGET_MS) < budget);
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
            for _ in 0..3 {
                assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Cached);
                assert_eq!(viewport.completed, 0);
                key.versions[0] += 1;
                assert_eq!(viewport.prepare(&device, key.clone(), true, deferred, false), Work::Preview);
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

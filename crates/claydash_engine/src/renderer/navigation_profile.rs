//! Manual bounded GPU navigation profile for a saved scene.
use super::*;
use glam::{Vec2, Vec3};

#[test]
#[ignore = "manual saved-scene GPU profile"]
fn profile_navigation_gpu() {
    use std::time::{Duration, Instant};
    let path = std::env::var("CLAYDASH_PROFILE_SCENE").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/astra_car.claydash"
        )
        .into()
    });
    let size = std::env::var("CLAYDASH_PROFILE_SIZE").unwrap_or_else(|_| "320x200".into());
    let (width, height) = size.split_once('x').unwrap();
    let (width, height) = (
        width.parse::<u32>().unwrap(),
        height.parse::<u32>().unwrap(),
    );
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut scene: Vec<SdfObject> =
        serde_json::from_value(document["subtree"]["sdf_objects"]["value"]["VecSDFObject"].clone())
            .unwrap();
    let mut world: World =
        serde_json::from_value(document["subtree"]["world"]["value"]["World"].clone()).unwrap();
    let variant = std::env::var("CLAYDASH_PROFILE_VARIANT").unwrap_or_else(|_| "scene".into());
    match variant.as_str() {
        "scene" => {}
        "no_bloom" => world.bloom_enabled = false,
        "no_emission" => {
            for object in &mut scene {
                object.material.emission_intensity = 0.0;
            }
        }
        "no_lattice" => {
            for object in &mut scene {
                object.lattice = None;
            }
        }
        "opaque_solid" => {
            for object in &mut scene {
                let color = object.material.color;
                object.material = crate::model::Material::preset(crate::model::MaterialKind::Solid);
                object.material.color = color;
                object.material.reflectivity = 0.0;
                object.material.metallic = 0.0;
            }
        }
        "no_ao" => {}
        "one_action" => {}
        "fast_native" => {}
        "no_lofts" => {
            scene.retain(|object| !matches!(object.params, crate::model::SdfParams::LoftParams(_)))
        }
        "no_sheets" => scene.retain(|object| {
            !matches!(object.params, crate::model::SdfParams::SheetPanelParams(_))
        }),
        "no_relief" | "no_surface" | "no_emboss" => {
            for object in &mut scene {
                if let crate::model::SdfParams::SheetPanelParams(panel) = &mut object.params {
                    if variant != "no_emboss" {
                        panel.surface = None;
                    }
                    if variant != "no_surface" {
                        panel.embosses.clear();
                        panel.path_embosses.clear();
                    }
                }
            }
        }
        _ => panic!("unknown profile variant {variant}"),
    }
    let mut renderer =
        pollster::block_on(Renderer::new_headless(crate::renderer::HeadlessOptions {
            width,
            height,
            ..Default::default()
        }))
        .unwrap();
    renderer.device.set_device_lost_callback(|reason, message| {
        eprintln!("PROFILE DEVICE LOST: {reason:?}: {message}");
    });
    renderer
        .device
        .on_uncaptured_error(std::sync::Arc::new(|error| {
            eprintln!("PROFILE GPU ERROR: {error:?}");
        }));
    if variant == "no_ao" {
        renderer.shader_source = renderer.shader_source.replace(
            "fn ambient_occlusion(",
            "fn diagnostic_original_ambient_occlusion(",
        );
        renderer.shader_source.push_str(
            "\nfn ambient_occlusion(point: vec3<f32>, normal: vec3<f32>) -> f32 { return 1.0; }\n",
        );
    }
    if variant == "one_action" {
        renderer.shader_source = renderer.shader_source.replace("action < 12", "action < 1");
    }
    if variant == "fast_native" {
        renderer.shader_source = renderer
            .shader_source
            .replace("override FAST_PREVIEW: bool = false;", "")
            .replace("FAST_PREVIEW", "true");
    }
    if matches!(variant.as_str(), "no_ao" | "one_action" | "fast_native") {
        renderer.scene_pipeline_cache.clear();
    }
    let context = egui::Context::default();
    let mut camera = crate::camera::Camera::new();
    camera.viewport = renderer.size();
    camera.position = Vec3::new(-5.0, 3.0, 7.0);
    camera.target = Vec3::ZERO;
    let stationary = std::env::var_os("CLAYDASH_PROFILE_STATIONARY").is_some();
    if stationary {
        camera.position = Vec3::new(-14.499846, 6.315958, -2.584872);
        camera.up = Vec3::new(0.41120136, 0.7101021, -0.5715493);
    }
    eprintln!(
        "Navigation GPU profile: {path}, {} objects, {width}x{height}, {variant}",
        scene.len()
    );
    for refine in [false, true] {
        if stationary && !refine {
            continue;
        }
        let mut cpu = Vec::new();
        let mut gpu = Vec::new();
        let mut previews = Vec::new();
        let mut scene_times = Vec::new();
        let frames = std::env::var("CLAYDASH_PROFILE_FRAMES")
            .ok()
            .map(|n| n.parse().unwrap())
            .unwrap_or(10);
        let turns = std::env::var("CLAYDASH_PROFILE_TURNS")
            .ok()
            .map(|n| n.parse::<u32>().unwrap())
            .unwrap_or(0);
        let require_finish = std::env::var_os("CLAYDASH_PROFILE_FINISH").is_some();
        let limit_seconds = std::env::var("CLAYDASH_PROFILE_SECONDS").ok().map(|v|v.parse::<u64>().unwrap()).unwrap_or(120);
        let ready = |renderer: &Renderer| renderer.viewport_refined();
        let pass_started = Instant::now();
        let mut zero_timings = 0;
        for frame in 0..frames {
            if let Ok(pixels) = std::env::var("CLAYDASH_PROFILE_PIXELS") {
                renderer
                    .viewport
                    .fixed_profile_budget(pixels.parse().unwrap());
            }
            if frame == 0 {
                if let Ok(pixels) = std::env::var("CLAYDASH_PROFILE_START_PIXELS") {
                    renderer
                        .viewport
                        .fixed_profile_budget(pixels.parse().unwrap());
                }
            }
            let navigating = !stationary || frame < turns;
            if navigating {
                if std::env::var_os("CLAYDASH_PROFILE_ORBIT").is_some() {
                    camera.orbit(Vec2::new(14.0, 7.0));
                } else {
                    camera.pan(Vec2::new(3.0, 1.0));
                }
            }
            let mut output = context.run_ui(egui::RawInput::default(), |_| {});
            let start = Instant::now();
            renderer.render(
                &camera,
                &scene,
                &[],
                [7, 7],
                0,
                world,
                &[],
                &context,
                &mut output,
                false,
                false,
                true,
                refine,
                false,
                navigating,
                false,
                false,
            );
            let submitted = Instant::now();
            renderer
                .device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(15)),
                })
                .expect("bounded navigation frame");
            let completion_ms = submitted.elapsed().as_secs_f64() * 1000.0;
            assert!(
                renderer.gpu_fault().is_none(),
                "GPU fault: {:?}",
                renderer.gpu_fault()
            );
            #[cfg(unix)]
            {
                if renderer.viewport.diagnostics()["completed_gpu_ms"].as_f64() == Some(0.0) {
                    zero_timings += 1;
                } else {
                    zero_timings = 0;
                }
                if zero_timings == 3 {
                    assert!(renderer.clear_probe_pixels().iter().any(|v| *v == 255),
                        "GPU stopped executing even independent clear commands; abort the profile instead of retrying a poisoned queue");
                }
            }
            if stationary {
                if frame < 10 || frame % 30 == 0 || frame + 1 == frames || renderer.viewport_refined() {
                    eprintln!("frame {frame}: {}", renderer.viewport.diagnostics());
                }
                std::thread::sleep(Duration::from_millis(17));
            }
            if frame > 1 {
                cpu.push(submitted.duration_since(start).as_secs_f64() * 1000.0);
                gpu.push(completion_ms);
                previews.push(renderer.preview_pixels());
                #[cfg(unix)]
                if let Some(ms) = renderer.viewport.diagnostics()["completed_gpu_ms"].as_f64() {
                    scene_times.push(ms);
                }
            }
            if require_finish && ready(&renderer) {
                eprintln!(
                    "{} after {frame} frames in {:.3} s", "Full material converged",
                    pass_started.elapsed().as_secs_f64()
                );
                break;
            }
            assert!(
                pass_started.elapsed() < Duration::from_secs(limit_seconds),
                "bounded saved-scene profile"
            );
        }
        eprintln!("Navigation frame loop elapsed {:.3} s", pass_started.elapsed().as_secs_f64());
        if require_finish {
            assert!(
                ready(&renderer),
                "saved scene must finish, not merely keep the UI loop running"
            );
        }
        cpu.sort_by(f64::total_cmp);
        gpu.sort_by(f64::total_cmp);
        previews.sort_unstable();
        scene_times.sort_by(f64::total_cmp);
        if let Ok(path) = std::env::var("CLAYDASH_PROFILE_IMAGE") {
            let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("navigation profile progress capture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: renderer.render_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            let mut encoder = renderer.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
                renderer.viewport.composite(&mut pass);
            }
            renderer.queue.submit([encoder.finish()]);
            let actual = renderer.benchmark_pixels(&target);
            std::fs::write(&path, &actual).unwrap();
            if require_finish && variant == "scene" && std::env::var_os("CLAYDASH_PROFILE_SKIP_REFERENCE").is_none() {
                let reference = renderer.direct_reference_pixels(false);
                std::fs::write(format!("{path}.direct.ppm"), &reference).unwrap();
                let fast = renderer.direct_reference_pixels(true);
                std::fs::write(format!("{path}.fast.ppm"), &fast).unwrap();
                let probe = renderer.clear_probe_pixels();
                eprintln!(
                    "Clear probe white components {}",
                    probe.iter().filter(|v| **v == 255).count()
                );
                eprintln!(
                    "Reference pixels: full nonzero {}, fast nonzero {}",
                    reference.iter().filter(|v| **v != 0).count(),
                    fast.iter().filter(|v| **v != 0).count()
                );
                let changed = actual
                    .iter()
                    .zip(&reference)
                    .filter(|(a, b)| a != b)
                    .count();
                assert_eq!(
                    changed, 0,
                    "saved scene must match an independent direct native draw"
                );
                assert!(
                    actual.iter().filter(|b| **b != 0).count() > 100,
                    "scene verification must contain actual rendered pixels"
                );
            }
        }
        eprintln!("{} moving: CPU p50 {:.3} ms p95 {:.3} ms; GPU p50 {:.3} ms p95 {:.3} ms; preview {} pixels",
            if refine { "Full material" } else { "Simple shading" }, cpu[cpu.len() / 2], cpu[cpu.len() * 95 / 100], gpu[gpu.len() / 2], gpu[gpu.len() * 95 / 100], previews[previews.len() / 2]);
        if !scene_times.is_empty() {
            eprintln!(
                "Scene timestamps p50 {:.3} ms",
                scene_times[scene_times.len() / 2]
            );
        }
        eprintln!(
            "BVH nodes {}, Boolean path {}, hybrid {}, capture cache {}",
            renderer.node_count,
            renderer.has_booleans,
            renderer.hybrid_enabled,
            renderer.group_capture_cache.len()
        );
    }
}

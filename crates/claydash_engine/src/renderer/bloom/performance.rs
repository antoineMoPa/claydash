//! Opt-in native screenshot and timestamp benchmark for the authored flying cars.
use crate::{
    camera::ProjectionMode,
    model::{BackgroundMode, SdfObject, World},
    renderer::{HeadlessOptions, Renderer},
    Camera,
};
use std::{path::PathBuf, sync::mpsc, time::Duration};
use wgpu::util::DeviceExt;

fn load(path: PathBuf) -> (Vec<SdfObject>, World) {
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let tree = &document["subtree"];
    let objects =
        serde_json::from_value(tree["sdf_objects"]["value"]["VecSDFObject"].clone()).unwrap();
    let world = tree
        .get("world")
        .map(|node| serde_json::from_value(node["value"]["World"].clone()).unwrap())
        .unwrap_or_default();
    (objects, world)
}

fn camera_for(name: &str, detail: bool, size: [u32; 2]) -> Camera {
    let (target, position) = match (name, detail) {
        ("flying_sedan", false) => ([0.0, 0.8, 0.0], [5.6, 3.4, -7.8]),
        ("flying_sedan", true) => ([1.2, 0.36, -1.2], [3.1, 1.3, -3.3]),
        ("retro_flying_car", true) => ([-1.94, 0.67, 1.69], [-4.0, 1.6, 4.0]),
        ("astra_car", true) => ([-1.68, 0.47, 1.55], [-4.0, 1.6, 3.9]),
        (_, false) => ([0.0, 0.8, 0.0], [-7.4, 3.8, 6.2]),
        _ => unreachable!(),
    };
    let mut camera = Camera::new();
    camera.target = glam::Vec3::from_array(target);
    camera.position = glam::Vec3::from_array(position);
    camera.viewport = glam::Vec2::new(size[0] as f32, size[1] as f32);
    camera.projection_mode = ProjectionMode::Perspective;
    camera
}

fn summary(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    let median = if values.len() % 2 == 0 {
        (values[values.len() / 2 - 1] + values[values.len() / 2]) * 0.5
    } else {
        values[values.len() / 2]
    };
    serde_json::json!({"median_ms":median, "p95_ms":values[((values.len() as f64 * 0.95).ceil() as usize - 1).min(values.len()-1)], "samples_ms":values})
}

fn measure(
    renderer: &mut Renderer,
    camera: &Camera,
    cached: bool,
    fast: bool,
    refresh_bloom: bool,
) -> f64 {
    let device = &renderer.device;
    let queue = &renderer.queue;
    let size = [camera.viewport.x as u32, camera.viewport.y as u32];
    static FRAME_REVISION: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);
    if !cached {
        FRAME_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let revision = FRAME_REVISION.load(std::sync::atomic::Ordering::Relaxed);
    let key = crate::viewport::ViewKey {
        matrix: (camera.projection() * camera.view()).to_cols_array_2d(),
        position: camera.position.to_array(),
        projection: 0,
        versions: [1, revision],
        size,
        refine: false,
    };
    let work = renderer.viewport.prepare(device, key, false, false, false);
    assert_eq!(
        work,
        if cached {
            crate::viewport::Work::Cached
        } else {
            crate::viewport::Work::Preview
        }
    );
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bloom benchmark composite target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.render_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let query = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("whole viewport GPU timestamps"),
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &[0u8; 16],
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        // Start at the first GPU pass; excludes CPU upload, compilation and allocation.
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("benchmark start"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
                query_set: &query,
                beginning_of_pass_write_index: Some(0),
                end_of_pass_write_index: None,
            }),
            ..Default::default()
        });
    }
    let pipeline = match (renderer.has_booleans, fast) {
        (true, false) => &renderer.boolean_pipeline.as_ref().unwrap().1,
        (true, true) => &renderer.fast_boolean_pipeline.as_ref().unwrap().1,
        (false, false) => &renderer.pipeline,
        (false, true) => &renderer.fast_pipeline,
    };
    let viewport_readback = renderer.viewport.encode(
        device,
        queue,
        &mut encoder,
        work,
        pipeline,
        &renderer.bind_group,
        None,
        None,
    );
    if refresh_bloom {
        renderer.viewport.benchmark_bloom_passes(&mut encoder);
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("benchmark composite / end"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
                query_set: &query,
                beginning_of_pass_write_index: None,
                end_of_pass_write_index: Some(1),
            }),
            ..Default::default()
        });
        renderer.viewport.composite(&mut pass);
    }
    encoder.resolve_query_set(&query, 0..2, &resolve, 0);
    encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, 16);
    queue.submit([encoder.finish()]);
    renderer.viewport.submitted(queue, work, viewport_readback);
    let (tx, rx) = mpsc::channel();
    read.slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(120)),
        })
        .unwrap();
    rx.recv_timeout(Duration::from_secs(120)).unwrap().unwrap();
    let mapped = read.slice(..).get_mapped_range().unwrap();
    let times: &[u64] = bytemuck::cast_slice(&mapped);
    let ms = (times[1] - times[0]) as f64 * queue.get_timestamp_period() as f64 / 1e6;
    drop(mapped);
    read.unmap();
    ms
}

#[test]
#[ignore = "requires Metal GPU and CLAYDASH_BLOOM_CAR_OUTPUT"]
fn flying_car_bloom_screenshots_and_performance() {
    let output = PathBuf::from(std::env::var("CLAYDASH_BLOOM_CAR_OUTPUT").expect("output folder"));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut results = Vec::new();
    let names = std::env::var("CLAYDASH_BLOOM_CAR_CASE")
        .unwrap_or_else(|_| "flying_sedan,retro_flying_car,astra_car".to_owned());
    for name in names.split(',') {
        let (objects, world) = load(root.join(format!("examples/{name}.claydash")));
        let (original, _) = load(output.join(format!("{name}-original.claydash")));
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 1280,
            height: 720,
            ..Default::default()
        }))
        .unwrap();
        assert!(renderer
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY));
        eprintln!("Car {name}: {} objects", objects.len());
        if std::env::var("CLAYDASH_BLOOM_BENCH_PASS").as_deref() == Ok("isolated") {
            for size in [[640, 360], [1280, 720]] {
                renderer.resize(winit::dpi::PhysicalSize::new(size[0], size[1]));
                let camera = camera_for(name, false, size);
                renderer.invalidate_scene();
                renderer.upload_scene_with_world(&camera, &objects, &[], [1, 1], world);
                renderer.viewport.bloom_settings(&renderer.queue, world);
                for _ in 0..3 {
                    measure(&mut renderer, &camera, false, true, false);
                }
                // Paired measurements use the same cached scene and mask. Alternate
                // with/without the blur chain to reduce GPU clock / load drift.
                let mut cached = Vec::new();
                let mut refreshed = Vec::new();
                for i in 0..60 {
                    if i % 2 == 0 {
                        cached.push(measure(&mut renderer, &camera, true, true, false));
                        refreshed.push(measure(&mut renderer, &camera, true, true, true));
                    } else {
                        refreshed.push(measure(&mut renderer, &camera, true, true, true));
                        cached.push(measure(&mut renderer, &camera, true, true, false));
                    }
                }
                let deltas = refreshed
                    .iter()
                    .zip(&cached)
                    .map(|(on, off)| on - off)
                    .collect();
                let result = serde_json::json!({"scene":name,"size":size,"effect_passes":summary(deltas),"blur_and_composite":summary(refreshed),"cached_composite":summary(cached)});
                eprintln!("ISOLATED {}", result);
                results.push(result);
                std::fs::write(
                    output.join(format!("{name}-isolated-performance.json")),
                    serde_json::to_vec_pretty(&results).unwrap(),
                )
                .unwrap();
            }
            continue;
        }

        let capture_world = World {
            background: BackgroundMode::Flat,
            flat_color: [0.022, 0.025, 0.03],
            ..world
        };
        for detail in [false, true] {
            let views = std::env::var("CLAYDASH_BLOOM_CAR_VIEWS")
                .unwrap_or_else(|_| "overview,detail".into());
            if !views
                .split(',')
                .any(|view| view == if detail { "detail" } else { "overview" })
            {
                continue;
            }
            let camera = camera_for(name, detail, [1280, 720]);
            let view = if detail { "detail" } else { "overview" };
            for (mode, scene, settings) in [
                (
                    "original",
                    &original,
                    World {
                        bloom_enabled: false,
                        ..capture_world
                    },
                ),
                (
                    "emission",
                    &objects,
                    World {
                        bloom_enabled: false,
                        ..capture_world
                    },
                ),
                ("bloom", &objects, capture_world),
            ] {
                let modes = std::env::var("CLAYDASH_BLOOM_MODES")
                    .unwrap_or_else(|_| "original,emission,bloom".into());
                if !modes.split(',').any(|selected| selected == mode) {
                    continue;
                }
                let frame = renderer
                    .render_offscreen(&camera, scene, settings, &[], Duration::from_secs(120))
                    .unwrap();
                if mode != "original" {
                    let baseline = image::open(output.join(format!("{name}-{view}-original.png")))
                        .unwrap()
                        .to_rgba8();
                    for (x, y) in [(0, 0), (1279, 0), (0, 719), (1279, 719), (640, 0)] {
                        let offset = (y * 1280 + x) as usize * 4;
                        assert_eq!(
                            &frame.rgba[offset..offset + 4],
                            baseline.get_pixel(x, y).0.as_slice(),
                            "background corruption in {name}-{view}-{mode}"
                        );
                    }
                }
                let path = output.join(format!("{name}-{view}-{mode}.png"));
                image::save_buffer(
                    &path,
                    &frame.rgba,
                    frame.width,
                    frame.height,
                    image::ColorType::Rgba8,
                )
                .unwrap();
                eprintln!("Captured {}", path.display());
            }
        }
        if std::env::var("CLAYDASH_BLOOM_BENCH_PASS").as_deref() == Ok("capture-only") {
            continue;
        }
        for size in [[640, 360], [1280, 720]] {
            // Resize through public API before encoding full native-resolution draws.
            renderer.resize(winit::dpi::PhysicalSize::new(size[0], size[1]));
            let camera = camera_for(name, false, size);
            for fast in [false, true] {
                for (mode, scene, settings) in [
                    (
                        "original",
                        &original,
                        World {
                            bloom_enabled: false,
                            ..world
                        },
                    ),
                    (
                        "emission",
                        &objects,
                        World {
                            bloom_enabled: false,
                            ..world
                        },
                    ),
                    ("bloom", &objects, world),
                ] {
                    renderer.invalidate_scene();
                    renderer.upload_scene_with_world(&camera, scene, &[], [1, 1], settings);
                    renderer.viewport.bloom_settings(&renderer.queue, settings);
                    assert!(
                        !renderer.hybrid_enabled,
                        "benchmark must use the same exact path for all modes"
                    );
                    for _ in 0..4 {
                        measure(&mut renderer, &camera, false, fast, false);
                    }
                    let samples = (0..20)
                        .map(|_| measure(&mut renderer, &camera, false, fast, false))
                        .collect();
                    let cached = (0..20)
                        .map(|_| measure(&mut renderer, &camera, true, fast, false))
                        .collect();
                    let result = serde_json::json!({"scene":name,"size":size,"mode":mode,"shading":if fast {"interactive"} else {"refined"},"gpu":summary(samples),"cached_gpu":summary(cached)});
                    eprintln!("BENCH {}", result);
                    results.push(result);
                    std::fs::write(
                        output.join(format!("{name}-performance.json")),
                        serde_json::to_vec_pretty(&results).unwrap(),
                    )
                    .unwrap();
                }
            }
        }
    }
}

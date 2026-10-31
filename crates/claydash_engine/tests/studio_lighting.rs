#![cfg(not(target_arch = "wasm32"))]

use claydash_engine::{
    camera::ProjectionMode,
    model::{PrimitiveKind, RenderPipelineMode, SdfObject, SdfParams},
    renderer::HeadlessOptions,
    Camera, Renderer, World,
};
use glam::{Vec3, Vec4};

const CAPTURE_SIZE: u32 = 384;

fn architectural_study() -> Vec<SdfObject> {
    let mut scene = Vec::new();
    for (position, size) in [
        (Vec3::new(0.0, -0.12, 0.0), Vec3::new(1.6, 0.12, 1.0)),
        (Vec3::new(-0.7, 0.65, 0.0), Vec3::new(0.08, 0.65, 0.6)),
        (Vec3::new(0.2, 0.3, 0.0), Vec3::new(0.45, 0.3, 0.45)),
        (Vec3::new(0.0, 1.35, 0.0), Vec3::new(1.0, 0.05, 0.65)),
    ] {
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.color = Vec4::new(0.78, 0.76, 0.72, 1.0);
        object.material.color = object.color;
        object.material.roughness = 0.85;
        object.transform.translation = position;
        if let SdfParams::BoxParams(params) = &mut object.params {
            params.box_q = size;
            params.corner_radius = 0.015;
        }
        scene.push(object);
    }
    scene
}

#[test]
#[ignore = "requires a GPU adapter"]
fn studio_lighting_is_independent_of_scene_translation() {
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: CAPTURE_SIZE,
        height: CAPTURE_SIZE,
        ..Default::default()
    }))
    .expect("headless GPU initialization");
    for pipeline in RenderPipelineMode::ALL {
        let mut camera = Camera::new();
        camera.position = Vec3::new(3.2, 2.6, 4.5);
        camera.target = Vec3::new(0.0, 0.55, 0.0);
        camera.projection_mode = ProjectionMode::Orthographic;
        let mut scene = architectural_study();
        let world = World {
            render_pipeline: pipeline,
            bloom_enabled: false,
            ..World::default()
        };
        let mut captures = Vec::new();
        for index in 0..2 {
            let frame = renderer
                .render_offscreen(
                    &camera,
                    &scene,
                    world,
                    &[],
                    std::time::Duration::from_secs(60),
                )
                .expect("architectural study capture");
            let visible_pixels = frame
                .rgba
                .chunks_exact(4)
                .filter(|pixel| *pixel != &frame.rgba[..4])
                .count();
            assert!(
                visible_pixels > 5_000,
                "{pipeline:?}: study must be visible"
            );
            if let Ok(folder) = std::env::var("CLAYDASH_LIGHTING_CAPTURE_DIR") {
                image::save_buffer(
                    std::path::Path::new(&folder).join(format!("studio-{pipeline:?}-{index}.png")),
                    &frame.rgba,
                    frame.width,
                    frame.height,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            captures.push(frame.rgba);
            let offset = Vec3::new(8.0, 0.0, -4.0);
            for object in &mut scene {
                object.transform.translation += offset;
            }
            camera.position += offset;
            camera.target += offset;
        }
        // Permit subpixel ray-marching differences at edges after translation.
        let error: u64 = captures[0]
            .iter()
            .zip(&captures[1])
            .map(|(a, b)| a.abs_diff(*b) as u64)
            .sum();
        let mean_error = error as f64 / captures[0].len() as f64;
        eprintln!("{pipeline:?} translation mean channel error: {mean_error:.4}");
        assert!(
            mean_error < 1.0,
            "{pipeline:?}: translation changed studio lighting: {mean_error}"
        );
    }
}

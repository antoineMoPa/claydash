//! Windowless, bounded scene benchmark. Uses the renderer's existing benchmark flags.
#[cfg(not(target_arch = "wasm32"))]
use claydash_engine::{model::SdfObject, renderer::HeadlessOptions, Camera, Renderer, World};

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let path = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--benchmark-scene="))
        .expect("--benchmark-scene=PATH is required");
    let size = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--benchmark-size="))
        .unwrap_or("160x100");
    let (width, height) = size.split_once('x').expect("size must be WIDTHxHEIGHT");
    let width: u32 = width.parse().unwrap();
    let height: u32 = height.parse().unwrap();
    assert!(width > 0 && height > 0);
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let tree = &document["subtree"];
    let objects: Vec<SdfObject> =
        serde_json::from_value(tree["sdf_objects"]["value"]["VecSDFObject"].clone()).unwrap();
    let mut world: World = tree
        .get("world")
        .map(|node| serde_json::from_value(node["value"]["World"].clone()).unwrap())
        .unwrap_or_default();
    if args.iter().any(|arg| arg == "--benchmark-deferred") {
        world.render_pipeline = claydash_engine::model::RenderPipelineMode::Deferred;
    }
    let mut camera = Camera::new();
    let camera_vector = |flag: &str, fallback: glam::Vec3| {
        args.iter()
            .find_map(|arg| arg.strip_prefix(flag))
            .map_or(fallback, |value| {
                let values: Vec<f32> = value.split(',').map(|part| part.parse().unwrap()).collect();
                let values: [f32; 3] = values.try_into().expect("camera vector must be X,Y,Z");
                assert!(values.iter().all(|value| value.is_finite()));
                glam::Vec3::from_array(values)
            })
    };
    camera.position = camera_vector("--camera-position=", glam::Vec3::new(7.4, 4.2, -9.0));
    camera.target = camera_vector("--camera-target=", glam::Vec3::new(0.0, 1.3, 0.0));
    assert!(camera.position != camera.target);
    camera.viewport = glam::Vec2::new(width as f32, height as f32);
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width,
        height,
        ..Default::default()
    }))
    .expect("headless GPU initialization");
    renderer.benchmark_scene(&camera, &objects, &[], [1, 1], world, "scene");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    panic!("scene_benchmark requires a native GPU backend");
}

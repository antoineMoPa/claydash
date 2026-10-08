//! Run with `cargo run -p claydash_engine --example query_scene`.
use claydash_engine::model::{raymarch_hit, scene_sample, PrimitiveKind, SdfObject};
use glam::Vec3;

fn main() {
    let scene = vec![SdfObject::create_kind(PrimitiveKind::Sphere)];
    println!("Distance at origin: {:?}", scene_sample(Vec3::ZERO, &scene));
    println!(
        "Ray hit: {:?}",
        raymarch_hit(Vec3::new(0.0, 0.0, 5.0), -Vec3::Z, &scene)
    );
}

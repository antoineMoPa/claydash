//! Manual, CPU-only navigation profile using a real saved scene.
use super::*;
use glam::Vec3;

#[test]
#[ignore = "manual saved-scene CPU profile"]
fn profile_navigation_cpu() {
    use std::time::Instant;
    let path = std::env::var("CLAYDASH_PROFILE_SCENE")
        .unwrap_or_else(|_| "examples/astra_car.claydash".into());
    let mut app = App::new();
    app.replace_scene(crate::document::read_scene(std::path::Path::new(&path)).unwrap());
    app.camera.viewport = Vec2::new(1165.0, 1448.0);
    app.camera.position = Vec3::new(-5.0, 3.0, 7.0);
    app.camera.target = Vec3::ZERO;
    let scene = objects_ref(&app.tree);
    eprintln!(
        "Navigation CPU profile: {path}, {} objects, pid {}",
        scene.len(),
        std::process::id()
    );
    let iterations = std::env::var("CLAYDASH_PROFILE_ITERATIONS")
        .ok()
        .and_then(|count| count.parse::<usize>().ok())
        .unwrap_or(12);
    let report = |name: &str, mut samples: Vec<f64>| {
        samples.sort_by(f64::total_cmp);
        eprintln!(
            "{name}: p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms",
            samples[samples.len() / 2],
            samples[(samples.len() - 1) * 95 / 100],
            samples[samples.len() - 1]
        );
    };
    let mut revisions = Vec::new();
    let mut picks = Vec::new();
    for frame in 0..iterations {
        let start = Instant::now();
        std::hint::black_box(rendering::mesh_source_revision(scene));
        revisions.push(start.elapsed().as_secs_f64() * 1000.0);
        let pointer = app.camera.viewport * Vec2::new(0.3 + (frame % 5) as f32 * 0.1, 0.5);
        let (origin, direction) = app.camera.ray(pointer);
        let start = Instant::now();
        std::hint::black_box(crate::model::raymarch_hit(origin, direction, scene));
        picks.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    report("mesh source revision", revisions);
    report("pan anchor / pick", picks);
    let mut ui_samples = Vec::new();
    for frame in 0..iterations + 3 {
        app.camera.pan(Vec2::new(1.0, 0.25));
        let start = Instant::now();
        let mut output = app.egui.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| {
                app.ui.draw(
                    ui,
                    &mut app.tree,
                    &mut app.commands,
                    &mut app.camera,
                    &mut app.document,
                    None,
                    None,
                );
            },
        );
        std::hint::black_box(
            app.egui
                .tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point),
        );
        output.textures_delta.clear();
        if frame >= 3 {
            ui_samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    report("UI + tessellation while panning", ui_samples);
}

//! Document replacement must retire old progress, even when revisions collide.
use super::*;
use std::time::{Duration, Instant};

fn frame(renderer: &mut Renderer, context: &egui::Context, camera: &Camera,
    scene: &[SdfObject], world: World, interacting: bool, capture: bool) {
    let mut output = context.run_ui(egui::RawInput::default(), |_| {});
    renderer.render(camera, scene, &[], [7, 7], 0, world, &[], context,
        &mut output, capture, false, true, true, false, interacting, false, false);
}

fn finish(renderer: &mut Renderer, context: &egui::Context, camera: &Camera,
    scene: &[SdfObject], world: World) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(Instant::now() < deadline, "replacement document did not converge");
        frame(renderer, context, camera, scene, world, false, true);
        renderer.device.poll(wgpu::PollType::Wait { submission_index: None,
            timeout: Some(Duration::from_secs(5)) }).unwrap();
        assert!(renderer.gpu_fault().is_none(), "{:?}", renderer.gpu_fault());
        if let Some(capture) = renderer.take_capture() {
            assert!(renderer.viewport_refined());
            return capture.unwrap().rgba;
        }
        std::thread::sleep(Duration::from_millis(17));
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn second_document_same_revisions_pans_and_bloom_match_fresh_renderer() {
    let options = HeadlessOptions { width: 65, height: 49, ..Default::default() };
    let mut renderer = pollster::block_on(Renderer::new_headless(options)).unwrap();
    let context = egui::Context::default();
    let mut camera = Camera::new();
    camera.viewport = renderer.size();
    let mut neon = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    neon.material.emission_intensity = 4.0;
    neon.material.emission_color = glam::Vec3::new(0.0, 0.2, 1.0);
    let first_scene = vec![neon];
    let first_world = World { bloom_enabled: true, ..Default::default() };
    // Submit an old generation and replace its document before polling its
    // completion. This is the exact reset sequence used by App::replace_scene.
    frame(&mut renderer, &context, &camera, &first_scene, first_world, true, false);
    #[cfg(unix)]
    {
        let pending = renderer.viewport.diagnostics();
        assert_eq!(pending["submitted_batches"], 1);
        assert!(matches!(pending["scheduler_state"].as_str(), Some("gpu_pending" | "completed")),
            "first scene must own a submitted physical fence: {pending}");
    }
    renderer.reset_optimized_fields();
    renderer.invalidate_scene();
    let mut panel = SdfObject::create_kind(crate::model::PrimitiveKind::SheetPanel);
    panel.material.color = glam::Vec4::new(0.8, 0.15, 0.02, 1.0);
    let mut sphere = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    sphere.transform.translation = glam::Vec3::new(0.6, 0.4, 0.2);
    sphere.transform.scale = glam::Vec3::splat(0.4);
    let mut second_scene = vec![panel, sphere];
    let mut previous = None;
    // Keep revisions identical across documents and subsequent settings
    // invalidations, so only the explicit lifecycle reset can prevent reuse.
    for (index, (emission, bloom)) in [(0.0, true), (4.0, true), (4.0, false)].into_iter().enumerate() {
        second_scene[1].material.emission_intensity = emission;
        second_scene[1].material.emission_color = glam::Vec3::new(0.0, 0.2, 1.0);
        let world = World { bloom_enabled: bloom, ..Default::default() };
        renderer.invalidate_scene();
        for _ in 0..6 {
            // Hold the final camera fixed for the emission/bloom comparisons.
            if index == 0 { camera.pan(glam::Vec2::new(1.0, 0.5)); }
            frame(&mut renderer, &context, &camera, &second_scene, world, true, false);
            renderer.device.poll(wgpu::PollType::Wait { submission_index: None,
                timeout: Some(Duration::from_secs(5)) }).unwrap();
        }
        let actual = finish(&mut renderer, &context, &camera, &second_scene, world);
        let mut fresh = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 65, height: 49, ..Default::default()
        })).unwrap();
        let reference = fresh.render_offscreen(&camera, &second_scene, world, &[],
            Duration::from_secs(30)).unwrap();
        assert_eq!(actual, reference.rgba,
            "scene replacement/pan left stale pixels: emission={emission}, bloom={bloom}");
        if let Some(before) = previous {
            assert_ne!(actual, before, "emission/bloom changes must affect the image");
        }
        previous = Some(actual);
    }
}

#[test]
#[cfg(unix)]
#[ignore = "requires a GPU adapter"]
fn material_thumbnail_buffer_restore_preserves_native_refinement() {
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 65, height: 49, ..Default::default()
    })).unwrap();
    let context = egui::Context::default();
    let mut camera = Camera::new();
    camera.viewport = renderer.size();
    let scene = vec![SdfObject::create_kind(crate::model::PrimitiveKind::Sphere)];
    let world = World::default();
    renderer.viewport.set_initial_budget(64);
    for _ in 0..3 {
        frame(&mut renderer, &context, &camera, &scene, world, false, false);
        renderer.device.poll(wgpu::PollType::Wait { submission_index: None,
            timeout: Some(Duration::from_secs(5)) }).unwrap();
        std::thread::sleep(Duration::from_millis(17));
    }
    let before = renderer.viewport.diagnostics();
    assert!(before["completed"].as_u64().unwrap() > 0);
    assert!(!renderer.viewport_refined(), "fixture must have native work remaining");
    // Material thumbnails share scene buffers and mark them dirty before and
    // after drawing their sphere. Restoring those buffers is not a scene edit.
    renderer.uploaded_scene_versions = [i32::MIN; 2];
    renderer.upload_scene_with_exposure(&camera, &scene, &[], [0, 0], 1.5);
    renderer.uploaded_scene_versions = [i32::MIN; 2];
    renderer.upload_scene_with_world(&camera, &scene, &[], [7, 7], world);
    let after = renderer.viewport.diagnostics();
    assert_eq!(after["render_generation"], before["render_generation"],
        "restoring thumbnail buffers must not discard the viewport generation");
    assert_eq!(after["completed"], before["completed"],
        "restoring thumbnail buffers must preserve published native pixels");
    frame(&mut renderer, &context, &camera, &scene, world, false, false);
    renderer.device.poll(wgpu::PollType::Wait { submission_index: None,
        timeout: Some(Duration::from_secs(5)) }).unwrap();
    let resumed = renderer.viewport.diagnostics();
    assert_eq!(resumed["render_generation"], before["render_generation"]);
    assert!(resumed["completed"].as_u64().unwrap() > before["completed"].as_u64().unwrap());
    renderer.upload_scene_with_world(&camera, &scene, &[], [8, 7], world);
    let edited = renderer.viewport.diagnostics();
    assert!(edited["render_generation"].as_u64().unwrap()
        > resumed["render_generation"].as_u64().unwrap());
    assert_eq!(edited["completed"], 0, "real source edits must discard stale progress");
    // Explicit same-revision geometry changes (for example completed proxy
    // bakes) must still retire this generation, unlike a temporary upload.
    renderer.invalidate_scene();
    assert!(renderer.viewport.diagnostics()["render_generation"].as_u64().unwrap()
        > edited["render_generation"].as_u64().unwrap());
}

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
#[cfg(unix)]
#[ignore = "requires a GPU adapter"]
fn gaussian_only_view_uses_one_native_raster_batch() {
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 97, height: 73, ..Default::default()
    })).unwrap();
    let context = egui::Context::default();
    let mut camera = Camera::new();
    camera.viewport = renderer.size();
    camera.position = glam::Vec3::new(0.0, 0.0, 4.0);
    camera.target = glam::Vec3::ZERO;
    let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    object.imported_splats = Some(std::sync::Arc::new(crate::gaussian_ply::GaussianCloud {
        splats: vec![crate::gaussian_ply::Gaussian {
            position: glam::Vec3::ZERO, scale: glam::Vec3::splat(0.35),
            rotation: glam::Quat::IDENTITY, color: [1.0, 0.0, 0.0],
            opacity: 0.9, sh_rest: Vec::new(),
        }],
    }));
    let scene = [object];
    frame(&mut renderer, &context, &camera, &scene, World::default(), false, false);
    let diagnostics = renderer.viewport.diagnostics();
    assert_eq!(renderer.uploaded_object_count, 0);
    assert_eq!(diagnostics["preview_size"], serde_json::json!([97, 73]));
    assert_eq!(diagnostics["completed"], 0);
    assert_eq!(diagnostics["submitted_batches"], 1);
    let image = finish(&mut renderer, &context, &camera, &scene, World::default());
    assert!(image.chunks_exact(4).any(|pixel| pixel[0] > pixel[1].saturating_add(30)));
    assert_eq!(renderer.viewport.diagnostics()["submitted_batches"], 1,
        "static raster scene should not schedule SDF refinement");
}

#[test]
#[cfg(unix)]
#[ignore = "requires a GPU adapter"]
fn gaussian_and_sdf_refinement_converges_with_visible_splats() {
    let options = HeadlessOptions { width: 97, height: 73, ..Default::default() };
    let mut renderer = pollster::block_on(Renderer::new_headless(options)).unwrap();
    let context = egui::Context::default();
    let mut camera = Camera::new();
    camera.viewport = renderer.size();
    camera.position = glam::Vec3::new(0.0, 0.0, 4.0);
    camera.target = glam::Vec3::ZERO;
    let mut cloud = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    cloud.imported_splats = Some(std::sync::Arc::new(crate::gaussian_ply::GaussianCloud {
        splats: vec![crate::gaussian_ply::Gaussian {
            position: glam::Vec3::new(0.4, 0.0, 0.0), scale: glam::Vec3::splat(0.45),
            rotation: glam::Quat::IDENTITY, color: [1.0, 0.0, 0.0],
            opacity: 0.8, sh_rest: Vec::new(),
        }],
    }));
    let sphere = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    let scene = [sphere, cloud];
    renderer.upload_scene_with_world(&camera, &scene, &[], [7, 7], World::default());
    renderer.viewport.set_initial_budget(64);
    frame(&mut renderer, &context, &camera, &scene, World::default(), false, false);
    assert_eq!(renderer.uploaded_object_count, 1);
    assert!(renderer.viewport.diagnostics()["preview_size"][0].as_u64().unwrap() < 97);
    let actual = finish(&mut renderer, &context, &camera, &scene, World::default());
    let red_pixels = actual.chunks_exact(4)
        .filter(|pixel| pixel[0] > pixel[1].saturating_add(20)
            && pixel[0] > pixel[2].saturating_add(20)).count();
    assert!(red_pixels > 100, "Gaussian disappeared from mixed scene: {red_pixels} red pixels");
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

#[test]
#[ignore = "requires a GPU adapter"]
fn imported_gaussian_ply_renders_without_proxy_and_follows_transform() {
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 65, height: 49, ..Default::default()
    })).unwrap();
    let mut camera = Camera::new();
    camera.viewport = renderer.size();
    camera.position = Vec3::new(0., 0., 4.);
    camera.target = Vec3::ZERO;
    let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    object.imported_splats = Some(std::sync::Arc::new(crate::gaussian_ply::GaussianCloud {
        splats: vec![crate::gaussian_ply::Gaussian { position: Vec3::ZERO,
            scale: Vec3::new(0.5, 0.1, 0.1), rotation: glam::Quat::IDENTITY,
            color: [1., 0., 0.], opacity: 0.9, sh_rest: vec![] }],
    }));
    let world = World::default();
    let blank = renderer.render_offscreen(&camera, &[], world, &[], Duration::from_secs(30)).unwrap();
    let rendered = renderer.render_offscreen(&camera, &[object.clone()], world, &[], Duration::from_secs(30)).unwrap();
    assert!(renderer.hybrid_enabled);
    assert_eq!(renderer.uploaded_object_count, 0, "asset must not render a proxy box");
    assert_eq!(renderer.splat_instances.len(), 1);
    assert_ne!(blank.rgba, rendered.rgba, "Gaussian must be visible");
    let red_pixels = rendered.rgba.chunks_exact(4).filter(|p| p[0] > p[1].saturating_add(30) && p[0] > p[2].saturating_add(30)).count();
    assert!(red_pixels > 10, "expected red Gaussian, got {red_pixels} red pixels");
    object.transform.translation.x = 1.;
    let moved = renderer.render_offscreen(&camera, &[object], world, &[], Duration::from_secs(30)).unwrap();
    assert_ne!(rendered.rgba, moved.rgba);
    let mut source = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    source.transform.translation.x = -0.7;
    source.material.emission_intensity = 2.;
    source.material.emission_color = Vec3::new(0., 0., 1.);
    let world = World { bloom_enabled: true, ..world };
    let mut asset = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    asset.imported_splats = Some(std::sync::Arc::new(crate::gaussian_ply::GaussianCloud {
        splats: vec![crate::gaussian_ply::Gaussian { position: Vec3::new(0.5,0.,0.),
            scale: Vec3::new(0.1,0.4,0.1), rotation: glam::Quat::from_rotation_z(0.7),
            color: [1.,0.,0.], opacity: 0.9, sh_rest: vec![] }],
    }));
    let mixed = renderer.render_offscreen(&camera, &[source, asset], world, &[], Duration::from_secs(30)).unwrap();
    assert_ne!(mixed.rgba, blank.rgba);
    assert!(renderer.gpu_fault().is_none());
}

#[test]
#[ignore = "requires a GPU adapter"]
fn imported_mesh_uses_live_material_instead_of_placeholder_box() {
    use crate::model::GroupRenderRepresentation;
    use crate::renderer::poisson_mesh::geometry::Mesh;
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 65, height: 65, ..Default::default()
    })).unwrap();
    let mut camera = Camera::new();
    camera.position = Vec3::new(0., 0., 3.);
    camera.target = Vec3::ZERO;
    let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    object.render_representation = GroupRenderRepresentation::DirectMesh;
    object.imported_mesh = Some(std::sync::Arc::new(Mesh::from_parts(
        vec![Vec3::new(-0.7, -0.7, 0.), Vec3::new(0.7, -0.7, 0.),
            Vec3::new(0.7, 0.7, 0.), Vec3::new(-0.7, 0.7, 0.)],
        vec![[0, 1, 2], [0, 2, 3]], vec![object.uuid; 2])));
    let world = World::default();
    let capture = |renderer: &mut Renderer, object: &SdfObject| renderer.render_offscreen(
        &camera, std::slice::from_ref(object), world, &[], Duration::from_secs(30)
    ).unwrap();
    object.color = glam::Vec4::new(0.9, 0.05, 0.05, 1.);
    object.material.color = object.color;
    let red = capture(&mut renderer, &object);
    object.color = glam::Vec4::new(0.05, 0.05, 0.9, 1.);
    object.material.color = object.color;
    let blue = capture(&mut renderer, &object);
    let pixel = (32 * 65 + 32) * 4;
    assert!(red.rgba[pixel] > red.rgba[pixel + 2], "mesh must show its red material");
    assert!(blue.rgba[pixel + 2] > blue.rgba[pixel], "mesh must update to blue material");
}

#[test]
#[ignore = "requires a GPU adapter"]
fn offscreen_capture_uses_requested_extent_without_resizing_renderer() {
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 97, height: 73, ..Default::default()
    })).unwrap();
    let original_size = renderer.size();
    let context = egui::Context::default();
    let mut camera = Camera::new();
    for [width, height] in [[384,256],[97,73]] {
        camera.viewport = glam::Vec2::new(width as f32, height as f32);
        let deadline = Instant::now() + Duration::from_secs(30);
        let image = loop {
            assert!(Instant::now() < deadline, "offscreen extent capture timed out");
            frame(&mut renderer, &context, &camera, &[], World::default(), false, true);
            renderer.device.poll(wgpu::PollType::Wait { submission_index:None,
                timeout:Some(Duration::from_secs(5)) }).unwrap();
            if let Some(image) = renderer.take_capture() { break image.unwrap(); }
        };
        assert_eq!([image.width,image.height],[width,height]);
        assert_eq!(image.rgba.len(),(width * height * 4) as usize);
        assert_eq!(renderer.size(),original_size);
    }
}

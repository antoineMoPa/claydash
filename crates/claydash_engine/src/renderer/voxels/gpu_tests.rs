use super::*;
use crate::model::{BooleanOperation, GroupRenderRepresentation, PrimitiveKind, SdfParams, SphereParams};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires a GPU adapter and native readback"]
fn coarse_boolean_cubes_render_and_respect_exact_foreground_depth() {
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 192, height: 192, ..Default::default()
    })).expect("headless voxel GPU renderer");
    let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
    root.params = SdfParams::SphereParams(SphereParams { radius: 1.0 });
    root.color = glam::Vec4::new(0.95, 0.35, 0.08, 1.0);
    let mut cut = SdfObject::create_kind(PrimitiveKind::Sphere);
    cut.boolean_parent = Some(root.uuid);
    cut.operation = BooleanOperation::Subtract;
    cut.params = SdfParams::SphereParams(SphereParams { radius: 0.65 });
    cut.transform.translation = Vec3::new(0.65, 0.35, 0.5);
    let mut foreground = SdfObject::create_kind(PrimitiveKind::Sphere);
    foreground.params = SdfParams::SphereParams(SphereParams { radius: 0.3 });
    foreground.transform.translation = Vec3::new(-0.2, -0.15, 2.0);
    foreground.color = glam::Vec4::new(0.03, 0.15, 0.95, 1.0);
    let mut camera = Camera::new();
    camera.position = Vec3::new(0.0, 0.0, 6.0);
    camera.target = Vec3::ZERO;
    let world = World { screen_space_ambient_occlusion: false, ..World::default() };
    let capture = |renderer: &mut Renderer, source: &[SdfObject]| {
        renderer.render_offscreen(&camera, source, world, &[], Duration::from_secs(60))
            .expect("voxel readback")
    };
    let foreground_only = capture(&mut renderer, &[foreground.clone()]);
    let exact = capture(&mut renderer, &[root.clone(), cut.clone(), foreground.clone()]);
    root.render_representation = GroupRenderRepresentation::Voxels;
    root.voxels.resolution = 12;
    let source = vec![root.clone(), cut, foreground];
    let context = egui::Context::default();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        renderer.advance_computations(&camera, &source, &[], [1, 1], 1, world, &context, true);
        if renderer.voxels.ready.contains_key(&root.uuid) { break; }
        assert!(Instant::now() < deadline, "voxel build timeout");
        assert!(renderer.voxels.failed.is_empty(), "voxel bake failed");
        std::thread::sleep(Duration::from_millis(5));
    }
    let ready = &renderer.voxels.ready[&root.uuid];
    assert!(ready.geometry.cubes > 20);
    assert!(ready.geometry.mesh.normals.iter().all(|normals|
        normals.iter().all(|normal| normal.abs().element_sum() == 1.0)));
    let voxel = capture(&mut renderer, &source);
    image::save_buffer("/tmp/claydash-voxels-review.png", &voxel.rgba,
        voxel.width, voxel.height, image::ColorType::Rgba8).expect("review PNG");
    let changed = exact.rgba.chunks_exact(4).zip(voxel.rgba.chunks_exact(4))
        .filter(|(a, b)| a.iter().zip(b.iter()).take(3)
            .any(|(left, right)| left.abs_diff(*right) > 8)).count();
    assert!(changed > 200, "coarse cube faces must visibly differ from exact curved SDF: {changed}");
    // A center patch of the blue foreground sphere must retain its blue material.
    // Exact and mixed raster frames use different lighting; orange cube faces
    // overwriting these pixels would remove the blue dominance.
    let mut compared = 0;
    for y in 102..109 {
        for x in 81..88 {
            let offset = (y * 192 + x) * 4;
            let a = &foreground_only.rgba[offset..offset + 3];
            if a[2] > a[0].saturating_add(30) {
                let b = &voxel.rgba[offset..offset + 3];
                assert!(b[2] > b[0].saturating_add(30) && b[2] > b[1].saturating_add(30),
                    "foreground sphere was occluded by voxel mesh at {x},{y}: {a:?} vs {b:?}");
                compared += 1;
            }
        }
    }
    assert!(compared > 8, "foreground test patch must contain blue sphere pixels");
}

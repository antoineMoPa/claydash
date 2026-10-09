#![cfg(not(target_arch = "wasm32"))]
use claydash_engine::{
    model::{PrimitiveKind, SdfObject},
    renderer::HeadlessOptions,
    Camera, Renderer, World,
};

#[test]
#[ignore = "requires a GPU adapter; runs without a window or display server"]
fn renders_refined_scene_and_reuses_renderer() {
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 65,
        height: 49,
        ..Default::default()
    }))
    .expect("headless GPU initialization");
    let scene = vec![SdfObject::create_kind(PrimitiveKind::Sphere)];
    let capture = |renderer: &mut Renderer, objects: &[SdfObject]| {
        renderer
            .render_offscreen(
                &Camera::new(),
                objects,
                World::default(),
                &[],
                std::time::Duration::from_secs(30),
            )
            .expect("refined readback")
    };
    let first = capture(&mut renderer, &scene);
    assert_eq!((first.width, first.height), (65, 49));
    assert_eq!(first.rgba.len(), 65 * 49 * 4);
    assert!(first.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
    assert!(first
        .rgba
        .chunks_exact(4)
        .any(|pixel| pixel != &first.rgba[..4]));
    let empty = capture(&mut renderer, &[]);
    assert_ne!(first.rgba, empty.rgba);
    let repeat = capture(&mut renderer, &scene);
    assert_eq!(first.rgba, repeat.rgba);
    assert!(renderer
        .render_offscreen(
            &Camera::new(),
            &scene,
            World::default(),
            &[],
            std::time::Duration::ZERO
        )
        .err()
        .unwrap()
        .contains("timed out"));
    assert!(!renderer.capture_pending());
    assert_eq!(first.rgba, capture(&mut renderer, &scene).rgba);
    renderer.resize(winit::dpi::PhysicalSize::new(97, 61));
    let resized = capture(&mut renderer, &scene);
    assert_eq!(
        (resized.width, resized.height, resized.rgba.len()),
        (97, 61, 97 * 61 * 4)
    );
}

#[test]
#[ignore = "requires a GPU adapter; runs without a window or display server"]
fn radial_face_cut_matches_explicit_rotated_cut_groups() {
    use claydash_engine::model::{BooleanOperation, RepetitionAxis, RepetitionMode, SdfParams};
    use glam::{Quat, Vec3};
    let mut spoke = SdfObject::create_kind(PrimitiveKind::Box);
    if let SdfParams::BoxParams(params) = &mut spoke.params {
        params.box_q = Vec3::new(0.65, 0.15, 0.15);
        params.corner_radius = 0.0;
    }
    let pivot = Vec3::new(-0.8, 0.0, 0.0);
    spoke.repetition.enabled = true;
    spoke.repetition.mode = RepetitionMode::Radial {
        axis: RepetitionAxis::Z,
        count: 6,
        pivot,
    };
    let mut cut = SdfObject::create_kind(PrimitiveKind::Box);
    if let SdfParams::BoxParams(params) = &mut cut.params {
        params.box_q = Vec3::new(0.08, 0.08, 0.2);
        params.corner_radius = 0.0;
    }
    cut.boolean_parent = Some(spoke.uuid);
    cut.operation = BooleanOperation::Subtract;
    let radial = vec![spoke.clone(), cut.clone()];
    let mut explicit = Vec::new();
    for copy in 0..6 {
        let rotation = Quat::from_rotation_z(std::f32::consts::TAU * copy as f32 / 6.0);
        let translation = pivot - rotation * pivot;
        let mut source = spoke.duplicate();
        source.repetition.enabled = false;
        source.transform.rotation = rotation;
        source.transform.translation = translation;
        let mut operand = cut.duplicate();
        operand.boolean_parent = Some(source.uuid);
        operand.transform.rotation = rotation;
        operand.transform.translation = translation;
        explicit.extend([source, operand]);
    }
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 129,
        height: 129,
        ..Default::default()
    }))
    .expect("headless GPU initialization");
    let mut camera = Camera::new();
    camera.target = pivot;
    camera.position = pivot + Vec3::new(0.0, 0.0, 4.0);
    camera.up = Vec3::Y;
    camera.projection_mode = claydash_engine::camera::ProjectionMode::Orthographic;
    let render = |renderer: &mut Renderer, scene: &[SdfObject]| {
        renderer
            .render_offscreen(
                &camera,
                scene,
                World::default(),
                &[],
                std::time::Duration::from_secs(60),
            )
            .expect("radial cut readback")
            .rgba
    };
    let actual = render(&mut renderer, &radial);
    let expected = render(&mut renderer, &explicit);
    image::save_buffer_with_format("/tmp/claydash-radial-facecut-review.png", &actual, 129, 129,
        image::ColorType::Rgba8, image::ImageFormat::Png).unwrap();
    image::save_buffer_with_format("/tmp/claydash-radial-facecut-expected.png", &expected, 129, 129,
        image::ColorType::Rgba8, image::ImageFormat::Png).unwrap();
    // Floating-point transform order can move a few antialiased edge pixels.
    let error: u64 = actual
        .iter()
        .zip(&expected)
        .map(|(a, b)| a.abs_diff(*b) as u64)
        .sum();
    assert!(
        error < actual.len() as u64 * 2,
        "mean pixel error: {}",
        error as f64 / actual.len() as f64
    );
    let uncut = render(&mut renderer, &[spoke]);
    assert_ne!(
        actual, uncut,
        "the repeated cuts must change the rendered spokes"
    );
}

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

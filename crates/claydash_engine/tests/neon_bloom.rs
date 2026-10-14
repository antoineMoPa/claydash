#![cfg(not(target_arch = "wasm32"))]
use claydash_engine::{
    model::{BackgroundMode, PrimitiveKind, SdfObject},
    renderer::HeadlessOptions,
    Camera, Renderer, World,
};
use glam::{Vec3, Vec4};

#[test]
#[ignore = "requires a GPU adapter"]
fn selective_neon_bloom_extends_silhouette_and_respects_occlusion() {
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 192,
        height: 192,
        ..Default::default()
    }))
    .unwrap();
    let mut camera = Camera::new();
    camera.position = Vec3::new(0.0, 0.0, 4.0);
    camera.target = Vec3::ZERO;
    let mut neon = SdfObject::create_kind(PrimitiveKind::Sphere);
    neon.material.color = Vec4::new(0.04, 0.0, 0.02, 1.0);
    neon.color = neon.material.color;
    neon.material.emission_color = Vec3::new(1.0, 0.015, 0.3);
    neon.material.emission_intensity = 4.0;
    let world = World {
        background: BackgroundMode::Transparent,
        ..Default::default()
    };
    let capture = |renderer: &mut Renderer, scene: &[SdfObject], world| {
        renderer
            .render_offscreen(
                &camera,
                scene,
                world,
                &[],
                std::time::Duration::from_secs(60),
            )
            .unwrap()
            .rgba
    };
    let no_bloom = capture(
        &mut renderer,
        &[neon.clone()],
        World {
            bloom_enabled: false,
            ..world
        },
    );
    let bloom = capture(&mut renderer, &[neon.clone()], world);
    let halo = bloom
        .chunks_exact(4)
        .zip(no_bloom.chunks_exact(4))
        .filter(|(a, b)| b[3] == 0 && a[3] > 0 && a[0] > 0)
        .count();
    assert!(
        halo > 300,
        "bloom must extend beyond the surface and export with alpha: {halo}"
    );
    let center = (96 * 192 + 96) * 4;
    assert!(
        no_bloom[center] > 200,
        "emission must remain bright with bloom off"
    );
    assert!(
        bloom[center + 1] < bloom[center],
        "neon must retain its emission color"
    );
    let deferred = capture(
        &mut renderer,
        &[neon.clone()],
        World {
            render_pipeline: claydash_engine::model::RenderPipelineMode::Deferred,
            ..world
        },
    );
    let deferred_off = capture(
        &mut renderer,
        &[neon.clone()],
        World {
            render_pipeline: claydash_engine::model::RenderPipelineMode::Deferred,
            bloom_enabled: false,
            ..world
        },
    );
    let deferred_halo = deferred
        .chunks_exact(4)
        .zip(deferred_off.chunks_exact(4))
        .filter(|(a, b)| b[3] == 0 && a[3] > 0 && a[0] > 0)
        .count();
    assert!(
        deferred_halo > 300,
        "deferred bloom must extend the silhouette: {deferred_halo}"
    );
    let tight = capture(
        &mut renderer,
        &[neon.clone()],
        World {
            bloom_radius: 4.0,
            ..world
        },
    );
    let visible = |pixels: &[u8]| pixels.chunks_exact(4).filter(|p| p[3] > 0).count();
    assert!(
        visible(&tight) < visible(&bloom),
        "radius must control the visible halo"
    );
    let black = World {
        background: BackgroundMode::Flat,
        flat_color: [0.005, 0.005, 0.005],
        ..world
    };
    let black_on = capture(&mut renderer, &[neon.clone()], black);
    let black_off = capture(
        &mut renderer,
        &[neon.clone()],
        World {
            bloom_enabled: false,
            ..black
        },
    );
    let mut plain = neon.clone();
    plain.material.emission_intensity = 0.0;
    plain.material.color = Vec4::ONE;
    plain.color = Vec4::ONE;
    let plain_on = capture(&mut renderer, &[plain.clone()], world);
    let plain_off = capture(
        &mut renderer,
        &[plain.clone()],
        World {
            bloom_enabled: false,
            ..world
        },
    );
    assert_eq!(
        plain_on, plain_off,
        "bright non-emissive materials must not bloom or retain stale halos"
    );
    let mut blocker = SdfObject::create_kind(PrimitiveKind::Box);
    blocker.transform.translation = Vec3::new(0.0, 0.0, 1.5);
    blocker.transform.scale = Vec3::splat(2.0);
    let blocked = capture(&mut renderer, &[neon, blocker.clone()], world);
    let blocker_only = capture(&mut renderer, &[blocker], world);
    assert_eq!(
        blocked, blocker_only,
        "fully hidden neon must not emit a halo through the occluder"
    );
    if let Ok(folder) = std::env::var("CLAYDASH_BLOOM_CAPTURE_DIR") {
        for (name, pixels) in [
            ("neon-off", &no_bloom),
            ("neon-on", &bloom),
            ("plain", &plain_on),
            ("occluded", &blocked),
            ("neon-black-on", &black_on),
            ("neon-black-off", &black_off),
            ("neon-deferred", &deferred),
        ] {
            let file =
                std::fs::File::create(std::path::Path::new(&folder).join(format!("{name}.png")))
                    .unwrap();
            let mut encoder = png::Encoder::new(file, 192, 192);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(pixels)
                .unwrap();
        }
    }
}

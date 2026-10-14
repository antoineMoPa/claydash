#![cfg(not(target_arch = "wasm32"))]
use claydash_engine::{
    model::{PrimitiveKind, SdfObject},
    renderer::HeadlessOptions,
    Camera, Renderer, World,
};

#[test]
#[ignore = "requires a GPU adapter"]
fn compound_sheet_surface_and_variable_path_render_distinct_geometry() {
    use claydash_engine::model::{SdfParams,SheetPanelParams,SheetSurface,SheetPathEmboss,SheetPathStation,SheetPathProfile,SheetCut};
    use glam::{Vec2,Vec3};
    let mut renderer=pollster::block_on(Renderer::new_headless(HeadlessOptions {width:256,height:256,..Default::default()})).unwrap();
    let mut camera=Camera::new(); camera.position=Vec3::new(0.8,0.7,1.5);camera.target=Vec3::new(0.0,0.0,0.06);
    let mut panel=SheetPanelParams {
        size:Vec2::new(0.9,0.7),thickness:0.008,
        surface:Some(SheetSurface {heights:[0.0,0.04,0.04,0.0,0.04,0.12,0.12,0.04,0.04,0.12,0.12,0.04,0.0,0.04,0.04,0.0]}),
        cuts:vec![SheetCut::Circle {center:Vec2::new(0.23,-0.2),radius:0.04}],
        path_embosses:vec![SheetPathEmboss {stations:vec![
            SheetPathStation {point:Vec2::new(-0.35,0.0),width:0.04,depth:0.01,profile:SheetPathProfile::Rounded},
            SheetPathStation {point:Vec2::new(0.0,0.08),width:0.075,depth:0.025,profile:SheetPathProfile::Crease},
            SheetPathStation {point:Vec2::new(0.35,0.0),width:0.04,depth:0.01,profile:SheetPathProfile::Rounded},
        ],taper_start:0.12,taper_end:0.12}],..Default::default()
    };
    assert!(panel.validate().is_ok());
    let render=|renderer:&mut Renderer,panel:SheetPanelParams| {
        let mut object=SdfObject::create_kind(PrimitiveKind::SheetPanel);object.params=SdfParams::SheetPanelParams(panel);
        renderer.render_offscreen(&camera,&[object],World::default(),&[],std::time::Duration::from_secs(30)).unwrap().rgba
    };
    let compound=render(&mut renderer,panel.clone());
    panel.path_embosses.clear(); let crown=render(&mut renderer,panel.clone());
    panel.surface=None;let flat=render(&mut renderer,panel);
    let changed=|a:&[u8],b:&[u8]| a.chunks_exact(4).zip(b.chunks_exact(4)).filter(|(a,b)|a!=b).count();
    assert!(changed(&compound,&crown)>100,"variable tapered path must change rendered surface");
    assert!(changed(&crown,&flat)>1000,"compound crown must change rendered surface");
}

#[test]
#[ignore = "requires a GPU adapter"]
fn circular_sheet_emboss_renders_an_integrated_arch_lip_and_recess() {
    use claydash_engine::model::{SdfParams, SheetCut, SheetEmboss, SheetPanelParams, SheetStage};
    use glam::{Vec2, Vec3};
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 384, height: 384, ..Default::default()
    })).expect("headless GPU initialization");
    let mut camera = Camera::new();
    camera.position = Vec3::new(1.15, 0.75, 1.5);
    camera.target = Vec3::new(0.0, -0.1, 0.02);
    let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    object.color = glam::Vec4::new(0.2, 0.45, 0.7, 1.0);
    object.material.color = object.color;
    let mut panel = SheetPanelParams {
        size: Vec2::new(0.9, 0.7), thickness: 0.008,
        cuts: vec![SheetCut::Circle { center: Vec2::new(0.0, -0.32), radius: 0.28 }],
        embosses: vec![SheetEmboss::Circle { center: Vec2::new(0.0, -0.32), radius: 0.28, depth: 0.035, transition: 0.075 }],
        ..Default::default()
    };
    assert!(panel.validate().is_ok());
    let render = |renderer: &mut Renderer, scene: &[SdfObject]| renderer.render_offscreen(
        &camera, scene, World::default(), &[], std::time::Duration::from_secs(30)
    ).expect("emboss sheet capture").rgba;
    let empty = render(&mut renderer, &[]);
    let mut images = Vec::new();
    for stage in [SheetStage::Blank, SheetStage::Cut, SheetStage::Formed] {
        panel.stage = stage;
        object.params = SdfParams::SheetPanelParams(panel.clone());
        let pixels = render(&mut renderer, std::slice::from_ref(&object));
        let visible = pixels.chunks_exact(4).zip(empty.chunks_exact(4)).filter(|(a, b)| a != b).count();
        assert!(visible > 1000, "{stage:?}: missing sheet geometry");
        if let Ok(folder) = std::env::var("CLAYDASH_EMBOSS_CAPTURE_DIR") {
            let file = std::fs::File::create(std::path::Path::new(&folder).join(format!("emboss-{stage:?}.png"))).unwrap();
            let mut encoder = png::Encoder::new(file, 384, 384);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
        }
        images.push(pixels);
    }
    assert_ne!(images[0], images[1], "circular cut must open the arch");
    assert_ne!(images[1], images[2], "emboss must form a visible integrated lip");
    let SheetEmboss::Circle { depth, .. } = &mut panel.embosses[0];
    *depth = -0.035;
    object.params = SdfParams::SheetPanelParams(panel);
    assert_ne!(images[2], render(&mut renderer, std::slice::from_ref(&object)), "signed depth must render a recess");
}

#[test]
#[ignore = "requires a GPU adapter"]
fn perpendicular_sheet_bends_render_cuts_stages_and_recipe_order() {
    use claydash_engine::model::{SdfParams, SheetBend, SheetCut, SheetPanelParams, SheetStage};
    use glam::{Vec2, Vec3};
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 256, height: 256, ..Default::default()
    })).expect("headless GPU initialization");
    let mut camera = Camera::new();
    camera.position = Vec3::new(1.7, -1.4, 2.5);
    camera.target = Vec3::ZERO;
    let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    let mut panel = SheetPanelParams::default();
    panel.bends = vec![
        SheetBend { start: 0.15, direction: 0.0, radius: 0.22, angle: 1.2 },
        SheetBend { start: -0.1, direction: std::f32::consts::FRAC_PI_2, radius: 0.2, angle: -1.1 },
    ];
    panel.cuts.push(SheetCut::Circle { center: Vec2::new(-0.55, -0.25), radius: 0.15 });
    assert!(panel.validate().is_ok());
    let render = |renderer: &mut Renderer, scene: &[SdfObject]| renderer.render_offscreen(
        &camera, scene, World::default(), &[], std::time::Duration::from_secs(30)
    ).expect("directional sheet capture").rgba;
    let empty = render(&mut renderer, &[]);
    let mut images = Vec::new();
    for stage in [SheetStage::Blank, SheetStage::Cut, SheetStage::Formed] {
        panel.stage = stage;
        object.params = SdfParams::SheetPanelParams(panel.clone());
        let pixels = render(&mut renderer, std::slice::from_ref(&object));
        let visible = pixels.chunks_exact(4).zip(empty.chunks_exact(4)).filter(|(a, b)| a != b).count();
        assert!(visible > 500, "{stage:?} must render visible geometry ({visible} pixels)");
        images.push(pixels);
    }
    assert_ne!(images[0], images[1], "material cuts must render");
    assert_ne!(images[1], images[2], "two perpendicular bends must render");
    if let Ok(path) = std::env::var("CLAYDASH_DIRECTIONAL_SHEET_CAPTURE") {
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(file, 256, 256);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&images[2]).unwrap();
    }
    panel.bends.reverse();
    assert!(panel.validate().is_ok());
    object.params = SdfParams::SheetPanelParams(panel);
    assert_ne!(images[2], render(&mut renderer, std::slice::from_ref(&object)));
}

#[test]
#[ignore = "requires a GPU adapter"]
fn sheet_manufacturing_stages_render_distinct_geometry() {
    use claydash_engine::model::{SdfParams, SheetBend, SheetCut, SheetPanelParams, SheetStage};
    use glam::{Vec2, Vec3};
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 192, height: 192, ..Default::default()
    })).expect("headless GPU initialization");
    let mut camera = Camera::new();
    camera.position = Vec3::new(1.0, 0.8, 3.0);
    camera.target = Vec3::ZERO;
    let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    let mut panel = SheetPanelParams::default();
    panel.cuts.push(SheetCut::Circle { center: Vec2::new(0.0, -0.4), radius: 0.45 });
    panel.bends.push(SheetBend { direction: 0.0, start: -0.8, radius: 0.4, angle: 1.5 });
    let render = |renderer: &mut Renderer, scene: &[SdfObject]| renderer.render_offscreen(
        &camera, scene, World::default(), &[], std::time::Duration::from_secs(30)
    ).expect("sheet capture").rgba;
    let empty = render(&mut renderer, &[]);
    let mut images = Vec::new();
    for stage in [SheetStage::Blank, SheetStage::Cut, SheetStage::Formed] {
        panel.stage = stage;
        object.params = SdfParams::SheetPanelParams(panel.clone());
        let pixels = render(&mut renderer, std::slice::from_ref(&object));
        let changed = pixels.chunks_exact(4).zip(empty.chunks_exact(4)).filter(|(a, b)| a != b).count();
        assert!(changed > 200, "{stage:?} must render visible sheet geometry, got {changed} pixels");
        images.push(pixels);
    }
    assert_ne!(images[0], images[1], "cuts must change the blank");
    assert_ne!(images[1], images[2], "bends must change the cut pattern");
}

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

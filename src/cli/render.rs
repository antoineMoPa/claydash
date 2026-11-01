//! Standalone scene-file rendering, independent of the editor and agent transport.
use claydash_engine::{
    camera::{Camera, ProjectionMode},
    model::SdfObject,
    renderer::{HeadlessOptions, Renderer},
    scene_language, World,
};
use glam::{Vec2, Vec3};
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderView {
    #[default]
    Isometric,
    Top,
    Left,
    Right,
}

impl RenderView {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "isometric" => Ok(Self::Isometric),
            "top" => Ok(Self::Top),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            _ => Err(format!(
                "unknown render view '{value}'; expected isometric, top, left, or right"
            )),
        }
    }

    fn camera_axes(self) -> (Vec3, Vec3) {
        match self {
            Self::Isometric => (Vec3::splat(1.0 / 3.0_f32.sqrt()), Vec3::Y),
            Self::Top => (Vec3::Y, Vec3::NEG_Z),
            Self::Left => (Vec3::NEG_X, Vec3::Y),
            Self::Right => (Vec3::X, Vec3::Y),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RenderArgs {
    pub source: PathBuf,
    pub output: PathBuf,
    pub size: [u32; 2],
    pub view: RenderView,
}

impl RenderArgs {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut source = None;
        let mut output = None;
        let mut size = [1024, 768];
        let mut size_given = false;
        let mut view = None;
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--output" | "-o" => {
                    if output.is_some() {
                        return Err("--output may only be supplied once".into());
                    }
                    output = Some(PathBuf::from(
                        args.next().ok_or("--output requires a PNG path")?,
                    ));
                }
                "--view" => {
                    if view.is_some() {
                        return Err("--view may only be supplied once".into());
                    }
                    view = Some(RenderView::parse(
                        args.next()
                            .ok_or("--view requires isometric, top, left, or right")?,
                    )?);
                }
                "--size" => {
                    if size_given {
                        return Err("--size may only be supplied once".into());
                    }
                    let value = args.next().ok_or("--size requires WIDTHxHEIGHT")?;
                    let (width, height) = value
                        .split_once('x')
                        .ok_or("--size requires WIDTHxHEIGHT")?;
                    size = [
                        width.parse().map_err(|_| "invalid render width")?,
                        height.parse().map_err(|_| "invalid render height")?,
                    ];
                    if size.iter().any(|n| !(1..=4096).contains(n)) {
                        return Err("render dimensions must be 1–4096 pixels".into());
                    }
                    size_given = true;
                }
                _ if arg.starts_with('-') => return Err(format!("unknown render option: {arg}")),
                _ => {
                    if source.is_some() {
                        return Err("render requires exactly one scene file".into());
                    }
                    source = Some(PathBuf::from(arg));
                }
            }
        }
        let source = source.ok_or("render requires a scene file: claydash render scene.claydef")?;
        let output = output.unwrap_or_else(|| source.with_extension("png"));
        if output.extension().and_then(|s| s.to_str()) != Some("png") {
            return Err("render output must have a .png extension".into());
        }
        if output == source {
            return Err("render output must differ from the source file".into());
        }
        Ok(Self {
            source,
            output,
            size,
            view: view.unwrap_or_default(),
        })
    }

    fn scene(&self) -> Result<(Vec<SdfObject>, Camera), String> {
        let source = std::fs::read_to_string(&self.source)
            .map_err(|e| format!("read {}: {e}", self.source.display()))?;
        let objects = scene_language::compile(&source)
            .map_err(|e| format!("{}:{e}", self.source.display()))?
            .into_iter()
            .map(|(_, object)| object)
            .collect::<Vec<_>>();
        let camera = fitted_camera(&objects, self.size, self.view)?;
        Ok((objects, camera))
    }

    pub fn run(self) -> Result<(), String> {
        // Read, compile and frame before requesting a GPU or writing the output.
        let (objects, camera) = self.scene()?;
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: self.size[0],
            height: self.size[1],
            ..Default::default()
        }))?;
        let frame = renderer.render_offscreen(
            &camera,
            &objects,
            World::default(),
            &[],
            Duration::from_secs(60),
        )?;
        let png = crate::render_export::png_bytes(&frame)?;
        std::fs::write(&self.output, png)
            .map_err(|e| format!("write {}: {e}", self.output.display()))?;
        println!(
            "Rendered {} to {} ({} × {})",
            self.source.display(),
            self.output.display(),
            frame.width,
            frame.height
        );
        Ok(())
    }
}

fn fitted_camera(
    objects: &[SdfObject],
    size: [u32; 2],
    view: RenderView,
) -> Result<Camera, String> {
    let (min, max) = scene_language::bounds(objects)?;
    let diameter = (max - min).length();
    let span = diameter.max(0.1) * 1.15;
    let aspect = size[0] as f32 / size[1] as f32;
    let distance = span / (0.84 * aspect.min(1.0));
    if distance + diameter * 0.5 >= 100.0 {
        return Err("scene exceeds render camera clipping range; scale it down".into());
    }
    let mut camera = Camera::new();
    camera.target = (min + max) * 0.5;
    let (direction, up) = view.camera_axes();
    camera.position = camera.target + direction * distance;
    camera.up = up;
    camera.projection_mode = ProjectionMode::Orthographic;
    camera.viewport = Vec2::new(size[0] as f32, size[1] as f32);
    camera.viewport_origin = Vec2::ZERO;
    Ok(camera)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Result<RenderArgs, String> {
        RenderArgs::parse(&values.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn render_arguments_choose_output_and_reject_malformed_requests() {
        assert_eq!(
            args(&["scene.claydef"]).unwrap(),
            RenderArgs {
                source: "scene.claydef".into(),
                output: "scene.png".into(),
                size: [1024, 768],
                view: RenderView::Isometric,
            }
        );
        assert_eq!(
            args(&[
                "scene.claydef",
                "--output",
                "image.png",
                "--size",
                "256x512"
            ])
            .unwrap()
            .size,
            [256, 512]
        );
        for values in [
            &[][..],
            &["a.claydef", "b.claydef"],
            &["a.claydef", "--size", "0x2"],
            &["a.claydef", "--size", "2"],
            &["a.claydef", "--output"],
            &["a.claydef", "--output", "a.jpg"],
            &["a.claydef", "--view"],
            &["a.claydef", "--view", "unknown"],
            &["a.claydef", "--view", "top", "--view", "left"],
            &["a.claydef", "--typo"],
        ] {
            assert!(args(values).is_err(), "{values:?}");
        }
    }
    #[test]
    fn view_option_selects_typed_presets() {
        for (name, expected) in [
            ("isometric", RenderView::Isometric),
            ("top", RenderView::Top),
            ("left", RenderView::Left),
            ("right", RenderView::Right),
        ] {
            assert_eq!(
                args(&["scene.claydef", "--view", name]).unwrap().view,
                expected
            );
        }
    }
    #[test]
    fn cardinal_views_keep_top_upright_and_side_views_opposite() {
        let objects = scene_language::compile("part = box(size: [2,1,3], at: [12,3,5]);")
            .unwrap()
            .into_iter()
            .map(|(_, object)| object)
            .collect::<Vec<_>>();
        let top = fitted_camera(&objects, [256, 256], RenderView::Top).unwrap();
        assert!(top.position.y > top.target.y);
        assert_eq!(top.up, Vec3::NEG_Z);
        let center = top.project(top.target, 1.0).unwrap();
        assert!(top.project(top.target + Vec3::NEG_Z, 1.0).unwrap().y < center.y);
        let left = fitted_camera(&objects, [256, 256], RenderView::Left).unwrap();
        let right = fitted_camera(&objects, [256, 256], RenderView::Right).unwrap();
        assert!(left.position.x < left.target.x && right.position.x > right.target.x);
        assert!(left.project(left.target + Vec3::Z, 1.0).unwrap().x > center.x);
        assert!(right.project(right.target + Vec3::Z, 1.0).unwrap().x < center.x);
        for view in [
            RenderView::Isometric,
            RenderView::Top,
            RenderView::Left,
            RenderView::Right,
        ] {
            let camera = fitted_camera(&objects, [256, 512], view).unwrap();
            let (min, max) = scene_language::bounds(&objects).unwrap();
            for x in [min.x, max.x] {
                for y in [min.y, max.y] {
                    for z in [min.z, max.z] {
                        let pixel = camera.project(Vec3::new(x, y, z), 1.0).unwrap();
                        assert!(
                            (0.0..=256.0).contains(&pixel.x) && (0.0..=512.0).contains(&pixel.y),
                            "{view:?}: {pixel:?}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn invalid_recipe_fails_before_gpu_creation_and_preserves_output() {
        let folder =
            std::env::temp_dir().join(format!("claydash-render-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&folder).unwrap();
        let source = folder.join("scene.claydef");
        let output = folder.join("scene.png");
        std::fs::write(&source, "part = box(size: [0,1,1]);").unwrap();
        std::fs::write(&output, b"original output").unwrap();
        let result = RenderArgs {
            source,
            output: output.clone(),
            size: [96, 96],
            view: RenderView::Isometric,
        }
        .run();
        assert!(result
            .unwrap_err()
            .contains("size components must be positive"));
        assert_eq!(std::fs::read(&output).unwrap(), b"original output");
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn framing_follows_geometry_and_accounts_for_portrait_images() {
        let objects = scene_language::compile("part = box(size: [2,1,1], at: [12,3,5]);")
            .unwrap()
            .into_iter()
            .map(|(_, object)| object)
            .collect::<Vec<_>>();
        let landscape = fitted_camera(&objects, [512, 256], RenderView::Isometric).unwrap();
        let portrait = fitted_camera(&objects, [256, 512], RenderView::Isometric).unwrap();
        assert_eq!(landscape.target, Vec3::new(12., 3., 5.));
        assert!(
            (portrait.position.distance(portrait.target)
                / landscape.position.distance(landscape.target)
                - 2.)
                .abs()
                < 1e-5
        );
        assert!(fitted_camera(&[], [256, 256], RenderView::Isometric).is_err());
    }
}

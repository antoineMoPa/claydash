//! Aligned captures with an image-space, depth-independent mesh silhouette overlay.
use super::*;
use crate::renderer::CapturedFrame;

const MAX_RASTER_PIXEL_TESTS: u64 = 8_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonArgs {
    #[serde(default)]
    pub points: Vec<super::markers::PreviewPoint>,
    pub reference_ids: Vec<uuid::Uuid>,
    pub candidate_ids: Vec<uuid::Uuid>,
    pub reference_opacity: Option<f32>,
    #[serde(default)]
    pub reference_style: ReferenceStyle,
    pub views: Option<Vec<ComparisonView>>,
    pub panel_size: Option<u32>,
    pub framing: Option<Framing>,
    #[serde(default)]
    pub isolation: CaptureIsolation,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceStyle {
    #[default]
    Silhouette,
    Outline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonView {
    Front,
    Side,
    Top,
    ThreeQuarter,
}

impl ComparisonView {
    fn axes(self) -> (Vec3, Vec3) {
        match self {
            Self::Front => (Vec3::Z, Vec3::Y),
            Self::Side => (Vec3::X, Vec3::Y),
            Self::Top => (Vec3::Y, Vec3::NEG_Z),
            Self::ThreeQuarter => (Vec3::splat(1.0) / 3.0_f32.sqrt(), Vec3::Y),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Front => "Front",
            Self::Side => "Side",
            Self::Top => "Top",
            Self::ThreeQuarter => "Three quarter",
        }
    }
}

#[derive(Clone, Copy, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Framing {
    pub center: Vec3,
    pub span: f32,
}

pub struct ComparisonCapture {
    pub references: Vec<SdfObject>,
    candidates: Vec<SdfObject>,
    views: Vec<ComparisonView>,
    framing: Framing,
    opacity: f32,
    style: ReferenceStyle,
    panel_size: u32,
    revision: u64,
    points: Vec<super::markers::PreviewPoint>,
    point_metadata: Vec<Value>,
    reference_frame: Option<CapturedFrame>,
    tiles: Vec<[CapturedFrame; 3]>,
    cameras: Vec<Value>,
}

pub fn prepare(
    scene: &[SdfObject],
    args: ComparisonArgs,
    camera: Camera,
    revision: u64,
) -> Result<(ComparisonCapture, Camera), String> {
    let panel_size = args.panel_size.unwrap_or(256);
    if !(96..=capture::MAX_CAPTURE_SIZE).contains(&panel_size) {
        return Err("panel_size must be 96–2048 pixels".into());
    }
    super::markers::validate(&args.points)?;
    let opacity = args.reference_opacity.unwrap_or(0.5);
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err("reference_opacity must be between 0 and 1".into());
    }
    if args
        .reference_ids
        .iter()
        .any(|id| args.candidate_ids.contains(id))
    {
        return Err("reference_ids and candidate_ids must be disjoint".into());
    }
    // References are explicit individual meshes, never implicitly expanded Boolean groups.
    let references = capture::capture_parts(scene, &args.reference_ids)?;
    let candidates =
        capture::capture_scene(scene, Some(&args.candidate_ids), None, args.isolation)?.unwrap();
    if references
        .iter()
        .any(|reference| reference.imported_mesh.is_none())
    {
        return Err("reference_ids must name imported or agent-built triangle meshes".into());
    }
    if references.iter().any(|reference| {
        candidates
            .iter()
            .any(|candidate| reference.uuid == candidate.uuid)
    }) {
        return Err("candidate isolation includes a reference object; use isolation: parts or separate groups".into());
    }
    let triangle_count: usize = references
        .iter()
        .map(|reference| reference.imported_mesh.as_ref().unwrap().triangles.len())
        .sum();
    if triangle_count > 2_000_000 {
        return Err("comparison supports at most 2,000,000 reference triangles".into());
    }
    let views = args.views.unwrap_or_else(|| {
        vec![
            ComparisonView::Front,
            ComparisonView::Side,
            ComparisonView::Top,
            ComparisonView::ThreeQuarter,
        ]
    });
    if views.is_empty()
        || views.len() > 4
        || views
            .iter()
            .enumerate()
            .any(|(i, view)| views[..i].contains(view))
    {
        return Err("views must contain 1–4 distinct views".into());
    }
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for objects in [&references, &candidates] {
        for object in objects {
            if let Some(mesh) = &object.imported_mesh {
                let matrix = model::object_world_matrix(objects, object.uuid);
                for point in &mesh.positions {
                    let world = matrix.transform_point3(*point);
                    minimum = minimum.min(world);
                    maximum = maximum.max(world);
                }
            } else if object.boolean_parent.is_none() {
                if let Some((min, max)) = model::lattice_bounds(objects, object.uuid) {
                    let matrix = model::lattice_world_matrix(objects, object.uuid);
                    for x in [min.x, max.x] {
                        for y in [min.y, max.y] {
                            for z in [min.z, max.z] {
                                let world = matrix.transform_point3(Vec3::new(x, y, z));
                                minimum = minimum.min(world);
                                maximum = maximum.max(world);
                            }
                        }
                    }
                }
            }
        }
    }
    if !minimum.is_finite() || !maximum.is_finite() {
        return Err("objects have no finite capture bounds".into());
    }
    let framing = args.framing.unwrap_or(Framing {
        center: (minimum + maximum) * 0.5,
        span: (maximum - minimum).length().max(0.1) * 1.15,
    });
    if !framing.center.is_finite() || !framing.span.is_finite() || framing.span <= 0.0 {
        return Err("framing requires a finite center and positive span".into());
    }
    let distance = framing.span / 0.84;
    let center = (minimum + maximum) * 0.5 - framing.center;
    let half_extent = (maximum - minimum) * 0.5;
    for view in &views {
        let (direction, _) = view.axes();
        let depth_center = distance - direction.dot(center);
        let depth_half_extent = direction.abs().dot(half_extent);
        if depth_center - depth_half_extent <= 0.01 || depth_center + depth_half_extent >= 100.0 {
            return Err(format!("{} comparison bounds exceed camera clipping range; scale objects down or adjust framing", view.name()));
        }
    }
    Ok((
        ComparisonCapture {
            references,
            candidates,
            views,
            framing,
            opacity,
            style: args.reference_style,
            panel_size,
            revision,
            points: args.points,
            point_metadata: Vec::new(),
            reference_frame: None,
            tiles: Vec::new(),
            cameras: Vec::new(),
        },
        camera,
    ))
}

impl ComparisonCapture {
    pub fn current_objects(&self) -> &[SdfObject] {
        if self.reference_frame.is_some() {
            &self.candidates
        } else {
            &self.references
        }
    }

    pub fn camera(&self, base: &Camera) -> Camera {
        let mut camera = base.clone();
        let (direction, up) = self.views[self.tiles.len()].axes();
        camera.target = self.framing.center;
        camera.position = camera.target + direction * (self.framing.span / 0.84);
        camera.up = up;
        camera.projection_mode = ProjectionMode::Orthographic;
        camera.viewport = Vec2::splat(self.panel_size as f32);
        camera.viewport_origin = Vec2::ZERO;
        camera
    }

    pub fn accept_frame(&mut self, mut candidate: CapturedFrame, camera: &Camera) -> AgentResult {
        if candidate.width != self.panel_size || candidate.height != self.panel_size {
            return Err(
                "comparison viewport changed size during capture; retry the request".into(),
            );
        }
        let Some(mut reference) = self.reference_frame.take() else {
            self.reference_frame = Some(candidate);
            return Ok(Value::Null);
        };
        let mut overlay = candidate.clone();
        if self.opacity > 0.0 {
            let mask = mesh_mask(
                &self.references,
                camera,
                self.panel_size,
                MAX_RASTER_PIXEL_TESTS,
            )?;
            composite(&mut overlay, &mask, self.opacity, self.style);
        }
        let view = self.views[self.tiles.len()];
        for (frame, label) in [
            (&mut reference, "reference"),
            (&mut candidate, "candidate"),
            (&mut overlay, "overlay"),
        ] {
            super::capture_overlay::annotate(frame, camera, &format!("{} {}", view.name(), label));
        }
        let offsets = [0, 1, 2].map(|row| {
            [
                self.tiles.len() as u32 * self.panel_size,
                row * self.panel_size,
            ]
        });
        self.point_metadata.extend(super::markers::annotate(
            &mut reference,
            camera,
            &self.points,
            view.name(),
            &offsets,
        ));
        super::markers::annotate(&mut candidate, camera, &self.points, view.name(), &offsets);
        super::markers::annotate(&mut overlay, camera, &self.points, view.name(), &offsets);
        let inverse = camera.view().inverse();
        self.cameras.push(json!({"view": view.name(), "position": camera.position, "target": camera.target,
            "up": camera.up, "horizontal_axis": inverse.x_axis.truncate(), "vertical_axis": inverse.y_axis.truncate()}));
        self.tiles.push([reference, candidate, overlay]);
        if self.tiles.len() < self.views.len() {
            return Ok(Value::Null);
        }
        let sheet = comparison_sheet(&self.tiles)?;
        let mut result = capture::captured_image(&sheet)?;
        result["revision"] = json!(self.revision);
        if !self.points.is_empty() {
            result["point_previews"] = json!(self.point_metadata);
            result["point_display"] = json!("red center pixel with translucent 4px radius circle; drawn on top, without depth occlusion");
        }
        result["framing"] = json!(self.framing);
        result["views"] = json!(self.cameras);
        result["rows"] = json!(["reference", "candidate", "overlay"]);
        result["panel_size"] = json!(self.panel_size);
        result["reference_opacity"] = json!(self.opacity);
        result["compositing"] = json!("cyan mesh silhouette or boundary over opaque candidate; image-space overlay ignores candidate depth");
        Ok(result)
    }
}

// Rasterize the union of projected triangles, including both sides. No depth test is
// needed: this mask deliberately exposes the complete reference silhouette through candidates.
fn mesh_mask(
    objects: &[SdfObject],
    camera: &Camera,
    size: u32,
    max_pixel_tests: u64,
) -> Result<Vec<bool>, String> {
    let mut pixel_tests = 0;
    let mut mask = vec![false; (size * size) as usize];
    for object in objects {
        let mesh = object.imported_mesh.as_ref().unwrap();
        let matrix = model::object_world_matrix(objects, object.uuid);
        let points = mesh
            .positions
            .iter()
            .map(|point| {
                camera
                    .project(matrix.transform_point3(*point), 1.0)
                    .map(|p| Vec2::new(p.x, p.y))
            })
            .collect::<Vec<_>>();
        for triangle in &mesh.triangles {
            let [Some(a), Some(b), Some(c)] = triangle.map(|index| points[index as usize]) else {
                continue;
            };
            if !a.is_finite() || !b.is_finite() || !c.is_finite() {
                continue;
            }
            let edge = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
            let area = edge(a, b, c);
            if area.abs() < 1e-8 {
                continue;
            }
            let min = a.min(b).min(c).floor().max(Vec2::ZERO);
            let max = a.max(b).max(c).ceil().min(Vec2::splat(size as f32 - 1.0));
            if min.x > max.x || min.y > max.y {
                continue;
            }
            pixel_tests += (max.x as u64 - min.x as u64 + 1) * (max.y as u64 - min.y as u64 + 1);
            if pixel_tests > max_pixel_tests {
                return Err("reference silhouette exceeds projected raster-work limit; reduce panel_size or simplify the reference mesh".into());
            }
            for y in min.y as i32..=max.y as i32 {
                for x in min.x as i32..=max.x as i32 {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    if [edge(a, b, p), edge(b, c, p), edge(c, a, p)]
                        .iter()
                        .all(|value| value * area >= 0.0)
                    {
                        mask[(y as u32 * size + x as u32) as usize] = true;
                    }
                }
            }
        }
    }
    Ok(mask)
}

fn composite(frame: &mut CapturedFrame, mask: &[bool], opacity: f32, style: ReferenceStyle) {
    let width = frame.width as usize;
    let height = frame.height as usize;
    for y in 0..height {
        for x in 0..width {
            let at = y * width + x;
            if !mask[at] {
                continue;
            }
            if matches!(style, ReferenceStyle::Outline)
                && x > 0
                && y > 0
                && x + 1 < width
                && y + 1 < height
                && mask[at - 1]
                && mask[at + 1]
                && mask[at - width]
                && mask[at + width]
            {
                continue;
            }
            for (channel, tint) in [40.0, 220.0, 255.0].into_iter().enumerate() {
                frame.rgba[at * 4 + channel] = (frame.rgba[at * 4 + channel] as f32
                    * (1.0 - opacity)
                    + tint * opacity)
                    .round() as u8;
            }
        }
    }
}

fn comparison_sheet(tiles: &[[CapturedFrame; 3]]) -> Result<CapturedFrame, String> {
    let first = &tiles[0][0];
    if tiles
        .iter()
        .flatten()
        .any(|frame| frame.width != first.width || frame.height != first.height)
    {
        return Err("comparison views have different sizes".into());
    }
    let width = first.width * tiles.len() as u32;
    let height = first.height * 3;
    let mut rgba = vec![0; (width * height * 4) as usize];
    for (column, tile) in tiles.iter().enumerate() {
        for (row, frame) in tile.iter().enumerate() {
            for y in 0..frame.height {
                let source = (y * frame.width * 4) as usize;
                let target = (((row as u32 * frame.height + y) * width
                    + column as u32 * frame.width)
                    * 4) as usize;
                rgba[target..target + frame.width as usize * 4]
                    .copy_from_slice(&frame.rgba[source..source + frame.width as usize * 4]);
            }
        }
    }
    Ok(CapturedFrame {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (App, uuid::Uuid, uuid::Uuid) {
        let mut app = App::new();
        let candidate = model::objects_ref(&app.tree)[0].uuid;
        let reference = app
            .import_glb(
                "reference",
                &super::super::super::file_action_tests::triangle_glb(),
            )
            .unwrap()[0];
        (app, reference, candidate)
    }
    fn args(reference: uuid::Uuid, candidate: uuid::Uuid) -> ComparisonArgs {
        serde_json::from_value(
            json!({"reference_ids":[reference], "candidate_ids":[candidate], "panel_size":128}),
        )
        .unwrap()
    }
    fn frame(color: [u8; 4]) -> CapturedFrame {
        CapturedFrame {
            width: 128,
            height: 128,
            rgba: color.repeat(128 * 128),
        }
    }

    #[test]
    fn comparison_is_a_read_only_snapshot_with_reusable_framing() {
        let (app, reference, candidate) = fixture();
        let before = document::serialize_scene(&app.tree).unwrap();
        let selected = model::selected(&app.tree);
        let revision = app.scene_revision();
        let (mut capture, camera) = prepare(
            model::objects_ref(&app.tree),
            args(reference, candidate),
            app.camera.clone(),
            revision,
        )
        .unwrap();
        let framing = capture.framing;
        assert_eq!(
            capture.references[0].render_representation,
            GroupRenderRepresentation::DirectMesh
        );
        assert!(capture.references[0].boolean_parent.is_none());
        for _ in 0..4 {
            let view_camera = capture.camera(&camera);
            assert_eq!(view_camera.target, framing.center);
            assert!(
                ((view_camera.position - view_camera.target).length() * 0.84 - framing.span).abs()
                    < 1e-5
            );
            assert!(capture
                .accept_frame(frame([20, 30, 40, 255]), &view_camera)
                .unwrap()
                .is_null());
            assert_eq!(capture.current_objects()[0].uuid, candidate);
            let result = capture
                .accept_frame(frame([80, 90, 100, 255]), &view_camera)
                .unwrap();
            if capture.tiles.len() == 4 {
                assert_eq!(result["width"], 512);
                assert_eq!(result["height"], 384);
                assert_eq!(result["revision"], revision);
                assert_eq!(result["framing"], json!(framing));
                assert_eq!(result["views"].as_array().unwrap().len(), 4);
            } else {
                assert!(result.is_null());
            }
        }
        assert_eq!(document::serialize_scene(&app.tree).unwrap(), before);
        assert_eq!(model::selected(&app.tree), selected);
        assert_eq!(app.scene_revision(), revision);
        let mut retry = args(reference, candidate);
        retry.framing = Some(framing);
        let (retry, _) = prepare(
            model::objects_ref(&app.tree),
            retry,
            app.camera.clone(),
            revision,
        )
        .unwrap();
        assert_eq!(json!(retry.framing), json!(framing));
    }

    #[test]
    fn raster_mask_follows_world_transforms_and_overlay_preserves_candidate_pixels() {
        let (app, reference, candidate) = fixture();
        let (capture, camera) = prepare(
            model::objects_ref(&app.tree),
            args(reference, candidate),
            app.camera.clone(),
            1,
        )
        .unwrap();
        let camera = capture.camera(&camera);
        let mask = mesh_mask(&capture.references, &camera, 128, MAX_RASTER_PIXEL_TESTS).unwrap();
        assert!(mask.iter().any(|value| *value));
        // This triangle lives at x=2..3, y=0..1 after its GLB node transform.
        let point = camera.project(Vec3::new(2.2, 0.2, 0.0), 1.0).unwrap();
        assert!(mask[point.y as usize * 128 + point.x as usize]);
        let mut image = frame([80, 90, 100, 255]);
        composite(&mut image, &mask, 0.5, ReferenceStyle::Silhouette);
        for (i, covered) in mask.iter().enumerate() {
            assert_eq!(
                &image.rgba[i * 4..i * 4 + 4],
                if *covered {
                    &[60, 155, 178, 255]
                } else {
                    &[80, 90, 100, 255]
                }
            );
        }
        let mut image = frame([80, 90, 100, 255]);
        composite(&mut image, &mask, 0.0, ReferenceStyle::Silhouette);
        assert_eq!(image.rgba, frame([80, 90, 100, 255]).rgba);
        composite(&mut image, &mask, 1.0, ReferenceStyle::Outline);
        let outline_pixels = image
            .rgba
            .chunks_exact(4)
            .filter(|pixel| *pixel == [40, 220, 255, 255])
            .count();
        assert!(outline_pixels > 0 && outline_pixels < mask.iter().filter(|value| **value).count());
    }

    #[test]
    fn comparison_rejects_invalid_requests_before_rendering() {
        let (app, reference, candidate) = fixture();
        let scene = model::objects_ref(&app.tree);
        for request in [
            json!({"reference_ids":[],"candidate_ids":[candidate]}),
            json!({"reference_ids":[reference],"candidate_ids":[]}),
            json!({"reference_ids":[reference],"candidate_ids":[reference]}),
            json!({"reference_ids":[candidate],"candidate_ids":[reference]}),
            json!({"reference_ids":[reference],"candidate_ids":[candidate],"views":[]}),
            json!({"reference_ids":[reference],"candidate_ids":[candidate],"views":["front","front"]}),
            json!({"reference_ids":[reference],"candidate_ids":[candidate],"reference_opacity":1.1}),
            json!({"reference_ids":[reference],"candidate_ids":[candidate],"framing":{"center":[0,0,0],"span":0}}),
            json!({"reference_ids":[reference],"candidate_ids":[candidate],"framing":{"center":[0,0,0],"span":1000}}),
        ] {
            let args = serde_json::from_value(request).unwrap();
            assert!(prepare(scene, args, app.camera.clone(), 1).is_err());
        }
        let mut high_res = args(reference, candidate);
        high_res.panel_size = Some(1024);
        assert!(prepare(scene, high_res, app.camera.clone(), 1).is_ok());
        assert!(serde_json::from_value::<ComparisonArgs>(json!({"reference_ids":[reference],"candidate_ids":[candidate],"reference_style":"unknown"})).is_err());
    }

    #[test]
    fn fixed_front_framing_allows_lateral_crops_but_checks_depth() {
        let (mut app, reference, candidate) = fixture();
        let mut objects = model::objects(&app.tree);
        objects
            .iter_mut()
            .find(|object| object.uuid == candidate)
            .unwrap()
            .transform
            .translation = Vec3::new(200.0, 0.0, 0.0);
        model::set_objects(&mut app.tree, objects);
        let mut options = args(reference, candidate);
        options.views = Some(vec![ComparisonView::Front]);
        options.framing = Some(Framing {
            center: Vec3::ZERO,
            span: 10.0,
        });
        assert!(prepare(
            model::objects_ref(&app.tree),
            options,
            app.camera.clone(),
            1,
        )
        .is_ok());
        let mut options = args(reference, candidate);
        options.views = Some(vec![ComparisonView::Side]);
        options.framing = Some(Framing {
            center: Vec3::ZERO,
            span: 10.0,
        });
        assert!(prepare(
            model::objects_ref(&app.tree),
            options,
            app.camera.clone(),
            1,
        )
        .is_err());
    }

    #[test]
    fn silhouette_rasterization_rejects_work_over_budget() {
        let (app, reference, candidate) = fixture();
        let (capture, camera) = prepare(
            model::objects_ref(&app.tree),
            args(reference, candidate),
            app.camera.clone(),
            1,
        )
        .unwrap();
        let camera = capture.camera(&camera);
        let error = mesh_mask(&capture.references, &camera, 128, 10).unwrap_err();
        assert!(error.contains("raster-work limit"));
    }

    #[test]
    fn comparison_is_registered_for_mcp_and_cli_output() {
        assert!(matches!(
            serde_json::from_value::<Request>(request_payload("GetUserView", json!({}))).unwrap(),
            Request::GetUserView
        ));
        assert!(mcp::mcp_tools()
            .iter()
            .any(|tool| tool["name"] == "capture_comparison"));
        let json =
            json!({"reference_ids":[uuid::Uuid::new_v4()],"candidate_ids":[uuid::Uuid::new_v4()]});
        let (request, path) = parse_agent_arguments(
            "CaptureComparison",
            &[json.to_string(), "--output".into(), "comparison.png".into()],
        )
        .unwrap();
        assert!(matches!(
            serde_json::from_value::<Request>(request).unwrap(),
            Request::CaptureComparison(_)
        ));
        assert_eq!(path.unwrap(), PathBuf::from("comparison.png"));
    }
}

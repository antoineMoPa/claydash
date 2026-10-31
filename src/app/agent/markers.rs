//! Read-only world-coordinate previews, projected into the exact capture camera.
use super::*;
use crate::renderer::CapturedFrame;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewPoint {
    pub position: Vec3,
    pub label: Option<String>,
}

pub fn validate(points: &[PreviewPoint]) -> Result<(), String> {
    if points.len() > 64 {
        return Err("points supports at most 64 coordinate previews".into());
    }
    for point in points {
        if !point.position.is_finite() {
            return Err("point positions must be finite world coordinates".into());
        }
        if point
            .label
            .as_ref()
            .is_some_and(|label| label.len() > 24 || !label.is_ascii())
        {
            return Err("point labels must be at most 24 ASCII characters".into());
        }
    }
    Ok(())
}

pub fn annotate(
    frame: &mut CapturedFrame,
    camera: &Camera,
    points: &[PreviewPoint],
    view: &str,
    sheet_offsets: &[[u32; 2]],
) -> Vec<Value> {
    let matrix = camera.projection() * camera.view();
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let clip = matrix * point.position.extend(1.0);
            let mut result =
                json!({"index":index, "label":point.label, "position":point.position, "view":view});
            if clip.w <= 0.0 || !clip.is_finite() {
                result["status"] = json!("behind_camera");
                return result;
            }
            let ndc = clip.truncate() / clip.w;
            let pixel = Vec2::new(
                (ndc.x * 0.5 + 0.5) * camera.viewport.x,
                (0.5 - ndc.y * 0.5) * camera.viewport.y,
            );
            result["pixel"] = json!(pixel);
            if !(0.0..=1.0).contains(&ndc.z) {
                result["status"] = json!("outside_depth_range");
            } else if pixel.x < 0.0
                || pixel.y < 0.0
                || pixel.x >= frame.width as f32
                || pixel.y >= frame.height as f32
            {
                result["status"] = json!("outside_frame");
            } else {
                let center = [pixel.x.floor() as u32, pixel.y.floor() as u32];
                draw_point(frame, center, point.label.as_deref());
                result["status"] = json!("visible");
                result["center_pixel"] = json!(center);
                result["sheet_pixels"] = json!(sheet_offsets
                    .iter()
                    .map(|offset| [center[0] + offset[0], center[1] + offset[1]])
                    .collect::<Vec<_>>());
            }
            result
        })
        .collect()
}

fn draw_point(frame: &mut CapturedFrame, center: [u32; 2], label: Option<&str>) {
    for dy in -4_i32..=4 {
        for dx in -4_i32..=4 {
            let distance = dx * dx + dy * dy;
            if distance > 16 {
                continue;
            }
            let x = center[0] as i32 + dx;
            let y = center[1] as i32 + dy;
            if x < 0 || y < 0 || x >= frame.width as i32 || y >= frame.height as i32 {
                continue;
            }
            let at = ((y as u32 * frame.width + x as u32) * 4) as usize;
            let opacity = if distance == 0 {
                255
            } else if distance >= 10 {
                190
            } else {
                100
            };
            for (channel, red) in [255_u32, 0, 0].into_iter().enumerate() {
                frame.rgba[at + channel] = ((frame.rgba[at + channel] as u32 * (255 - opacity)
                    + red * opacity)
                    / 255) as u8;
            }
            frame.rgba[at + 3] = 255;
        }
    }
    if let Some(label) = label.filter(|label| !label.is_empty()) {
        super::capture_overlay::point_label(frame, center, label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn camera() -> Camera {
        let mut camera = Camera::new();
        camera.position = Vec3::new(0.0, 0.0, 5.0);
        camera.target = Vec3::ZERO;
        camera.up = Vec3::Y;
        camera.viewport = Vec2::splat(100.0);
        camera.viewport_origin = Vec2::new(200.0, 50.0);
        camera
    }
    #[test]
    fn projects_into_cropped_pixels_and_draws_exact_red_center_with_fixed_radius() {
        for projection in [ProjectionMode::Perspective, ProjectionMode::Orthographic] {
            let mut camera = camera();
            camera.projection_mode = projection;
            let mut frame = CapturedFrame {
                width: 100,
                height: 100,
                rgba: vec![200; 40_000],
            };
            let points = [PreviewPoint {
                position: Vec3::ZERO,
                label: None,
            }];
            let result = annotate(&mut frame, &camera, &points, "Front", &[[100, 200]]);
            assert_eq!(result[0]["pixel"], json!([50.0, 50.0]));
            assert_eq!(result[0]["sheet_pixels"], json!([[150, 250]]));
            let at = (50 * 100 + 50) * 4;
            assert_eq!(&frame.rgba[at..at + 4], &[255, 0, 0, 255]);
            assert_eq!(frame.rgba[at + 5 * 4], 200);
            assert!(frame.rgba[at + 3 * 4] > 200);
        }
    }
    #[test]
    fn reports_occluded_points_as_xray_and_offscreen_and_behind_points_without_clamping() {
        let camera = camera();
        let mut frame = CapturedFrame {
            width: 100,
            height: 100,
            rgba: vec![0; 40_000],
        };
        let points = [
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 6.0),
        ]
        .map(|position| PreviewPoint {
            position,
            label: None,
        });
        let result = annotate(&mut frame, &camera, &points, "Perspective", &[[0, 0]]);
        assert_eq!(result[0]["status"], "visible");
        assert_eq!(result[1]["status"], "outside_frame");
        assert_eq!(result[2]["status"], "behind_camera");
        assert!(result[1]["center_pixel"].is_null());
    }
    #[test]
    fn validates_finite_positions_and_bounded_input() {
        assert!(validate(&[PreviewPoint {
            position: Vec3::splat(f32::NAN),
            label: None
        }])
        .is_err());
        assert!(validate(&vec![
            PreviewPoint {
                position: Vec3::ZERO,
                label: None
            };
            65
        ])
        .is_err());
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn point_preview_uses_opt_in_resolution_and_preserves_scene_and_cameras() {
        let mut app = App::new();
        let before = document::serialize_scene(&app.tree).unwrap();
        let user = app.camera.clone();
        let agent = app.agent_view_camera();
        let request = serde_json::from_value(request_payload(
            "PreviewPoints",
            json!({
                "points":[{"position":agent.target,"label":"P1"}],"resolution":[1024,1024]
            }),
        ))
        .unwrap();
        let (reply, response) = mpsc::channel();
        app.process_agent_request(Inbound { request, reply });
        let mut capture = app.agent_capture.take().unwrap();
        let camera = capture.camera();
        assert_eq!(camera.viewport, Vec2::splat(1024.0));
        assert!(capture.objects.is_some());
        let result = capture
            .accept_frame(
                CapturedFrame {
                    width: 1024,
                    height: 1024,
                    rgba: vec![150; 1024 * 1024 * 4],
                },
                &camera,
            )
            .unwrap();
        assert_eq!(
            result["point_previews"][0]["center_pixel"],
            json!([512, 512])
        );
        assert_eq!(result["point_previews"][0]["status"], "visible");
        assert!(response.try_recv().is_err());
        assert_eq!(document::serialize_scene(&app.tree).unwrap(), before);
        assert_eq!(app.camera.position, user.position);
        assert_eq!(app.camera.viewport, user.viewport);
        assert_eq!(app.agent_view_camera().viewport, agent.viewport);
        assert_eq!(app.agent_view_camera().position, agent.position);
    }
    #[test]
    fn preview_requires_points_and_schema_advertises_precision_options() {
        let mut app = App::new();
        let request = serde_json::from_value(request_payload("PreviewPoints", json!({}))).unwrap();
        let (reply, response) = mpsc::channel();
        app.process_agent_request(Inbound { request, reply });
        assert!(response
            .recv()
            .unwrap()
            .unwrap_err()
            .contains("at least one point"));
        let tools = mcp::mcp_tools();
        let preview = tools
            .iter()
            .find(|tool| tool["name"] == "preview_points")
            .unwrap();
        assert_eq!(
            preview["inputSchema"]["properties"]["resolution"]["items"]["maximum"],
            2048
        );
        assert_eq!(preview["inputSchema"]["required"], json!(["points"]));
        let mut camera = Camera::new();
        assert!(capture::set_resolution(&mut camera, Some([4096, 1024])).is_err());
        assert!(capture::set_resolution(&mut camera, Some([1024, 1024])).is_ok());
    }
}

use super::*;

pub enum CaptureView {
    Viewport,
    Orthographic {
        panel_size: u32,
        distance: f32,
        frames: Vec<crate::renderer::CapturedFrame>,
    },
}

pub struct AgentCapture {
    pub reply: Sender<AgentResult>,
    pub objects: Option<Vec<SdfObject>>,
    pub mode: CaptureMode,
    pub view: CaptureView,
}

impl AgentCapture {
    pub fn camera(&self, live: &Camera) -> Camera {
        let mut camera = live.clone();
        if let CaptureView::Orthographic {
            panel_size,
            distance,
            frames,
        } = &self.view
        {
            let (direction, up) = match frames.len() {
                0 => (Vec3::X, Vec3::Y),
                1 => (Vec3::Y, Vec3::NEG_Z),
                _ => (Vec3::Z, Vec3::Y),
            };
            camera.target = Vec3::ZERO;
            camera.position = direction * *distance;
            camera.up = up;
            camera.projection_mode = ProjectionMode::Orthographic;
            camera.viewport = Vec2::splat(*panel_size as f32);
            camera.viewport_origin = Vec2::ZERO;
        }
        camera
    }

    pub fn accept_frame(
        &mut self,
        frame: crate::renderer::CapturedFrame,
        camera: &Camera,
    ) -> AgentResult {
        let cropped = crate::render_export::crop_to_viewport(frame, camera);
        if let CaptureView::Orthographic { frames, .. } = &mut self.view {
            frames.push(cropped);
            if frames.len() < 3 {
                return Ok(Value::Null);
            }
            let sheet = orthographic_sheet(frames)?;
            return captured_image(&sheet);
        }
        captured_image(&cropped)
    }
}

fn captured_image(frame: &crate::renderer::CapturedFrame) -> AgentResult {
    use base64::Engine;
    let bytes = crate::render_export::png_bytes(frame)?;
    Ok(json!({"width": frame.width, "height": frame.height,
        "data": base64::engine::general_purpose::STANDARD.encode(bytes)}))
}

pub(super) fn orthographic_sheet(
    frames: &[crate::renderer::CapturedFrame],
) -> Result<crate::renderer::CapturedFrame, String> {
    let [x, y, z] = frames else {
        return Err("expected three orthographic views".into());
    };
    if x.width != y.width || x.width != z.width || x.height != y.height || x.height != z.height {
        return Err("orthographic views have different sizes".into());
    }
    let width = x.width * 3;
    let height = x.height + 24;
    let mut rgba = vec![0; (width * height * 4) as usize];
    for (column, frame) in frames.iter().enumerate() {
        for row in 0..frame.height {
            let source = (row * frame.width * 4) as usize;
            let target = (((row + 24) * width + column as u32 * frame.width) * 4) as usize;
            rgba[target..target + (frame.width * 4) as usize]
                .copy_from_slice(&frame.rgba[source..source + (frame.width * 4) as usize]);
        }
    }
    for pixel in rgba[..(width * 24 * 4) as usize].chunks_exact_mut(4) {
        pixel.copy_from_slice(&[35, 43, 54, 255]);
    }
    for (column, glyph) in [
        [0b10001, 0b01010, 0b00100, 0b01010, 0b10001],
        [0b10001, 0b01010, 0b00100, 0b00100, 0b00100],
        [0b11111, 0b00010, 0b00100, 0b01000, 0b11111],
    ]
    .iter()
    .enumerate()
    {
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let px = column as u32 * x.width + 10 + col * 2 + dx;
                            let py = 7 + row as u32 * 2 + dy;
                            let offset = ((py * width + px) * 4) as usize;
                            rgba[offset..offset + 4].copy_from_slice(&[240, 244, 249, 255]);
                        }
                    }
                }
            }
        }
    }
    Ok(crate::renderer::CapturedFrame {
        width,
        height,
        rgba,
    })
}

pub(super) fn capture_objects(
    scene: &[SdfObject],
    requested: &[uuid::Uuid],
) -> Result<Vec<SdfObject>, String> {
    if requested.is_empty() {
        return Err("object_ids must contain at least one object".to_string());
    }
    let lookup: HashMap<_, _> = scene.iter().map(|object| (object.uuid, object)).collect();
    let mut roots = HashSet::new();
    let mut selected_inlays = HashSet::new();
    let mut groups_with_inlays = HashSet::new();
    for id in requested {
        let Some(object) = lookup.get(id) else {
            return Err(format!("object not found: {id}"));
        };
        let mut root = if let Some(inlay) = object.surface_inlay {
            selected_inlays.insert(*id);
            inlay.host
        } else {
            *id
        };
        for _ in 0..scene.len() {
            let Some(parent) = lookup.get(&root).and_then(|object| object.boolean_parent) else {
                break;
            };
            root = parent;
        }
        if !lookup.contains_key(&root) {
            return Err(format!("object group missing for {id}"));
        }
        if object.surface_inlay.is_none() {
            groups_with_inlays.insert(root);
        }
        roots.insert(root);
    }
    let group_ids = commands::selected_subtree_ids(scene, &roots.into_iter().collect::<Vec<_>>());
    let groups_with_inlays =
        commands::selected_subtree_ids(scene, &groups_with_inlays.into_iter().collect::<Vec<_>>());
    let groups_with_inlays: HashSet<_> = groups_with_inlays.into_iter().collect();
    let group_ids: HashSet<_> = group_ids.into_iter().collect();
    Ok(scene
        .iter()
        .filter(|object| {
            if let Some(inlay) = object.surface_inlay {
                selected_inlays.contains(&object.uuid) || groups_with_inlays.contains(&inlay.host)
            } else {
                group_ids.contains(&object.uuid)
            }
        })
        .cloned()
        .collect())
}

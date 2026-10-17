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

pub(super) fn capture_scene(
    scene: &[SdfObject], requested: Option<&[uuid::Uuid]>, stage: Option<model::SheetStage>,
    isolation: CaptureIsolation,
) -> Result<Option<Vec<SdfObject>>, String> {
    let mut objects = match (requested, isolation) {
        (Some(ids), CaptureIsolation::Groups) => Some(capture_objects(scene, ids)?),
        (Some(ids), CaptureIsolation::Parts) => Some(capture_parts(scene, ids)?),
        (None, CaptureIsolation::Parts) => return Err("isolation: parts requires object_ids".into()),
        (None, CaptureIsolation::Groups) if stage.is_some() => Some(scene.to_vec()),
        (None, CaptureIsolation::Groups) => None,
    };
    if let (Some(objects), Some(stage)) = (&mut objects, stage) {
        // A recipe snapshot must use source geometry rather than saved formed captures.
        let parents: HashMap<_, _> = objects.iter().map(|object| (object.uuid, object.boolean_parent)).collect();
        let mut affected = HashSet::new();
        for sheet in objects.iter().filter(|object| matches!(object.params, SdfParams::SheetPanelParams(_))) {
            let mut cursor = Some(sheet.uuid);
            while let Some(id) = cursor {
                if !affected.insert(id) { break; }
                cursor = parents.get(&id).copied().flatten();
            }
        }
        for object in objects {
            if affected.contains(&object.uuid) {
                object.render_representation = GroupRenderRepresentation::ExactSdf;
            }
            if let SdfParams::SheetPanelParams(panel) = &mut object.params { panel.stage = stage; }
        }
    }
    Ok(objects)
}

pub(super) fn capture_parts(
    scene: &[SdfObject],
    requested: &[uuid::Uuid],
) -> Result<Vec<SdfObject>, String> {
    if requested.is_empty() {
        return Err("object_ids must contain at least one object".into());
    }
    let lookup: HashMap<_, _> = scene.iter().map(|object| (object.uuid, object)).collect();
    let mut parts = Vec::new();
    let mut seen = HashSet::new();
    for id in requested {
        let object = lookup.get(id).ok_or_else(|| format!("object not found: {id}"))?;
        if !seen.insert(*id) {
            continue;
        }
        if object.surface_inlay.is_some() {
            return Err(format!("surface inlay {id} requires its host; use isolation: groups"));
        }
        let world_group = model::group_world_matrix(scene, *id);
        let (scale, rotation, translation) = world_group.to_scale_rotation_translation();
        let group_transform = Transform { scale, rotation, translation };
        if !group_transform.matrix().abs_diff_eq(world_group, 1e-4) {
            return Err(format!("part {id} has a sheared group transform that cannot be captured alone"));
        }
        let mut part = (*object).clone();
        part.boolean_parent = None;
        part.operation = BooleanOperation::Union;
        part.group_transform = group_transform;
        part.render_representation = GroupRenderRepresentation::ExactSdf;
        parts.push(part);
    }
    Ok(parts)
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

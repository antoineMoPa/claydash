use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthAcceleratorStatus {
    NotComputedYet,
    Baking(u32),
    Ready,
}
impl DepthAcceleratorStatus {
    pub fn label(self) -> String {
        match self {
            Self::NotComputedYet => "Not computed yet".into(),
            Self::Baking(percent) => format!("{percent}%"),
            Self::Ready => "Ready".into(),
        }
    }
}

pub fn depth_accelerator_status(
    context: &egui::Context,
    source: &[SdfObject],
    root: uuid::Uuid,
    source_revision: i32,
) -> DepthAcceleratorStatus {
    let source = &source[..source.len().min(MAX_OBJECTS)];
    let Some(object) = source.iter().find(|object| {
        object.uuid == root
            && (object.render_representation.is_depth_accelerator()
                || matches!(object.render_representation, GroupRenderRepresentation::GaussianSplats
                    | GroupRenderRepresentation::BoxDepthAtlas | GroupRenderRepresentation::SphereDepthAtlas))
    }) else {
        return DepthAcceleratorStatus::NotComputedYet;
    };
    let progress = context
        .data(|data| {
            data.get_temp::<HashMap<uuid::Uuid, (i32, u32)>>(egui::Id::new(
                "depth-accelerator-progress",
            ))
        })
        .and_then(|entries| entries.get(&root).copied());
    if let Some((revision, percent)) = progress {
        if revision == source_revision {
            return DepthAcceleratorStatus::Baking(percent.min(99));
        }
    }
    let key = context
        .data(|data| {
            data.get_temp::<HashMap<uuid::Uuid, u64>>(egui::Id::new("depth-accelerator-ready"))
        })
        .and_then(|keys| keys.get(&root).copied());
    let Some(key) = key else {
        return DepthAcceleratorStatus::NotComputedYet;
    };
    let check_id = egui::Id::new("depth-accelerator-readiness-check").with(root);
    if let Some((revision, checked_key, status)) =
        context.data(|data| data.get_temp::<(i32, u64, DepthAcceleratorStatus)>(check_id))
    {
        if revision == source_revision && checked_key == key {
            return status;
        }
    }
    let status = if key == capture_key(source, object) {
        DepthAcceleratorStatus::Ready
    } else {
        DepthAcceleratorStatus::NotComputedYet
    };
    // Fingerprint large source scenes only after an edit or a new bake.
    context.data_mut(|data| data.insert_temp(check_id, (source_revision, key, status)));
    status
}

pub(crate) fn publish_depth_accelerator_status(
    context: &egui::Context,
    source: &[SdfObject],
    cache: &HashMap<uuid::Uuid, CachedGroupCapture>,
    progress: Option<(uuid::Uuid, i32, u32)>,
) {
    let keys: HashMap<_, _> = source
        .iter()
        .filter(|object| {
            object.render_representation.is_depth_accelerator()
                || matches!(object.render_representation, GroupRenderRepresentation::GaussianSplats
                    | GroupRenderRepresentation::BoxDepthAtlas | GroupRenderRepresentation::SphereDepthAtlas)
        })
        .filter_map(|object| {
            cache
                .get(&object.uuid)
                .filter(|entry| {
                    matches!(
                        (&entry.capture, object.render_representation),
                        (
                            CachedCapture::Sphere(_),
                            GroupRenderRepresentation::SphereAccelerator | GroupRenderRepresentation::SphereDepthAtlas
                        ) | (
                            CachedCapture::Box(_),
                            GroupRenderRepresentation::BoxAccelerator
                                | GroupRenderRepresentation::BoxDepthAtlas
                                | GroupRenderRepresentation::GaussianSplats
                        )
                    )
                })
                .map(|entry| (object.uuid, entry.key))
        })
        .collect();
    let progress_map: HashMap<_, _> = progress
        .map(|(root, revision, percent)| (root, (revision, percent.min(99))))
        .into_iter()
        .collect();
    let changed = context.data_mut(|data| {
        let id = egui::Id::new("depth-accelerator-ready");
        let changed = data.get_temp::<HashMap<uuid::Uuid, u64>>(id).as_ref() != Some(&keys);
        data.insert_temp(id, keys);
        let progress_id = egui::Id::new("depth-accelerator-progress");
        let progress_changed = data
            .get_temp::<HashMap<uuid::Uuid, (i32, u32)>>(progress_id)
            .as_ref()
            != Some(&progress_map);
        data.insert_temp(progress_id, progress_map);
        changed || progress_changed
    });
    if changed {
        context.request_repaint();
    }
    if progress.is_some() {
        context.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

pub(crate) fn restored_capture(
    source: &[SdfObject],
    root: &SdfObject,
    key: u64,
) -> Option<CachedCapture> {
    let saved = root.saved_group_capture.as_deref()?;
    if saved.version != 1 || saved.source_key != key {
        return None;
    }
    let (texels, owners) = match (&saved.capture, root.render_representation) {
        (
            CachedCapture::Box(atlas),
            GroupRenderRepresentation::BoxDepthAtlas
            | GroupRenderRepresentation::BoxAccelerator
            | GroupRenderRepresentation::GaussianSplats,
        ) => {
            let gaussian = root.render_representation == GroupRenderRepresentation::GaussianSplats;
            let resolution = if gaussian {
                root.gaussian_splats.resolution
            } else {
                BOX_DEPTH_RESOLUTION
            };
            let layers = if gaussian { 8 } else { 1 };
            let expected = box_capture_sample_count(resolution, layers)?;
            if atlas.resolution != resolution
                || atlas.layers != layers
                || atlas.texels.len() != expected
                || atlas.normals.len() != expected
                || !atlas.local_min.is_finite()
                || !atlas.local_max.is_finite()
                || atlas.local_max.min_element() <= 0.0
                || atlas.local_min != -atlas.local_max
                || atlas.normals.iter().any(|normal| !normal.is_finite())
            {
                return None;
            }
            (&atlas.texels, &atlas.owners)
        }
        (
            CachedCapture::Sphere(atlas),
            GroupRenderRepresentation::SphereDepthAtlas
            | GroupRenderRepresentation::SphereAccelerator,
        ) => {
            if atlas.width != SPHERE_DEPTH_WIDTH
                || atlas.height != SPHERE_DEPTH_HEIGHT
                || atlas.texels.len() != (SPHERE_DEPTH_WIDTH * SPHERE_DEPTH_HEIGHT) as usize
                || !atlas.radius.is_finite()
                || atlas.radius <= 0.0
            {
                return None;
            }
            (&atlas.texels, &atlas.owners)
        }
        _ => return None,
    };
    let ids: HashSet<_> = source.iter().map(|object| object.uuid).collect();
    if texels.len() != owners.len()
        || texels.iter().flatten().any(|value| !value.is_finite())
        || !owners.iter().any(Option::is_some)
        || owners.iter().flatten().any(|id| !ids.contains(id))
    {
        return None;
    }
    Some(saved.capture.clone())
}

pub(crate) fn objects_with_saved_captures(
    source: &[SdfObject],
    objects: &mut [SdfObject],
    cache: &HashMap<uuid::Uuid, CachedGroupCapture>,
) {
    let source = &source[..source.len().min(MAX_OBJECTS)];
    for object in objects {
        if !matches!(
            object.render_representation,
            GroupRenderRepresentation::BoxDepthAtlas
                | GroupRenderRepresentation::SphereDepthAtlas
                | GroupRenderRepresentation::SphereAccelerator
                | GroupRenderRepresentation::BoxAccelerator
                | GroupRenderRepresentation::GaussianSplats
        ) {
            object.saved_group_capture = None;
            continue;
        }
        let key = capture_key(source, object);
        if let Some(entry) = cache.get(&object.uuid).filter(|entry| entry.key == key) {
            object.saved_group_capture =
                Some(std::sync::Arc::new(crate::model::SavedGroupCapture {
                    version: 1,
                    source_key: key,
                    capture: entry.capture.clone(),
                }));
        } else if restored_capture(source, object, key).is_none() {
            object.saved_group_capture = None;
        }
    }
}

#[derive(serde::Serialize)]
struct CaptureObjectKey<'a> {
    uuid: uuid::Uuid,
    transform: &'a Transform,
    group_transform: Option<&'a Transform>,
    root_group_scale: Option<Vec3>,
    color: glam::Vec4,
    object_type: i32,
    params: &'a SdfParams,
    operation: crate::model::BooleanOperation,
    boolean_parent: Option<uuid::Uuid>,
    softness: f32,
    material: &'a crate::model::Material,
    material_id: Option<uuid::Uuid>,
    repetition: &'a crate::model::Repetition,
    mirror: &'a Option<crate::model::Mirror>,
    lattice: &'a Option<crate::model::Lattice>,
    path_extrusion: &'a Option<crate::model::PathExtrusion>,
    surface_inlay: &'a Option<crate::model::SurfaceInlay>,
    image_stencil: &'a Option<crate::model::ImageStencil>,
}

pub(crate) fn capture_key(source: &[SdfObject], root: &SdfObject) -> u64 {
    // External world-space references can change relative geometry when a
    // group moves. Conservatively include its pose if any are present.
    let independent_pose = root.boolean_parent.is_none()
        && !source.iter().any(|object| {
            matches!(&object.params, SdfParams::TextParams(text) if text.path.is_some())
                || object
                    .path_extrusion
                    .is_some_and(|path| path.profile_curve.is_some())
                || object.surface_inlay.is_some()
        });
    let objects: Vec<_> = source
        .iter()
        .map(|object| CaptureObjectKey {
            uuid: object.uuid,
            transform: &object.transform,
            group_transform: (object.uuid != root.uuid || !independent_pose)
                .then_some(&object.group_transform),
            root_group_scale: (object.uuid == root.uuid && independent_pose)
                .then_some(object.group_transform.scale),
            color: object.color,
            object_type: object.object_type,
            params: &object.params,
            operation: object.operation,
            boolean_parent: object.boolean_parent,
            softness: object.softness,
            material: &object.material,
            material_id: object.material_id,
            repetition: &object.repetition,
            mirror: &object.mirror,
            lattice: &object.lattice,
            path_extrusion: &object.path_extrusion,
            surface_inlay: &object.surface_inlay,
            image_stencil: &object.image_stencil,
        })
        .collect();
    // Preserve legacy fingerprints at the default resolution.
    let splat_resolution = (root.render_representation
        == GroupRenderRepresentation::GaussianSplats
        && root.gaussian_splats.resolution != 64)
        .then_some(root.gaussian_splats.resolution);
    let mut bytes = serde_json::to_vec(&(root.uuid, root.render_representation, objects))
        .expect("capture source is serializable");
    if let Some(resolution) = splat_resolution {
        bytes.extend_from_slice(&resolution.to_le_bytes());
    }
    // FNV-1a has a fixed definition, so saved fingerprints survive restarts
    // and Rust toolchain changes. Bump the saved field version if inputs change.
    bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

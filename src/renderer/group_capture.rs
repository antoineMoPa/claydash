use super::*;

use std::collections::{HashMap, HashSet};

use crate::model::{BoxParams, GroupRenderRepresentation, PrimitiveKind, SphereParams, Transform};

pub(super) fn prepared_render_text(
    source: &[SdfObject],
    render_object: &SdfObject,
) -> Option<std::sync::Arc<crate::model::PreparedText>> {
    source
        .iter()
        .find(|object| object.uuid == render_object.uuid)
        .and_then(|object| crate::model::prepared_scene_text(source, object).ok())
}

pub(super) struct PreparedGroupScene {
    pub objects: Vec<SdfObject>,
    pub box_depth_atlases: HashMap<uuid::Uuid, std::sync::Arc<BoxDepthAtlas>>,
    pub sphere_depth_atlases: HashMap<uuid::Uuid, std::sync::Arc<SphereDepthAtlas>>,
    pub neural_fields: HashMap<uuid::Uuid, std::sync::Arc<super::neural_sdf::NeuralField>>,
    pub gaussian_splats: HashSet<uuid::Uuid>,
}

pub(super) use crate::model::SavedDepthAtlas as CachedCapture;

pub(super) struct CachedGroupCapture {
    pub(super) key: u64,
    capture: CachedCapture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepthAcceleratorStatus {
    NotComputedYet,
    Ready,
}
impl DepthAcceleratorStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NotComputedYet => "Not computed yet",
            Self::Ready => "Ready",
        }
    }
}

pub(crate) fn depth_accelerator_status(
    context: &egui::Context,
    source: &[SdfObject],
    root: uuid::Uuid,
    source_revision: i32,
) -> DepthAcceleratorStatus {
    let source = &source[..source.len().min(MAX_OBJECTS)];
    let Some(object) = source.iter().find(|object| {
        object.uuid == root
            && (object.render_representation.is_depth_accelerator()
                || object.render_representation == GroupRenderRepresentation::GaussianSplats)
    }) else {
        return DepthAcceleratorStatus::NotComputedYet;
    };
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

pub(super) fn publish_depth_accelerator_status(
    context: &egui::Context,
    source: &[SdfObject],
    cache: &HashMap<uuid::Uuid, CachedGroupCapture>,
) {
    let keys: HashMap<_, _> = source
        .iter()
        .filter(|object| {
            object.render_representation.is_depth_accelerator()
                || object.render_representation == GroupRenderRepresentation::GaussianSplats
        })
        .filter_map(|object| {
            cache
                .get(&object.uuid)
                .filter(|entry| {
                    matches!(
                        (&entry.capture, object.render_representation),
                        (
                            CachedCapture::Sphere(_),
                            GroupRenderRepresentation::SphereAccelerator
                        ) | (
                            CachedCapture::Box(_),
                            GroupRenderRepresentation::BoxAccelerator
                                | GroupRenderRepresentation::GaussianSplats
                        )
                    )
                })
                .map(|entry| (object.uuid, entry.key))
        })
        .collect();
    let changed = context.data_mut(|data| {
        let id = egui::Id::new("depth-accelerator-ready");
        let changed = data.get_temp::<HashMap<uuid::Uuid, u64>>(id).as_ref() != Some(&keys);
        data.insert_temp(id, keys);
        changed
    });
    if changed {
        context.request_repaint();
    }
}

fn restored_capture(source: &[SdfObject], root: &SdfObject, key: u64) -> Option<CachedCapture> {
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

pub(super) fn objects_with_saved_captures(
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

pub(super) fn capture_key(source: &[SdfObject], root: &SdfObject) -> u64 {
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

#[cfg(test)]
pub(super) fn prepare_group_scene(
    source: &[SdfObject],
    cache: &mut HashMap<uuid::Uuid, CachedGroupCapture>,
) -> PreparedGroupScene {
    prepare_group_scene_with_neural(source, cache, &HashMap::new(), &HashSet::new())
}

#[cfg(test)]
pub(super) fn compute_group_scene(
    source: &[SdfObject],
    cache: &mut HashMap<uuid::Uuid, CachedGroupCapture>,
) -> PreparedGroupScene {
    let requests = source
        .iter()
        .filter(|object| object.render_representation == GroupRenderRepresentation::GaussianSplats)
        .map(|object| object.uuid)
        .collect();
    prepare_group_scene_with_neural(source, cache, &HashMap::new(), &requests)
}

#[cfg(test)]
pub(super) fn prepare_group_scene_with_neural(
    source: &[SdfObject],
    cache: &mut HashMap<uuid::Uuid, CachedGroupCapture>,
    ready_neural: &HashMap<uuid::Uuid, std::sync::Arc<super::neural_sdf::NeuralField>>,
    compute_requests: &HashSet<uuid::Uuid>,
) -> PreparedGroupScene {
    prepare_group_scene_with_budget(
        source,
        cache,
        ready_neural,
        compute_requests,
        MAX_BOX_DEPTH_TEXELS,
    )
}

pub(super) fn prepare_group_scene_with_budget(
    source: &[SdfObject],
    cache: &mut HashMap<uuid::Uuid, CachedGroupCapture>,
    ready_neural: &HashMap<uuid::Uuid, std::sync::Arc<super::neural_sdf::NeuralField>>,
    compute_requests: &HashSet<uuid::Uuid>,
    atlas_budget: usize,
) -> PreparedGroupScene {
    let mut neural_fields = HashMap::new();
    cache.retain(|id, _| source.iter().any(|object| object.uuid == *id));
    let parents: HashMap<_, _> = source
        .iter()
        .map(|object| (object.uuid, object.boolean_parent))
        .collect();
    let group_ids: HashSet<_> = source
        .iter()
        .filter_map(|object| object.boolean_parent)
        .collect();
    let mut proxies = HashMap::new();
    let mut box_depth_atlases = HashMap::new();
    let mut sphere_depth_atlases = HashMap::new();
    let mut gaussian_splats = HashSet::new();
    let mut atlas_texels = 0;
    for root in source {
        if root.render_representation == GroupRenderRepresentation::ExactSdf {
            continue;
        }
        let mut ancestor = root.boolean_parent;
        let mut captured_ancestor = false;
        for _ in 0..source.len() {
            let Some(id) = ancestor else { break };
            let Some(parent) = source.iter().find(|object| object.uuid == id) else {
                break;
            };
            if parent.render_representation.is_depth_accelerator()
                || ready_neural
                    .get(&id)
                    .is_some_and(|field| field.raymarch_last_segment)
            {
                captured_ancestor = true;
                break;
            }
            ancestor = parent.boolean_parent;
        }
        if captured_ancestor {
            continue;
        }
        let Some((minimum, maximum)) = crate::model::lattice_bounds(source, root.uuid) else {
            continue;
        };
        let center = (minimum + maximum) * 0.5;
        let half_extent = (maximum - minimum) * 0.5;
        if !center.is_finite() || !half_extent.is_finite() || half_extent.min_element() <= 0.0 {
            continue;
        }
        let mut proxy = root.clone();
        let key = capture_key(source, root);
        if !compute_requests.contains(&root.uuid)
            && !cache.get(&root.uuid).is_some_and(|entry| entry.key == key)
        {
            if let Some(capture) = restored_capture(source, root, key) {
                cache.insert(root.uuid, CachedGroupCapture { key, capture });
            }
        }
        match root.render_representation {
            GroupRenderRepresentation::BoxDepthAtlas
            | GroupRenderRepresentation::BoxAccelerator
            | GroupRenderRepresentation::GaussianSplats => {
                let gaussian =
                    root.render_representation == GroupRenderRepresentation::GaussianSplats;
                if gaussian && !root.gaussian_splats.is_valid() {
                    continue;
                }
                let resolution = if gaussian {
                    root.gaussian_splats.resolution
                } else {
                    BOX_DEPTH_RESOLUTION
                };
                let start = if gaussian {
                    BoxCaptureStart::OutsideBounds
                } else {
                    BoxCaptureStart::AtBounds
                };
                // Reject impossible uploads before allocating or marching any rays.
                let layers = if gaussian { 8 } else { 1 };
                let Some(samples) = box_capture_sample_count(resolution, layers) else {
                    continue;
                };
                let records_per_sample = if gaussian { 3 } else { 2 };
                if samples > atlas_budget.saturating_sub(atlas_texels) / records_per_sample {
                    continue;
                }
                let requested = compute_requests.contains(&root.uuid);
                let cached = cache
                    .get(&root.uuid)
                    .filter(|entry| entry.key == key && !requested);
                if gaussian && !requested && cached.is_none() {
                    continue;
                }
                let atlas =
                    if let Some(CachedCapture::Box(atlas)) = cached.map(|entry| &entry.capture) {
                        atlas.clone()
                    } else if let Some(atlas) =
                        bake_box_depth_atlas(source, root.uuid, resolution, start)
                    {
                        std::sync::Arc::new(atlas)
                    } else {
                        continue;
                    };
                let required_texels = if gaussian {
                    // Upload stores three vec4 records per capture sample, then
                    // two per node in a binary BVH of occupied samples. The
                    // old seven-record estimate rejected a second Gaussian
                    // group even when both actual payloads fit in the buffer.
                    let occupied = atlas.owners.iter().filter(|owner| owner.is_some()).count();
                    atlas.texels.len() * 3 + occupied.saturating_mul(2).saturating_sub(1) * 2
                } else {
                    atlas.texels.len() * 2
                        + if root.render_representation == GroupRenderRepresentation::BoxAccelerator
                        {
                            4
                        } else {
                            0
                        }
                };
                if atlas_texels + required_texels > atlas_budget {
                    continue;
                }
                atlas_texels += required_texels;
                cache.insert(
                    root.uuid,
                    CachedGroupCapture {
                        key,
                        capture: CachedCapture::Box(atlas.clone()),
                    },
                );
                let proxy_extent = if gaussian {
                    let support =
                        1.5 * (atlas.local_max - atlas.local_min).max_element() / resolution as f32;
                    atlas.local_max + Vec3::splat(support)
                } else {
                    atlas.local_max
                };
                box_depth_atlases.insert(root.uuid, atlas);
                if root.render_representation == GroupRenderRepresentation::BoxAccelerator {
                    continue;
                }
                proxy.object_type = PrimitiveKind::Box.object_type();
                proxy.params = SdfParams::BoxParams(BoxParams {
                    box_q: proxy_extent,
                    corner_radius: 0.0,
                });
                if gaussian {
                    gaussian_splats.insert(root.uuid);
                }
            }
            GroupRenderRepresentation::SphereDepthAtlas
            | GroupRenderRepresentation::SphereAccelerator => {
                let (width, height) = (SPHERE_DEPTH_WIDTH, SPHERE_DEPTH_HEIGHT);
                let cached = cache.get(&root.uuid).filter(|entry| entry.key == key);
                let atlas = if let Some(CachedCapture::Sphere(atlas)) =
                    cached.map(|entry| &entry.capture)
                {
                    atlas.clone()
                } else if let Some(atlas) =
                    bake_sphere_depth_atlas(source, root.uuid, width, height)
                {
                    std::sync::Arc::new(atlas)
                } else {
                    continue;
                };
                let required_texels = atlas.texels.len() * 2
                    + if root.render_representation == GroupRenderRepresentation::SphereAccelerator
                    {
                        4
                    } else {
                        0
                    };
                if atlas_texels + required_texels > atlas_budget {
                    continue;
                }
                atlas_texels += required_texels;
                cache.insert(
                    root.uuid,
                    CachedGroupCapture {
                        key,
                        capture: CachedCapture::Sphere(atlas.clone()),
                    },
                );
                proxy.object_type = PrimitiveKind::Sphere.object_type();
                proxy.params = SdfParams::SphereParams(SphereParams {
                    radius: atlas.radius,
                });
                sphere_depth_atlases.insert(root.uuid, atlas);
                if root.render_representation == GroupRenderRepresentation::SphereAccelerator {
                    // Keep the source subtree for exact finishing and shading.
                    continue;
                }
            }
            GroupRenderRepresentation::NeuralSdf => {
                let Some(field) = ready_neural.get(&root.uuid) else {
                    continue;
                };
                let records = field.network.payload_records()
                    + if field.raymarch_last_segment {
                        4 + 32 * 32 * 32
                    } else {
                        0
                    };
                if atlas_texels + records > atlas_budget {
                    continue;
                }
                atlas_texels += records;
                neural_fields.insert(root.uuid, field.clone());
                if field.raymarch_last_segment {
                    continue;
                }
                proxy.object_type = PrimitiveKind::Box.object_type();
                proxy.params = SdfParams::BoxParams(BoxParams {
                    box_q: Vec3::splat(field.half_extent),
                    corner_radius: 0.0,
                });
                proxy.repetition = Default::default();
                proxy.mirror = None;
                proxy.path_extrusion = None;
                proxy.surface_inlay = None;
            }
            GroupRenderRepresentation::ExactSdf => continue,
        }
        if group_ids.contains(&root.uuid) {
            proxy.transform = Transform {
                translation: center,
                ..Transform::default()
            };
        } else {
            proxy.transform.translation +=
                root.transform.rotation * (root.transform.scale * center);
        }
        proxy.image_stencil = None;
        if root.boolean_parent.is_none() {
            // Independent captures contain the deformed surface already.
            proxy.lattice = None;
        }
        proxies.insert(root.uuid, proxy);
    }

    let mut rendered = Vec::with_capacity(source.len());
    for object in source {
        let mut parent = object.boolean_parent;
        let mut hidden = false;
        for _ in 0..source.len() {
            let Some(id) = parent else { break };
            if proxies.contains_key(&id) {
                hidden = true;
                break;
            }
            parent = parents.get(&id).copied().flatten();
        }
        if hidden {
            continue;
        }
        rendered.push(
            proxies
                .get(&object.uuid)
                .cloned()
                .unwrap_or_else(|| object.clone()),
        );
    }
    // The admitted atlas payload bounds retained CPU captures too. Returning
    // a group to Exact frees its capture; selecting a capture bakes it automatically.
    cache.retain(|id, _| {
        box_depth_atlases.contains_key(id) || sphere_depth_atlases.contains_key(id)
    });
    PreparedGroupScene {
        objects: rendered,
        box_depth_atlases,
        sphere_depth_atlases,
        gaussian_splats,
        neural_fields,
    }
}

pub(super) fn visible_selection(
    source: &[SdfObject],
    rendered: &[SdfObject],
    selected: &[uuid::Uuid],
) -> HashSet<uuid::Uuid> {
    let visible_ids: HashSet<_> = rendered.iter().map(|object| object.uuid).collect();
    let parents: HashMap<_, _> = source
        .iter()
        .map(|object| (object.uuid, object.boolean_parent))
        .collect();
    let mut mapped = HashSet::new();
    for id in selected {
        if visible_ids.contains(id) {
            mapped.insert(*id);
            continue;
        }
        let mut parent = parents.get(id).copied().flatten();
        for _ in 0..source.len() {
            let Some(candidate) = parent else { break };
            if visible_ids.contains(&candidate) {
                mapped.insert(candidate);
                break;
            }
            parent = parents.get(&candidate).copied().flatten();
        }
    }
    mapped
}

#[cfg(test)]
mod saved_capture_tests {
    use super::*;

    #[test]
    fn splats_only_bake_on_request_and_edits_require_recompute() {
        let context = egui::Context::default();
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::GaussianSplats;
        root.gaussian_splats.resolution = 16;
        let id = root.uuid;
        let mut cache = HashMap::new();
        let idle = prepare_group_scene(&[root.clone()], &mut cache);
        assert!(idle.gaussian_splats.is_empty());
        assert!(cache.is_empty());
        let computed = compute_group_scene(&[root.clone()], &mut cache);
        let atlas = computed.box_depth_atlases[&id].clone();
        publish_depth_accelerator_status(&context, &[root.clone()], &cache);
        assert_eq!(
            depth_accelerator_status(&context, &[root.clone()], id, 0),
            DepthAcceleratorStatus::Ready
        );
        let recomputed = compute_group_scene(&[root.clone()], &mut cache);
        assert!(!std::sync::Arc::ptr_eq(
            &atlas,
            &recomputed.box_depth_atlases[&id]
        ));
        for resolution_edit in [false, true] {
            if resolution_edit {
                root.gaussian_splats.resolution = 17;
            } else {
                root.transform.scale *= 1.5;
            }
            let edited = prepare_group_scene(&[root.clone()], &mut cache);
            assert!(edited.gaussian_splats.is_empty());
            publish_depth_accelerator_status(&context, &[root.clone()], &cache);
            assert_eq!(
                depth_accelerator_status(
                    &context,
                    &[root.clone()],
                    id,
                    if resolution_edit { 2 } else { 1 }
                ),
                DepthAcceleratorStatus::NotComputedYet
            );
            let computed = compute_group_scene(&[root.clone()], &mut cache);
            assert!(computed.gaussian_splats.contains(&id));
        }
    }

    #[test]
    fn numeric_splat_resolution_exceeds_old_caps_and_rejects_impossible_uploads() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::GaussianSplats;
        root.gaussian_splats.resolution = 129;
        let mut cache = HashMap::new();
        let prepared = prepare_group_scene_with_budget(
            &[root.clone()],
            &mut cache,
            &HashMap::new(),
            &HashSet::from([root.uuid]),
            1 << 24,
        );
        assert_eq!(prepared.box_depth_atlases[&root.uuid].resolution, 129);
        root.gaussian_splats.resolution = u32::MAX;
        let prepared = prepare_group_scene_with_budget(
            &[root.clone()],
            &mut cache,
            &HashMap::new(),
            &HashSet::from([root.uuid]),
            1 << 24,
        );
        assert!(prepared.box_depth_atlases.is_empty());
        assert!(!prepared.gaussian_splats.contains(&root.uuid));
        assert_eq!(prepared.objects[0].object_type, root.object_type);
    }

    #[test]
    fn splat_resolution_rebuilds_capture_and_unchanged_resolution_reuses_it() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::GaussianSplats;
        root.gaussian_splats.resolution = 16;
        let mut cache = HashMap::new();
        let first = compute_group_scene(&[root.clone()], &mut cache);
        let atlas = first.box_depth_atlases[&root.uuid].clone();
        assert_eq!(atlas.resolution, 16);
        let reused = prepare_group_scene(&[root.clone()], &mut cache);
        assert!(std::sync::Arc::ptr_eq(
            &atlas,
            &reused.box_depth_atlases[&root.uuid]
        ));
        root.gaussian_splats.resolution = 32;
        let rebuilt = compute_group_scene(&[root.clone()], &mut cache);
        assert_eq!(rebuilt.box_depth_atlases[&root.uuid].resolution, 32);
        assert!(!std::sync::Arc::ptr_eq(
            &atlas,
            &rebuilt.box_depth_atlases[&root.uuid]
        ));
        let mut saved = [root.clone()];
        objects_with_saved_captures(&[root], &mut saved, &cache);
        let restored: Vec<SdfObject> =
            serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(restored[0].gaussian_splats.resolution, 32);
        let mut reopened_cache = HashMap::new();
        prepare_group_scene(&restored, &mut reopened_cache);
        match &restored[0].saved_group_capture.as_ref().unwrap().capture {
            CachedCapture::Box(saved_atlas) => match &reopened_cache[&restored[0].uuid].capture {
                CachedCapture::Box(reopened) => {
                    assert!(std::sync::Arc::ptr_eq(saved_atlas, reopened))
                }
                _ => panic!("wrong capture type"),
            },
            _ => panic!("wrong saved capture type"),
        }
    }

    #[test]
    fn depth_captures_compute_on_selection_and_source_edit() {
        for mode in [
            GroupRenderRepresentation::BoxDepthAtlas,
            GroupRenderRepresentation::SphereDepthAtlas,
            GroupRenderRepresentation::SphereAccelerator,
            GroupRenderRepresentation::BoxAccelerator,
        ] {
            let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
            root.render_representation = mode;
            let id = root.uuid;
            let mut source = vec![root];
            let mut cache = HashMap::new();
            let prepared = prepare_group_scene_with_neural(
                &source,
                &mut cache,
                &HashMap::new(),
                &HashSet::new(),
            );
            assert!(cache.contains_key(&id));
            assert_eq!(
                prepared.gaussian_splats.contains(&id),
                mode == GroupRenderRepresentation::GaussianSplats
            );
            let original_key = cache[&id].key;
            source[0].transform.scale *= 2.0;
            let prepared = prepare_group_scene_with_neural(
                &source,
                &mut cache,
                &HashMap::new(),
                &HashSet::new(),
            );
            assert!(cache.contains_key(&id));
            assert_ne!(cache[&id].key, original_key);
            assert_eq!(
                prepared.box_depth_atlases.contains_key(&id),
                matches!(
                    mode,
                    GroupRenderRepresentation::BoxDepthAtlas
                        | GroupRenderRepresentation::BoxAccelerator
                        | GroupRenderRepresentation::GaussianSplats
                )
            );
            assert_eq!(
                prepared.sphere_depth_atlases.contains_key(&id),
                matches!(
                    mode,
                    GroupRenderRepresentation::SphereDepthAtlas
                        | GroupRenderRepresentation::SphereAccelerator
                )
            );
        }
    }

    #[test]
    fn saved_depth_and_gaussian_captures_reopen_without_baking() {
        for mode in [
            GroupRenderRepresentation::BoxDepthAtlas,
            GroupRenderRepresentation::SphereDepthAtlas,
            GroupRenderRepresentation::SphereAccelerator,
            GroupRenderRepresentation::BoxAccelerator,
            GroupRenderRepresentation::GaussianSplats,
        ] {
            let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
            root.render_representation = mode;
            let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
            child.boolean_parent = Some(root.uuid);
            child.transform.translation.x = 0.2;
            child.color = glam::Vec4::new(0.1, 0.2, 0.9, 1.0);
            let id = root.uuid;
            let source = vec![root, child];
            let mut cache = HashMap::new();
            let original = compute_group_scene(&source, &mut cache);
            let mut objects = source.clone();
            objects_with_saved_captures(&source, &mut objects, &cache);
            assert!(objects[0].saved_group_capture.is_some());
            let mut tree = crate::model::DataTree::default();
            crate::model::set_objects(&mut tree, objects);
            let bytes = crate::document::serialize_scene(&tree).unwrap();
            let scene = crate::document::deserialize_scene(&bytes).unwrap();
            let mut reopened = crate::model::DataTree::default();
            reopened.set_tree("scene", scene);
            let loaded = crate::model::objects_ref(&reopened);
            let mut fresh_cache = HashMap::new();
            let prepared = prepare_group_scene(loaded, &mut fresh_cache);
            let saved = loaded[0].saved_group_capture.as_ref().unwrap();
            match &saved.capture {
                CachedCapture::Box(atlas) => {
                    assert!(std::sync::Arc::ptr_eq(
                        atlas,
                        &prepared.box_depth_atlases[&id]
                    ));
                    let before = &original.box_depth_atlases[&id];
                    assert_eq!(atlas.texels, before.texels);
                    assert_eq!(atlas.owners, before.owners);
                    assert_eq!(atlas.normals, before.normals);
                    assert_eq!(atlas.local_max, before.local_max);
                    assert_eq!(atlas.layers, before.layers);
                }
                CachedCapture::Sphere(atlas) => {
                    assert!(std::sync::Arc::ptr_eq(
                        atlas,
                        &prepared.sphere_depth_atlases[&id]
                    ));
                    let before = &original.sphere_depth_atlases[&id];
                    assert_eq!(atlas.texels, before.texels);
                    assert_eq!(atlas.owners, before.owners);
                    assert_eq!(atlas.radius, before.radius);
                }
            }
            assert_eq!(
                prepared.gaussian_splats.contains(&id),
                mode == GroupRenderRepresentation::GaussianSplats
            );
            assert_eq!(
                prepared.objects.len(),
                if mode.is_depth_accelerator() { 2 } else { 1 }
            );
            let mut edited = loaded.to_vec();
            edited[1].transform.scale *= 2.0;
            let key = capture_key(&edited, &edited[0]);
            assert!(restored_capture(&edited, &edited[0], key).is_none());
            let mut snapshot = edited.clone();
            objects_with_saved_captures(&edited, &mut snapshot, &fresh_cache);
            assert!(snapshot[0].saved_group_capture.is_none());
        }
    }

    #[test]
    fn invalid_saved_atlases_are_rejected() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::BoxDepthAtlas;
        let mut source = vec![root];
        let key = capture_key(&source, &source[0]);
        let original = bake_box_depth_atlas(
            &source,
            source[0].uuid,
            BOX_DEPTH_RESOLUTION,
            BoxCaptureStart::AtBounds,
        )
        .unwrap();
        for case in 0..7 {
            let mut atlas = original.clone();
            let mut version = 1;
            match case {
                0 => version = 2,
                1 => {
                    atlas.texels.pop();
                }
                2 => {
                    atlas.owners.pop();
                }
                3 => {
                    atlas.normals.pop();
                }
                4 => atlas.texels[0][0] = f32::NAN,
                5 => atlas.owners[0] = Some(uuid::Uuid::new_v4()),
                6 => atlas.local_max = Vec3::ZERO,
                _ => unreachable!(),
            }
            source[0].saved_group_capture =
                Some(std::sync::Arc::new(crate::model::SavedGroupCapture {
                    version,
                    source_key: key,
                    capture: CachedCapture::Box(std::sync::Arc::new(atlas)),
                }));
            assert!(
                restored_capture(&source, &source[0], key).is_none(),
                "case {case}"
            );
        }
    }
}

#[cfg(test)]
mod sphere_accelerator_tests {
    use super::*;

    #[test]
    fn accelerator_bakes_same_sphere_texture_and_retains_exact_subtree() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::SphereAccelerator;
        let mut child = SdfObject::create_kind(PrimitiveKind::Box);
        child.boolean_parent = Some(root.uuid);
        child.operation = crate::model::BooleanOperation::Subtract;
        child.transform.translation.x = 0.2;
        // A capture on a descendant must not replace the exact finish geometry.
        child.render_representation = GroupRenderRepresentation::BoxDepthAtlas;
        let id = root.uuid;
        let source = [root, child];
        let mut cache = HashMap::new();
        let prepared =
            prepare_group_scene_with_neural(&source, &mut cache, &HashMap::new(), &HashSet::new());
        assert_eq!(prepared.objects.len(), 2);
        let atlas = &prepared.sphere_depth_atlases[&id];
        let expected =
            bake_sphere_depth_atlas(&source, id, SPHERE_DEPTH_WIDTH, SPHERE_DEPTH_HEIGHT).unwrap();
        assert_eq!(atlas.texels, expected.texels);
        assert_eq!(atlas.owners, expected.owners);
        assert_eq!(prepared.objects.len(), 2);
        assert!(prepared.box_depth_atlases.is_empty());
        for (actual, original) in prepared.objects.iter().zip(&source) {
            assert_eq!(actual.object_type, original.object_type);
            assert_eq!(actual.boolean_parent, original.boolean_parent);
            assert_eq!(actual.operation, original.operation);
        }
        let mut edited = source.clone();
        edited[0].sphere_accelerator.configurable_epsilon = 0.2;
        let reused =
            prepare_group_scene_with_neural(&edited, &mut cache, &HashMap::new(), &HashSet::new());
        assert!(
            std::sync::Arc::ptr_eq(atlas, &reused.sphere_depth_atlases[&id]),
            "changing refinement distance must not rebake"
        );
    }
}

#[cfg(test)]
mod depth_accelerator_status_tests {
    use super::*;

    #[test]
    fn readiness_tracks_baking_source_edits_and_cache_restoration() {
        for mode in [
            GroupRenderRepresentation::SphereAccelerator,
            GroupRenderRepresentation::BoxAccelerator,
        ] {
            let context = egui::Context::default();
            let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
            root.render_representation = mode;
            let id = root.uuid;
            let mut source = vec![root];
            let mut cache = HashMap::new();
            let mut revision = 0;
            publish_depth_accelerator_status(&context, &source, &cache);
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::NotComputedYet
            );
            prepare_group_scene_with_neural(&source, &mut cache, &HashMap::new(), &HashSet::new());
            publish_depth_accelerator_status(&context, &source, &cache);
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::Ready
            );
            match mode {
                GroupRenderRepresentation::SphereAccelerator => {
                    source[0].sphere_accelerator.configurable_epsilon = 0.2
                }
                GroupRenderRepresentation::BoxAccelerator => {
                    source[0].box_accelerator.configurable_epsilon = 0.2
                }
                _ => unreachable!(),
            }
            revision += 1;
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::Ready
            );
            source[0].transform.scale *= 1.5;
            revision += 1;
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::NotComputedYet,
                "an edit must invalidate readiness before the next render"
            );
            prepare_group_scene_with_neural(&source, &mut cache, &HashMap::new(), &HashSet::new());
            publish_depth_accelerator_status(&context, &source, &cache);
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::Ready
            );
            let mut saved = source.clone();
            objects_with_saved_captures(&source, &mut saved, &cache);
            let mut restored = HashMap::new();
            prepare_group_scene_with_neural(
                &saved,
                &mut restored,
                &HashMap::new(),
                &HashSet::new(),
            );
            publish_depth_accelerator_status(&context, &saved, &restored);
            assert_eq!(
                depth_accelerator_status(&context, &saved, id, revision + 1),
                DepthAcceleratorStatus::Ready
            );
        }
    }
}

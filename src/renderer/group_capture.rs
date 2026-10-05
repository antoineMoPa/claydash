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
    pub(super) capture: CachedCapture,
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct CaptureBakeJob {
    pub root: uuid::Uuid,
    pub key: u64,
    pub source_revision: i32,
    pub progress: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pub receiver: std::sync::mpsc::Receiver<Option<CachedCapture>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for CaptureBakeJob {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn start_capture_bake(
    source: &[SdfObject],
    root: uuid::Uuid,
    key: u64,
    source_revision: i32,
) -> Option<CaptureBakeJob> {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    let object = source.iter().find(|object| object.uuid == root)?;
    let representation = object.render_representation;
    let resolution = object.gaussian_splats.resolution;
    let (sender, receiver) = std::sync::mpsc::channel();
    let progress = std::sync::Arc::new(AtomicU32::new(0));
    let worker_progress = progress.clone();
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let source = source.to_vec();
    std::thread::spawn(move || {
        let report = |percent: u32| {
            worker_progress.store(percent.min(99), Ordering::Relaxed);
            !worker_cancel.load(Ordering::Relaxed)
        };
        let capture = match representation {
            GroupRenderRepresentation::SphereDepthAtlas
            | GroupRenderRepresentation::SphereAccelerator => {
                bake_sphere_depth_atlas_with_progress(
                    &source,
                    root,
                    SPHERE_DEPTH_WIDTH,
                    SPHERE_DEPTH_HEIGHT,
                    report,
                )
                .map(|atlas| CachedCapture::Sphere(std::sync::Arc::new(atlas)))
            }
            GroupRenderRepresentation::BoxDepthAtlas
            | GroupRenderRepresentation::BoxAccelerator
            | GroupRenderRepresentation::GaussianSplats => {
                let gaussian = representation == GroupRenderRepresentation::GaussianSplats;
                let resolution = if gaussian {
                    resolution
                } else {
                    BOX_DEPTH_RESOLUTION
                };
                let start = if gaussian {
                    BoxCaptureStart::OutsideBounds
                } else {
                    BoxCaptureStart::AtBounds
                };
                bake_box_depth_atlas_with_progress(&source, root, resolution, start, report)
                    .map(|atlas| CachedCapture::Box(std::sync::Arc::new(atlas)))
            }
            _ => None,
        };
        let _ = sender.send(capture);
    });
    Some(CaptureBakeJob {
        root,
        key,
        source_revision,
        progress,
        receiver,
        cancel,
    })
}

mod cache;
pub(crate) use cache::{
    capture_key, depth_accelerator_status, objects_with_saved_captures,
    publish_depth_accelerator_status, restored_capture, DepthAcceleratorStatus,
};

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

#[cfg(test)]
pub(super) fn prepare_group_scene_with_budget(
    source: &[SdfObject],
    cache: &mut HashMap<uuid::Uuid, CachedGroupCapture>,
    ready_neural: &HashMap<uuid::Uuid, std::sync::Arc<super::neural_sdf::NeuralField>>,
    compute_requests: &HashSet<uuid::Uuid>,
    atlas_budget: usize,
) -> PreparedGroupScene {
    prepare_group_scene_with_pending(
        source,
        cache,
        ready_neural,
        compute_requests,
        atlas_budget,
        &HashSet::new(),
    )
}

pub(super) fn prepare_group_scene_with_pending(
    source: &[SdfObject],
    cache: &mut HashMap<uuid::Uuid, CachedGroupCapture>,
    ready_neural: &HashMap<uuid::Uuid, std::sync::Arc<super::neural_sdf::NeuralField>>,
    compute_requests: &HashSet<uuid::Uuid>,
    atlas_budget: usize,
    pending_bakes: &HashSet<uuid::Uuid>,
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
                    } else if pending_bakes.contains(&root.uuid) {
                        continue;
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
                } else if pending_bakes.contains(&root.uuid) {
                    continue;
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
mod tests;

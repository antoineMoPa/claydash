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
    pub gaussian_splats: HashSet<uuid::Uuid>,
}

pub(super) enum CachedCapture {
    Box(std::sync::Arc<BoxDepthAtlas>),
    Sphere(std::sync::Arc<SphereDepthAtlas>),
}

pub(super) struct CachedGroupCapture {
    key: u64,
    capture: CachedCapture,
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

fn capture_key(source: &[SdfObject], root: &SdfObject) -> u64 {
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
    let bytes = serde_json::to_vec(&(root.uuid, root.render_representation, objects))
        .expect("capture source is serializable");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&bytes, &mut hasher);
    std::hash::Hasher::finish(&hasher)
}

pub(super) fn prepare_group_scene(
    source: &[SdfObject],
    cache: &mut HashMap<uuid::Uuid, CachedGroupCapture>,
) -> PreparedGroupScene {
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
        match root.render_representation {
            GroupRenderRepresentation::BoxDepthAtlas
            | GroupRenderRepresentation::GaussianSplats => {
                let gaussian =
                    root.render_representation == GroupRenderRepresentation::GaussianSplats;
                let resolution = if gaussian {
                    GAUSSIAN_BOX_RESOLUTION
                } else {
                    BOX_DEPTH_RESOLUTION
                };
                let start = if gaussian {
                    BoxCaptureStart::OutsideBounds
                } else {
                    BoxCaptureStart::AtBounds
                };
                let cached = cache.get(&root.uuid).filter(|entry| entry.key == key);
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
                };
                if atlas_texels + required_texels > MAX_BOX_DEPTH_TEXELS {
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
                proxy.object_type = PrimitiveKind::Box.object_type();
                proxy.params = SdfParams::BoxParams(BoxParams {
                    box_q: proxy_extent,
                    corner_radius: 0.0,
                });
                if gaussian {
                    gaussian_splats.insert(root.uuid);
                }
            }
            GroupRenderRepresentation::SphereDepthAtlas => {
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
                let required_texels = atlas.texels.len() * 2;
                if atlas_texels + required_texels > MAX_BOX_DEPTH_TEXELS {
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
    // a group to Exact frees its capture; a later conversion bakes it again.
    cache.retain(|id, _| {
        box_depth_atlases.contains_key(id) || sphere_depth_atlases.contains_key(id)
    });
    PreparedGroupScene {
        objects: rendered,
        box_depth_atlases,
        sphere_depth_atlases,
        gaussian_splats,
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

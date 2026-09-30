use super::*;

pub(crate) fn selected_subtree_ids(
    scene: &[SdfObject],
    selection: &[uuid::Uuid],
) -> Vec<uuid::Uuid> {
    let mut ids = selection.to_vec();
    loop {
        let previous_len = ids.len();
        for object in scene {
            if object
                .boolean_parent
                .is_some_and(|parent| ids.contains(&parent))
                && !ids.contains(&object.uuid)
            {
                ids.push(object.uuid);
            }
        }
        if ids.len() == previous_len {
            return ids;
        }
    }
}

pub(crate) fn effective_selected_ids(tree: &DataTree) -> Vec<uuid::Uuid> {
    let selection = selected(tree);
    match crate::model::selection_scope(tree) {
        crate::model::SelectionScope::Group => selected_subtree_ids(&objects(tree), &selection),
        crate::model::SelectionScope::Exact => selection,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransformTargetKind {
    Object,
    Group,
    Camera,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TransformTarget {
    pub id: uuid::Uuid,
    pub kind: TransformTargetKind,
    pub transform: crate::model::Transform,
    pub parent_world: glam::Mat4,
    pub world: glam::Mat4,
}

pub(crate) fn selected_group_id(tree: &DataTree) -> Option<uuid::Uuid> {
    let selection = selected(tree);
    let id = *selection.first()?;
    (selection.len() == 1
        && crate::model::selection_scope(tree) == crate::model::SelectionScope::Group
        && crate::model::has_boolean_children(&objects(tree), id))
    .then_some(id)
}

pub(crate) fn transform_targets(tree: &DataTree) -> Vec<TransformTarget> {
    let scene = objects(tree);
    let selection = selected(tree);
    let group_scope = crate::model::selection_scope(tree) == crate::model::SelectionScope::Group;
    let mut targets: Vec<_> = selection
        .iter()
        .filter(|id| {
            if !group_scope {
                return true;
            }
            let mut parent = scene
                .iter()
                .find(|object| object.uuid == **id)
                .and_then(|object| object.boolean_parent);
            while let Some(ancestor) = parent {
                if selection.contains(&ancestor) {
                    return false;
                }
                parent = scene
                    .iter()
                    .find(|object| object.uuid == ancestor)
                    .and_then(|object| object.boolean_parent);
            }
            true
        })
        .filter_map(|id| {
            let object = scene.iter().find(|object| object.uuid == *id)?;
            let parent_world = crate::model::parent_group_world_matrix(&scene, *id);
            if group_scope && crate::model::has_boolean_children(&scene, *id) {
                Some(TransformTarget {
                    id: *id,
                    kind: TransformTargetKind::Group,
                    transform: object.group_transform,
                    parent_world,
                    world: crate::model::group_world_matrix(&scene, *id),
                })
            } else {
                let group_world = crate::model::group_world_matrix(&scene, *id);
                Some(TransformTarget {
                    id: *id,
                    kind: TransformTargetKind::Object,
                    transform: object.transform,
                    parent_world: group_world,
                    world: group_world * object.transform.matrix(),
                })
            }
        })
        .collect();
    for camera in crate::model::scene_cameras(tree) {
        if selection.contains(&camera.uuid) {
            targets.push(TransformTarget {
                id: camera.uuid,
                kind: TransformTargetKind::Camera,
                transform: camera.transform,
                parent_world: glam::Mat4::IDENTITY,
                world: camera.transform.matrix(),
            });
        }
    }
    targets
}

pub(crate) fn set_transform_target(
    scene: &mut [SdfObject],
    cameras: &mut [crate::camera::SceneCamera],
    target: TransformTargetKind,
    id: uuid::Uuid,
    transform: crate::model::Transform,
) -> bool {
    match target {
        TransformTargetKind::Object => {
            if let Some(object) = scene.iter_mut().find(|object| object.uuid == id) {
                if object.transform == transform {
                    return false;
                }
                object.transform = transform;
                return true;
            }
        }
        TransformTargetKind::Group => {
            if let Some(object) = scene.iter_mut().find(|object| object.uuid == id) {
                if object.group_transform == transform {
                    return false;
                }
                object.group_transform = transform;
                return true;
            }
        }
        TransformTargetKind::Camera => {
            if let Some(camera) = cameras.iter_mut().find(|camera| camera.uuid == id) {
                if camera.transform == transform {
                    return false;
                }
                camera.transform = transform;
                return true;
            }
        }
    }
    false
}

use super::*;

pub(crate) fn exit_camera_view(tree: &mut DataTree) {
    tree.set_transient_path(
        "editor.camera_view",
        crate::model::ClaydashValue::Bool(false),
    );
}

pub(super) fn toggle_camera_view(tree: &mut DataTree, viewport_camera: &Camera) {
    let mut cameras = crate::model::scene_cameras(tree);
    if cameras.is_empty() {
        let camera = crate::camera::SceneCamera::from_view("Camera 1", viewport_camera);
        tree.set_path(
            "scene.active_camera",
            crate::model::ClaydashValue::Uuid(camera.uuid),
        );
        crate::model::set_selected(tree, vec![camera.uuid]);
        cameras.push(camera);
        crate::model::set_scene_cameras(tree, cameras);
    }
    let enabled = !matches!(
        tree.get_path("editor.camera_view"),
        crate::model::ClaydashValue::Bool(true)
    );
    tree.set_transient_path(
        "editor.camera_view",
        crate::model::ClaydashValue::Bool(enabled),
    );
}

pub(super) fn sync_camera_view(tree: &DataTree, viewport_camera: &mut Camera) {
    if !matches!(
        tree.get_path("editor.camera_view"),
        crate::model::ClaydashValue::Bool(true)
    ) {
        return;
    }
    let Some(active) = crate::model::active_camera_id(tree) else {
        return;
    };
    if let Some(camera) = crate::model::scene_cameras(tree)
        .iter()
        .find(|camera| camera.uuid == active)
    {
        camera.apply_to_view(viewport_camera);
    }
}

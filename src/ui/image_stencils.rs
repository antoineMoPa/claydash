use super::*;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static PENDING_IMAGE: std::cell::RefCell<Option<(uuid::Uuid, String, Vec<u8>)>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn request_image(ctx: &egui::Context, target: uuid::Uuid) -> Option<(String, Vec<u8>)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (ctx, target);
        let path = rfd::FileDialog::new()
            .add_filter("Image", &["png", "jpg", "jpeg", "webp"])
            .pick_file()?;
        let bytes = std::fs::read(&path).ok()?;
        if bytes.len() > 16 * 1024 * 1024 || image::load_from_memory(&bytes).is_err() {
            return None;
        }
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some((name, bytes))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("Image", &["png", "jpg", "jpeg", "webp"])
                .pick_file()
                .await
            {
                let name = file.file_name();
                let bytes = file.read().await;
                if bytes.len() <= 16 * 1024 * 1024 && image::load_from_memory(&bytes).is_ok() {
                    PENDING_IMAGE
                        .with(|pending| *pending.borrow_mut() = Some((target, name, bytes)));
                    ctx.request_repaint();
                }
            }
        });
        None
    }
}

pub(super) fn apply_image(tree: &mut DataTree, id: uuid::Uuid, name: String, bytes: Vec<u8>) {
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == id) else {
        return;
    };
    let mut replacement = crate::model::ImageStencil::new(name, bytes, false);
    if let Some(previous) = &object.image_stencil {
        replacement.offset = previous.offset;
        replacement.rotation = previous.rotation;
        replacement.both_sides = previous.both_sides;
        replacement.image_plane = previous.image_plane;
        if !previous.image.is_empty() {
            replacement.size = previous.size;
        }
    }
    if replacement.image_plane {
        if let SdfParams::BoxParams(box_params) = &mut object.params {
            box_params.box_q.x = replacement.size.x * 0.5;
            box_params.box_q.y = replacement.size.y * 0.5;
        }
    }
    object.image_stencil = Some(replacement);
    set_objects(tree, scene);
    set_selected(tree, vec![id]);
    tree.make_undo_redo_snapshot();
}

pub(super) fn apply_pending_image(_ctx: &egui::Context, _tree: &mut DataTree) {
    #[cfg(target_arch = "wasm32")]
    if let Some((id, name, bytes)) = PENDING_IMAGE.with(|pending| pending.borrow_mut().take()) {
        apply_image(_tree, id, name, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_plane_is_available_through_shared_command_registry() {
        let mut commands = Commands::new();
        commands::register_all(&mut commands);
        assert!(commands.commands.contains_key("add-image-plane"));
        let mut tree = DataTree::default();
        commands::execute(&commands, "add-image-plane", &mut tree);
        let plane = objects(&tree);
        assert_eq!(plane.len(), 1);
        assert!(plane[0].image_stencil.as_ref().unwrap().image_plane);
        assert_eq!(selected(&tree), vec![plane[0].uuid]);
    }

    #[test]
    fn image_plane_and_projected_stencil_survive_scene_save() {
        let mut tree = DataTree::default();
        let plane_id = commands::create_image_plane(&mut tree);
        apply_image(&mut tree, plane_id, "plane.png".into(), vec![1, 2, 3]);
        let plane = objects(&tree).remove(0);
        assert!(plane.image_stencil.as_ref().unwrap().image_plane);
        assert!(matches!(plane.params, SdfParams::BoxParams(_)));
        let bytes = crate::document::serialize_scene(&tree).unwrap();
        let scene = crate::document::deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);
        assert_eq!(
            objects(&restored)[0].image_stencil.as_ref().unwrap().image,
            vec![1, 2, 3]
        );
        let sphere = SdfObject::create_kind(PrimitiveKind::Sphere);
        let id = sphere.uuid;
        let mut scene = objects(&tree);
        scene.push(sphere);
        set_objects(&mut tree, scene);
        apply_image(&mut tree, id, "decal.png".into(), vec![4]);
        assert!(
            !objects(&tree)[1]
                .image_stencil
                .as_ref()
                .unwrap()
                .image_plane
        );
    }
}

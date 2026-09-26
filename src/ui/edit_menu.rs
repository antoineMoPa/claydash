use super::*;
use serde::{Deserialize, Serialize};

const CLIPBOARD_KIND: &str = "claydash-selected-objects-v1";

#[cfg(target_arch = "wasm32")]
thread_local! {
    static PENDING_PASTE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[derive(Serialize, Deserialize)]
struct ObjectClipboard {
    kind: String,
    objects: Vec<SdfObject>,
    cameras: Vec<crate::camera::SceneCamera>,
}

fn serialize_selection(tree: &DataTree) -> Option<String> {
    let ids = commands::effective_selected_ids(tree);
    let objects = objects(tree)
        .into_iter()
        .filter(|object| ids.contains(&object.uuid))
        .collect();
    let cameras = crate::model::scene_cameras(tree)
        .into_iter()
        .filter(|camera| ids.contains(&camera.uuid))
        .collect();
    let payload = ObjectClipboard {
        kind: CLIPBOARD_KIND.into(),
        objects,
        cameras,
    };
    if payload.objects.is_empty() && payload.cameras.is_empty() {
        return None;
    }
    serde_json::to_string(&payload).ok()
}

fn paste_selection(tree: &mut DataTree, text: &str) -> bool {
    let Ok(mut payload) = serde_json::from_str::<ObjectClipboard>(text) else {
        return false;
    };
    if payload.kind != CLIPBOARD_KIND
        || payload.objects.len() + payload.cameras.len() > 4096
        || payload.objects.is_empty() && payload.cameras.is_empty()
    {
        return false;
    }
    let ids: std::collections::HashMap<_, _> = payload
        .objects
        .iter()
        .map(|object| object.uuid)
        .chain(payload.cameras.iter().map(|camera| camera.uuid))
        .map(|id| (id, uuid::Uuid::new_v4()))
        .collect();
    let mut pasted = Vec::new();
    for object in &mut payload.objects {
        object.uuid = ids[&object.uuid];
        object.boolean_parent = object.boolean_parent.and_then(|id| ids.get(&id).copied());
        if object.boolean_parent.is_none() {
            object.operation = BooleanOperation::Union;
            object.transform.translation.x += 0.2;
        }
        if let Some(material_id) = object.material_id {
            let matching = crate::model::material_assets(tree)
                .iter()
                .any(|asset| asset.uuid == material_id && asset.material == object.material);
            if !matching {
                object.material_id =
                    Some(crate::model::ensure_material_asset(tree, object.material));
            }
        }
        pasted.push(object.uuid);
    }
    for camera in &mut payload.cameras {
        camera.uuid = ids[&camera.uuid];
        camera.transform.translation.x += 0.2;
        pasted.push(camera.uuid);
    }
    let mut scene = objects(tree);
    scene.extend(payload.objects);
    set_objects(tree, scene);
    let mut cameras = crate::model::scene_cameras(tree);
    cameras.extend(payload.cameras);
    crate::model::set_scene_cameras(tree, cameras);
    set_selected(tree, pasted);
    tree.make_undo_redo_snapshot();
    commands::start_grab(tree);
    true
}

pub(super) fn draw_edit_menu(ui: &mut egui::Ui, tree: &mut DataTree, file_error_open: bool) {
    let mut copy = false;
    let mut paste = false;
    ui.menu_button("Edit", |ui| {
        copy = ui
            .add_enabled(
                !selected(tree).is_empty(),
                egui::Button::new("Copy").shortcut_text("Cmd/Ctrl+C"),
            )
            .clicked();
        if copy {
            ui.close();
        }
        paste = ui
            .add(egui::Button::new("Paste").shortcut_text("Cmd/Ctrl+V"))
            .clicked();
        if paste {
            ui.close();
        }
    });
    let ctx = ui.ctx();
    if !ctx.egui_wants_keyboard_input() && !file_error_open {
        let copy_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::C);
        let paste_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::V);
        ctx.input_mut(|input| {
            copy |= input.consume_shortcut(&copy_shortcut);
            paste |= input.consume_shortcut(&paste_shortcut);
            copy |= input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy));
        });
    }
    if copy {
        if let Some(text) = serialize_selection(tree) {
            #[cfg(all(not(target_arch = "wasm32"), not(test)))]
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(text.clone());
            }
            #[cfg(target_arch = "wasm32")]
            if let Some(window) = web_sys::window() {
                let _ = window.navigator().clipboard().write_text(&text);
            }
            ctx.copy_text(text);
        }
    }
    let paste_event = if !ctx.egui_wants_keyboard_input() && !file_error_open {
        ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        })
    } else {
        None
    };
    if let Some(text) = paste_event.as_deref() {
        paste_selection(tree, text);
    }
    if paste && paste_event.is_none() {
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            if let Ok(text) = clipboard.get_text() {
                paste_selection(tree, &text);
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(window) = web_sys::window() {
                    if let Ok(value) = wasm_bindgen_futures::JsFuture::from(
                        window.navigator().clipboard().read_text(),
                    )
                    .await
                    {
                        if let Some(text) = value.as_string() {
                            PENDING_PASTE.with(|pending| *pending.borrow_mut() = Some(text));
                            ctx.request_repaint();
                        }
                    }
                }
            });
        }
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(text) = PENDING_PASTE.with(|pending| pending.borrow_mut().take()) {
        paste_selection(tree, &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_objects_round_trip_with_new_ids_and_internal_parent() {
        let mut tree = DataTree::default();
        let root = SdfObject::create_kind(PrimitiveKind::Box);
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        set_objects(&mut tree, vec![root.clone(), child.clone()]);
        set_selected(&mut tree, vec![root.uuid]);
        let text = serialize_selection(&tree).unwrap();
        assert!(paste_selection(&mut tree, &text));
        let scene = objects(&tree);
        assert_eq!(scene.len(), 4);
        assert_ne!(scene[2].uuid, root.uuid);
        assert_eq!(scene[3].boolean_parent, Some(scene[2].uuid));
        assert_eq!(selected(&tree), vec![scene[2].uuid, scene[3].uuid]);
        assert!(matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(crate::model::EditorState::Grabbing)
        ));
    }

    #[test]
    fn unrelated_clipboard_text_is_ignored() {
        let mut tree = DataTree::default();
        assert!(!paste_selection(&mut tree, "hello"));
        assert!(objects(&tree).is_empty());
    }

    #[test]
    fn native_copy_event_copies_selected_objects() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Box);
        set_objects(&mut tree, vec![object.clone()]);
        set_selected(&mut tree, vec![object.uuid]);
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Copy],
                ..Default::default()
            },
            |ui| draw_edit_menu(ui, &mut tree, false),
        );
        let copied = output
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text),
                _ => None,
            })
            .expect("copy text output");
        let payload: ObjectClipboard = serde_json::from_str(copied).unwrap();
        assert_eq!(payload.objects.len(), 1);
        assert_eq!(payload.objects[0].uuid, object.uuid);
        output.textures_delta.clear();
    }
}

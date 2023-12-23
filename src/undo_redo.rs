use bevy::prelude::*;
use claydash_data::{ClaydashData, ClaydashValue};
use observable_key_value_tree::ObservableKVTree;
use command_central::CommandBuilder;
use bevy_command_central_plugin::CommandCentralState;

pub struct ClaydashUndoRedoPlugin;

impl Plugin for ClaydashUndoRedoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClaydashData>()
            .add_systems(Startup, setup_undo_redo_commands);
    }
}

fn setup_undo_redo_commands(mut bevy_command_central: ResMut<CommandCentralState>) {
    let commands = &mut bevy_command_central.commands;

    CommandBuilder::new()
        .title("Undo")
        .system_name("undo")
        .docs("Undo last action.")
        .shortcut("Shift+Z")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(undo)))
        .write(commands);

    CommandBuilder::new()
        .title("Redo")
        .system_name("redo")
        .docs("Redo last action.")
        .shortcut("Shift+Y")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(redo)))
        .write(commands);
}

fn undo(
    tree: &mut ObservableKVTree<ClaydashValue>
) {
    let versions = tree.get_path("editor.versions").unwrap_vec_i32_or(vec!());
    let mut current_version_index = tree.get_path("editor.current_version_index").unwrap_i32_or(0);

    if current_version_index == 0 {
        // nothing to undo
        return;
    }

    if versions.len() == 0 {
        // nothing to undo
        return;
    }

    current_version_index -= 1;

    let version = versions[current_version_index as usize];

    tree.go_to_snapshot_with_version(version);

    // Make sure we keep same versions array after moving to a snapshot
    tree.set_path_without_notifying("editor.versions", ClaydashValue::VecI32(versions));
    tree.set_path_without_notifying("editor.current_version_index", ClaydashValue::I32(current_version_index));


    dump_undo_state(tree);
}

fn redo(
    tree: &mut ObservableKVTree<ClaydashValue>
) {
    let versions = tree.get_path("editor.versions").unwrap_vec_i32_or(vec!());
    let mut current_version_index = tree.get_path("editor.current_version_index").unwrap_i32_or(0);

    if current_version_index == versions.len() as i32 - 1 {
        // nothing to redo
        return;
    }

    if versions.len() == 0 {
        // nothing to redo
        return;
    }

    current_version_index += 1;

    let version = versions[current_version_index as usize];

    tree.go_to_snapshot_with_version(version);

    // Make sure we keep same versions array after moving to a snapshot
    tree.set_path_without_notifying("editor.versions", ClaydashValue::VecI32(versions));
    tree.set_path_without_notifying("editor.current_version_index", ClaydashValue::I32(current_version_index));

    dump_undo_state(tree);
}

pub fn dump_undo_state(tree: &mut ObservableKVTree<ClaydashValue>) {
    let versions = tree.get_path("editor.versions").unwrap_vec_i32_or(vec!());
    let current_version_index = tree.get_path("editor.current_version_index").unwrap_i32_or(0);

    for (index, version) in versions.iter().enumerate() {
        let arrow =  if index == current_version_index as usize  { " <-" }  else { "" };
        println!("{} {}", version, arrow);
    }
}

pub fn make_undo_redo_snapshot(tree: &mut ObservableKVTree<ClaydashValue>) {
    let previous_versions = tree.get_path("editor.versions");
    let version = tree.make_snapshot();

    let versions: Vec<i32> = match previous_versions {
        ClaydashValue::VecI32(versions) => { versions },
        _ => {
            vec!(version)
        }
    };

    // Slice, since after an action, we can't redo.
    let current_version_index = tree.get_path("editor.current_version_index").unwrap_i32_or(versions.len() as i32 - 1);

    let mut new_versions = versions[0..(current_version_index as usize + 1)].to_vec();

    new_versions.push(version);

    tree.set_path_without_notifying("editor.versions", ClaydashValue::VecI32(
        new_versions.clone()
    ));

    tree.set_path_without_notifying("editor.current_version_index", ClaydashValue::I32(new_versions.len() as i32 - 1));

    dump_undo_state(tree);
}

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
    let operations = tree.get_path("scene.operations");
    // This is a number where:
    //  - 0 = we are at the last edit
    //  - -1 = we just did undo
    //  - -2 = we did 2 undos
    let mut undo_redo_pointer = tree.get_path("scene.undo_pointer").unwrap_i32_or(0);
    match operations {
        ClaydashValue::VecI32(versions) => {
            undo_redo_pointer -= 1;
            let end = versions.len() as i32 - 1;
            tree.revert_snapshot_version(end - undo_redo_pointer);
            tree.set_path("scene.undo_pointer", ClaydashValue::I32(undo_redo_pointer));
        }
        _ => { }
    }
}

fn redo(
    tree: &mut ObservableKVTree<ClaydashValue>
) {
    let mut undo_redo_pointer = tree.get_path("scene.undo_pointer").unwrap_i32_or(0);
    let operations = tree.get_path("scene.operations");

    match operations {
        ClaydashValue::VecI32(versions) => {
            undo_redo_pointer -= 1;
            let end = versions.len() as i32 - 1;
            tree.revert_snapshot_version(end - undo_redo_pointer);
            tree.set_path("scene.undo_pointer", ClaydashValue::I32(undo_redo_pointer));
        }
        _ => { }
    }
}

pub fn make_undo_redo_snapshot(tree: &mut ObservableKVTree<ClaydashValue>) {
    let previous_operations = tree.get_path("scene.operations");
    let version = tree.make_snapshot();

    let versions: Vec<i32> = match previous_operations {
        ClaydashValue::VecI32(versions) => {
            let mut versions: Vec<i32> = versions.clone();
            versions.push(version);
            versions
        },
        _ => {
            vec!(version)
        }
    };

    tree.set_path_without_notifying("scene.operations", ClaydashValue::VecI32(versions));
}

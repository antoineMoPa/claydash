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
}

fn undo(
    tree: &mut ObservableKVTree<ClaydashValue>
) {
    let previous_operations = tree.get_path("scene.operation");

    match previous_operations {
        ClaydashValue::VecI32(versions) => {
            match versions.last() {
                Some(last) => {
                    tree.revert_snapshot(*last);
                    let versions = &versions[0..versions.len()-1];
                    tree.set_path_without_notifying("scene.operation", ClaydashValue::VecI32(versions.to_owned()));
                },
                _ => {}
            }
        }
        _ => { }
    }
}

pub fn make_undo_redo_snapshot(tree: &mut ObservableKVTree<ClaydashValue>) {
    let previous_operations = tree.get_path("scene.operation");
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

    tree.set_path_without_notifying("scene.operation", ClaydashValue::VecI32(versions));
}

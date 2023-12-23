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
    tree.undo();
    tree.dump_undo_state();
}

fn redo(
    tree: &mut ObservableKVTree<ClaydashValue>
) {
    tree.redo();
    tree.dump_undo_state();
}

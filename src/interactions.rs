use bevy::prelude::*;
use command_central::CommandBuilder;
use bevy_command_central_plugin::CommandCentralState;
use claydash_data::{ClaydashData, ClaydashValue};
use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker
};
use claydash_data::EditorState::*;

pub struct ClaydashInteractionPlugin;

impl Plugin for ClaydashInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClaydashData>()
            .add_systems(Startup, register_interaction_commands)
            .add_systems(Update, update_transformations)
            .add_systems(Update, run_shortcut_commands);
    }
}

pub fn register_interaction_commands(mut bevy_command_central: ResMut<CommandCentralState>) {
    let commands = &mut bevy_command_central.commands;
    CommandBuilder::new()
        .title("Grab")
        .system_name("grab")
        .docs("Start moving selection.")
        .shortcut("G")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(start_grab)))
        .write(commands);

    CommandBuilder::new()
        .title("Scale")
        .system_name("scale")
        .docs("Start scaling selection.")
        .shortcut("S")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(start_scale)))
        .write(commands);

    CommandBuilder::new()
        .title("Quit")
        .system_name("quit")
        .docs("Quit and cancel current editing state.")
        .shortcut("Escape")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(escape)))
        .write(commands);

    CommandBuilder::new()
        .title("Finish")
        .system_name("finish")
        .docs("Finish and apply current editing state.")
        .shortcut("Return")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(finish)))
        .write(commands);
}

fn start_grab(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.state", ClaydashValue::EditorState(Grabbing));
}

fn start_scale(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.state", ClaydashValue::EditorState(Scaling));
}

fn escape(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.state", ClaydashValue::EditorState(Scaling));
}

fn finish(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.state", ClaydashValue::EditorState(Scaling));
}

fn str_to_key(key_str: &String) -> KeyCode {
    return match key_str.as_str() {
        "G" => KeyCode::G,
        "S" => KeyCode::S,
        "Escape" => KeyCode::Escape,
        "Return" => KeyCode::Return,
        _ => {
            panic!("str not mapped to a keycode {}", key_str)
        }
    };
}

pub fn run_shortcut_commands(
    mut bevy_command_central: ResMut<CommandCentralState>,
    mut data_resource: ResMut<ClaydashData>,
    keys: Res<Input<KeyCode>>
){
    let commands = &mut bevy_command_central.commands.commands;
    let tree = &mut data_resource.as_mut().tree;

    for (_key, command) in commands.iter() {
        if command.shortcut.is_empty() {
            continue;
        }
        if keys.just_released(str_to_key(&command.shortcut)) {
            match command.parameters["callback"].value.clone().unwrap() {
                ClaydashValue::Fn(callback) => callback(tree),
                _ => {}
            };
        }
    }
}


fn update_transformations(
    mut data_resource: ResMut<ClaydashData>,
) {
    let tree = &mut data_resource.as_mut().tree;

    match tree.get_path("editor.state").unwrap_or(ClaydashValue::EditorState(Start)) {
        ClaydashValue::EditorState(Grabbing) => {
            println!("TODO: grab");
        }
        _ => {}
    }
}

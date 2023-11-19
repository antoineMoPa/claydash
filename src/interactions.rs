use bevy::prelude::*;
use command_central::CommandBuilder;
use bevy_command_central_plugin::CommandCentralState;
use claydash_data::{ClaydashData, ClaydashValue};
use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker
};

pub fn register_interaction_commands(mut bevy_command_central: ResMut<CommandCentralState>) {
    let commands = &mut bevy_command_central.commands;
    CommandBuilder::new()
        .title("Grab")
        .system_name("grab")
        .docs("Start moving selection")
        .shortcut("g")
        .insert_param("start_position", "Initial mouse position.", Some(ClaydashValue::Vec3(Vec3::ZERO)))
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(start_grab)))
        .write(commands);
}

fn start_grab(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    println!("start grab")
}

fn str_to_key(key_str: &String) -> KeyCode {
    if key_str.to_uppercase() == "G" {
        return KeyCode::G;
    }
    panic!("str not mapped to a keycode {}", key_str);
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

use bevy::{
    prelude::*,
    input::keyboard::KeyCode, ecs::system::SystemState
};
use claydash_data::{ClaydashValue, ClaydashData};
use bevy_command_central_plugin::CommandCentralState;
use observable_key_value_tree::{
    ObservableKVTree,
};
use bevy_sdf_object::SDFObject;
use command_central::CommandBuilder;
use claydash_data::EditorState::*;
use sdf_consts::TYPE_BOX;

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
        .title("Constrain editing to X axis")
        .system_name("constrain_x")
        .docs("Add a X constraint to current editing mode.")
        .shortcut("X")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(constrain_x)))
        .write(commands);

    CommandBuilder::new()
        .title("Constrain editing to Y axis")
        .system_name("constrain_y")
        .docs("Add a Y constraint to current editing mode.")
        .shortcut("Y")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(constrain_y)))
        .write(commands);

    CommandBuilder::new()
        .title("Constrain editing to Z axis")
        .system_name("constrain_z")
        .docs("Add a Z constraint to current editing mode.")
        .shortcut("Z")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(constrain_z)))
        .write(commands);

    CommandBuilder::new()
        .title("Scale")
        .system_name("scale")
        .docs("Start scaling selection.")
        .shortcut("S")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(start_scale)))
        .write(commands);

    CommandBuilder::new()
        .title("Rotate")
        .system_name("rotate")
        .docs("Start rotating selection.")
        .shortcut("R")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(start_rotate)))
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

    CommandBuilder::new()
        .title("Delete")
        .system_name("delete")
        .docs("Delete/Remove selection.")
        .shortcut("Back")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(delete)))
        .write(commands);

    CommandBuilder::new()
        .title("Select all/none")
        .system_name("select_all_or_none")
        .docs("Toggle selecting all objects.")
        .shortcut("Shift+A")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(select_all_or_none)))
        .write(commands);

    CommandBuilder::new()
        .title("Duplicate")
        .system_name("duplicate")
        .docs("Duplicate selection.")
        .shortcut("Shift+D")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(duplicate)))
        .write(commands);

    CommandBuilder::new()
        .title("Spawn Sphere")
        .system_name("spawn-sphere")
        .docs("Add a sphere at the given position")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(spawn_sphere)))
        .write(commands);

    CommandBuilder::new()
        .title("Spawn Box")
        .system_name("spawn-box")
        .docs("Adds a cube at the given position")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(spawn_box)))
        .write(commands);
}

fn set_objects_initial_properties(
    tree: &mut  ObservableKVTree<ClaydashValue>
) {
    let mut objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(data) => data,
        _ => { return; }
    };

    let selected_object_uuids = tree.get_path("scene.selected_uuids").unwrap_vec_uuid_or(Vec::new());

    let mut selected_object_sum_position: Vec3 = Vec3::ZERO;
    let mut selected_object_count: i32 = 0;

    // Find center of all selected objects
    // It will be the reference point when transforming objects.
    for object in objects.iter_mut() {
        if selected_object_uuids.contains(&object.uuid) {
            selected_object_sum_position += object.transform.translation;
            selected_object_count += 1;
        }
    }
    let mut initial_selection_transform = Transform::IDENTITY;
    initial_selection_transform.translation = selected_object_sum_position / (selected_object_count as f32);
    tree.set_path("editor.initial_selection_transform", ClaydashValue::Transform(initial_selection_transform));

    tree.set_path("editor.initial_radius", ClaydashValue::F32(0.3));

    // Find position of all objects relative to that center
    for object in objects.iter_mut() {
        if selected_object_uuids.contains(&object.uuid) {
            let mut transform_relative_to_center = object.transform;
            transform_relative_to_center.translation -= initial_selection_transform.translation;
            tree.set_path(&format!("editor.initial_transform.{}", object.uuid), ClaydashValue::Transform(object.transform));
            tree.set_path(&format!("editor.initial_transform_relative_to_selection.{}", object.uuid), ClaydashValue::Transform(transform_relative_to_center));
        }
    }
}

pub fn run_shortcut_commands(
    world: &mut World,
){
    let mut system_state: SystemState<(
        ResMut<CommandCentralState>,
        ResMut<ClaydashData>,
        Query<&Window>,
        Res<Input<KeyCode>>
    )> = SystemState::new(world);

    let (mut bevy_command_central,
         mut data_resource,
         windows,
         keys) = system_state.get_mut(world);


    let commands = &mut bevy_command_central.commands.commands;
    let tree = &mut data_resource.as_mut().tree;
    let mut shortcut_sequence: String = String::new();
    for key in keys.get_just_pressed() {
        // Modifiers are not part of sequence themselves
        match key {
            KeyCode::ShiftLeft => { return }
            KeyCode::SuperLeft => { return },
            KeyCode::ControlLeft => { return },
            _ => {}
        }
        let keyname = key_to_name(key);
        // Mac command button is equivalent to shift in our system.
        let has_shift = keys.any_pressed(vec!(KeyCode::ShiftLeft, KeyCode::SuperLeft));
        let has_control = keys.pressed(KeyCode::ControlLeft);

        let modifiers = match (has_control, has_shift) {
            (true, true) => { "Ctrl+Shift+" },
            (true, false) => { "Ctrl+" },
            (false, true) => { "Shift+" },
            _ => { "" }
        };

        let combo_name = format!("{}{}", modifiers, keyname);

        shortcut_sequence += &combo_name;
    }

    for (_key, command) in commands.iter() {
        if command.shortcut.is_empty() {
            continue;
        }
        if shortcut_sequence == command.shortcut {
            let window = windows.single();
            tree.set_path(
                "editor.initial_mouse_position",
                ClaydashValue::Vec2(window.cursor_position().unwrap_or(Vec2::ZERO))
            );
            match command.parameters["callback"].value.clone().unwrap() {
                ClaydashValue::Fn(callback) => callback(tree),
                _ => {}
            };
        }
    }
}

fn key_to_name(key: &KeyCode) -> String {
    return match key {
        KeyCode::A => "A",
        KeyCode::B => "B",
        KeyCode::C => "C",
        KeyCode::D => "D",
        KeyCode::E => "E",
        KeyCode::F => "F",
        KeyCode::G => "G",
        KeyCode::H => "H",
        KeyCode::I => "I",
        KeyCode::J => "J",
        KeyCode::K => "K",
        KeyCode::L => "L",
        KeyCode::M => "M",
        KeyCode::N => "N",
        KeyCode::O => "O",
        KeyCode::P => "P",
        KeyCode::Q => "Q",
        KeyCode::R => "R",
        KeyCode::S => "S",
        KeyCode::T => "T",
        KeyCode::U => "U",
        KeyCode::V => "V",
        KeyCode::W => "W",
        KeyCode::X => "X",
        KeyCode::Y => "Y",
        KeyCode::Z => "Z",
        KeyCode::Escape => "Escape",
        KeyCode::Return => "Return",
        KeyCode::Back => "Back",
        KeyCode::ShiftLeft => "Shift",
        // Mac command button is equivalent to shift in our system.
        // Shift+A == Command+A
        KeyCode::SuperLeft => "Shift",
        KeyCode::ControlLeft => "Ctrl",
        _ => {
            println!("note: last typed keycode not mapped to key.");
            ""
        }
    }.to_string();
}


fn reset_constraints(tree: &mut ObservableKVTree<ClaydashValue>) {
    tree.set_path("editor.constrain_x", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_y", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_z", ClaydashValue::Bool(false));
}

fn start_grab(tree: &mut ObservableKVTree<ClaydashValue>) {
    reset_constraints(tree);
    set_objects_initial_properties(tree);
    tree.set_path("editor.state", ClaydashValue::EditorState(Grabbing));
}

fn toggle_path(tree: &mut ObservableKVTree<ClaydashValue>, path: String) {
    let current_value = tree.get_path("editor.constrain_x").unwrap_bool_or(false);
    tree.set_path(&path, ClaydashValue::Bool(!current_value));
}

fn constrain_x(tree: &mut ObservableKVTree<ClaydashValue>) {
    toggle_path(tree, "editor.constrain_x".to_string());
}

fn constrain_y(tree: &mut ObservableKVTree<ClaydashValue>) {
    toggle_path(tree, "editor.constrain_y".to_string());
}

fn constrain_z(tree: &mut ObservableKVTree<ClaydashValue>) {
    toggle_path(tree, "editor.constrain_z".to_string());
}

fn start_scale(tree: &mut ObservableKVTree<ClaydashValue>) {
    reset_constraints(tree);
    set_objects_initial_properties(tree);
    tree.set_path("editor.state", ClaydashValue::EditorState(Scaling));
}

fn start_rotate(tree: &mut ObservableKVTree<ClaydashValue>) {
    reset_constraints(tree);
    set_objects_initial_properties(tree);
    tree.set_path("editor.state", ClaydashValue::EditorState(Rotating));
}

/// Cancel edit and bring back transforms to original value.
fn escape(tree: &mut ObservableKVTree<ClaydashValue>) {
    let state = tree.get_path("editor.state").unwrap_editor_state_or(Start);

    match state {
        Start  => {
            // Not currently editing.
            return;
        },
        _ => {}
    }

    tree.set_path("editor.state", ClaydashValue::EditorState(Start));

    let selected_object_uuids = tree.get_path("scene.selected_uuids").unwrap_vec_uuid_or(Vec::new());


    let mut sdf_objects: Vec<SDFObject> = tree.get_path("scene.sdf_objects").unwrap_vec_sdf_object_or(Vec::new());

    for object in sdf_objects.iter_mut() {
        if !selected_object_uuids.contains(&object.uuid) {
            continue;
        }
        let initial_transform = tree
            .get_path(&format!("editor.initial_transform.{}", object.uuid))
            .unwrap_transform_or(Transform::IDENTITY);
        object.transform = initial_transform;
    }

    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
}

fn finish(tree: &mut ObservableKVTree<ClaydashValue>) {
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));
}

fn duplicate(tree: &mut ObservableKVTree<ClaydashValue>) {
    // Find selected objects
    let selected_object_uuids = tree.get_path("scene.selected_uuids").unwrap_vec_uuid_or(Vec::new());

    let mut sdf_objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(objects) => { objects },
        _ => { return; }
    };

    let mut duplicated_objects: Vec<SDFObject> = sdf_objects.iter().filter(| sdf_object | {
        selected_object_uuids.contains(&sdf_object.uuid)
    }).map(|object| {
        object.duplicate()
    }).collect();

    // List duplicated objects uuids
    let duplicated_uuids: Vec<uuid::Uuid> = duplicated_objects.iter().map(|object| {
        object.uuid
    }).collect();

    // Update the tree with duplicated objects
    sdf_objects.append(&mut duplicated_objects);
    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
    tree.set_path("scene.selected_uuids", ClaydashValue::VecUuid(duplicated_uuids));

    // Move these new objects
    start_grab(tree);
}

fn select_all_or_none(tree: &mut ObservableKVTree<ClaydashValue>) {
    let selected_uuids = tree.get_path("scene.selected_uuids").unwrap_vec_uuid_or(Vec::new());
    let sdf_objects = tree.get_path("scene.sdf_objects").unwrap_vec_sdf_object_or(Vec::new());


    if selected_uuids.len() == sdf_objects.len() {
        // Everything is selected: now select none
        tree.set_path("scene.selected_uuids", ClaydashValue::VecUuid(default()));
    } else {
        // Select all
        tree.set_path(
            "scene.selected_uuids",
            ClaydashValue::VecUuid(sdf_objects.iter().map(|object| { object.uuid }).collect())
        );
        tree.set_path("editor.state", ClaydashValue::EditorState(Start));
    }
}

fn delete(tree: &mut ObservableKVTree<ClaydashValue>) {
    // Find selected objects
    let selected_object_uuids = tree.get_path("scene.selected_uuids").unwrap_vec_uuid_or(Vec::new());

    let filtered_objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(objects) => {
            objects.iter().filter(|object| {
                !selected_object_uuids.contains(&object.uuid)
            }).cloned().collect()
        },
        _ => { return; }
    };

    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(filtered_objects));
}

fn spawn_sphere(tree: &mut ObservableKVTree<ClaydashValue>) {
    let color = match tree.get_path("editor.colorpicker.color") {
        ClaydashValue::Vec4(data) => data,
        _ => Vec4::new(0.4, 0.2, 0.0, 1.0),
    };

    let mut sdf_objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(objects) => { objects },
        _ => { vec!() }
    };

    let mut new_object = SDFObject::create(sdf_consts::TYPE_SPHERE);
    new_object.color = color;
    let uuid = new_object.uuid;

    sdf_objects.push(new_object);

    // Update the tree with duplicated objects
    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));

    tree.set_path("scene.selected_uuids", ClaydashValue::VecUuid(vec!(uuid)));

    // Move new objects
    start_grab(tree);
}

fn spawn_box(tree: &mut ObservableKVTree<ClaydashValue>) {
    let color = match tree.get_path("editor.colorpicker.color") {
        ClaydashValue::Vec4(data) => data,
        _ => Vec4::new(0.4, 0.2, 0.0, 1.0),
    };

    let mut sdf_objects: Vec<SDFObject> = tree.get_path("scene.sdf_objects").unwrap_vec_sdf_object_or(Vec::new());

    let mut new_object = SDFObject::create(TYPE_BOX);
    new_object.color = color;

    let uuid = new_object.uuid;

    sdf_objects.push(new_object);

    // Update the tree with duplicated objects
    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));

    tree.set_path("scene.selected_uuids", ClaydashValue::VecUuid(vec!(uuid)));

    // Move new objects
    start_grab(tree);
}

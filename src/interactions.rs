use bevy::{
    prelude::*,
    input::{keyboard::KeyCode, Input}
};
use bevy_command_central_plugin::CommandCentralState;
use bevy_mod_picking::{backend::HitData, prelude::*};
use bevy_sdf_object::SDFObject;
use claydash_data::{ClaydashData, ClaydashValue, EditorState::*};
use command_central::CommandBuilder;
use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker
};

pub struct ClaydashInteractionPlugin;

impl Plugin for ClaydashInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClaydashData>()
            .add_systems(Startup, register_interaction_commands)
            .add_systems(Update, run_shortcut_commands)
            .add_systems(Update, update_transformations);
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

}

fn reset_constraints(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.constrain_x", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_y", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_z", ClaydashValue::Bool(false));
}

fn start_grab(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    reset_constraints(tree);
    tree.set_path("editor.state", ClaydashValue::EditorState(Grabbing));
}

fn toggle_path(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>, path: String) {
    let current_value = match tree.get_path("editor.constrain_x").unwrap() {
        ClaydashValue::Bool(value) => value,
        _ => false
    };
    tree.set_path(&path, ClaydashValue::Bool(!current_value));
}

fn constrain_x(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    toggle_path(tree, "editor.constrain_x".to_string());
}

fn constrain_y(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    toggle_path(tree, "editor.constrain_y".to_string());
}

fn constrain_z(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    toggle_path(tree, "editor.constrain_z".to_string());
}

fn start_scale(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    reset_constraints(tree);
    tree.set_path("editor.state", ClaydashValue::EditorState(Scaling));
}

fn escape(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));
    println!("TODO: cancel edit and go back to initial position.");
}

fn finish(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));
}

fn delete(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    // Find selected objects
    let selected_object_uuids = match tree.get_path("scene.selected_uuids").unwrap_or(ClaydashValue::None) {
        ClaydashValue::UUIDList(uuids) => uuids,
        _ => { return default(); }
    };

    let filtered_objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects").unwrap() {
        ClaydashValue::VecSDFObject(objects) => {
            objects.iter().filter(|object| {
                !selected_object_uuids.contains(&object.uuid)
            }).cloned().collect()
        },
        _ => { return; }
    };

    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(filtered_objects));
}

fn str_to_key(key_str: &String) -> KeyCode {
    return match key_str.as_str() {
        "X" => KeyCode::X,
        "Y" => KeyCode::Y,
        "Z" => KeyCode::Z,
        "G" => KeyCode::G,
        "S" => KeyCode::S,
        "Escape" => KeyCode::Escape,
        "Return" => KeyCode::Return,
        "Back" => KeyCode::Back,
        _ => {
            panic!("str not mapped to a keycode {}", key_str)
        }
    };
}

pub fn run_shortcut_commands(
    mut bevy_command_central: ResMut<CommandCentralState>,
    mut data_resource: ResMut<ClaydashData>,
    windows: Query<&Window>,
    keys: Res<Input<KeyCode>>
){
    let commands = &mut bevy_command_central.commands.commands;
    let tree = &mut data_resource.as_mut().tree;

    for (_key, command) in commands.iter() {
        if command.shortcut.is_empty() {
            continue;
        }
        if keys.just_released(str_to_key(&command.shortcut)) {
            let window = windows.single();
            tree.set_path(
                "editor.initial_mouse_position",
                ClaydashValue::Vec2(window.cursor_position().unwrap_or(Vec2::ZERO))
            );
            set_objects_initial_position(tree);
            match command.parameters["callback"].value.clone().unwrap() {
                ClaydashValue::Fn(callback) => callback(tree),
                _ => {}
            };
        }
    }
}

fn set_objects_initial_position(
    tree: &mut  ObservableKVTree<ClaydashValue, SimpleUpdateTracker>
) {
    let mut objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects").unwrap() {
        ClaydashValue::VecSDFObject(data) => data,
        _ => { return; }
    };

    let selected_object_uuids = match tree.get_path("scene.selected_uuids").unwrap_or(ClaydashValue::None) {
        ClaydashValue::UUIDList(uuids) => uuids,
        _ => { return default(); }
    };

    for object in objects.iter_mut() {
        if selected_object_uuids.contains(&object.uuid) {
            tree.set_path(&format!("editor.initial_position.{}", object.uuid), ClaydashValue::Vec3(object.position));
            tree.set_path(&format!("editor.initial_scale.{}", object.uuid), ClaydashValue::Vec3(object.scale));
        }
    }
}

fn update_transformations(
    mut data_resource: ResMut<ClaydashData>,
    windows: Query<&Window>,
    camera_transforms: Query<&mut Transform, With<Camera>>,
) {
    // Based on camera rotation, find what direction mouse moves corresponds to in
    // 3D space.
    let camera_transform: &Transform = camera_transforms.single();
    let x_vec = camera_transform.right();
    let y_vec = camera_transform.up();

    let tree = &mut data_resource.as_mut().tree;

    let state = tree.get_path("editor.state").unwrap_or(ClaydashValue::EditorState(Start)).into();

    // Return early if not editing
    match state {
        ClaydashValue::EditorState(Start) => { return; },
        _ => {}
    }

    // Find cursor info
    let window = windows.single();
    let cursor_position = window.cursor_position().unwrap_or(Vec2::ZERO);
    let initial_cursor_position: Vec2 = match tree.get_path("editor.initial_mouse_position") {
        Some(ClaydashValue::Vec2(vec)) => vec,
        _ => Vec2::ZERO
    };
    let delta_cursor_position = cursor_position - initial_cursor_position;

    // Find selected objects
    let mut objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects").unwrap() {
        ClaydashValue::VecSDFObject(data) => data,
        _ => { return; }
    };

    let selected_object_uuids = match tree.get_path("scene.selected_uuids").unwrap_or(ClaydashValue::None) {
        ClaydashValue::UUIDList(uuids) => uuids,
        _ => { return default(); }
    };

    let constrain_x = match tree.get_path("editor.constrain_x").unwrap() {
        ClaydashValue::Bool(value) => value,
        _ => false
    };
    let constrain_y = match tree.get_path("editor.constrain_y").unwrap() {
        ClaydashValue::Bool(value) => value,
        _ => false
    };
    let constrain_z = match tree.get_path("editor.constrain_z").unwrap() {
        ClaydashValue::Bool(value) => value,
        _ => false
    };

    match state {
        ClaydashValue::EditorState(Grabbing) => {
            for object in objects.iter_mut() {
                if selected_object_uuids.contains(&object.uuid) {
                    let initial_position = match tree.get_path(&format!("editor.initial_position.{}", object.uuid)).unwrap_or(ClaydashValue::Vec3(Vec3::ZERO)) {
                        ClaydashValue::Vec3(position) => position,
                        _ => Vec3::ZERO
                    };

                    object.position = initial_position +
                        MOUSE_SENSIBILITY * (
                            delta_cursor_position.x * x_vec +
                                -delta_cursor_position.y * y_vec
                        );
                }
            }
            tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
        },
        ClaydashValue::EditorState(Scaling) => {
            for object in objects.iter_mut() {
                if selected_object_uuids.contains(&object.uuid) {
                    let initial_scale = match tree.get_path(&format!("editor.initial_scale.{}", object.uuid)).unwrap_or(ClaydashValue::Vec3(Vec3::ONE)) {
                        ClaydashValue::Vec3(scale) => scale,
                        _ => Vec3::ONE
                    };

                    let has_constrains = constrain_x || constrain_y || constrain_z;
                    let constraints = if has_constrains { Vec3::new(
                        if constrain_x { 1.0 } else { 0.0 },
                        if constrain_y { 1.0 } else { 0.0 },
                        if constrain_z { 1.0 } else { 0.0 },
                    )} else { Vec3::ONE };

                    object.scale = initial_scale +
                        (delta_cursor_position.x + delta_cursor_position.y) *
                        SCALE_MOUSE_SENSIBILITY * Vec3::ONE * constraints;
                }
            }
            tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
        },
        _ => {}
    };
}

// How much objects move in space when mouse moves by 1px.
const MOUSE_SENSIBILITY: f32 = 1.0 / 100.0;
const SCALE_MOUSE_SENSIBILITY: f32 = 1.0 / 300.0;

/// Handle selection
/// Also, handle reseting state on click after transforming objects.
pub fn on_mouse_down(
    event: Listener<Pointer<Down>>,
    mut data_resource: ResMut<ClaydashData>,
    camera_transforms: Query<&mut Transform, With<Camera>>,
) {
    let tree = &mut data_resource.as_mut().tree;
    let state = tree.get_path("editor.state").unwrap_or(ClaydashValue::EditorState(Start)).into();

    match state {
        ClaydashValue::EditorState(Start) => { },
        _ => {
            // Exit grab/scale on click
            tree.set_path("editor.state", ClaydashValue::EditorState(Start));
            return;
        }
    }

    let tree = &mut data_resource.as_mut().tree;
    match tree.get_path("scene.sdf_objects").unwrap() {
        ClaydashValue::VecSDFObject(objects) => {
            let hit: &HitData = &event.hit;
            let position = match hit.position {
                Some(position) => position,
                _ => { return; }
            };
            let camera_transform: &Transform = camera_transforms.single();
            let camera_position = camera_transform.translation;
            let ray = position - camera_position;
            let maybe_hit_uuid = bevy_sdf_object::raymarch(position, ray, objects);

            match maybe_hit_uuid {
                Some(hit) => {
                    let mut selected_uuids: Vec<uuid::Uuid> = match tree.get_path("scene.selected_uuids").unwrap_or_default() {
                        ClaydashValue::UUIDList(list) => list,
                        _ => vec!()
                    };
                    let is_selected = selected_uuids.contains(&hit);

                    if is_selected {
                        // un-select object
                        tree.set_path(
                            "scene.selected_uuids",
                            ClaydashValue::UUIDList(selected_uuids
                                                    .into_iter()
                                                    .filter(|item| *item != hit).collect())
                        );
                    } else {
                        selected_uuids.push(hit);
                        tree.set_path(
                            "scene.selected_uuids",
                            ClaydashValue::UUIDList(selected_uuids)
                        );
                    }
                },
                _ => { return; }
            }
        },
        _ => {}
    }
}

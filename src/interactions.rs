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
            .add_systems(Update, update_selection_color)
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

fn reset_constraints(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.constrain_x", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_y", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_z", ClaydashValue::Bool(false));
}

fn start_grab(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    reset_constraints(tree);
    set_objects_initial_properties(tree);
    tree.set_path("editor.state", ClaydashValue::EditorState(Grabbing));
}

fn toggle_path(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>, path: String) {
    let current_value = tree.get_path("editor.constrain_x").unwrap_bool_or(false);
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
    set_objects_initial_properties(tree);
    tree.set_path("editor.state", ClaydashValue::EditorState(Scaling));
}

fn start_rotate(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    reset_constraints(tree);
    set_objects_initial_properties(tree);
    tree.set_path("editor.state", ClaydashValue::EditorState(Rotating));
}

fn escape(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));
    println!("TODO: cancel edit and go back to initial position.");
}

fn finish(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));
}

fn duplicate(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
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

fn select_all_or_none(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
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

fn delete(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
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

fn spawn_sphere(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    let color = match tree.get_path("editor.colorpicker.color") {
        ClaydashValue::Vec4(data) => data,
        _ => Vec4::new(0.4, 0.2, 0.0, 1.0),
    };

    let mut sdf_objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(objects) => { objects },
        _ => { vec!() }
    };

    let new_object = SDFObject {
        object_type: sdf_consts::TYPE_SPHERE,
        color,
        ..default()
    };
    let uuid = new_object.uuid;

    sdf_objects.push(new_object);

    // Update the tree with duplicated objects
    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));

    tree.set_path("scene.selected_uuids", ClaydashValue::VecUuid(vec!(uuid)));

    // Move new objects
    start_grab(tree);
}

fn spawn_box(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    let color = match tree.get_path("editor.colorpicker.color") {
        ClaydashValue::Vec4(data) => data,
        _ => Vec4::new(0.4, 0.2, 0.0, 1.0),
    };

    let mut sdf_objects: Vec<SDFObject> = tree.get_path("scene.sdf_objects").unwrap_vec_sdf_object_or(Vec::new());

    let new_object = SDFObject {
        object_type: sdf_consts::TYPE_BOX,
        color,
        ..default()
    };
    let uuid = new_object.uuid;

    sdf_objects.push(new_object);

    // Update the tree with duplicated objects
    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));

    tree.set_path("scene.selected_uuids", ClaydashValue::VecUuid(vec!(uuid)));

    // Move new objects
    start_grab(tree);
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

pub fn run_shortcut_commands(
    mut bevy_command_central: ResMut<CommandCentralState>,
    mut data_resource: ResMut<ClaydashData>,
    windows: Query<&Window>,
    keys: Res<Input<KeyCode>>
){
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

fn set_objects_initial_properties(
    tree: &mut  ObservableKVTree<ClaydashValue, SimpleUpdateTracker>
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

    // Find position of all objects relative to that center
    for object in objects.iter_mut() {
        if selected_object_uuids.contains(&object.uuid) {
            let mut transform_relative_to_center = object.transform;
            transform_relative_to_center.translation -= initial_selection_transform.translation;
            tree.set_path(&format!("editor.initial_transform.{}", object.uuid), ClaydashValue::Transform(transform_relative_to_center));
        }
    }
}

fn update_selection_color(
    mut data_resource: ResMut<ClaydashData>,
) {
    let tree = &mut data_resource.as_mut().tree;
    if !tree.was_path_updated("editor.colorpicker.color") {
        return;
    }
    let color: Vec4 = tree.get_path("editor.colorpicker.color").unwrap_vec4_or(Vec4::ZERO);

    let mut objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(data) => data,
        _ => { return; }
    };

    let selected_object_uuids = tree.get_path("scene.selected_uuids").unwrap_vec_uuid_or(Vec::new());

    for object in objects.iter_mut() {
        if selected_object_uuids.contains(&object.uuid) {
            object.color = color;
        }
    }

    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
}

fn update_transformations(
    mut data_resource: ResMut<ClaydashData>,
    windows: Query<&Window>,
    camera_global_transforms: Query<&mut GlobalTransform, With<Camera>>,
    camera: Query<&Camera>,
) {
    // Based on camera rotation, find what direction mouse moves corresponds to in
    // 3D space.
    let camera = camera.single();
    let camera_global_transform = camera_global_transforms.single();

    let tree = &mut data_resource.as_mut().tree;

    let state = tree.get_path("editor.state").unwrap_editor_state_or(Start);

    // Return early if not editing
    match state {
        Start => { return; },
        _ => {}
    }

    // Find cursor info
    let window = windows.single();
    let cursor_position = window.cursor_position().unwrap_or(Vec2::ZERO);
    let initial_cursor_position: Vec2 = match tree.get_path("editor.initial_mouse_position") {
        ClaydashValue::Vec2(vec) => vec,
        _ => Vec2::ZERO
    };
    let delta_cursor_position = cursor_position - initial_cursor_position;

    let mut objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(data) => data,
        _ => { return; }
    };

    let selected_object_uuids = match tree.get_path("scene.selected_uuids") {
        ClaydashValue::VecUuid(uuids) => uuids,
        _ => { return default(); }
    };

    let initial_selection_transform = match tree.get_path("editor.initial_selection_transform") {
        ClaydashValue::Transform(t) => t,
        _ => { return default(); }
    };

    let constrain_x = match tree.get_path("editor.constrain_x") {
        ClaydashValue::Bool(value) => value,
        _ => false
    };
    let constrain_y = match tree.get_path("editor.constrain_y") {
        ClaydashValue::Bool(value) => value,
        _ => false
    };
    let constrain_z = match tree.get_path("editor.constrain_z") {
        ClaydashValue::Bool(value) => value,
        _ => false
    };

    match state {
        Grabbing => {
            let selection_translation: Vec3 = match camera.viewport_to_world(camera_global_transform, cursor_position) {
                Some(ray) => {
                    let initial_transform = tree.get_path("editor.initial_selection_transform")
                        .unwrap_transform_or(Transform::IDENTITY);
                    let selection_to_viewport_dist = (initial_transform.translation - ray.origin).length();
                    ray.origin + ray.direction * selection_to_viewport_dist
                },
                _ => { return; }
            };

            for object in objects.iter_mut() {
                if selected_object_uuids.contains(&object.uuid) {
                    let initial_transform = tree.get_path(&format!("editor.initial_transform.{}", object.uuid))
                        .unwrap_transform_or(Transform::IDENTITY);

                    object.transform.translation = initial_transform.translation + selection_translation;
                }
            }
            tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
        },
        Scaling => {
            for object in objects.iter_mut() {
                if selected_object_uuids.contains(&object.uuid) {
                    let initial_transform = match tree.get_path(&format!("editor.initial_transform.{}", object.uuid)) {
                        ClaydashValue::Transform(t) => t,
                        _ => Transform::IDENTITY
                    };

                    let has_constrains = constrain_x || constrain_y || constrain_z;
                    let constraints = if has_constrains { Vec3::new(
                        if constrain_x { 1.0 } else { 0.0 },
                        if constrain_y { 1.0 } else { 0.0 },
                        if constrain_z { 1.0 } else { 0.0 },
                    )} else { Vec3::ONE };

                    object.transform.scale = initial_transform.scale +
                        (delta_cursor_position.x + delta_cursor_position.y) *
                        SCALE_MOUSE_SENSIBILITY * Vec3::ONE * constraints;
                }
            }
            tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
        },
        Rotating => {
            for object in objects.iter_mut() {
                if !selected_object_uuids.contains(&object.uuid) {
                    continue;
                }
                match get_object_angle_relative_to_camera_ray(camera, camera_global_transform, cursor_position, object) {
                    Some((axis, angle)) => {
                        let rotation = Quat::from_axis_angle(axis.normalize(), -angle);
                        object.transform.rotation = rotation;
                    }
                    _ => {}
                };
            }
            tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
        },
        _ => {}
    };
}

fn get_object_angle_relative_to_camera_ray(
    camera: &Camera,
    camera_global_transform: &GlobalTransform,
    cursor_position: Vec2,
    object: &SDFObject
) -> Option<(Vec3, f32)> {
    let camera_right = camera_global_transform.right();
    let camera_up = camera_global_transform.up();

    match camera.viewport_to_world(camera_global_transform, cursor_position) {
        Some(ray) => {
            let object_to_viewport_dist = (object.transform.translation - ray.origin).length();
            let object_position_relative_to_camera = object.transform.translation - camera_global_transform.translation();
            let object_position_relative_to_camera_up = object_position_relative_to_camera.dot(camera_up);
            let object_position_relative_to_camera_right = object_position_relative_to_camera.dot(camera_right);

            let cursor_position_near_object = ray.origin + ray.direction * object_to_viewport_dist;
            let cursor_relative_to_up_vector = cursor_position_near_object.dot(camera_up) - object_position_relative_to_camera_up;
            let cursor_relative_to_right_vector = cursor_position_near_object.dot(camera_right) - object_position_relative_to_camera_right;

            return Some((camera_global_transform.forward(), cursor_relative_to_up_vector.atan2(cursor_relative_to_right_vector)));
        },
        _ => {
            return None;
        }
    };
}

// How much objects move in space when mouse moves by 1px.
const SCALE_MOUSE_SENSIBILITY: f32 = 1.0 / 300.0;

/// Handle selection
/// Also, handle reseting state on click after transforming objects.
pub fn on_mouse_down(
    event: Listener<Pointer<Down>>,
    keys: Res<Input<KeyCode>>,
    mut data_resource: ResMut<ClaydashData>,
    camera_transforms: Query<&mut Transform, With<Camera>>,
) {
    let tree = &mut data_resource.as_mut().tree;
    let state = tree.get_path("editor.state").unwrap_editor_state_or(Start);

    match state {
        Start => { },
        _ => {
            // Exit grab/scale on click
            tree.set_path("editor.state", ClaydashValue::EditorState(Start));
            return;
        }
    }

    let tree = &mut data_resource.as_mut().tree;
    match tree.get_path("scene.sdf_objects") {
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
                    let mut selected_uuids: Vec<uuid::Uuid> = tree.get_path("scene.selected_uuids").unwrap_vec_uuid_or(Vec::new());
                    let is_selected = selected_uuids.contains(&hit);
                    let has_shift = keys.pressed(KeyCode::ShiftLeft);

                    if is_selected {
                        // Remove object from selection
                        match has_shift {
                            true => {
                                // Shift is pressed: remove from selection
                                selected_uuids = selected_uuids
                                    .into_iter()
                                    .filter(|item| *item != hit).collect();
                            }
                            false => {
                                // Shift not pressed.
                                if selected_uuids.len() == 1 {
                                    // Last object in selection: un-select
                                    selected_uuids = selected_uuids
                                        .into_iter()
                                        .filter(|item| *item != hit).collect();
                                } else {
                                    // Replace entire selection with only this object
                                    selected_uuids = vec!(hit);
                                }
                            }
                        };

                        // un-select object
                        tree.set_path(
                            "scene.selected_uuids",
                            ClaydashValue::VecUuid(selected_uuids)
                        );
                    } else {
                        // Add object to selection
                        match has_shift {
                            true => {
                                // Shift is pressed: Additive selection
                                selected_uuids.push(hit);
                            }
                            false => {
                                // Shift is not pressed: Replace selection with new hit
                                selected_uuids = vec!(hit);
                            }
                        };

                        tree.set_path(
                            "scene.selected_uuids",
                            ClaydashValue::VecUuid(selected_uuids)
                        );
                    }
                },
                _ => { return; }
            }
        },
        _ => {}
    }
}

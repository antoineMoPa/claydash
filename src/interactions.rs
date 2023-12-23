use bevy::{
    prelude::*,
    input::{keyboard::KeyCode, Input}
};
use bevy_mod_picking::{backend::HitData, prelude::*};
use bevy_sdf_object::SDFObject;
use claydash_data::{ClaydashData, ClaydashValue, EditorState::*};
mod interaction_commands_and_shortcuts;

pub struct ClaydashInteractionPlugin;

impl Plugin for ClaydashInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClaydashData>()
            .add_systems(Startup, (
                interaction_commands_and_shortcuts::register_interaction_commands,
            ))
            .add_systems(Update, ((interaction_commands_and_shortcuts::run_shortcut_commands),
                                  update_selection_color,
                                  update_transformations));
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

    let mut objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(data) => data,
        _ => { return; }
    };

    let selected_object_uuids = match tree.get_path("scene.selected_uuids") {
        ClaydashValue::VecUuid(uuids) => uuids,
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

    let has_constraints = constrain_x || constrain_y || constrain_z;
    let constraints = if has_constraints { Vec3::new(
        if constrain_x { 1.0 } else { 0.0 },
        if constrain_y { 1.0 } else { 0.0 },
        if constrain_z { 1.0 } else { 0.0 },
    )} else { Vec3::ONE };

    let initial_selection_transform = tree.get_path("editor.initial_selection_transform")
        .unwrap_transform_or(Transform::IDENTITY);

    let selection_translation: Vec3 = match camera.viewport_to_world(camera_global_transform, cursor_position) {
         Some(ray) => {
             let selection_to_viewport_dist = (initial_selection_transform.translation - ray.origin).length();
             ray.origin + ray.direction * selection_to_viewport_dist
         },
         _ => { return; }
     };

    match state {
        Grabbing => {
            for object in objects.iter_mut() {
                if selected_object_uuids.contains(&object.uuid) {
                    let initial_transform = tree
                        .get_path(&format!("editor.initial_transform_relative_to_selection.{}", object.uuid))
                        .unwrap_transform_or(Transform::IDENTITY);

                    object.transform.translation = initial_transform.translation + selection_translation * constraints;
                }
            }
            tree.set_path_without_notifying("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
        },
        Scaling => {
            for object in objects.iter_mut() {
                if selected_object_uuids.contains(&object.uuid) {
                    let cursor_position_near_object = get_cursor_position_at_selection_dist(
                        camera,
                        camera_global_transform,
                        cursor_position,
                        selection_translation
                    ).unwrap_or(Vec3::ZERO);

                    let initial_radius = tree.get_path("editor.initial_radius").unwrap_f32();
                    let current_radius = (cursor_position_near_object - initial_selection_transform.translation).length();
                    let scale = current_radius / initial_radius - 1.0;

                    let initial_transform = tree.get_path(&format!("editor.initial_transform.{}", object.uuid))
                        .unwrap_transform_or(Transform::IDENTITY);
                    let initial_transform_relative_to_selection = tree
                        .get_path(&format!("editor.initial_transform_relative_to_selection.{}", object.uuid))
                        .unwrap_transform_or(Transform::IDENTITY);

                    object.transform = initial_transform;
                    object.transform.scale += scale * constraints;
                    object.transform.translation += scale * constraints * initial_transform_relative_to_selection.translation;
                }
            }
            tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
        },
        Rotating => {
            for object in objects.iter_mut() {
                if !selected_object_uuids.contains(&object.uuid) {
                    continue;
                }
                match get_object_angle_relative_to_camera_ray(
                    camera,
                    camera_global_transform,
                    cursor_position,
                    &initial_selection_transform,
                ) {
                    Some((axis, angle)) => {
                        let initial_transform = tree.get_path(&format!("editor.initial_transform.{}", object.uuid))
                            .unwrap_transform_or(Transform::IDENTITY);

                        let selection_center = initial_selection_transform.translation;

                        let axis = if has_constraints { constraints  } else { axis };
                        let rotation = Quat::from_axis_angle(axis, -angle);

                        object.transform = initial_transform;
                        object.transform.rotate_around(selection_center, rotation);
                    }
                    _ => {}
                };
            }
            tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
        },
        _ => {}
    };
}

fn get_cursor_position_at_selection_dist(
    camera: &Camera,
    camera_global_transform: &GlobalTransform,
    cursor_position: Vec2,
    selection_translation: Vec3
) -> Option<Vec3> {
    match camera.viewport_to_world(camera_global_transform, cursor_position) {
        Some(ray) => {
            let object_to_viewport_dist = (selection_translation - ray.origin).length();
            return Some(ray.origin + ray.direction * object_to_viewport_dist);
        },
        _ => {
            return None;
        }
    };
}

fn get_object_angle_relative_to_camera_ray(
    camera: &Camera,
    camera_global_transform: &GlobalTransform,
    cursor_position: Vec2,
    object_transform: &Transform
) -> Option<(Vec3, f32)> {
    let camera_right = camera_global_transform.right();
    let camera_up = camera_global_transform.up();

    let cursor_position_near_object = get_cursor_position_at_selection_dist(
        camera,
        camera_global_transform,
        cursor_position,
        object_transform.translation
    );

    match cursor_position_near_object {
        Some(cursor_position_near_object) => {
            let object_position_relative_to_camera = object_transform.translation - camera_global_transform.translation();
            let object_position_relative_to_camera_up = object_position_relative_to_camera.dot(camera_up);
            let object_position_relative_to_camera_right = object_position_relative_to_camera.dot(camera_right);


            let cursor_relative_to_up_vector = cursor_position_near_object.dot(camera_up) - object_position_relative_to_camera_up;
            let cursor_relative_to_right_vector = cursor_position_near_object.dot(camera_right) - object_position_relative_to_camera_right;

            return Some((camera_global_transform.forward(), cursor_relative_to_up_vector.atan2(cursor_relative_to_right_vector)));
        },
        _ => {
            return None;
        }
    };
}

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
            tree.make_undo_redo_snapshot();
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

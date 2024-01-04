use bevy::{
    prelude::*,
    input::{keyboard::KeyCode, Input},
};
use bevy_mod_picking::{backend::HitData, prelude::*};
use crate::claydash_data::get_active_object_index;
use crate::bevy_sdf_object::{SDFObject, control_points_hit, ControlPoint, SDFObjectParams, ControlPointType};
use crate::claydash_data::{ClaydashData, ClaydashValue, EditorState::*};
use observable_key_value_tree::ObservableKVTree;
mod interaction_commands_and_shortcuts;
use lazy_static::lazy_static;
use std::sync::{Arc, Mutex};

pub struct ClaydashInteractionPlugin;

impl Plugin for ClaydashInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClaydashData>()
            .add_systems(Startup, (
                interaction_commands_and_shortcuts::register_interaction_commands,
            ))
            .add_systems(Update, ((interaction_commands_and_shortcuts::run_shortcut_commands),
                                  update_transformations,
                                  update_control_points_text,
                                  update_control_points_text_position));
    }
}

#[derive(Component)]
struct ControlPointText {
    position: Vec3,
}

lazy_static! {
    static ref LAST_SYNCED_TEXT_VERSION: Arc<Mutex<i32>> = Arc::new(Mutex::new(-1));
}

fn update_control_points_text(
    mut data_resource: ResMut<ClaydashData>,
    mut commands: Commands,
    query: Query<Entity, With<ControlPointText>>,
    asset_server: Res<AssetServer>,
) {
    let data = data_resource.as_mut();

    let last_updated_version = LAST_SYNCED_TEXT_VERSION.try_lock();

    let version = data.tree.path_version("scene");

    let mut last_updated_version = match last_updated_version {
        Ok(version) => { version  }
        _ => { return }
    };

    if version > *last_updated_version  {
        // Remove previous text
        for text in &query {
            commands.entity(text).despawn();
        }

        let active_object_index = get_active_object_index(&data.tree);
        let objects = data.tree.get_path("scene.sdf_objects");

        match active_object_index  {
            Some(index) => {
                // Show control points
                let object: &SDFObject = &objects.unwrap_vec_sdf_object()[index];

                for point in object.get_control_points().iter() {
                    let label = &point.label;

                    commands.spawn((
                        TextBundle::from_section(
                            label.to_owned(),
                            TextStyle {
                                font: asset_server.load("fonts/FiraMono-Medium.ttf"),
                                font_size: 14.0,
                                color: Color::WHITE,
                            },
                        )
                            .with_text_alignment(TextAlignment::Center)
                            .with_style(Style {
                                position_type: PositionType::Absolute,
                                ..default()
                            }),
                        ControlPointText { position: point.position },
                    ));
                }

            },
            _ => {}
        }

        *last_updated_version = version;
    }
}

fn update_control_points_text_position(
    mut query: Query<(&mut Style, &ControlPointText)>,
    camera_global_transforms: Query<&mut GlobalTransform, With<Camera>>,
    camera: Query<&Camera>,
) {
    let camera = camera.single();
    let camera_global_transform = camera_global_transforms.single();

    for (mut style, control_point_text) in query.iter_mut() {
        let position = camera.world_to_viewport(camera_global_transform, control_point_text.position);
        let position = match position {
            Some(position) => position,
            _ => { continue }
        };

        let x = position.x + 5.0;
        let y = position.y + 5.0;

        style.left = Val::Px(x);
        style.top = Val::Px(y);
    }
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


fn update_control_points(
    tree: &mut ObservableKVTree<ClaydashValue>,
    cursor_position: Vec2,
    camera: &Camera,
    camera_global_transform: &GlobalTransform,
) {
    let uuid = tree.get_path("editor.current_control_point_object_uuid").unwrap_uuid_or_default();
    let control_point_type = tree.get_path("editor.current_control_point_type").unwrap_control_point_type_or_default();

    let mut objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(data) => data,
        _ => { return; }
    };

    let active_object = objects.iter_mut().find(|obj| { obj.uuid == uuid });

    match active_object {
        Some(active_object) => {
            let control_points = active_object.get_control_points();

            let mut control_point: Option<ControlPoint> = None;

            for point in control_points.iter() {
                if point.control_point_type == control_point_type {
                    control_point = Some(point.clone());
                }
            }

            let control_point = control_point.unwrap();

            let cursor_position_near_control_point = get_cursor_position_at_selection_dist(
                camera,
                camera_global_transform,
                cursor_position,
                control_point.position
            );

            let cursor_position_near_control_point = cursor_position_near_control_point.unwrap();
            let scale = active_object.transform.scale;
            let r = active_object.transform.rotation.inverse();

            match &mut active_object.params {
                SDFObjectParams::BoxParams(params) => {
                    let new_position = cursor_position_near_control_point - active_object.transform.translation;
                    let new_position = r * new_position;

                    match control_point.control_point_type {
                        ControlPointType::BoxX => {
                            params.box_q.x = new_position.x / scale.x;
                        },
                        ControlPointType::BoxY => {
                            params.box_q.y = new_position.y / scale.y;
                        }
                        ControlPointType::BoxZ => {
                            params.box_q.z = new_position.z / scale.z;
                        },
                        _ => {
                            panic!("unhandled control point type.");
                        }
                    }
                },
                SDFObjectParams::SphereParams(params) => {
                    params.radius = ((cursor_position_near_control_point - active_object.transform.translation) / scale).length();
                },
            };

            tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
        }
        _ => {
            return;
        }
    }
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

    // Find cursor info
    let window = windows.single();
    let cursor_position = window.cursor_position().unwrap_or(Vec2::ZERO);

    // Return early if not editing
    match state {
        Start => { return; },
        GrabbingControlPoint => {
            update_control_points(tree, cursor_position, camera, camera_global_transform);
            return;
        }
        _ => {}
    }

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
            let camera_transform: &Transform = camera_transforms.single();
            let camera_position = camera_transform.translation;

            let hit: &HitData = &event.hit;
            let position = match hit.position {
                Some(position) => position,
                _ => { return; }
            };
            let ray = position - camera_position;

            let control_point_hit = control_points_hit(
                camera_position,
                ray.normalize(),
                &objects
            );

            match control_point_hit {
                Some(control_point) => {
                    tree.set_path("editor.state", ClaydashValue::EditorState(GrabbingControlPoint));
                    tree.set_path(
                        "editor.current_control_point_object_uuid",
                        ClaydashValue::Uuid(control_point.object_uuid)
                    );
                    tree.set_path(
                        "editor.current_control_point_type",
                        ClaydashValue::ControlPointType(control_point.control_point_type)
                    );

                    return;
                }
                None => {}
            }

            let maybe_hit_uuid = crate::bevy_sdf_object::raymarch(position, ray, objects);

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

pub fn on_mouse_up(
    mut data_resource: ResMut<ClaydashData>,
) {
    let tree = &mut data_resource.as_mut().tree;
    tree.set_path("editor.state", ClaydashValue::EditorState(Start));
    tree.make_undo_redo_snapshot();
}

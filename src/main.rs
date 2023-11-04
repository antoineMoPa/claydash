// This is only for native builds
#[allow(unused_imports)]
use std::fs::read_to_string;
use command_central::{
    CommandInfo,
    CommandBuilder,
    CommandMap,
};
use claydash_ui::{ClaydashUIPlugin, ClaydashUIState};
use smooth_bevy_cameras::{
    LookTransformPlugin,
    controllers::orbit::{
        OrbitCameraPlugin,
        OrbitCameraBundle,
        OrbitCameraController
    }
};

use bevy_command_central_plugin::{
    CommandCentralState,
    BevyCommandCentralPlugin,
    ParamType
};

use sdf_consts::*;

use bevy::{
    input::{keyboard::KeyCode, Input},
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
};

use bevy_sdf_object::*;
use bevy_mod_picking::prelude::*;
use bevy_mod_picking::backend::HitData;

#[allow(unused_imports)]
use wasm_bindgen::{prelude::*};


fn main() {
    App::new()
        .insert_resource(ClearColor(Color::rgb(0.0, 0.0, 0.0)))
        .insert_resource(AmbientLight {
            color: Color::rgb(1.0, 0.8, 0.9),
            brightness: 0.6,
        })
        .add_plugins((
            DefaultPlugins,
            BevyCommandCentralPlugin,
            bevy_framepace::FramepacePlugin,
            DefaultPickingPlugins,
            FrameTimeDiagnosticsPlugin,
            LogDiagnosticsPlugin::default(),
            LookTransformPlugin,
            OrbitCameraPlugin::default(),
            BevySDFObjectPlugin,
            ClaydashUIPlugin,
        ))
        .add_systems(Startup, remove_picking_logs)
        .add_systems(Startup, register_commands)
        .add_systems(Startup, setup_frame_limit)
        .add_systems(Startup, setup_camera)
        .add_systems(Startup, setup_window_size)
        .add_systems(Startup, build_projection_surface)
        .add_systems(Update, keyboard_input_system)
        .add_systems(Update, run_commands)
        .add_systems(Update, update_camera)
        .run();
}

/// By default, the object bevy_mod_picking is too verbose.
fn remove_picking_logs (
    mut logging_next_state: ResMut<NextState<debug::DebugPickingMode>>,
) {
    logging_next_state.set(debug::DebugPickingMode::Disabled);
}

/// Prevent using too much CPU. 60 fps should be enough. 30fps feels not so smooth.
fn setup_frame_limit(mut settings: ResMut<bevy_framepace::FramepaceSettings>) {
    settings.limiter = bevy_framepace::Limiter::from_framerate(60.0);
}

#[cfg(target_arch = "wasm32")]
fn setup_window_size(mut windows: Query<&mut Window>) {
    let wasm_window = match web_sys::window() {
        Some(wasm_window) => wasm_window,
        _ => {
            return;
        }
    };
    let (target_width, target_height) = (
        wasm_window.inner_width().unwrap().as_f64().unwrap() as f32,
        wasm_window.inner_height().unwrap().as_f64().unwrap() as f32,
    );

    let mut window = windows.single_mut();
    window.resolution.set(target_width, target_height);
}

#[cfg(not(target_arch = "wasm32"))]
fn setup_window_size() {
}

/// Keyboard input system
/// Lept for later, currently empty.
fn keyboard_input_system(
    keyboard_input: Res<Input<KeyCode>>,
) {
    if keyboard_input.pressed(KeyCode::W) {
        // todo
    }
}

/// Setup orbit camera controls.
fn setup_camera(
    mut commands: Commands,
) {
    commands.spawn((
        Camera3dBundle {
            //transform: Transform::from_xyz(0.0, 0.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        },
        RaycastPickCamera::default())
    ).insert(
        OrbitCameraBundle::new(
            OrbitCameraController::default(),
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0., 0., 0.),
            Vec3::Y,
        )
    );
}

/// Build an object with our SDF material.
fn build_projection_surface(
    mut windows: Query<&mut Window>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    let window = windows.single_mut();
    let window_aspect_ratio = (window.resolution.physical_width() as f32) / (window.resolution.physical_height() as f32);

    // cube
    commands.spawn((
        MaterialMeshBundle {
            mesh: meshes.add(Mesh::from(shape::Cube { size: 1.5 })),
            transform: Transform {
                translation: Vec3::new(0.0, 0.0, 0.0),
                rotation: Quat::from_xyzw(0.5, 0.5, 0.5, 0.5), // Face the camera
                scale: Vec3::new(1.0, 1.0, window_aspect_ratio),
                ..default()
            },
            material: materials.add(SDFObjectMaterial { ..default() }),
            ..default()
        },
        PickableBundle::default(),      // Makes the entity pickable
        RaycastPickTarget::default(),   // Marker for the `bevy_picking_raycast` backend
        On::<Pointer<Down>>::run(on_mouse_down),
    ));
}

/// Register commands that can be used in command_cental
fn register_commands(
    mut bevy_command_central: ResMut<CommandCentralState>
) {
    register_spawn_sphere(&mut bevy_command_central.commands);
    register_spawn_cube(&mut bevy_command_central.commands);
    register_clear_everything(&mut bevy_command_central.commands);
}

fn register_spawn_sphere(commands: &mut CommandMap<ParamType>) {
    CommandBuilder::new()
        .title("Spawn Sphere")
        .system_name("spawn-sphere")
        .docs("Add a sphere at the given position")
        .insert_param("position", "New object position vector", Some(ParamType{
            vec3_value: Some(Vec3::ZERO),
            ..default()
        }))
        .insert_param("color", "New object color.", Some(ParamType{
            vec3_value: Some(Vec3::ZERO),
            ..default()
        }))
        .write(commands);
}

fn register_spawn_cube(commands: &mut CommandMap<ParamType>) {
    CommandBuilder::new()
        .title("Spawn Cube")
        .system_name("spawn-cube")
        .docs("Add a cube at the given position")
        .insert_param("position", "New object position vector", Some(ParamType{
            vec3_value: Some(Vec3::ZERO),
            ..default()
        }))
        .insert_param("color", "New object color.", Some(ParamType{
            vec3_value: Some(Vec3::ZERO),
            ..default()
        }))
        .write(commands);
}

fn register_clear_everything(commands: &mut CommandMap<ParamType>) {
    CommandBuilder::new()
        .title("Clear Everything")
        .system_name("clear-everything")
        .docs("Remove all sdfs in the object.")
        .write(commands);
}

/// Handle click to add a sphere.
fn on_mouse_down(
    event: Listener<Pointer<Down>>,
    buttons: Res<Input<MouseButton>>,
    mut bevy_command_central: ResMut<CommandCentralState>
) {
    if buttons.just_pressed(MouseButton::Left) {
        let command: CommandInfo<ParamType> = bevy_command_central.commands.read_command(&"spawn-sphere".to_string()).unwrap();

        let hit: &HitData = &event.hit;
        let position = match hit.position {
            Some(position) => position,
            _ => { return; }
        };

        let mut params = command.parameters;
        params.get_mut("position").unwrap().value = Some(ParamType {
            vec3_value: Some(position),
            ..default()
        });

        bevy_command_central.commands.run_with_params(&"spawn-sphere".to_string(), &params);
    }
}

/// Run any command that has been issued since last update
fn run_commands(
    material_handle: Query<&Handle<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
    mut bevy_command_central: ResMut<CommandCentralState>,
    claydash_ui_state: ResMut<ClaydashUIState>
) {
    let spawn_sphere_command = bevy_command_central.commands.check_if_has_to_run(&"spawn-sphere".to_string());
    match spawn_sphere_command {
        Some(command) => {
            let position = command.parameters.get("position").unwrap().value.unwrap().vec3_value.unwrap_or(Vec3::ZERO);

            let handle = material_handle.single();
            let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();
            let mut last_sdf = 0;

            // Find last object
            for (i, sdf_type) in material.sdf_types.iter().enumerate()  {
                if sdf_type.w == TYPE_END {
                    last_sdf = i;
                    break;
                }
            }

            material.sdf_types[last_sdf].w = TYPE_SPHERE;
            material.sdf_positions[last_sdf] = Vec4::new(position.x, position.y, position.z, 0.0);
            // TODO: do we really need to clone?
            material.sdf_colors[last_sdf] = claydash_ui_state.color.clone();


            material.sdf_types[last_sdf + 1].w = TYPE_END;
            material.sdf_positions[last_sdf + 1] = Vec4::new(0.0, 0.0, 0.0, 0.0);


            info!("Spawning sphere! x: {}, y: {}, z: {}", material.sdf_positions[0].x, material.sdf_positions[0].y, material.sdf_positions[0].z);
        },
        _ => {
            // Nothing to do
        }
    }

    let spawn_cube_command = bevy_command_central.commands.check_if_has_to_run(&"spawn-cube".to_string());
    match spawn_cube_command {
        Some(command) => {
            let position = command.parameters
                .get("position")
                .unwrap()
                .value
                .unwrap()
                .vec3_value
                .unwrap_or(Vec3::ZERO);

            let handle = material_handle.single();
            let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();
            let mut last_sdf = 0;

            // Find last object
            for (i, sdf_type) in material.sdf_types.iter().enumerate()  {
                if sdf_type.w == TYPE_END {
                    last_sdf = i;
                    break;
                }
            }

            material.sdf_types[last_sdf].w = TYPE_CUBE;
            material.sdf_positions[last_sdf] = Vec4::new(position.x, position.y, position.z, 0.0);
            // TODO: do we really need to clone?
            material.sdf_colors[last_sdf] = claydash_ui_state.color.clone();

            material.sdf_types[last_sdf + 1].w = TYPE_END;
            material.sdf_positions[last_sdf + 1] = Vec4::new(0.0, 0.0, 0.0, 0.0);


            info!("Spawning sphere! x: {}, y: {}, z: {}", material.sdf_positions[0].x, material.sdf_positions[0].y, material.sdf_positions[0].z);
        },
        _ => {
            // Nothing to do
        }
    }

    let clear_everything_command = bevy_command_central.commands.check_if_has_to_run(&"clear-everything".to_string());
    match clear_everything_command {
        Some(_command) => {
            let handle = material_handle.single();
            let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();
            material.sdf_types[0].w = TYPE_END;
        },
        _ => {
            // Nothing to do
        }
    }

}



/// Update camera position uniform
fn update_camera(
    material_handle: Query<&Handle<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
    camera_transforms: Query<&mut Transform, With<Camera>>,
) {
    let camera_transform: &Transform = camera_transforms.single();
    let handle = material_handle.single();
    let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();

    material.camera.x = camera_transform.translation.x; // Uniform is a Vec4
    material.camera.y = camera_transform.translation.y; // due to bit alignement.
    material.camera.z = camera_transform.translation.z; // ...so we can't directly assign.
}

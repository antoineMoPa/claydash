// This is only for native builds
#[allow(unused_imports)]
use std::fs::read_to_string;
use command_central::CommandBuilder;
use observable_key_value_tree::{ObservableKVTree, SimpleUpdateTracker};
use smooth_bevy_cameras::{
    LookTransformPlugin,
    controllers::orbit::{
        OrbitCameraPlugin,
        OrbitCameraBundle,
        OrbitCameraController
    }
};

use bevy_command_central_plugin::{BevyCommandCentralPlugin, CommandCentralState};

use bevy::{
    input::{keyboard::KeyCode, Input},
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
};

use bevy_sdf_object::*;
use bevy_mod_picking::prelude::*;

#[allow(unused_imports)]
use wasm_bindgen::prelude::*;

use crate::interactions::ClaydashInteractionPlugin;

use claydash_data::{ClaydashDataPlugin, ClaydashValue};

mod interactions;
mod claydash_ui;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::rgb(0.0, 0.0, 0.0)))
        .insert_resource(AmbientLight {
            color: Color::rgb(1.0, 0.8, 0.9),
            brightness: 0.6,
        })
        .add_plugins((
            ClaydashDataPlugin,
            DefaultPlugins,
            BevyCommandCentralPlugin,
            bevy_framepace::FramepacePlugin,
            DefaultPickingPlugins,
            FrameTimeDiagnosticsPlugin,
            LogDiagnosticsPlugin::default(),
            LookTransformPlugin,
            OrbitCameraPlugin::default(),
            BevySDFObjectPlugin,
            claydash_ui::ClaydashUIPlugin,
            ClaydashInteractionPlugin,
        ))

        .add_systems(Startup, (remove_picking_logs,
                               setup_frame_limit,
                               setup_camera,
                               setup_window_size,
                               build_projection_surface,
                               register_debug_commands))
        .add_systems(Update, keyboard_input_system)
        .add_systems(Update, update_camera)
        .run();
}

pub fn register_debug_commands(mut bevy_command_central: ResMut<CommandCentralState>) {
    let commands = &mut bevy_command_central.commands;
    CommandBuilder::new()
        .title("Dump Tree")
        .system_name("dump-tree")
        .docs("Dump internal data tree to shell. This is a troubleshooting command for developers.")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(dump_tree)))
        .write(commands);
}

pub fn dump_tree(tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>) {
    let serialized = serde_json::to_string_pretty(&tree).unwrap();
    println!("{}", serialized);
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
    commands.spawn(
        Camera3dBundle {
            //transform: Transform::from_xyz(0.0, 0.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        }
    ).insert(
        OrbitCameraBundle::new(
            OrbitCameraController::default(),
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::ZERO,
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
                translation: Vec3::ZERO,
                rotation: Quat::from_xyzw(0.5, 0.5, 0.5, 0.5), // Face the camera
                scale: Vec3::new(1.0, 1.0, window_aspect_ratio),
                ..default()
            },
            material: materials.add(SDFObjectMaterial { ..default() }),
            ..default()
        },
        PickableBundle::default(),      // Makes the entity pickable
        On::<Pointer<Down>>::run(interactions::on_mouse_down)
    ));
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

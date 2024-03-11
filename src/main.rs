mod sdf_object;
mod command_central_egui;
mod command_central_plugin;
mod claydash_data;
mod interactions;
mod claydash_ui;
mod undo_redo;

// This is only for native builds
#[allow(unused_imports)]
use std::fs::read_to_string;
use command_central::CommandBuilder;
use observable_key_value_tree::ObservableKVTree;
use smooth_bevy_cameras::{
    LookTransformPlugin,
    controllers::orbit::{
        OrbitCameraPlugin,
        OrbitCameraBundle,
        OrbitCameraController
    }
};

use command_central_plugin::{BevyCommandCentralPlugin, CommandCentralState};

use bevy::{
    input::{keyboard::KeyCode, ButtonInput},
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*, render::render_resource::{AsBindGroup, ShaderRef},
};

use sdf_object::*;
use bevy_mod_picking::prelude::*;

use undo_redo::ClaydashUndoRedoPlugin;
#[allow(unused_imports)]
use wasm_bindgen::prelude::*;

use crate::interactions::ClaydashInteractionPlugin;

use claydash_data::{ClaydashDataPlugin, ClaydashValue, ClaydashData};


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
            MaterialPlugin::<GridMaterial>::default(),
            ClaydashUndoRedoPlugin
        ))
        .add_systems(Startup, (remove_picking_logs,
                               setup_frame_limit,
                               setup_camera,
                               setup_window_size,
                               build_projection_surface,
                               register_debug_commands,
                               setup_grid,
                               default_duck))
        .add_systems(Update, keyboard_input_system)
        .add_systems(Update, update_camera)
        .run();
}

mod duck;

pub fn migrate_scene_from_vec_sdf_object_to_sdf_object_tree(tree: &mut ObservableKVTree<ClaydashValue>) {
    let sdf_objects = tree.get_path("scene.sdf_objects").unwrap_vec_sdf_object_or(vec!());

    let mut sdf_object_tree = tree.get_path("scene.sdf_object_tree").unwrap_sdf_object_tree_or_default();

    for sdf_object in sdf_objects {
        sdf_object_tree.add_object(sdf_object, SDFOperation::Union);
    }

    tree.set_path("scene.sdf_object_tree", ClaydashValue::SDFObjectTree(sdf_object_tree));
    tree.set_path("scene.sdf_objects", ClaydashValue::None);
}

pub fn default_duck(mut data_resource: ResMut<ClaydashData>) {
    let tree = &mut data_resource.as_mut().tree;
    let duck_scene: Result<ObservableKVTree<ClaydashValue>, serde_json::Error> = serde_json::from_str(duck::DEFAULT_DUCK);
    tree.set_tree("scene", duck_scene.unwrap());
    migrate_scene_from_vec_sdf_object_to_sdf_object_tree(tree);

    // Add snapshot for initial state
    tree.make_undo_redo_snapshot();
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

pub fn dump_tree(tree: &mut ObservableKVTree<ClaydashValue>) {
    let serialized = serde_json::to_string_pretty(&tree).unwrap();
    println!("{}", serialized);
}

/// By default, the object bevy_mod_picking is too verbose.
fn remove_picking_logs (
    //mut logging_next_state: ResMut<NextState<debug::DebugPickingMode>>,
) {
    //logging_next_state.set(debug::DebugPickingMode::Disabled);
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
fn setup_window_size(mut _windows: Query<&mut Window>) {
    // Can be useful when debugging slow shaders:
    // window.resolution.set(60.0, 60.0);
}

/// Keyboard input system
/// Lept for later, currently empty.
fn keyboard_input_system(
    keyboard_input: Res<ButtonInput<KeyCode>>,
) {
    if keyboard_input.pressed(KeyCode::KeyW) {
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
            Vec3::new(-3.3, 0.8, 1.7),
            Vec3::ZERO,
            Vec3::Y,
        )
    );
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GridMaterial { }

impl Material for GridMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/grid_material.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
	AlphaMode::Opaque
    }
}


fn setup_grid(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GridMaterial>>,
) {
    commands.spawn(MaterialMeshBundle {
        mesh: meshes.add(Mesh::from(Plane3d::default().mesh().size(10.0, 10.0))),
        transform: Transform::from_xyz(0.0, 0.0, 0.0),
        material: materials.add(GridMaterial { }),
        ..default()
    });
}

/// Build an object with our SDF material.
fn build_projection_surface(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    // cube
    commands.spawn((
        MaterialMeshBundle {
            mesh: meshes.add(Mesh::from(Cuboid { half_size: Vec3 {
                x: 1.0,
                y: 1.0,
                z: 1.0
            }})),
            transform: Transform {
                translation: Vec3::ZERO,
                scale: Vec3::ONE,
                ..default()
            },
            material: materials.add(SDFObjectMaterial {
                ..default()
            }),
            ..default()
        },
        PickableBundle::default(),      // Makes the entity pickable
        On::<Pointer<Down>>::run(interactions::on_mouse_down),
        On::<Pointer<Up>>::run(interactions::on_mouse_up)
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

    let camera_right = camera_transform.right();
    material.camera_right.x = camera_right.x;
    material.camera_right.y = camera_right.y;
    material.camera_right.z = camera_right.z;

    let camera_up = camera_transform.up();
    material.camera_up.x = camera_up.x;
    material.camera_up.y = camera_up.y;
    material.camera_up.z = camera_up.z;
}

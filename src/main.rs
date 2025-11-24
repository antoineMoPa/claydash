mod bevy_sdf_object;
mod command_central_egui;
mod command_central_plugin;
mod claydash_data;
mod interactions;
mod claydash_ui;
mod undo_redo;
mod object_generation;

// This is only for native builds
#[allow(unused_imports)]
use std::fs::read_to_string;
use command_central::CommandBuilder;
use observable_key_value_tree::{ObservableKVTree};
use smooth_bevy_cameras::{
    LookTransformPlugin,
    controllers::orbit::{
        OrbitCameraPlugin,
        OrbitCameraBundle,
        OrbitCameraController,
        ControlEvent
    }
};

use command_central_plugin::{BevyCommandCentralPlugin, CommandCentralState};

use bevy::{
    input::{keyboard::KeyCode, touchpad::TouchpadMagnify, Input},
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*, render::render_resource::{AsBindGroup, ShaderRef},
};

use bevy_sdf_object::*;
use bevy_mod_picking::prelude::*;

use undo_redo::ClaydashUndoRedoPlugin;
#[allow(unused_imports)]
use wasm_bindgen::prelude::*;

use crate::interactions::ClaydashInteractionPlugin;
use crate::object_generation::ObjectGenerationPlugin;

use claydash_data::{ClaydashDataPlugin, ClaydashValue, ClaydashData};


fn main() {
    // Load .env file for native builds
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Err(e) = dotenvy::dotenv() {
            eprintln!("Warning: Could not load .env file: {}", e);
            eprintln!("Make sure FAL_KEY is set in your .env file");
        }
    }
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
            ClaydashUndoRedoPlugin,
            ObjectGenerationPlugin
        ))
        .add_systems(Startup, (remove_picking_logs,
                               setup_frame_limit,
                               setup_camera,
                               setup_window_size,
                               build_projection_surface,
                               register_debug_commands,
                               setup_grid,
                               init_empty_scene))
        .add_systems(Update, keyboard_input_system)
        .add_systems(Update, handle_touchpad_magnify)
        .add_systems(Update, update_camera)
        .run();
}

mod duck;

/// Initialize scene on startup - loads scene.claydash if it exists, otherwise creates empty scene
pub fn init_empty_scene(mut data_resource: ResMut<ClaydashData>) {
    let tree = &mut data_resource.as_mut().tree;

    // Try to load scene.claydash from current directory
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::path::Path;
        let scene_path = Path::new("scene.claydash");

        if scene_path.exists() {
            eprintln!("📂 Found scene.claydash, loading...");
            match std::fs::read(scene_path) {
                Ok(data) => {
                    match serde_json::from_slice::<ObservableKVTree<ClaydashValue>>(&data) {
                        Ok(scene) => {
                            tree.set_tree("scene", scene);
                            tree.make_undo_redo_snapshot();
                            eprintln!("✅ Loaded scene.claydash successfully");
                            return;
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to parse scene.claydash: {}", e);
                            eprintln!("   Creating empty scene instead...");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Failed to read scene.claydash: {}", e);
                    eprintln!("   Creating empty scene instead...");
                }
            }
        }
    }

    // Create empty scene with minimal required structure
    let mut empty_scene = ObservableKVTree::<ClaydashValue>::default();
    empty_scene.set_path("sdf_objects", ClaydashValue::VecSDFObject(Vec::new()));
    empty_scene.set_path("selected_uuids", ClaydashValue::VecUuid(Vec::new()));

    tree.set_tree("scene", empty_scene);

    // Add snapshot for initial state
    tree.make_undo_redo_snapshot();

    eprintln!("✅ Initialized empty scene");
}

pub fn default_duck(mut data_resource: ResMut<ClaydashData>) {
    let tree = &mut data_resource.as_mut().tree;
    let scene: Result<ObservableKVTree<ClaydashValue>, serde_json::Error> = serde_json::from_str(duck::DEFAULT_DUCK);
    tree.set_tree("scene", scene.unwrap());

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

/// Handle touchpad pinch-to-zoom gestures (macOS)
fn handle_touchpad_magnify(
    mut touchpad_events: EventReader<TouchpadMagnify>,
    mut control_events: EventWriter<ControlEvent>,
) {
    for event in touchpad_events.read() {
        // TouchpadMagnify delta is the magnification amount
        // Positive = zoom in, Negative = zoom out
        // Convert to zoom scalar: smaller radius for zoom in, larger for zoom out
        let zoom_scalar = 1.0 - event.0 * 0.5;
        control_events.send(ControlEvent::Zoom(zoom_scalar));
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
            OrbitCameraController {
                mouse_wheel_zoom_sensitivity: 0.08,  // More sensitive for Mac-like zoom (default: 0.2)
                pixels_per_line: 20.0,  // Smoother pixel-based scrolling (default: 53.0)
                ..OrbitCameraController::default()
            },
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
    use bevy_mod_picking::prelude::*;

    commands.spawn((
        MaterialMeshBundle {
            mesh: meshes.add(Mesh::from(shape::Plane { size: 10.0, subdivisions: 0 })),
            transform: Transform::from_xyz(0.0, 0.0, 0.0),
            material: materials.add(GridMaterial { }),
            ..default()
        },
        Pickable::default(),
        On::<Pointer<Down>>::run(interactions::on_mouse_down),
        On::<Pointer<Up>>::run(interactions::on_mouse_up),
    ));
}

/// Component marker for the SDF projection cube
#[derive(Component)]
struct SDFProjectionCube;

/// Build an object with our SDF material.
fn build_projection_surface(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    use bevy_mod_picking::prelude::*;

    // cube - Note: Explicitly set to IGNORE raycasts so it doesn't block clicking on models behind it
    // Clicking on SDF objects works via raymarching, not mesh picking
    commands.spawn((
        MaterialMeshBundle {
            mesh: meshes.add(Mesh::from(shape::Cube { size: 2.0 })),
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
        SDFProjectionCube,
        Pickable::IGNORE, // Explicitly ignore raycasts - this prevents the cube from blocking clicks
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

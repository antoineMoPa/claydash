mod bevy_sdf_object;
mod claydash_data;
mod claydash_ui;
mod command_central_egui;
mod command_central_plugin;
mod interactions;
mod undo_redo;

// This is only for native builds
use command_central::CommandBuilder;
use observable_key_value_tree::ObservableKVTree;
#[allow(unused_imports)]
use std::fs::read_to_string;

use command_central_plugin::{BevyCommandCentralPlugin, CommandCentralState};

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    input::{
        keyboard::KeyCode,
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    },
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};
use bevy_egui::input::egui_wants_any_pointer_input;

use bevy_sdf_object::*;

use undo_redo::ClaydashUndoRedoPlugin;

use crate::interactions::ClaydashInteractionPlugin;

use claydash_data::{ClaydashData, ClaydashDataPlugin, ClaydashValue};

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.0, 0.0, 0.0)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(1.0, 0.8, 0.9),
            brightness: 0.6,
            ..default()
        })
        .add_plugins((
            ClaydashDataPlugin,
            DefaultPlugins,
            BevyCommandCentralPlugin,
            bevy_framepace::FramepacePlugin,
            MeshPickingPlugin,
            FrameTimeDiagnosticsPlugin::default(),
            LogDiagnosticsPlugin::default(),
            BevySDFObjectPlugin,
            claydash_ui::ClaydashUIPlugin,
            ClaydashInteractionPlugin,
            MaterialPlugin::<GridMaterial>::default(),
            ClaydashUndoRedoPlugin,
        ))
        .add_systems(
            Startup,
            (
                setup_frame_limit,
                setup_camera,
                setup_window_size,
                build_projection_surface,
                register_debug_commands,
                setup_grid,
                default_duck,
            ),
        )
        .add_systems(Update, keyboard_input_system)
        .add_systems(
            Update,
            (
                update_orbit_camera.run_if(not(egui_wants_any_pointer_input)),
                update_camera,
            ),
        )
        .run();
}

mod duck;

pub fn default_duck(mut data_resource: ResMut<ClaydashData>) {
    let tree = &mut data_resource.as_mut().tree;
    let scene: Result<ObservableKVTree<ClaydashValue>, serde_json::Error> =
        serde_json::from_str(duck::DEFAULT_DUCK);
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
        .insert_param(
            "callback",
            "system callback",
            Some(ClaydashValue::Fn(dump_tree)),
        )
        .write(commands);
}

pub fn dump_tree(tree: &mut ObservableKVTree<ClaydashValue>) {
    let serialized = serde_json::to_string_pretty(&tree).unwrap();
    println!("{}", serialized);
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

    let mut window = windows.single_mut().unwrap();
    window.resolution.set(target_width, target_height);
}

#[cfg(not(target_arch = "wasm32"))]
fn setup_window_size() {}

/// Keyboard input system
/// Lept for later, currently empty.
fn keyboard_input_system(keyboard_input: Res<ButtonInput<KeyCode>>) {
    if keyboard_input.pressed(KeyCode::KeyW) {
        // todo
    }
}

#[derive(Component)]
struct OrbitCamera {
    target: Vec3,
}

/// Setup orbit camera controls.
fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-3.3, 0.8, 1.7).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera { target: Vec3::ZERO },
    ));
}

fn update_orbit_camera(
    mut camera: Single<(&mut Transform, &mut OrbitCamera)>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
) {
    let (transform, controller) = &mut *camera;
    let mut offset = transform.translation - controller.target;
    let mut radius = offset.length().max(0.001);

    if keyboard.pressed(KeyCode::ControlLeft) {
        let delta = mouse_motion.delta;
        let yaw = Quat::from_rotation_y(-delta.x * 0.003);
        let right = transform.right();
        let pitch = Quat::from_axis_angle(*right, -delta.y * 0.003);
        offset = yaw * offset;
        let pitched_offset = pitch * offset;
        if pitched_offset.normalize_or_zero().dot(Vec3::Y).abs() < 0.995 {
            offset = pitched_offset;
        }
    }

    if mouse_buttons.pressed(MouseButton::Right) {
        let delta = mouse_motion.delta;
        controller.target +=
            (*transform.right() * -delta.x + *transform.up() * delta.y) * radius * 0.001;
    }

    let scroll = match mouse_scroll.unit {
        MouseScrollUnit::Line => mouse_scroll.delta.y,
        MouseScrollUnit::Pixel => mouse_scroll.delta.y / 53.0,
    };
    radius = (radius * (1.0 - scroll * 0.2).max(0.01)).clamp(0.05, 1_000.0);
    transform.translation = controller.target + offset.normalize_or_zero() * radius;
    transform.look_at(controller.target, Vec3::Y);
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GridMaterial {}

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
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(5.0)).mesh())),
        MeshMaterial3d(materials.add(GridMaterial {})),
        Transform::default(),
    ));
}

/// Build an object with our SDF material.
fn build_projection_surface(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    // cube
    commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::from_size(Vec3::splat(2.0)))),
            MeshMaterial3d(materials.add(SDFObjectMaterial::default())),
            Transform::default(),
        ))
        .observe(interactions::on_mouse_down);
}

/// Update camera position uniform
fn update_camera(
    material_handle: Single<&MeshMaterial3d<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
    camera_transforms: Single<&Transform, With<Camera>>,
) {
    let camera_transform: &Transform = &camera_transforms;
    let mut material = materials.get_mut(&material_handle.0).unwrap();

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

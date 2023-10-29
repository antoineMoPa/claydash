// This is only for native builds
#[allow(unused_imports)]
use std::fs::read_to_string;
use command_central::Command;

use bevy_reflect::{
    TypePath,
    TypeUuid
};

use bevy::{

    input::{keyboard::KeyCode, Input},
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    pbr::{
        MaterialPipeline,
        MaterialPipelineKey,
        DirectionalLightShadowMap
    },
    prelude::*,
    render::{
        mesh::MeshVertexBufferLayout,
        render_resource::{
            AsBindGroup, RenderPipelineDescriptor, ShaderRef, SpecializedMeshPipelineError, ShaderDefVal,
        },
    },
};

use bevy_mod_picking::prelude::*;

#[allow(unused_imports)]
use wasm_bindgen::{prelude::*};

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::rgb(0.0, 0.0, 0.0)))
        .insert_resource(DirectionalLightShadowMap { size: 2048 })
        .insert_resource(AmbientLight {
            color: Color::rgb(1.0, 0.8, 0.9),
            brightness: 0.6,
        })
        .add_plugins(DefaultPlugins)
        .add_plugins(bevy_framepace::FramepacePlugin)
        .add_plugins(DefaultPickingPlugins)
        .add_plugins(MaterialPlugin::<CustomMaterial>::default())
        .add_plugins((
            FrameTimeDiagnosticsPlugin,
            LogDiagnosticsPlugin::default()
        ))
        .add_systems(Startup, remove_picking_logs)
        .add_systems(Startup, setup_commands)
        .add_systems(Startup, setup_frame_limit)
        .add_systems(Startup, setup_camera)
        .add_systems(Startup, setup_window_size)
        .add_systems(Startup, build_projection_surface)
        .add_systems(Update, keyboard_input_system)
        .add_systems(Update, launch_mouse_commands)
        .add_systems(Update, run_commands)
        .run();
}

fn remove_picking_logs (
    mut logging_next_state: ResMut<NextState<debug::DebugPickingMode>>,
) {
    logging_next_state.set(debug::DebugPickingMode::Disabled);
}

fn setup_commands() {
    let mut params = command_central::CommandMap::new();
    params.insert("x".to_string(), command_central::CommandParam {
        docs: "X position of the mouse.".to_string(),
        ..default()
    });
    params.insert("y".to_string(), command_central::CommandParam {
        docs: "Y position of the mouse.".to_string(),
        ..default()
    });

    command_central::add_command(&"test-command".to_string(), Command {
        title: "Test Command".to_string(),
        docs: "Here are some docs about the command".to_string(),
        ..Command::default()
    });
}

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

fn keyboard_input_system(
    keyboard_input: Res<Input<KeyCode>>,
) {
    if keyboard_input.pressed(KeyCode::W) {
        // todo
    }
}

const MAX_SDFS_PER_ENTITY: usize = 512;

#[derive(TypeUuid, TypePath, AsBindGroup, Debug, Clone)]
#[uuid = "84F24BEA-CC34-4A35-B223-C5C148A14722"]
struct CustomMaterial {
    #[uniform(0)]
    objects: [IVec4; MAX_SDFS_PER_ENTITY],
    #[uniform(1)]
    mouse: Vec3,
}

impl Default for CustomMaterial {
    fn default() -> Self {
        Self {
            objects: [IVec4 { w: 0, x: 1, y: 2, z: 3}; MAX_SDFS_PER_ENTITY],
            mouse: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
        }
    }
}

impl Material for CustomMaterial {
    fn fragment_shader() -> ShaderRef {
        return "shaders/all.wgsl".into();
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayout,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let fragment = descriptor.fragment.as_mut().unwrap();
        let val = ShaderDefVal::UInt("MAX_SDFS_PER_ENTITY".into(), MAX_SDFS_PER_ENTITY as u32);
        fragment.shader_defs.push(val);
        Ok(())
    }
}

fn setup_camera(
    mut commands: Commands,
) {
    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(0.0, 0.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        },
        RaycastPickCamera::default()
    ));
}

use bevy_mod_picking::backend::HitData;

fn on_pointer_move(
    event: Listener<Pointer<Move>>,
    material_handle: Query<&Handle<CustomMaterial>>,
    mut materials: ResMut<Assets<CustomMaterial>>,
) {
    let hit: &HitData = &event.hit;
    let position = match hit.position {
        Some(position) => position,
        _ => { return; }
    };

    let handle = material_handle.single();
    let material = materials.get_mut(handle).unwrap();

    material.mouse.x = position.x;
    material.mouse.y = position.y;
    material.mouse.z = position.z;
}

fn build_projection_surface(
    mut windows: Query<&mut Window>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CustomMaterial>>,
) {
    let window = windows.single_mut();
    let window_aspect_ratio = (window.resolution.physical_width() as f32) / (window.resolution.physical_height() as f32);

    // cube
    commands.spawn((
        MaterialMeshBundle {
            mesh: meshes.add(Mesh::from(shape::Plane { size: 1.0, subdivisions: 0 })),
            transform: Transform {
                translation: Vec3::new(0.0, 0.0, 0.0),
                rotation: Quat::from_xyzw(0.5, 0.5, 0.5, 0.5), // Face the camera
                scale: Vec3::new(1.0, 1.0, window_aspect_ratio),
                ..default()
            },
            material: materials.add(CustomMaterial { ..default() }),
            ..default()
        },
        PickableBundle::default(),      // Makes the entity pickable
        RaycastPickTarget::default(),   // Marker for the `bevy_picking_raycast` backend
        On::<Pointer<Move>>::run(on_pointer_move),
    ));
}

fn launch_mouse_commands(
    buttons: Res<Input<MouseButton>>,
    mut windows: Query<&mut Window>,
) {
    if buttons.just_pressed(MouseButton::Left) {
        let window = windows.single_mut();
        let position = window.cursor_position().unwrap();

        let mut params = command_central::CommandMap::new();
        params.insert("x".to_string(), command_central::CommandParam {
            docs: "X position of the mouse.".to_string(),
            float: Some(position.x),
            ..default()
        });
        params.insert("y".to_string(), command_central::CommandParam {
            docs: "Y position of the mouse.".to_string(),
            float: Some(position.y),
            ..default()
        });

        command_central::run_with_params(&"test-command".to_string(), &params);
    }
}

fn run_commands() {
    let command = command_central::check_if_has_to_run(&"test-command".to_string());
    match command {
        Some(command) => {
            let x = command.parameters.get("x").unwrap().float.unwrap();
            let y = command.parameters.get("y").unwrap().float.unwrap();
            info!("Running Command! x: {}, y: {}", x, y);
        },
        _ => {
            // Nothing to do
        }
    }
}

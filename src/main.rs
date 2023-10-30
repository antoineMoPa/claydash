// This is only for native builds
#[allow(unused_imports)]
use std::fs::read_to_string;
use command_central::CommandInfo;

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
        .add_plugins(MaterialPlugin::<SDFObjectMaterial>::default())
        .add_plugins((
            FrameTimeDiagnosticsPlugin,
            LogDiagnosticsPlugin::default()
        ))
        .add_systems(Startup, remove_picking_logs)
        .add_systems(Startup, register_commands)
        .add_systems(Startup, setup_frame_limit)
        .add_systems(Startup, setup_camera)
        .add_systems(Startup, setup_window_size)
        .add_systems(Startup, build_projection_surface)
        .add_systems(Update, keyboard_input_system)
        .add_systems(Update, run_commands)
        .run();
}

fn remove_picking_logs (
    mut logging_next_state: ResMut<NextState<debug::DebugPickingMode>>,
) {
    logging_next_state.set(debug::DebugPickingMode::Disabled);
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

const MAX_SDFS_PER_ENTITY: i32 = 512;

#[derive(TypeUuid, TypePath, AsBindGroup, Clone)]
#[uuid = "84F24BEA-CC34-4A35-B223-C5C148A14722"]
struct SDFObjectMaterial {
    #[uniform(0)]
    mouse: Vec4,
    #[storage(1, read_only)]
    sdf_types: Vec<i32>,
    #[storage(2, read_only)]
    sdf_positions: Vec<Vec4>, // Vec4 is 16 bit aligned. It makes our life easier than Vec3
}


// SDF types definition
const TYPE_END: i32 = 0; // No more SDF to process
const TYPE_SPHERE: i32 = 1;
const TYPE_RECTANGLE: i32 = 2;

impl Default for SDFObjectMaterial {
    fn default() -> Self {
        let mut default = Self {
            sdf_types: Vec::new(),
            sdf_positions: Vec::new(),
            mouse: Vec4 { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
        };

        default.sdf_types.push(TYPE_END);
        default.sdf_positions.push(Vec4::new(0.0, 0.0, 0.0, 0.0));

        return default;
    }
}


impl Material for SDFObjectMaterial {
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
        ShaderDefVal::Int("MAX_SDFS_PER_ENTITY".into(), MAX_SDFS_PER_ENTITY);

        let defs = &mut fragment.shader_defs;

        defs.push(ShaderDefVal::Int(
            "MAX_SDFS_PER_ENTITY".into(),
            MAX_SDFS_PER_ENTITY)
        );
        defs.push(ShaderDefVal::Int("TYPE_END".into(), TYPE_END));
        defs.push(ShaderDefVal::Int("TYPE_SPHERE".into(), TYPE_SPHERE));
        defs.push(ShaderDefVal::Int("TYPE_RECTANGLE".into(), TYPE_RECTANGLE));

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
    material_handle: Query<&Handle<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
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
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
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
            material: materials.add(SDFObjectMaterial { ..default() }),
            ..default()
        },
        PickableBundle::default(),      // Makes the entity pickable
        RaycastPickTarget::default(),   // Marker for the `bevy_picking_raycast` backend
        On::<Pointer<Move>>::run(on_pointer_move),
        On::<Pointer<Down>>::run(on_mouse_down),
    ));
}

fn register_commands() {
    let mut params = command_central::CommandParamMap::new();
    params.insert("x".to_string(), command_central::CommandParam {
        docs: "X position of the sphere.".to_string(),
        ..default()
    });
    params.insert("y".to_string(), command_central::CommandParam {
        docs: "Y position of the sphere.".to_string(),
        ..default()
    });
    params.insert("z".to_string(), command_central::CommandParam {
        docs: "Z position of the sphere.".to_string(),
        ..default()
    });

    command_central::add_command(&"spawn-sphere".to_string(), CommandInfo {
        title: "Spawn sphere".to_string(),
        docs: "Spawn a sphere at the given position in the current object.".to_string(),
        ..CommandInfo::default()
    });
}

fn on_mouse_down(
    event: Listener<Pointer<Down>>,
    buttons: Res<Input<MouseButton>>,
) {
    if buttons.just_pressed(MouseButton::Left) {
        let hit: &HitData = &event.hit;
        let position = match hit.position {
            Some(position) => position,
            _ => { return; }
        };

        let mut params = command_central::CommandParamMap::new();
        params.insert("x".to_string(), command_central::CommandParam {
            float: Some(position.x),
            ..default()
        });
        params.insert("y".to_string(), command_central::CommandParam {
            float: Some(position.y),
            ..default()
        });
        params.insert("z".to_string(), command_central::CommandParam {
            float: Some(position.z),
            ..default()
        });

        command_central::run_with_params(&"spawn-sphere".to_string(), &params);
    }
}

fn run_commands(
    material_handle: Query<&Handle<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    let command = command_central::check_if_has_to_run(&"spawn-sphere".to_string());
    match command {
        Some(command) => {
            let x = command.parameters.get("x").unwrap().float.unwrap();
            let y = command.parameters.get("y").unwrap().float.unwrap();
            let z = command.parameters.get("z").unwrap().float.unwrap();

            let handle = material_handle.single();
            let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();
            let mut last_sdf = 0;

            // Find last object
            for (i, sdf_type) in material.sdf_types.iter().enumerate()  {
                if sdf_type == &TYPE_END {
                    last_sdf = i;
                }
            }

            material.sdf_types.push(TYPE_END);
            material.sdf_positions.push(Vec4::new(0.0, 0.0, 0.0, 0.0));

            material.sdf_types[last_sdf] = TYPE_SPHERE;
            material.sdf_positions[last_sdf].x = x;
            material.sdf_positions[last_sdf].y = y;
            material.sdf_positions[last_sdf].z = z;

            info!("Spawning sphere! x: {}, y: {}, z: {}", material.sdf_positions[0].x, material.sdf_positions[0].y, material.sdf_positions[0].z);
        },
        _ => {
            // Nothing to do
        }
    }
}

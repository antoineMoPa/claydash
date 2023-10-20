// This is only for native builds
#[allow(unused_imports)]
use std::fs::read_to_string;

use bevy_reflect::{
    TypePath,
    TypeUuid
};

use bevy::{
    input::{keyboard::KeyCode, Input},
    pbr::DirectionalLightShadowMap,
    render::render_resource::{AsBindGroup, ShaderRef},
    prelude::*
};

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
        .add_plugins(MaterialPlugin::<CustomMaterial>::default())
        .add_systems(Startup, setup_camera)
        .add_systems(Startup, setup_graphics)
        .add_systems(Startup, setup_window_size)
        .add_systems(Startup, build_projection_surface)
        .add_systems(Update, keyboard_input_system)
        .add_systems(Update, cursor_system)
        .run();
}

#[cfg(target_arch = "wasm32")]
fn setup_window_size(mut windows: ResMut<Windows>) {
    let window = match windows.get_primary_mut() {
        Some(window) => window,
        _ => {
            return;
        }
    };
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

    window.set_resolution(target_width, target_height);
}

#[cfg(not(target_arch = "wasm32"))]
fn setup_window_size() {
}

fn setup_graphics(
    mut commands: Commands,
) {
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            shadows_enabled: true,
            illuminance: 1000.0,
            color: Color::rgb(0.5, 0.5, 2.0),
            ..default()
        },
        transform: Transform {
            translation: Vec3::new(0.0, 2.0, 0.0),
            rotation: Quat::from_xyzw(-1.0, -0.3, 0.0, 0.0),
            ..default()
        },
        ..default()
    });
}


fn keyboard_input_system(
    keyboard_input: Res<Input<KeyCode>>,
) {
    if keyboard_input.pressed(KeyCode::W) {
        // todo
    }
}


fn cursor_system(
    mut windows: Query<&mut Window>,
    _mouse_input: Res<Input<MouseButton>>,
) {
    let window = windows.single_mut();

    match window.cursor_position() {
        Some(p) => {
            //print!("{} {} \n", p.x, p.y);
        },
        _ => {
            return;
        }
    };

}



#[derive(TypeUuid, TypePath, AsBindGroup, Debug, Clone)]
#[uuid = "84F24BEA-CC34-4A35-B223-C5C148A14722"]
struct CustomMaterial {}

impl Material for CustomMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/all.wgsl".into()
    }
}

fn setup_camera(
    mut commands: Commands,
) {
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(0.0, 1.0, 0.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..default()
    });
}
fn build_projection_surface(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CustomMaterial>>,
) {
    // cube
    commands.spawn(MaterialMeshBundle {
        mesh: meshes.add(Mesh::from(shape::Plane { size: 1.0, subdivisions: 0 })),
        transform: Transform::from_xyz(0.0, 0.0, 0.0),
        material: materials.add(CustomMaterial {}),
        ..default()
    });
}

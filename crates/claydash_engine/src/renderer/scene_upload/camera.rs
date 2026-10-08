use super::*;

pub(super) fn packed_camera(
    camera: &Camera,
    exposure: f32,
    object_count: u32,
    node_count: u32,
    world: World,
) -> GpuCamera {
    GpuCamera {
        inverse_view_projection: (camera.projection() * camera.view())
            .inverse()
            .to_cols_array_2d(),
        view_projection: (camera.projection() * camera.view()).to_cols_array_2d(),
        position: camera.position.extend(exposure).to_array(),
        count: [
            object_count,
            node_count,
            camera.viewport.y.max(1.0) as u32,
            u32::from(camera.projection_mode == ProjectionMode::Orthographic),
        ],
        world_mode: [
            world.background.shader_id(),
            u32::from(world.screen_space_ambient_occlusion),
            u32::from(world.screen_space_reflections),
            camera.viewport.x.max(1.0) as u32,
        ],
        world_color: [
            world.flat_color[0],
            world.flat_color[1],
            world.flat_color[2],
            1.0,
        ],
        sun_direction: {
            let direction = world.sun_direction();
            [
                direction[0],
                direction[1],
                direction[2],
                world.sun_intensity,
            ]
        },
        sky_params: [world.turbidity, world.sun_temperature, 0.0, 0.0],
        night_params: [
            world.night_star_density,
            world.night_star_brightness,
            world.night_star_size,
            world.night_horizon_glow,
        ],
        night_color: [
            world.night_star_color[0],
            world.night_star_color[1],
            world.night_star_color[2],
            1.0,
        ],
        lighting_params: [world.ambient_light, 0.0, 0.0, 0.0],
    }
}

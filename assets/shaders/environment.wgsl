fn night_sky(ray: vec3<f32>) -> vec3<f32> {
    let glow = exp(-abs(ray.y) * 7.0) * camera.night_params.w;
    var color = vec3(0.003, 0.005, 0.017) + vec3(0.018, 0.013, 0.031) * glow;
    // Hash spherical cells so stars stay fixed in world space as the camera moves.
    let coordinates = vec2(atan2(ray.z, ray.x) * 0.15915494 + 0.5,
        asin(clamp(ray.y, -1.0, 1.0)) * 0.31830989 + 0.5) * vec2(360.0, 180.0);
    let cell = floor(coordinates);
    let seed = fract(sin(dot(cell, vec2(127.1, 311.7))) * 43758.5453);
    if seed > 1.0 - camera.night_params.x {
        let offset = vec2(
            fract(sin(dot(cell, vec2(269.5, 183.3))) * 43758.5453),
            fract(sin(dot(cell, vec2(419.2, 371.9))) * 43758.5453));
        let center = mix(vec2(0.4), vec2(0.6), offset);
        let distance = length(fract(coordinates) - center);
        let size = mix(0.10, 0.17, offset.y) * camera.night_params.z;
        let core = 1.0 - smoothstep(0.0, size * 0.55, distance);
        let halo = 1.0 - smoothstep(size * 0.4, min(size * 2.2, 0.39), distance);
        let tint = mix(vec3(1.0, 0.78, 0.65), vec3(0.72, 0.85, 1.0), offset.x);
        color += tint * camera.night_color.rgb * (core + halo * 0.45)
            * camera.night_params.y * mix(0.65, 1.25, offset.y);
    }
    return color;
}

fn background(ray: vec3<f32>) -> vec3<f32> {
    if camera.world_mode.x == 2u { return camera.world_color.rgb; }
    if camera.world_mode.x == 3u { return vec3(0.0); }
    if camera.world_mode.x == 4u { return night_sky(ray); }
    if camera.world_mode.x == 1u {
        let sun = normalize(camera.sun_direction.xyz);
        let elevation = sun.y;
        let daylight = smoothstep(-0.14, 0.20, elevation);
        let twilight = smoothstep(-0.25, -0.02, elevation) * (1.0 - smoothstep(0.08, 0.42, elevation));
        let haze = clamp((camera.sky_params.x - 1.0) / 9.0, 0.0, 1.0);
        let horizon = pow(clamp(ray.y * 0.5 + 0.5, 0.0, 1.0), mix(0.65, 1.4, haze));
        let zenith = mix(vec3(0.07, 0.19, 0.44), vec3(0.27, 0.40, 0.51), haze);
        let horizon_color = mix(vec3(0.57, 0.70, 0.87), vec3(0.78, 0.69, 0.56), haze);
        let day = mix(horizon_color, zenith, horizon);
        let night = mix(vec3(0.012, 0.018, 0.038), vec3(0.004, 0.010, 0.030), horizon);
        let sunset = vec3(1.0, 0.28, 0.07) * twilight * exp(-abs(ray.y) * mix(8.0, 3.0, haze));
        let sun_angle = max(dot(ray, sun), 0.0);
        let temperature = clamp((camera.sky_params.y - 2000.0) / 8000.0, 0.0, 1.0);
        let sun_color = mix(vec3(1.0, 0.43, 0.15), vec3(0.87, 0.94, 1.0), temperature);
        let disc = smoothstep(0.99993, 0.999995, sun_angle);
        let glow = pow(sun_angle, mix(55.0, 12.0, haze));
        let above_horizon = smoothstep(-0.025, 0.02, elevation);
        return mix(night, day, daylight) + sunset
            + sun_color * (disc * 8.0 + glow * mix(0.35, 1.2, haze))
                * camera.sun_direction.w * above_horizon;
    }
    let horizon = clamp(ray.y * 0.5 + 0.5, 0.0, 1.0);
    let sky = mix(vec3(0.055, 0.065, 0.085), vec3(0.38, 0.47, 0.62), horizon);
    let softbox = pow(max(dot(ray, normalize(vec3(-0.5, 0.8, 0.4))), 0.0), 36.0);
    let strip = pow(max(dot(ray, normalize(vec3(0.8, 0.3, -0.5))), 0.0), 90.0);
    return sky + vec3(2.8, 2.65, 2.4) * softbox + vec3(1.2, 1.5, 2.0) * strip;
}


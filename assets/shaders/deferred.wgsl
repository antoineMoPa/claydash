// Screen-space direct lighting of a primary-hit geometry buffer.
struct Camera {
    inverse_view_projection: mat4x4<f32>, position: vec4<f32>, count: vec4<u32>,
    world_mode: vec4<u32>, world_color: vec4<f32>, sun_direction: vec4<f32>, sky_params: vec4<f32>,
    view_projection: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var positions: texture_2d<f32>;
@group(1) @binding(1) var normals: texture_2d<f32>;
@group(1) @binding(2) var colors: texture_2d<f32>;
@group(1) @binding(3) var optics: texture_2d<f32>;

struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) clip: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var corners = array<vec2<f32>, 3>(vec2(-1.0, -3.0), vec2(3.0, 1.0), vec2(-1.0, 1.0));
    var output: VertexOutput;
    output.clip = corners[index];
    output.position = vec4(output.clip, 0.0, 1.0);
    return output;
}

fn pixel(uv: vec2<f32>) -> vec2<i32> {
    return clamp(vec2<i32>(uv * vec2<f32>(textureDimensions(positions))), vec2<i32>(0), vec2<i32>(textureDimensions(positions)) - 1);
}

fn background(ray: vec3<f32>) -> vec3<f32> {
    if camera.world_mode.x == 2u { return camera.world_color.rgb; }
    if camera.world_mode.x == 3u { return vec3(0.0); }
    let horizon = clamp(ray.y * 0.5 + 0.5, 0.0, 1.0);
    if camera.world_mode.x == 1u {
        let daylight = smoothstep(-0.14, 0.20, camera.sun_direction.y);
        let day = mix(vec3(0.57, 0.70, 0.87), vec3(0.07, 0.19, 0.44), horizon);
        let night = mix(vec3(0.012, 0.018, 0.038), vec3(0.004, 0.010, 0.030), horizon);
        let sun = pow(max(dot(ray, normalize(camera.sun_direction.xyz)), 0.0), 200.0);
        return mix(night, day, daylight) + vec3(1.0, 0.92, 0.78) * sun * camera.sun_direction.w * daylight;
    }
    let sky = mix(vec3(0.055, 0.065, 0.085), vec3(0.38, 0.47, 0.62), horizon);
    let softbox = pow(max(dot(ray, normalize(vec3(-0.5, 0.8, 0.4))), 0.0), 36.0);
    let strip = pow(max(dot(ray, normalize(vec3(0.8, 0.3, -0.5))), 0.0), 90.0);
    return sky + vec3(2.8, 2.65, 2.4) * softbox + vec3(1.2, 1.5, 2.0) * strip;
}

fn view_direction(position: vec3<f32>) -> vec3<f32> {
    if camera.count.w == 0u { return normalize(camera.position.xyz - position); }
    let near = camera.inverse_view_projection * vec4(0.0, 0.0, 0.0, 1.0);
    let far = camera.inverse_view_projection * vec4(0.0, 0.0, 1.0, 1.0);
    return normalize(near.xyz / near.w - far.xyz / far.w);
}

fn lit(position: vec3<f32>, normal: vec3<f32>, albedo: vec3<f32>, roughness: f32, metallic: f32, reflectivity: f32, ao: f32) -> vec3<f32> {
    let light = select(normalize(vec3(2.0, 3.0, 2.0) - position), normalize(camera.sun_direction.xyz), camera.world_mode.x == 1u);
    let view = view_direction(position);
    let halfway = normalize(light + view);
    let diffuse = max(dot(normal, light), 0.0);
    let specular = pow(max(dot(normal, halfway), 0.0), mix(256.0, 3.0, roughness * roughness));
    let daylight = smoothstep(-0.18, 0.16, camera.sun_direction.y);
    let ambient = select(0.16, mix(0.035, 0.20, daylight), camera.world_mode.x == 1u);
    let direct = select(0.75, camera.sun_direction.w * daylight, camera.world_mode.x == 1u);
    return albedo * (ambient * ao + diffuse * direct * mix(0.55, 1.0, ao)) * (1.0 - metallic)
        + mix(vec3(reflectivity), albedo, metallic) * specular * (1.0 - roughness * 0.5);
}

// SSAO_MODULE
// SSR_MODULE

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let coordinates = vec2<i32>(input.position.xy);
    let position = textureLoad(positions, coordinates, 0);
    if position.w < 0.0 {
        let far = camera.inverse_view_projection * vec4(input.clip, 1.0, 1.0);
        let near = camera.inverse_view_projection * vec4(input.clip, 0.0, 1.0);
        let ray = select(normalize(far.xyz / far.w - camera.position.xyz),
            normalize(far.xyz / far.w - near.xyz / near.w), camera.count.w != 0u);
        if camera.world_mode.x == 3u { return vec4(0.0); }
        return vec4(background(ray), 1.0);
    }
    let surface = textureLoad(normals, coordinates, 0);
    let color = textureLoad(colors, coordinates, 0);
    let ao = occlusion(position.xyz, surface.xyz, coordinates);
    var shaded = lit(position.xyz, surface.xyz, color.xyz, surface.w, color.w, position.w, ao);
    let reflected = reflection_color(position.xyz, surface.xyz, surface.w);
    let view = view_direction(position.xyz);
    let fresnel = pow(1.0 - max(dot(surface.xyz, view), 0.0), 5.0);
    var weight = clamp(position.w + color.w * 0.45 + fresnel * 0.35, 0.0, 0.9)
        * (1.0 - surface.w * 0.7);
    var alpha = 1.0;
    let optical = textureLoad(optics, coordinates, 0);
    if optical.x < 0.999 && color.w < 0.999 {
        let transmitted = transmission_color(position.xyz, surface.xyz, view, optical.y, optical.z);
        // Thin-surface tint; no exit/thickness ray is traced.
        shaded = mix(transmitted.rgb * mix(vec3(1.0), color.rgb, 0.25), shaded, optical.x);
        let ior = max(optical.y, 1.0);
        let f0 = max(position.w, pow((ior - 1.0) / (ior + 1.0), 2.0));
        weight = (f0 + (1.0 - f0) * fresnel) * (1.0 - surface.w * 0.7);
        alpha = mix(transmitted.a, 1.0, max(optical.x, weight));
    }
    let environment = background(reflect(-view, surface.xyz));
    shaded = mix(shaded, mix(environment, reflected.rgb, reflected.a), weight);
    return vec4(shaded, alpha);
}

// TRANSMISSION_MODULE

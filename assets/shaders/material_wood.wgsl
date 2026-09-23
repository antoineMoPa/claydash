// Wood owns six vec4 parameter slots following the two common slots.
const WOOD_RING: u32 = 2u;
const WOOD_SCALE: u32 = 3u;
const WOOD_GROWTH: u32 = 4u;
const WOOD_FIBER: u32 = 5u;
const WOOD_DAMAGE: u32 = 6u;
const WOOD_FINISH: u32 = 7u;
fn wood_hash(cell: vec3<f32>) -> f32 {
    return fract(sin(dot(cell, vec3(127.1, 311.7, 74.7))) * 43758.5453);
}

// Value and analytic gradient of a continuous, seeded 3D fiber field.
fn wood_noise(p: vec3<f32>) -> vec4<f32> {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let du = 6.0 * f * (1.0 - f);
    var value = 0.0;
    var gradient = vec3(0.0);
    for (var corner = 0; corner < 8; corner++) {
        let bit = vec3(f32(corner & 1), f32((corner >> 1) & 1), f32((corner >> 2) & 1));
        let w = mix(vec3(1.0) - u, u, bit);
        let sample = wood_hash(cell + bit);
        value += sample * w.x * w.y * w.z;
        gradient += sample * du * vec3((bit.x * 2.0 - 1.0) * w.y * w.z,
            w.x * (bit.y * 2.0 - 1.0) * w.z, w.x * w.y * (bit.z * 2.0 - 1.0));
    }
    return vec4(value - 0.5, gradient);
}

fn wood_hash2(cell: vec2<f32>) -> f32 {
    return fract(sin(dot(cell, vec2(127.1, 311.7))) * 43758.5453);
}

fn evaluate_wood(point: vec3<f32>, normal: vec3<f32>, object: Object, base: Surface, offset: u32) -> Surface {
    let wood_settings = material_params[offset + WOOD_RING];
    let wood_scale = material_params[offset + WOOD_SCALE];
    let wood_growth = material_params[offset + WOOD_GROWTH];
    let wood_fiber = material_params[offset + WOOD_FIBER];
    let wood_damage = material_params[offset + WOOD_DAMAGE];
    let wood_finish = material_params[offset + WOOD_FINISH];
    var color = base.color;
    var shade_normal = base.normal;
    var roughness = base.roughness;
    var coat = base.coat;
    var sheen = base.sheen;
    var fiber = base.fiber;
    let p = vec4(point, 1.0);
    // The material covers physical stock; resizing geometry exposes more wood.
    let local = vec3(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p)) * object.scale.xyz;
    let cut = wood_growth.x;
    let cc = cos(cut);
    let sc = sin(cut);
    let wood = vec3(local.x, cc * local.y - sc * local.z, sc * local.y + cc * local.z);
    let pixel = max(length(point - camera.position.xyz) * 0.0012, 0.001);
    let spacing = max(wood_settings.x, 0.01);
    let ring_visibility = 1.0 - smoothstep(0.3, 1.0, pixel / spacing);
    let drift = wood_growth.z * vec2(0.035 * sin(wood.y * 2.4), 0.027 * sin(wood.y * 3.7 + 1.6));
    let radial = wood.xz + vec2(0.11, 0.16) + drift;
    let radius = max(length(radial), 0.025);
    let knot_xy = local.xy - vec2(0.18, -0.10);
    let knot_radius = length(knot_xy);
    let branch = wood_damage.z * exp(-knot_radius * knot_radius * 19.0) * smoothstep(-0.3, 0.3, local.z);
    let phase = radius * 6.283185 / spacing
        + wood_growth.z * (0.65 * sin(radius * 7.3 + wood.y * 1.7)
            + 0.21 * sin(radius * 17.0 - wood.y * 2.2))
        + 0.16 * sin(wood.y * 4.0) + branch * 2.4 * sin(atan2(knot_xy.y, knot_xy.x));
    let group = phase / (6.283185 * 12.0);
    let group_index = floor(group);
    let group_tone = mix(wood_hash2(vec2(group_index, 3.0)),
        wood_hash2(vec2(group_index + 1.0, 3.0)), smoothstep(0.0, 1.0, fract(group))) - 0.5;
    let width_shift = 0.12 * wood_growth.z * sin(floor(phase / 6.283185) * 2.37);
    let latewood = mix(0.28, smoothstep(0.34 + width_shift, 0.83 + width_shift, cos(phase)), ring_visibility);
    var fiber_value = 0.0;
    var coarse_fiber = 0.0;
    var figure_fiber = 0.0;
    var fiber_gradient = vec3(0.0);
    for (var octave = 0; octave < 4; octave++) {
        let frequency = select(select(select(52.0, 13.0, octave == 2), 1.8, octave == 1), 0.4, octave == 0);
        let scale = vec3(frequency, frequency / max(wood_fiber.z, 2.0), frequency);
        let sample = wood_noise(wood * scale + vec3(branch * 1.4, 0.0, 0.0));
        let amplitude = select(exp2(-f32(octave - 2) * wood_fiber.w),
            select(0.65, 0.55, octave == 0), octave < 2)
            * (1.0 - smoothstep(0.25, 0.85, pixel * frequency));
        if octave == 0 { coarse_fiber = sample.x; }
        if octave == 1 { figure_fiber = sample.x; }
        fiber_value += amplitude * sample.x;
        fiber_gradient += amplitude * sample.yzw * scale;
    }
    let local_normal = normalize(vec3(dot(object.inverse_rows[0].xyz, normal) * object.scale.x,
        dot(object.inverse_rows[1].xyz, normal) * object.scale.y,
        dot(object.inverse_rows[2].xyz, normal) * object.scale.z));
    let end_grain = abs(cc * local_normal.y - sc * local_normal.z);
    color = mix(color * 1.17, color * 0.63, latewood * 0.76 * wood_settings.y);
    color *= 1.0 + wood_settings.y * (0.60 * group_tone
        + 0.045 * sin(floor(phase / 6.283185) * 2.17));
    color *= 1.0 + wood_fiber.y * (0.35 * fiber_value + 0.40 * coarse_fiber
        + 0.45 * (1.0 - 0.8 * end_grain) * figure_fiber);
    let pore_grid = wood.xz * 55.0;
    let pore_cell = floor(pore_grid);
    let pore_seed = wood_hash2(pore_cell);
    let pore_center = vec2(0.25) + 0.5 * vec2(wood_hash2(pore_cell + vec2(27.0, 7.0)),
        wood_hash2(pore_cell + vec2(12.0, 41.0)));
    let pore_offset = fract(pore_grid) - pore_center;
    let pore_distance = max(length(pore_offset), 0.001);
    let pore_radius = mix(0.045, 0.09, pore_seed);
    let pore_visibility = 1.0 - smoothstep(0.4, 1.5, pixel / (2.0 * pore_radius / 55.0));
    let pore_gate = step(0.73, pore_seed) * (1.0 - 0.55 * latewood);
    let pores = wood_settings.z * pore_gate * (1.0 - smoothstep(pore_radius, pore_radius + 0.04, pore_distance)) * pore_visibility;
    let knot_core = branch * (1.0 - smoothstep(0.06, 0.19, knot_radius));
    color *= 1.0 - 0.46 * pores - 0.56 * knot_core;
    let crack_angle = atan2(radial.y, radial.x) - 0.77 - 0.09 * sin(radius * 10.0);
    let crack = wood_damage.w * smoothstep(0.55, 0.87, end_grain)
        * (1.0 - smoothstep(0.012, 0.056, abs(sin(crack_angle))))
        * smoothstep(0.12, 0.24, radius) * (1.0 - smoothstep(0.48, 0.59, radius));
    color *= 1.0 - 0.78 * crack;
    let sand_angle = wood_damage.y;
    let sand_x = cos(sand_angle) * wood.x + sin(sand_angle) * wood.y;
    let sand_frequency = mix(20.0, 53.0, wood_damage.x);
    let scratch_visibility = 1.0 - smoothstep(0.25, 0.85, pixel * sand_frequency);
    let scratch = sin(sand_x * sand_frequency + 0.27 * sin(wood.z * 8.0));
    color *= 1.0 - scratch_visibility * (0.023 - 0.012 * wood_damage.x) * max(scratch, 0.0);
    var stain = vec3(0.22, 0.48, 0.94);
    if wood_finish.x > 0.5 && wood_finish.x < 1.5 { stain = vec3(0.20, 0.68, 0.91); }
    if wood_finish.x > 1.5 && wood_finish.x < 2.5 { stain = vec3(0.84, 1.18, 1.32); }
    if wood_finish.x > 2.5 { stain = vec3(0.83, 0.78, 0.69); }
    let uptake = wood_finish.y * (0.52 + 0.52 * pores + 0.20 * latewood
        + 0.18 * max(-fiber_value, 0.0));
    color *= exp(-1.25 * uptake * stain);
    coat = wood_finish.z;
    sheen = wood_finish.w;
    color *= (1.0 - 0.12 * coat) * mix(vec3(1.0), vec3(1.035, 0.970, 0.875), coat * wood_scale.w);
    var gradient = vec3(radial.x / radius, 0.0, radial.y / radius)
        * (0.0035 * wood_growth.y * 6.283185 / spacing * cos(phase) * ring_visibility);
    let pore_rim = 1.0 - smoothstep(0.0, 0.11, abs(pore_distance - pore_radius));
    let pore_slope = wood_settings.z * pore_gate * pore_visibility * 0.14 * pore_rim * pore_offset / pore_distance;
    gradient.x += pore_slope.x;
    gradient.z += pore_slope.y;
    gradient += wood_fiber.x * 0.0042 * fiber_gradient;
    gradient += vec3(cos(sand_angle), sin(sand_angle), 0.0) * scratch_visibility
        * (0.085 - 0.06 * wood_damage.x) * cos(sand_x * sand_frequency);
    gradient += vec3(radial.x / radius, 0.0, radial.y / radius) * crack * 0.17;
    let local_gradient = vec3(gradient.x, cc * gradient.y + sc * gradient.z,
        -sc * gradient.y + cc * gradient.z);
    let world_gradient = object.inverse_rows[0].xyz * local_gradient.x * object.scale.x
        + object.inverse_rows[1].xyz * local_gradient.y * object.scale.y
        + object.inverse_rows[2].xyz * local_gradient.z * object.scale.z;
    let slope = world_gradient - normal * dot(world_gradient, normal);
    shade_normal = normalize(normal - wood_growth.w * slope);
    let stock_fiber = normalize(vec3(sin(wood.y * 2.4 + radius * 2.0) * wood_settings.w * 0.35,
        1.0, cos(wood.y * 1.9 + radius) * wood_settings.w * 0.22));
    let local_fiber = vec3(stock_fiber.x, cc * stock_fiber.y + sc * stock_fiber.z,
        -sc * stock_fiber.y + cc * stock_fiber.z);
    fiber = normalize(normalize(object.inverse_rows[0].xyz) * local_fiber.x
        + normalize(object.inverse_rows[1].xyz) * local_fiber.y
        + normalize(object.inverse_rows[2].xyz) * local_fiber.z);
    roughness = clamp(roughness + 0.26 * pores + 0.05 * abs(fiber_value)
        + 0.13 * crack + 0.09 * knot_core - 0.17 * coat, 0.08, 0.96);
    return Surface(color, shade_normal, roughness, base.metallic, base.reflectivity, base.opacity, base.ior, coat, sheen, fiber, wood_settings.w);
}

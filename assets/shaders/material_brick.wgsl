// Three brick slots follow the two common slots: bond, relief, and mineral color.
const BRICK_BOND: u32 = 2u;
const BRICK_RELIEF: u32 = 3u;
const BRICK_MINERAL: u32 = 4u;

struct BrickProfile {
    height: f32,
    clay: f32,
    edge: f32,
    pit: f32,
    cell: vec2<f32>,
}

fn brick_hash(cell: vec2<f32>) -> f32 {
    return fract(sin(dot(cell, vec2(127.1, 311.7))) * 43758.5453);
}

fn brick_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let blend = f * f * (3.0 - 2.0 * f);
    return mix(mix(brick_hash(cell), brick_hash(cell + vec2(1.0, 0.0)), blend.x),
        mix(brick_hash(cell + vec2(0.0, 1.0)), brick_hash(cell + vec2(1.0, 1.0)), blend.x), blend.y);
}

fn brick_micro_height(face: vec2<f32>, relief: vec4<f32>, pixel: f32) -> vec2<f32> {
    let pore_cell = floor(face * 83.0);
    let pore_center = vec2(brick_hash(pore_cell + vec2(11.0, 3.0)),
        brick_hash(pore_cell + vec2(4.0, 17.0)));
    let pore_distance = length(fract(face * 83.0) - pore_center);
    let pore_visibility = 1.0 - smoothstep(0.004, 0.015, pixel);
    let pit = step(mix(0.97, 0.76, relief.z), brick_hash(pore_cell))
        * (1.0 - smoothstep(0.12, 0.38, pore_distance)) * pore_visibility;
    let small_grain = (brick_noise(face * 69.0) - 0.5) * pore_visibility;
    return vec2(pit, -relief.z * (0.22 * pit + 0.11 * max(-small_grain, 0.0)));
}

fn brick_profile(face: vec2<f32>, bond: vec4<f32>, relief: vec4<f32>, pixel: f32) -> BrickProfile {
    let width = max(bond.x, 0.05);
    let height = max(bond.y, 0.05);
    let joint = clamp(bond.z, 0.001, min(width, height) * 0.3);
    let row = floor(face.y / height);
    let offset = 0.5 * (row - 2.0 * floor(row * 0.5));
    let tile = vec2(face.x / width + offset, face.y / height);
    let cell = floor(tile);
    let edge = min(fract(tile), vec2(1.0) - fract(tile)) * vec2(width, height);
    let distance_to_joint = min(edge.x, edge.y);
    let irregular = (brick_noise(face * 37.0) - 0.5) * bond.w * 0.012;
    let chip_cell = floor(face * 53.0);
    let chip = step(0.79, brick_hash(chip_cell)) * bond.w
        * (1.0 - smoothstep(joint * 0.5, joint * 2.0, distance_to_joint)) * 0.008;
    let signed_edge = distance_to_joint - joint * 0.5 + irregular - chip;
    let clay = smoothstep(-0.0015, 0.0025, signed_edge);
    let micro = brick_micro_height(face, relief, pixel);
    let mortar_height = 0.08 + 0.055 * brick_noise(face * 28.0);
    let clay_height = 0.73 + 0.27 * smoothstep(0.0, max(relief.y, 0.002), signed_edge)
        + micro.y;
    return BrickProfile(clamp(mix(mortar_height, clay_height, clay), 0.0, 1.0),
        clay, signed_edge, micro.x, cell);
}

// Stable, low-frequency geometry field. Fine pores stay in the shading layer.
fn brick_structure_height(face: vec2<f32>, bond: vec4<f32>, relief: vec4<f32>) -> f32 {
    let width = max(bond.x, 0.05);
    let height = max(bond.y, 0.05);
    let joint = clamp(bond.z, 0.001, min(width, height) * 0.3);
    let row = floor(face.y / height);
    let offset = 0.5 * (row - 2.0 * floor(row * 0.5));
    let tile = vec2(face.x / width + offset, face.y / height);
    let edge = min(fract(tile), vec2(1.0) - fract(tile)) * vec2(width, height);
    let signed_edge = min(edge.x, edge.y) - joint * 0.5
        + (brick_noise(face * 13.0) - 0.5) * bond.w * 0.006;
    let clay = smoothstep(-0.002, 0.003, signed_edge);
    return mix(0.25, 0.80 + 0.20 * smoothstep(0.0, max(relief.y, 0.002), signed_edge), clay);
}

fn brick_geometry_visible(object: Object) -> bool {
    let header = material_headers[object.component.w];
    if header.kind != MATERIAL_BRICK { return false; }
    let bond = material_params[header.offset + BRICK_BOND];
    let relief = material_params[header.offset + BRICK_RELIEF];
    if relief.x < 0.001 { return false; }
    let eye = vec4(camera.position.xyz, 1.0);
    let local_eye = vec3(dot(object.inverse_rows[0], eye), dot(object.inverse_rows[1], eye),
        dot(object.inverse_rows[2], eye));
    let distance_to_box = max(length(local_eye) - length(object.params.xyz), 0.1) * object.params.w;
    let pixel = 0.83 * distance_to_box / f32(max(camera.count.z, 1u));
    return bond.z > 3.0 * pixel;
}

fn brick_parallax(face: vec2<f32>, view_uv: vec2<f32>, view_normal: f32,
    bond: vec4<f32>, relief: vec4<f32>, pixel: f32) -> vec2<f32> {
    let depth = max(relief.x, 0.0);
    if depth < 0.0005 || view_normal <= 0.05 { return face; }
    let slope = view_uv / max(view_normal, 0.25);
    var previous_face = face;
    var previous_depth = 0.0;
    var previous_gap = -depth * (1.0 - brick_profile(face, bond, relief, pixel).height);
    for (var i = 1; i <= 12; i++) {
        let travel = depth * f32(i) / 12.0;
        let sample_face = face - slope * travel;
        let gap = travel - depth * (1.0 - brick_profile(sample_face, bond, relief, pixel).height);
        if gap >= 0.0 {
            let fraction = clamp(-previous_gap / max(gap - previous_gap, 0.00001), 0.0, 1.0);
            return mix(previous_face, sample_face, fraction);
        }
        previous_face = sample_face;
        previous_depth = travel;
        previous_gap = gap;
    }
    return face - slope * previous_depth;
}

fn brick_cavity_shadow(face: vec2<f32>, height: f32, light_uv: vec2<f32>, light_normal: f32,
    bond: vec4<f32>, relief: vec4<f32>, pixel: f32) -> f32 {
    if relief.x < 0.0005 || light_normal <= 0.05 { return 1.0; }
    let slope = light_uv / max(light_normal, 0.25);
    let base_height = height * relief.x;
    var occlusion = 0.0;
    for (var i = 1; i <= 7; i++) {
        let travel = relief.x * f32(i) / 7.0;
        let surface_height = brick_profile(face + slope * travel, bond, relief, pixel).height * relief.x;
        occlusion = max(occlusion, smoothstep(0.001, 0.005, surface_height - base_height - travel));
    }
    return 1.0 - 0.56 * occlusion;
}

fn evaluate_brick(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, object: Object,
    base: Surface, offset: u32) -> Surface {
    let bond = material_params[offset + BRICK_BOND];
    let relief = material_params[offset + BRICK_RELIEF];
    let mineral = material_params[offset + BRICK_MINERAL];
    let stock = vec4(point, 1.0);
    let local_shape = vec3(dot(object.inverse_rows[0], stock), dot(object.inverse_rows[1], stock),
        dot(object.inverse_rows[2], stock));
    let local = local_shape * object.scale.xyz;
    let local_normal = normalize(vec3(dot(object.inverse_rows[0].xyz, normal) * object.scale.x,
        dot(object.inverse_rows[1].xyz, normal) * object.scale.y,
        dot(object.inverse_rows[2].xyz, normal) * object.scale.z));
    let local_view = normalize(vec3(dot(object.inverse_rows[0].xyz, view) * object.scale.x,
        dot(object.inverse_rows[1].xyz, view) * object.scale.y,
        dot(object.inverse_rows[2].xyz, view) * object.scale.z));
    let light = normalize(vec3(2.0, 3.0, 2.0) - point);
    let local_light = normalize(vec3(dot(object.inverse_rows[0].xyz, light) * object.scale.x,
        dot(object.inverse_rows[1].xyz, light) * object.scale.y,
        dot(object.inverse_rows[2].xyz, light) * object.scale.z));
    var face_normal = local_normal;
    if object.state.y == 2 {
        let q = abs(local_shape) - object.params.xyz;
        if q.x > q.y && q.x > q.z {
            face_normal = vec3(1.0, 0.0, 0.0);
        } else if q.y > q.z {
            face_normal = vec3(0.0, 1.0, 0.0);
        } else {
            face_normal = vec3(0.0, 0.0, 1.0);
        }
    }
    var axis_u = vec3(1.0, 0.0, 0.0);
    var axis_v = vec3(0.0, 1.0, 0.0);
    if abs(face_normal.x) > abs(face_normal.y) && abs(face_normal.x) > abs(face_normal.z) {
        axis_u = vec3(0.0, 0.0, 1.0);
    } else if abs(face_normal.y) > abs(face_normal.z) {
        axis_v = vec3(0.0, 0.0, 1.0);
    }
    let face = vec2(dot(local, axis_u), dot(local, axis_v));
    let screen_pixel = 0.83 * length(point - camera.position.xyz) / f32(max(camera.count.z, 1u));
    let pixel = max(screen_pixel, 0.001);
    let fine_detail = bond.z > 2.0 * screen_pixel;
    let view_uv = vec2(dot(local_view, axis_u), dot(local_view, axis_v));
    var sampled_face = face;
    let true_geometry = object.state.y == 2 && brick_geometry_visible(object);
    if !true_geometry && fine_detail {
        sampled_face = brick_parallax(face, view_uv, dot(local_view, local_normal), bond, relief, pixel);
    }
    let profile = brick_profile(sampled_face, bond, relief, pixel);
    var shade_normal = normal;
    if fine_detail {
        let delta = max(pixel * 0.55, 0.0025);
        var slope_u = 0.0;
        var slope_v = 0.0;
        if true_geometry {
            slope_u = (brick_micro_height(sampled_face + vec2(delta, 0.0), relief, pixel).y
                - brick_micro_height(sampled_face - vec2(delta, 0.0), relief, pixel).y) / (2.0 * delta);
            slope_v = (brick_micro_height(sampled_face + vec2(0.0, delta), relief, pixel).y
                - brick_micro_height(sampled_face - vec2(0.0, delta), relief, pixel).y) / (2.0 * delta);
        } else {
            slope_u = (brick_profile(sampled_face + vec2(delta, 0.0), bond, relief, pixel).height
                - brick_profile(sampled_face - vec2(delta, 0.0), bond, relief, pixel).height) / (2.0 * delta);
            slope_v = (brick_profile(sampled_face + vec2(0.0, delta), bond, relief, pixel).height
                - brick_profile(sampled_face - vec2(0.0, delta), bond, relief, pixel).height) / (2.0 * delta);
        }
        let local_gradient = (axis_u * slope_u + axis_v * slope_v) * relief.x
            * select(1.0, 0.4 * profile.clay, true_geometry);
        let world_gradient = object.inverse_rows[0].xyz * local_gradient.x * object.scale.x
            + object.inverse_rows[1].xyz * local_gradient.y * object.scale.y
            + object.inverse_rows[2].xyz * local_gradient.z * object.scale.z;
        let tangent_gradient = world_gradient - normal * dot(world_gradient, normal);
        shade_normal = normalize(normal - tangent_gradient);
    }
    let batch = brick_hash(profile.cell);
    let firing_cloud = brick_noise(sampled_face * 8.0 + profile.cell * 0.31) - 0.5;
    let sand = brick_noise(sampled_face * 64.0) - 0.5;
    let fired = 1.0 + relief.w * (0.33 * (batch - 0.5) + 0.27 * firing_cloud);
    var clay_color = base.color * fired * (1.0 + 0.24 * sand - 0.38 * profile.pit * relief.z);
    let chalk = smoothstep(0.35, 0.75, brick_noise(sampled_face * 17.0 + vec2(11.0, 3.0)))
        * (1.0 - smoothstep(0.0, max(bond.z * 3.0, 0.01), profile.edge));
    clay_color = mix(clay_color, vec3(0.78, 0.75, 0.69), mineral.w * chalk * 0.36);
    let mortar_grain = brick_noise(sampled_face * 31.0) - 0.5;
    let mortar_color = mineral.rgb * (0.91 + 0.16 * mortar_grain);
    let light_uv = vec2(dot(local_light, axis_u), dot(local_light, axis_v));
    var cavity = 1.0;
    if !true_geometry && fine_detail {
        cavity = brick_cavity_shadow(sampled_face, profile.height, light_uv,
            dot(local_light, local_normal), bond, relief, pixel);
    }
    let contact = mix(0.90, 1.0, smoothstep(0.06, 0.85, profile.height));
    let color = mix(mortar_color, clay_color, profile.clay) * cavity * contact;
    let roughness = clamp(mix(0.97, base.roughness + 0.06 * relief.z * abs(sand)
        + 0.20 * profile.pit, profile.clay), 0.65, 1.0);
    return Surface(color, shade_normal, roughness, base.metallic, base.reflectivity,
        base.opacity, base.ior, base.coat, base.sheen, base.fiber, base.figure);
}

// position.w is display exposure: 1.0 in the viewport, brighter in material previews.
struct Camera { inverse_view_projection: mat4x4<f32>, position: vec4<f32>, count: vec4<u32> }
struct Object {
    state: vec4<i32>,
    color: vec4<f32>,
    inverse_rows: array<vec4<f32>, 3>,
    params: vec4<f32>,
    material: vec4<f32>,
    repeat_spacing: vec4<f32>,
    repeat_count: vec4<i32>,
    component: vec4<u32>,
    wood: vec4<f32>,
    wood_scale: vec4<f32>,
    wood_growth: vec4<f32>,
    wood_fiber: vec4<f32>,
    wood_damage: vec4<f32>,
    wood_finish: vec4<f32>,
}
struct BvhNode { center_radius: vec4<f32>, metadata: vec4<u32>, aabb_min: vec4<f32>, aabb_max: vec4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> objects: array<Object>;
@group(0) @binding(2) var<storage, read> bvh: array<BvhNode>;
override USE_BVH: bool = true;
override HAS_BOOLEANS: bool = false;
const CSG_SIZE: u32 = 256u;

struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) clip: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -3.0), vec2(3.0, 1.0), vec2(-1.0, 1.0));
    var out: VertexOutput;
    out.clip = positions[index];
    out.position = vec4(out.clip, 0.0, 1.0);
    return out;
}

fn repeated_axis(value: f32, spacing: f32, count: i32) -> f32 {
    if count <= 1 { return value; }
    let safe_spacing = max(spacing, 0.001);
    let half = f32(count - 1) * 0.5;
    let cell = clamp(round(value / safe_spacing), -half, half);
    return value - cell * safe_spacing;
}

fn object_distance(point: vec3<f32>, object: Object) -> f32 {
    let homogeneous = vec4(point, 1.0);
    var local = vec3(
        dot(object.inverse_rows[0], homogeneous),
        dot(object.inverse_rows[1], homogeneous),
        dot(object.inverse_rows[2], homogeneous)
    );
    if object.repeat_count.w != 0 {
        local = vec3(
            repeated_axis(local.x, object.repeat_spacing.x, object.repeat_count.x),
            repeated_axis(local.y, object.repeat_spacing.y, object.repeat_count.y),
            repeated_axis(local.z, object.repeat_spacing.z, object.repeat_count.z)
        );
    }
    var distance = 100.0;
    if object.state.y == 1 {
        distance = length(local) - object.params.x;
    } else if object.state.y == 2 {
        let q = abs(local) - object.params.xyz;
        distance = length(max(q, vec3(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
    } else if object.state.y == 3 {
        let q = vec2(length(local.xz) - object.params.x, abs(local.y) - object.params.y);
        distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
    } else if object.state.y == 4 {
        let q = vec2(length(local.xz) - object.params.x, local.y);
        distance = length(q) - object.params.y;
    }
    return distance * object.params.w;
}

fn combine_operand(value: vec2<f32>, child: vec2<f32>, operand: Object, group: Object) -> vec2<f32> {
    let operation = operand.state.z;
    let b = select(child.x, -child.x, operation == 1);
    var result = value;
    if operation == 0 {
        if b < value.x { result = child; }
    } else {
        result.x = max(value.x, b);
        if operation == 2 && b > value.x { result.y = child.y; }
    }
    let softness = bitcast<f32>(group.component.z);
    if softness > 0.0 {
        let h = max(softness - abs(value.x - b), 0.0) / softness;
        let blend = softness * h * h * 0.25;
        result.x += select(blend, -blend, operation == 0);
    }
    return result;
}

// Each leaf is a complete boolean component in contiguous postorder.
// Local scratch is specialized to component size, independent of scene size.
fn component_distance(point: vec3<f32>, start: u32, root: u32) -> vec2<f32> {
    if !HAS_BOOLEANS || start == root {
        return vec2(object_distance(point, objects[root]), f32(root));
    }
    if CSG_SIZE == 2u {
        let child = vec2(object_distance(point, objects[start]), f32(start));
        var value = vec2(object_distance(point, objects[root]), f32(root));
        value = combine_operand(value, child, objects[start], objects[root]);
        return value;
    }
    var values: array<vec2<f32>, CSG_SIZE>;
    for (var i = start; i <= root; i++) {
        values[i - start] = vec2(object_distance(point, objects[i]), f32(i));
    }
    for (var i = start; i < root; i++) {
        let parent = u32(objects[i].state.w) - start;
        let child = values[i - start];
        var value = values[parent];
        value = combine_operand(value, child, objects[i], objects[parent + start]);
        values[parent] = value;
    }
    return values[root - start];
}

fn scene_distance(point: vec3<f32>) -> vec2<f32> {
    var closest = vec2(100.0, 0.0);
    var node_index = 0u;
    while node_index < camera.count.y {
        let node = bvh[node_index];
        let lower_bound = distance(point, node.center_radius.xyz) - node.center_radius.w;
        if !USE_BVH || lower_bound < closest.x {
            if node.metadata.x != 0xffffffffu {
                let candidate = component_distance(point, node.metadata.z, node.metadata.x);
                if candidate.x < closest.x { closest = candidate; }
            }
            node_index += 1u;
        } else { node_index = node.metadata.y; }
    }
    return closest;
}

fn scene_normal(point: vec3<f32>, index: u32) -> vec3<f32> {
    let e = 0.003;
    let a = vec3(1.0, -1.0, -1.0);
    let b = vec3(-1.0, -1.0, 1.0);
    let c = vec3(-1.0, 1.0, -1.0);
    let d = vec3(1.0, 1.0, 1.0);
    if !HAS_BOOLEANS {
        let object = objects[index];
        return normalize(a * object_distance(point + a * e, object)
            + b * object_distance(point + b * e, object)
            + c * object_distance(point + c * e, object)
            + d * object_distance(point + d * e, object));
    }
    return normalize(
        a * component_distance(point + a * e, objects[index].component.x, objects[index].component.y).x +
        b * component_distance(point + b * e, objects[index].component.x, objects[index].component.y).x +
        c * component_distance(point + c * e, objects[index].component.x, objects[index].component.y).x +
        d * component_distance(point + d * e, objects[index].component.x, objects[index].component.y).x
    );
}

fn background(ray: vec3<f32>) -> vec3<f32> {
    let horizon = clamp(ray.y * 0.5 + 0.5, 0.0, 1.0);
    let sky = mix(vec3(0.055, 0.065, 0.085), vec3(0.38, 0.47, 0.62), horizon);
    let softbox = pow(max(dot(ray, normalize(vec3(-0.5, 0.8, 0.4))), 0.0), 36.0);
    let strip = pow(max(dot(ray, normalize(vec3(0.8, 0.3, -0.5))), 0.0), 90.0);
    return sky + vec3(2.8, 2.65, 2.4) * softbox + vec3(1.2, 1.5, 2.0) * strip;
}

// Exact local-space ray intervals for the common convex primitives. Transform
// the direction without changing its length so interval values stay in world t.
fn primitive_interval(origin: vec3<f32>, direction: vec3<f32>, object: Object) -> vec2<f32> {
    let p = vec4(origin, 1.0);
    let v = vec4(direction, 0.0);
    let o = vec3(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p));
    let d = vec3(dot(object.inverse_rows[0], v), dot(object.inverse_rows[1], v), dot(object.inverse_rows[2], v));
    if object.state.y == 1 {
        let a = dot(d, d);
        let b = dot(o, d);
        let c = dot(o, o) - object.params.x * object.params.x;
        let discriminant = b * b - a * c;
        if discriminant < 0.0 { return vec2(1.0, -1.0); }
        let chord = sqrt(discriminant);
        return vec2(-b - chord, -b + chord) / a;
    }
    let safe_d = select(vec3(-1.0), vec3(1.0), d >= vec3(0.0)) * max(abs(d), vec3(1e-20));
    if object.state.y == 2 {
        let first = (-object.params.xyz - o) / safe_d;
        let second = (object.params.xyz - o) / safe_d;
        let entry = min(first, second);
        let exit = max(first, second);
        return vec2(max(entry.x, max(entry.y, entry.z)), min(exit.x, min(exit.y, exit.z)));
    }
    let a = dot(d.xz, d.xz);
    let b = dot(o.xz, d.xz);
    let c = dot(o.xz, o.xz) - object.params.x * object.params.x;
    var radial = vec2(-1e20, 1e20);
    if a < 1e-20 {
        if c > 0.0 { return vec2(1.0, -1.0); }
    } else {
        let discriminant = b * b - a * c;
        if discriminant < 0.0 { return vec2(1.0, -1.0); }
        let chord = sqrt(discriminant);
        radial = vec2(-b - chord, -b + chord) / a;
    }
    let caps = (vec2(-object.params.y, object.params.y) - o.y) / safe_d.y;
    return vec2(max(radial.x, min(caps.x, caps.y)), min(radial.y, max(caps.x, caps.y)));
}

fn has_analytic_interval(object: Object) -> bool {
    return object.repeat_count.w == 0 && object.state.y <= 3;
}

// Traverse bounds once per ray, then march only the intersected primitives.
// The nearest boundary of a union is the nearest primitive boundary for rays
// starting outside. Interior rays retain the union marcher to cross overlaps.
fn trace_objects(origin: vec3<f32>, direction: vec3<f32>, epsilon: f32) -> vec2<f32> {
    var closest = 100.0;
    var owner = -1.0;
    let inverse_direction = vec3(1.0) / (select(vec3(-1.0), vec3(1.0), direction >= vec3(0.0)) * max(abs(direction), vec3(1e-20)));
    var node_index = 0u;
    while node_index < camera.count.y {
        let node = bvh[node_index];
        let first = (node.aabb_min.xyz - vec3(0.01) - origin) * inverse_direction;
        let second = (node.aabb_max.xyz + vec3(0.01) - origin) * inverse_direction;
        let entry = min(first, second);
        let exit = max(first, second);
        let start = max(0.0, max(entry.x, max(entry.y, entry.z)));
        let end = min(closest, min(exit.x, min(exit.y, exit.z)));
        if start > end {
            node_index = node.metadata.y;
            continue;
        }
        if node.metadata.x != 0xffffffffu {
            let object = objects[node.metadata.x];
            if node.metadata.z == node.metadata.x && has_analytic_interval(object) {
                let interval = primitive_interval(origin, direction, object);
                let candidate = max(0.0, interval.x);
                if interval.y >= candidate && candidate < closest {
                    closest = candidate;
                    owner = f32(node.metadata.x);
                }
                node_index += 1u;
                continue;
            }
            var travel = start;
            for (var step = 0; step < 128; step++) {
                let sample = component_distance(origin + direction * travel, node.metadata.z, node.metadata.x);
                let value = sample.x;
                if value < epsilon {
                    closest = travel;
                    owner = sample.y;
                    break;
                }
                travel += value * 0.8;
                if travel > end { break; }
            }
        }
        node_index += 1u;
    }
    return vec2(closest, owner);
}

// A membership query only needs any containing component. Unlike a nearest
// distance query it prunes against zero from the first node and can exit early.
fn containing_component(point: vec3<f32>) -> vec2<f32> {
    var node_index = 0u;
    while node_index < camera.count.y {
        let node = bvh[node_index];
        if any(point < node.aabb_min.xyz) || any(point > node.aabb_max.xyz) {
            node_index = node.metadata.y;
            continue;
        }
        if node.metadata.x != 0xffffffffu {
            let sample = component_distance(point, node.metadata.z, node.metadata.x);
            if sample.x < -0.0015 { return sample; }
        }
        node_index += 1u;
    }
    return vec2(0.0, -1.0);
}

// Reflections stay on the incident side; transmission crosses the surface.
// The caller already knows which side contains the ray, avoiding a scene query.
fn trace(origin: vec3<f32>, direction: vec3<f32>, inside: bool, initial_owner: u32) -> vec2<f32> {
    if USE_BVH {
        if !inside { return trace_objects(origin, direction, 0.0015); }
        var owner = initial_owner;
        var travel = 0.0;
        for (var step = 0; step < 128; step++) {
            let component = objects[owner].component;
            var sample = vec2(0.0, f32(owner));
            if component.x == component.y && has_analytic_interval(objects[owner]) {
                let interval = primitive_interval(origin, direction, objects[owner]);
                travel = max(travel, interval.y);
            } else {
                sample = component_distance(origin + direction * travel, component.x, component.y);
            }
            if abs(sample.x) < 0.0015 {
                let other = containing_component(origin + direction * travel);
                if other.y < 0.0 { return vec2(travel, sample.y); }
                owner = u32(other.y);
                sample = other;
            }
            travel += max(abs(sample.x) * 0.8, 0.0008);
            if travel > 100.0 { break; }
        }
        return vec2(100.0, -1.0);
    }
    var travel = 0.0;
    for (var step = 0; step < 128; step++) {
        let sample = scene_distance(origin + direction * travel);
        if abs(sample.x) < 0.0015 { return vec2(travel, sample.y); }
        travel += max(abs(sample.x) * 0.8, 0.0008);
        if travel > 100.0 { break; }
    }
    return vec2(100.0, -1.0);
}

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

fn surface_light(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, object: Object) -> vec3<f32> {
    var color = object.color.rgb;
    var shade_normal = normal;
    var roughness = clamp(object.material.x, 0.03, 1.0);
    var coat = 0.0;
    var sheen = 0.0;
    var fiber = vec3(0.0, 1.0, 0.0);
    if object.component.w == 1u {
        let p = vec4(point, 1.0);
        // The material covers physical stock; resizing geometry exposes more wood.
        let local = vec3(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p)) * object.wood_scale.xyz;
        let cut = object.wood_growth.x;
        let cc = cos(cut);
        let sc = sin(cut);
        let wood = vec3(local.x, cc * local.y - sc * local.z, sc * local.y + cc * local.z);
        let pixel = max(length(point - camera.position.xyz) * 0.0012, 0.001);
        let spacing = max(object.wood.x, 0.01);
        let ring_visibility = 1.0 - smoothstep(0.3, 1.0, pixel / spacing);
        let drift = object.wood_growth.z * vec2(0.035 * sin(wood.y * 2.4), 0.027 * sin(wood.y * 3.7 + 1.6));
        let radial = wood.xz + vec2(0.11, 0.16) + drift;
        let radius = max(length(radial), 0.025);
        let knot_xy = local.xy - vec2(0.18, -0.10);
        let knot_radius = length(knot_xy);
        let branch = object.wood_damage.z * exp(-knot_radius * knot_radius * 19.0) * smoothstep(-0.3, 0.3, local.z);
        let phase = radius * 6.283185 / spacing
            + object.wood_growth.z * (0.65 * sin(radius * 7.3 + wood.y * 1.7)
                + 0.21 * sin(radius * 17.0 - wood.y * 2.2))
            + 0.16 * sin(wood.y * 4.0) + branch * 2.4 * sin(atan2(knot_xy.y, knot_xy.x));
        let group = phase / (6.283185 * 12.0);
        let group_index = floor(group);
        let group_tone = mix(wood_hash2(vec2(group_index, 3.0)),
            wood_hash2(vec2(group_index + 1.0, 3.0)), smoothstep(0.0, 1.0, fract(group))) - 0.5;
        let width_shift = 0.12 * object.wood_growth.z * sin(floor(phase / 6.283185) * 2.37);
        let latewood = mix(0.28, smoothstep(0.34 + width_shift, 0.83 + width_shift, cos(phase)), ring_visibility);
        var fiber_value = 0.0;
        var coarse_fiber = 0.0;
        var figure_fiber = 0.0;
        var fiber_gradient = vec3(0.0);
        for (var octave = 0; octave < 4; octave++) {
            let frequency = select(select(select(52.0, 13.0, octave == 2), 1.8, octave == 1), 0.4, octave == 0);
            let scale = vec3(frequency, frequency / max(object.wood_fiber.z, 2.0), frequency);
            let sample = wood_noise(wood * scale + vec3(branch * 1.4, 0.0, 0.0));
            let amplitude = select(exp2(-f32(octave - 2) * object.wood_fiber.w),
                select(0.65, 0.55, octave == 0), octave < 2)
                * (1.0 - smoothstep(0.25, 0.85, pixel * frequency));
            if octave == 0 { coarse_fiber = sample.x; }
            if octave == 1 { figure_fiber = sample.x; }
            fiber_value += amplitude * sample.x;
            fiber_gradient += amplitude * sample.yzw * scale;
        }
        let local_normal = normalize(vec3(dot(object.inverse_rows[0].xyz, normal) * object.wood_scale.x,
            dot(object.inverse_rows[1].xyz, normal) * object.wood_scale.y,
            dot(object.inverse_rows[2].xyz, normal) * object.wood_scale.z));
        let end_grain = abs(cc * local_normal.y - sc * local_normal.z);
        color = mix(color * 1.17, color * 0.63, latewood * 0.76 * object.wood.y);
        color *= 1.0 + object.wood.y * (0.60 * group_tone
            + 0.045 * sin(floor(phase / 6.283185) * 2.17));
        color *= 1.0 + object.wood_fiber.y * (0.35 * fiber_value + 0.40 * coarse_fiber
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
        let pores = object.wood.z * pore_gate * (1.0 - smoothstep(pore_radius, pore_radius + 0.04, pore_distance)) * pore_visibility;
        let knot_core = branch * (1.0 - smoothstep(0.06, 0.19, knot_radius));
        color *= 1.0 - 0.46 * pores - 0.56 * knot_core;
        let crack_angle = atan2(radial.y, radial.x) - 0.77 - 0.09 * sin(radius * 10.0);
        let crack = object.wood_damage.w * smoothstep(0.55, 0.87, end_grain)
            * (1.0 - smoothstep(0.012, 0.056, abs(sin(crack_angle))))
            * smoothstep(0.12, 0.24, radius) * (1.0 - smoothstep(0.48, 0.59, radius));
        color *= 1.0 - 0.78 * crack;
        let sand_angle = object.wood_damage.y;
        let sand_x = cos(sand_angle) * wood.x + sin(sand_angle) * wood.y;
        let sand_frequency = mix(20.0, 53.0, object.wood_damage.x);
        let scratch_visibility = 1.0 - smoothstep(0.25, 0.85, pixel * sand_frequency);
        let scratch = sin(sand_x * sand_frequency + 0.27 * sin(wood.z * 8.0));
        color *= 1.0 - scratch_visibility * (0.023 - 0.012 * object.wood_damage.x) * max(scratch, 0.0);
        var stain = vec3(0.22, 0.48, 0.94);
        if object.wood_finish.x > 0.5 && object.wood_finish.x < 1.5 { stain = vec3(0.20, 0.68, 0.91); }
        if object.wood_finish.x > 1.5 && object.wood_finish.x < 2.5 { stain = vec3(0.84, 1.18, 1.32); }
        if object.wood_finish.x > 2.5 { stain = vec3(0.83, 0.78, 0.69); }
        let uptake = object.wood_finish.y * (0.52 + 0.52 * pores + 0.20 * latewood
            + 0.18 * max(-fiber_value, 0.0));
        color *= exp(-1.25 * uptake * stain);
        coat = object.wood_finish.z;
        sheen = object.wood_finish.w;
        color *= (1.0 - 0.12 * coat) * mix(vec3(1.0), vec3(1.035, 0.970, 0.875), coat * object.wood_scale.w);
        var gradient = vec3(radial.x / radius, 0.0, radial.y / radius)
            * (0.0035 * object.wood_growth.y * 6.283185 / spacing * cos(phase) * ring_visibility);
        let pore_rim = 1.0 - smoothstep(0.0, 0.11, abs(pore_distance - pore_radius));
        let pore_slope = object.wood.z * pore_gate * pore_visibility * 0.14 * pore_rim * pore_offset / pore_distance;
        gradient.x += pore_slope.x;
        gradient.z += pore_slope.y;
        gradient += object.wood_fiber.x * 0.0042 * fiber_gradient;
        gradient += vec3(cos(sand_angle), sin(sand_angle), 0.0) * scratch_visibility
            * (0.085 - 0.06 * object.wood_damage.x) * cos(sand_x * sand_frequency);
        gradient += vec3(radial.x / radius, 0.0, radial.y / radius) * crack * 0.17;
        let local_gradient = vec3(gradient.x, cc * gradient.y + sc * gradient.z,
            -sc * gradient.y + cc * gradient.z);
        let world_gradient = object.inverse_rows[0].xyz * local_gradient.x * object.wood_scale.x
            + object.inverse_rows[1].xyz * local_gradient.y * object.wood_scale.y
            + object.inverse_rows[2].xyz * local_gradient.z * object.wood_scale.z;
        let slope = world_gradient - normal * dot(world_gradient, normal);
        shade_normal = normalize(normal - object.wood_growth.w * slope);
        let stock_fiber = normalize(vec3(sin(wood.y * 2.4 + radius * 2.0) * object.wood.w * 0.35,
            1.0, cos(wood.y * 1.9 + radius) * object.wood.w * 0.22));
        let local_fiber = vec3(stock_fiber.x, cc * stock_fiber.y + sc * stock_fiber.z,
            -sc * stock_fiber.y + cc * stock_fiber.z);
        fiber = normalize(normalize(object.inverse_rows[0].xyz) * local_fiber.x
            + normalize(object.inverse_rows[1].xyz) * local_fiber.y
            + normalize(object.inverse_rows[2].xyz) * local_fiber.z);
        roughness = clamp(roughness + 0.26 * pores + 0.05 * abs(fiber_value)
            + 0.13 * crack + 0.09 * knot_core - 0.17 * coat, 0.08, 0.96);
    }
    let light = normalize(vec3(2.0, 3.0, 2.0) - point);
    let halfway = normalize(light + view);
    let metallic = object.material.y;
    let diffuse = max(dot(normalize(mix(shade_normal, normal, 0.35 * coat)), light), 0.0);
    let specular = pow(max(dot(shade_normal, halfway), 0.0), mix(256.0, 3.0, roughness * roughness));
    let specular_color = mix(vec3(object.material.z), color, metallic);
    var fiber_light = 0.0;
    var coat_light = 0.0;
    if object.component.w == 1u {
        fiber_light = pow(max(1.0 - abs(dot(halfway, fiber)), 0.0), 18.0) * object.wood.w * 0.11 * diffuse;
        let coat_normal = normalize(mix(shade_normal, normal, 0.58 * coat));
        let reflection = reflect(-view, coat_normal);
        let strip_width = mix(0.14, 0.035, sheen);
        let strip = exp(-pow((reflection.x - 0.20) / strip_width, 2.0));
        coat_light = coat * (pow(max(dot(coat_normal, halfway), 0.0), mix(15.0, 145.0, sheen))
            * mix(0.22, 0.43, sheen) + 0.12 * strip);
    }
    let ambient = select(0.13, 0.23, object.component.w == 1u);
    return color * (ambient + diffuse * 0.75) * (1.0 - metallic)
        + specular_color * specular * (1.0 - roughness * 0.5) * (1.0 - 0.72 * coat)
        + color * fiber_light + vec3(1.0, 0.98, 0.93) * coat_light;
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let far = camera.inverse_view_projection * vec4(input.clip, 1.0, 1.0);
    let near = camera.inverse_view_projection * vec4(input.clip, 0.0, 1.0);
    let far_point = far.xyz / far.w;
    let near_point = near.xyz / near.w;
    let ray = select(normalize(far_point - camera.position.xyz), normalize(far_point - near_point), camera.count.w != 0u);
    let ray_origin = select(camera.position.xyz, near_point, camera.count.w != 0u);
    var point = ray_origin;
    var hit = false;
    var index = 0u;
    if USE_BVH {
        let sample = trace_objects(ray_origin, ray, 0.003);
        hit = sample.y >= 0.0;
        index = u32(max(sample.y, 0.0));
        point += ray * sample.x;
    } else {
        var travel = 0.0;
        for (var step = 0; step < 96; step++) {
            let scene = scene_distance(point);
            if scene.x < 0.003 { hit = true; index = u32(scene.y); break; }
            travel += scene.x * 0.8;
            if travel > 100.0 { break; }
            point = ray_origin + ray * travel;
        }
    }
    let sky = background(ray);
    if !hit { return vec4(sky, 1.0); }

    var direction = ray;
    var throughput = vec3(1.0);
    var radiance = vec3(0.0);
    let selected = f32(objects[index].state.x) * 0.12;
    var reflecting = false;
    var reflection_weight = vec3(0.0);
    var transmission_origin = vec3(0.0);
    var transmission_direction = vec3(0.0);
    var transmission_inside = false;
    var transmission_owner = 0u;
    var opaque = false;
    var bounce = 0;
    // Reflection and transmission share a single tracing call site to reduce
    // live shader state on integrated GPUs. Retain six surface interactions.
    for (var action = 0; action < 12; action++) {
        var next_origin = point;
        var next_direction = direction;
        var next_inside = false;
        var next_owner = index;
        if reflecting {
            var reflected_color = background(direction);
            if hit {
                reflected_color = surface_light(point, scene_normal(point, index), -direction, objects[index]);
            }
            radiance += reflection_weight * reflected_color;
            reflecting = false;
            if opaque { break; }
            next_origin = transmission_origin;
            next_direction = transmission_direction;
            next_inside = transmission_inside;
            next_owner = transmission_owner;
        } else {
            if !hit {
                radiance += throughput * background(direction);
                throughput = vec3(0.0);
                break;
            }
            if bounce >= 6 { break; }
            bounce += 1;
            let object = objects[index];
            let outward = scene_normal(point, index);
            let entering = dot(direction, outward) < 0.0;
            let normal = select(-outward, outward, entering);
            let ior = max(object.repeat_spacing.w, 1.0);
            let f0 = pow((ior - 1.0) / (ior + 1.0), 2.0);
            let fresnel = f0 + (1.0 - f0) * pow(1.0 - max(dot(-direction, normal), 0.0), 5.0);
            let reflection = reflect(direction, normal);
            let opacity = clamp(object.material.w, 0.0, 1.0);
            let metallic = clamp(object.material.y, 0.0, 1.0);
            let eta = select(ior, 1.0 / ior, entering);
            let transmitted = refract(direction, normal, eta);
            next_origin = point + normal * 0.007;
            next_direction = reflection;
            next_inside = !entering;
            if !(opacity < 0.999 && metallic < 0.999 && dot(transmitted, transmitted) < 0.001) {
                let roughness = clamp(object.material.x, 0.0, 1.0);
                let weight = clamp(max(fresnel, object.material.z), 0.0, 1.0);
                let tint = mix(vec3(1.0), object.color.rgb, metallic);
                reflection_weight = throughput * tint * weight * (1.0 - roughness * roughness);
                radiance += throughput * (vec3(0.22, 0.27, 0.35) * roughness * roughness * tint * weight
                    + surface_light(point, normal, -direction, object) * opacity * (1.0 - weight));
                transmission_direction = transmitted;
                transmission_origin = point - normal * 0.007;
                transmission_inside = entering;
                transmission_owner = index;
                throughput *= (1.0 - weight) * (1.0 - opacity) * mix(vec3(1.0), object.color.rgb, 0.12);
                opaque = opacity >= 0.999 || metallic >= 0.999;
                reflecting = true;
                // After the first glass interface, the reflected branch has
                // little energy. Keep Fresnel/environment lighting while the
                // transmitted branch continues to resolve scene geometry.
                if (bounce > 1 && !opaque) || max(reflection_weight.x, max(reflection_weight.y, reflection_weight.z)) == 0.0 {
                    radiance += reflection_weight * background(reflection);
                    reflecting = false;
                    if opaque { break; }
                    next_origin = transmission_origin;
                    next_direction = transmission_direction;
                    next_inside = transmission_inside;
                }
            }
        }
        let next = trace(next_origin, next_direction, next_inside, next_owner);
        hit = next.y >= 0.0;
        point = next_origin + next_direction * next.x;
        direction = next_direction;
        index = u32(max(next.y, 0.0));
    }
    // The final transmission miss contributes the environment even at the
    // bounce cap, just as it does after each earlier transmitted ray.
    if !reflecting && !hit && !opaque { radiance += throughput * background(direction); }
    return vec4((radiance + selected) * camera.position.w, 1.0);
}

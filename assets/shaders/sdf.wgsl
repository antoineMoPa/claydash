// position.w is display exposure: 1.0 in the viewport, brighter in material previews.
// count: object count, BVH node count, viewport height, orthographic flag.
struct Camera { inverse_view_projection: mat4x4<f32>, position: vec4<f32>, count: vec4<u32> }
struct Object {
    state: vec4<i32>,
    color: vec4<f32>,
    inverse_rows: array<vec4<f32>, 3>,
    group_inverse_rows: array<vec4<f32>, 3>,
    params: vec4<f32>,
    repeat_spacing: vec4<f32>,
    repeat_count: vec4<i32>,
    component: vec4<u32>,
    scale: vec4<f32>,
    modifier: vec4<u32>,
    mirror_axes: vec4<u32>,
}
struct BvhNode { center_radius: vec4<f32>, metadata: vec4<u32>, aabb_min: vec4<f32>, aabb_max: vec4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> objects: array<Object>;
@group(0) @binding(2) var<storage, read> bvh: array<BvhNode>;
@group(0) @binding(5) var<storage, read> polygon_points: array<vec2<f32>>;
override USE_BVH: bool = true;
override HAS_BOOLEANS: bool = false;
override TRANSPARENT_BACKGROUND: bool = false;
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

fn group_repeat_point(point: vec3<f32>, root: Object) -> vec3<f32> {
    let homogeneous = vec4(point, 1.0);
    let local = vec3(
        dot(root.group_inverse_rows[0], homogeneous),
        dot(root.group_inverse_rows[1], homogeneous),
        dot(root.group_inverse_rows[2], homogeneous)
    );
    let folded = vec3(
        repeated_axis(local.x, root.repeat_spacing.x, root.repeat_count.x),
        repeated_axis(local.y, root.repeat_spacing.y, root.repeat_count.y),
        repeated_axis(local.z, root.repeat_spacing.z, root.repeat_count.z)
    );
    let delta = local - folded;
    let row_x = root.group_inverse_rows[0].xyz;
    let row_y = root.group_inverse_rows[1].xyz;
    let row_z = root.group_inverse_rows[2].xyz;
    let determinant = dot(row_x, cross(row_y, row_z));
    if abs(determinant) < 1e-8 { return point; }
    let world_delta = (
        cross(row_y, row_z) * delta.x
        + cross(row_z, row_x) * delta.y
        + cross(row_x, row_y) * delta.z
    ) / determinant;
    return point - world_delta;
}

fn mirror_point(point: vec3<f32>, root: Object) -> vec3<f32> {
    if all(root.mirror_axes.xyz == vec3<u32>(0u)) { return point; }
    let homogeneous = vec4(point, 1.0);
    let local = vec3(
        dot(root.group_inverse_rows[0], homogeneous),
        dot(root.group_inverse_rows[1], homogeneous),
        dot(root.group_inverse_rows[2], homogeneous)
    );
    let folded = vec3(
        select(local.x, abs(local.x), root.mirror_axes.x != 0u),
        select(local.y, abs(local.y), root.mirror_axes.y != 0u),
        select(local.z, abs(local.z), root.mirror_axes.z != 0u)
    );
    let delta = local - folded;
    let row_x = root.group_inverse_rows[0].xyz;
    let row_y = root.group_inverse_rows[1].xyz;
    let row_z = root.group_inverse_rows[2].xyz;
    let determinant = dot(row_x, cross(row_y, row_z));
    if abs(determinant) < 1e-8 { return point; }
    let world_delta = (
        cross(row_y, row_z) * delta.x
        + cross(row_z, row_x) * delta.y
        + cross(row_x, row_y) * delta.z
    ) / determinant;
    return point - world_delta;
}

fn polygon_distance(point: vec2<f32>, offset: u32, count: u32) -> f32 {
    if count < 3u { return 100.0; }
    var distance_squared = 1e20;
    var inside = false;
    for (var index = 0u; index < 65u; index++) {
        if index >= count { break; }
        let next = select(index + 1u, 0u, index + 1u == count);
        let a = polygon_points[offset + index];
        let b = polygon_points[offset + next];
        let edge = b - a;
        let relative = point - a;
        let edge_length_squared = dot(edge, edge);
        if edge_length_squared > 0.0000001 {
            let closest = a + edge * clamp(dot(relative, edge) / edge_length_squared, 0.0, 1.0);
            let delta = point - closest;
            distance_squared = min(distance_squared, dot(delta, delta));
        }
        if (a.y > point.y) != (b.y > point.y) {
            let crossing_x = a.x + (point.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if point.x < crossing_x { inside = !inside; }
        }
    }
    return sqrt(distance_squared) * select(1.0, -1.0, inside);
}

fn bezier_control(offset: u32, index: u32) -> vec3<f32> {
    let xy = polygon_points[offset + index * 2u];
    let z = polygon_points[offset + index * 2u + 1u].x;
    return vec3(xy, z);
}

fn bezier_tangent(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, d: vec3<f32>, t: f32) -> vec3<f32> {
    let u = 1.0 - t;
    return (b - a) * (3.0 * u * u) + (c - b) * (6.0 * u * t) + (d - c) * (3.0 * t * t);
}

// Minimize distance to each cubic in the chain. The seed search avoids Newton
// converging from an unrelated part of a curved segment.
fn bezier_extrusion_distance(point: vec3<f32>, object: Object) -> f32 {
    let radius = object.params.x;
    let segment_count = object.mirror_axes.w & 0x7fffffffu;
    let closed = (object.mirror_axes.w & 0x80000000u) != 0u;
    if radius <= 0.0 || segment_count == 0u { return 100.0; }
    let offset = bitcast<u32>(object.params.y);
    let profile_count = u32(object.scale.w);
    var best_distance = 1e20;
    var best_position = vec3(0.0);
    var best_tangent = vec3(1.0, 0.0, 0.0);
    var best_t = 0.0;
    var best_segment = 0u;
    for (var segment = 0u; segment < 8u; segment++) {
        if segment >= segment_count { break; }
        let base = segment * 3u;
        let a = bezier_control(offset, base);
        let b = bezier_control(offset, base + 1u);
        let c = bezier_control(offset, base + 2u);
        let d = bezier_control(offset, base + 3u);
        // A cubic stays inside the convex hull of its four controls. Once a
        // closer segment is known, this bound can skip the seed and Newton work.
        let lower = min(min(a, b), min(c, d));
        let upper = max(max(a, b), max(c, d));
        let outside = max(max(lower - point, point - upper), vec3(0.0));
        if dot(outside, outside) >= best_distance { continue; }
        var seed = 0.0;
        var seed_distance = 1e20;
        // Evaluate the same 13 uniform seeds by forward differences, using
        // additions in the loop instead of expanding the cubic each time.
        let h = 1.0 / 12.0;
        let linear = (b - a) * 3.0;
        let quadratic = (a - b * 2.0 + c) * 3.0;
        let cubic = d - a + (b - c) * 3.0;
        var sample_position = a;
        var sample_step = linear * h + quadratic * (h * h) + cubic * (h * h * h);
        var sample_second = quadratic * (2.0 * h * h) + cubic * (6.0 * h * h * h);
        let sample_third = cubic * (6.0 * h * h * h);
        for (var step = 0u; step <= 12u; step++) {
            let t = f32(step) / 12.0;
            let delta = sample_position - point;
            let distance = dot(delta, delta);
            if distance < seed_distance { seed_distance = distance; seed = t; }
            sample_position += sample_step;
            sample_step += sample_second;
            sample_second += sample_third;
        }
        var t = seed;
        for (var iteration = 0u; iteration < 4u; iteration++) {
            let delta = ((cubic * t + quadratic) * t + linear) * t + a - point;
            let tangent = (cubic * (3.0 * t) + quadratic * 2.0) * t + linear;
            let second = cubic * (6.0 * t) + quadratic * 2.0;
            let denominator = dot(tangent, tangent) + dot(delta, second);
            if abs(denominator) < 1e-6 { break; }
            t = clamp(t - dot(delta, tangent) / denominator, 0.0, 1.0);
        }
        if profile_count == 0u {
            let position = ((cubic * t + quadratic) * t + linear) * t + a;
            let delta = position - point;
            best_distance = min(best_distance, min(seed_distance, dot(delta, delta)));
            continue;
        }
        for (var candidate_index = 0u; candidate_index < 4u; candidate_index++) {
            let candidate = select(select(select(0.0, seed, candidate_index == 1u), t, candidate_index == 2u), 1.0, candidate_index == 3u);
            let position = ((cubic * candidate + quadratic) * candidate + linear) * candidate + a;
            let delta = position - point;
            let distance = dot(delta, delta);
            if distance < best_distance {
                best_distance = distance;
                best_position = position;
                best_tangent = (cubic * (3.0 * candidate) + quadratic * 2.0) * candidate + linear;
                best_t = candidate;
                best_segment = segment;
            }
        }
    }
    if profile_count == 0u { return sqrt(best_distance) - radius; }
    let delta = point - best_position;
    let initial = bezier_tangent(bezier_control(offset, 0u), bezier_control(offset, 1u),
        bezier_control(offset, 2u), bezier_control(offset, 3u), 0.0);
    var start = vec3(1.0, 0.0, 0.0);
    if dot(initial, initial) > 1e-8 { start = normalize(initial); }
    var tangent = start;
    if dot(best_tangent, best_tangent) > 1e-8 { tangent = normalize(best_tangent); }
    let reference = select(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), abs(start.y) < 0.9);
    let initial_side = normalize(cross(start, reference));
    let axis = cross(start, tangent);
    let w = 1.0 + dot(start, tangent);
    let denominator = w * w + dot(axis, axis);
    var side = initial_side;
    if denominator >= 1e-6 {
        side = normalize(initial_side + 2.0 * cross(axis, cross(axis, initial_side) + w * initial_side) / denominator);
    }
    let up = cross(tangent, side);
    let cross_position = vec2(dot(delta, side), dot(delta, up));
    var cross_distance = 100.0;
    if profile_count == 1u {
        let q = abs(cross_position) - vec2(radius);
        cross_distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
    } else if profile_count >= 3u {
        cross_distance = polygon_distance(cross_position / max(radius, 0.0001), bitcast<u32>(object.params.z), profile_count) * radius;
    }
    var cap = -100.0;
    if !closed && best_segment == 0u && best_t <= 0.0001 {
        cap = -dot(point - bezier_control(offset, 0u), tangent);
    } else if !closed && best_segment + 1u == segment_count && best_t >= 0.9999 {
        cap = dot(point - bezier_control(offset, segment_count * 3u), tangent);
    }
    let q = vec2(cross_distance, cap);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
}

fn object_distance_at(sample_point: vec3<f32>, object: Object) -> f32 {
    let homogeneous = vec4(sample_point, 1.0);
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
    } else if object.state.y == 5 {
        let polygon = polygon_distance(
            local.xy,
            bitcast<u32>(object.params.y),
            bitcast<u32>(object.params.z)
        );
        let depth = abs(local.z) - object.params.x;
        let outside = length(max(vec2(polygon, depth), vec2(0.0)));
        distance = outside + min(max(polygon, depth), 0.0);
    } else if object.state.y == 6 {
        distance = bezier_extrusion_distance(local, object);
    }
    var world_distance = distance * object.params.w;
    if object.state.y == 2 && brick_geometry_visible(object) {
        let relief = material_params[material_headers[object.component.w].offset + BRICK_RELIEF];
        if world_distance < max(relief.x * 2.0, 0.06) {
            let bond = material_params[material_headers[object.component.w].offset + BRICK_BOND];
            let stock = local * object.scale.xyz;
            let box_q = abs(local) - object.params.xyz;
            let face = select(select(stock.xy, stock.xz, box_q.y > box_q.z), stock.zy,
                box_q.x > box_q.y && box_q.x > box_q.z);
            let height = brick_structure_height(face, bond, relief);
            world_distance += max(relief.x, 0.0) * (1.0 - height);
        }
    }
    return world_distance;
}

fn object_distance(point: vec3<f32>, object: Object) -> f32 {
    return object_distance_at(modifier_point(point, object), object);
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
    let mirrored_point = mirror_point(point, objects[root]);
    if !HAS_BOOLEANS || start == root {
        return vec2(object_distance(mirrored_point, objects[root]), f32(root));
    }
    if CSG_SIZE == 2u {
        let parent = objects[root];
        let operand = objects[start];
        let group_repeated = parent.repeat_count.w != 0;
        let group_point = select(mirrored_point, group_repeat_point(mirrored_point, parent), group_repeated);
        let parent_point = modifier_point(group_point, parent);
        var operand_point = parent_point;
        if operand.modifier.x != parent.modifier.x {
            operand_point = modifier_point(group_point, operand);
        }
        let child = vec2(object_distance_at(operand_point, operand), f32(start));
        var parent_shape = parent;
        if group_repeated { parent_shape.repeat_count.w = 0; }
        var value = vec2(object_distance_at(parent_point, parent_shape), f32(root));
        value = combine_operand(value, child, operand, parent);
        return value;
    }
    var values: array<vec2<f32>, CSG_SIZE>;
    let parent = objects[root];
    let group_repeated = parent.repeat_count.w != 0;
    let group_point = select(mirrored_point, group_repeat_point(mirrored_point, parent), group_repeated);
    let parent_point = modifier_point(group_point, parent);
    for (var i = start; i <= root; i++) {
        var object = objects[i];
        var sample_point = parent_point;
        if object.modifier.x != parent.modifier.x {
            sample_point = modifier_point(group_point, object);
        }
        if i == root && group_repeated { object.repeat_count.w = 0; }
        values[i - start] = vec2(object_distance_at(sample_point, object), f32(i));
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
        return normalize(a * object_distance(mirror_point(point + a * e, object), object)
            + b * object_distance(mirror_point(point + b * e, object), object)
            + c * object_distance(mirror_point(point + c * e, object), object)
            + d * object_distance(mirror_point(point + d * e, object), object));
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
    return object.repeat_count.w == 0 && object.state.y <= 3 && object.modifier.x == 0u
        && all(object.mirror_axes.xyz == vec3<u32>(0u))
        && !(object.state.y == 2 && brick_geometry_visible(object));
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
                travel += value * bitcast<f32>(object.modifier.y);
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

// MODIFIER_MODULES
// MATERIAL_MODULES

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
    if !hit {
        if TRANSPARENT_BACKGROUND { return vec4(0.0); }
        return vec4(background(ray), 1.0);
    }

    var direction = ray;
    var throughput = vec3(1.0);
    var radiance = vec3(0.0);
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
                let reflected_normal = scene_normal(point, index);
                reflected_color = surface_light(point, reflected_normal, -direction, objects[index],
                    material_surface(point, reflected_normal, -direction, objects[index]), false);
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
            let surface = material_surface(point, normal, -direction, object);
            let ior = max(surface.ior, 1.0);
            let f0 = pow((ior - 1.0) / (ior + 1.0), 2.0);
            let fresnel = f0 + (1.0 - f0) * pow(1.0 - max(dot(-direction, normal), 0.0), 5.0);
            let reflection = reflect(direction, normal);
            let opacity = clamp(surface.opacity, 0.0, 1.0);
            let metallic = clamp(surface.metallic, 0.0, 1.0);
            let eta = select(ior, 1.0 / ior, entering);
            let transmitted = refract(direction, normal, eta);
            next_origin = point + normal * 0.007;
            next_direction = reflection;
            next_inside = !entering;
            if !(opacity < 0.999 && metallic < 0.999 && dot(transmitted, transmitted) < 0.001) {
                let roughness = clamp(surface.roughness, 0.0, 1.0);
                let weight = clamp(max(fresnel, surface.reflectivity), 0.0, 1.0);
                let tint = mix(vec3(1.0), object.color.rgb, metallic);
                reflection_weight = throughput * tint * weight * (1.0 - roughness * roughness);
                radiance += throughput * (vec3(0.22, 0.27, 0.35) * roughness * roughness * tint * weight
                    + surface_light(point, normal, -direction, object, surface, bounce == 1) * opacity * (1.0 - weight));
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
    return vec4(radiance * camera.position.w, 1.0);
}

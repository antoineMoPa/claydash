// Component-local material ownership lookup for rasterized mesh points.
// These are the source scene's point-distance equations, without ray tracing
// or a scene-wide SDF traversal.
override HAS_POLYGON_PRISMS: bool = true;
override HAS_BEZIER_CURVES: bool = true;
override HAS_LOFTS: bool = true;
override HAS_TEXT: bool = true;
override HAS_LATTICE_MODIFIERS: bool = true;
override HAS_MIRRORS: bool = true;
override HAS_REPETITION: bool = true;
const POISSON_CSG_SIZE: u32 = 256u;
@group(0) @binding(5) var<storage, read> polygon_points: array<vec2<f32>>;
fn repeated_axis(value: f32, spacing: f32, count: i32) -> f32 {
    if count <= 1 { return value; }
    let safe_spacing = max(spacing, 0.001);
    let half = f32(count - 1) * 0.5;
    let cell = clamp(round(value / safe_spacing), -half, half);
    return value - cell * safe_spacing;
}

fn group_repeat_point(point: vec3<f32>, root: Object) -> vec3<f32> {
    if !HAS_REPETITION { return point; }
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
    if !HAS_MIRRORS { return point; }
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
    var a = polygon_points[offset];
    for (var index = 0u; index < 65u; index++) {
        if index >= count { break; }
        let next = select(index + 1u, 0u, index + 1u == count);
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
        a = b;
    }
    return sqrt(distance_squared) * select(1.0, -1.0, inside);
}

fn text_distance(point: vec3<f32>, object: Object) -> f32 {
    let offset = bitcast<u32>(object.params.y);
    let header = polygon_points[offset];
    let count = bitcast<u32>(header.x);
    let half_depth = header.y;
    var nearest = 100.0;
    for (var glyph = 0u; glyph < 256u; glyph++) {
        if glyph >= count { break; }
        let base = offset + 1u + glyph * 9u;
        let a = polygon_points[base];
        let b = polygon_points[base + 1u];
        let c = polygon_points[base + 2u];
        let d = polygon_points[base + 3u];
        let e = polygon_points[base + 4u];
        let f = polygon_points[base + 5u];
        let origin = vec3(a, b.x);
        let x_axis = vec3(b.y, c);
        let y_axis = vec3(d, e.x);
        let z_axis = vec3(e.y, f);
        let delta = point - origin;
        let flat = vec2(dot(delta, x_axis), dot(delta, y_axis));
        let depth = abs(dot(delta, z_axis)) - half_depth;
        let lower = polygon_points[base + 6u];
        let upper = polygon_points[base + 7u];
        let bbox_distance = length(max(max(lower - flat, flat - upper), vec2(0.0)));
        if length(vec2(bbox_distance, max(depth, 0.0))) > max(nearest, 0.0) { continue; }
        let record = polygon_points[base + 8u];
        let edge_offset = bitcast<u32>(record.x);
        let edge_count = bitcast<u32>(record.y);
        var edge_distance = 1e20;
        var inside = false;
        for (var edge_index = 0u; edge_index < 1024u; edge_index++) {
            if edge_index >= edge_count { break; }
            let start = polygon_points[edge_offset + edge_index * 2u];
            let end = polygon_points[edge_offset + edge_index * 2u + 1u];
            let edge = end - start;
            let closest = start + edge * clamp(dot(flat - start, edge) / max(dot(edge, edge), 1e-10), 0.0, 1.0);
            edge_distance = min(edge_distance, distance(flat, closest));
            if (start.y > flat.y) != (end.y > flat.y) {
                let crossing = start.x + (flat.y - start.y) * (end.x - start.x) / (end.y - start.y);
                if flat.x < crossing { inside = !inside; }
            }
        }
        let planar = select(edge_distance, -edge_distance, inside);
        let q = vec2(planar, depth);
        nearest = min(nearest, length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0));
    }
    return nearest;
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

fn loft_distance(point: vec3<f32>, object: Object) -> f32 {
    let offset = bitcast<u32>(object.params.x);
    let count = bitcast<u32>(object.params.y);
    let profile_count = bitcast<u32>(object.params.z);
    let stride = 3u + profile_count;
    if count < 2u { return 100.0; }
    let first_x = polygon_points[offset].x;
    let last_x = polygon_points[offset + (count - 1u) * stride].x;
    var section = 0u;
    for (var i = 0u; i + 1u < count; i++) {
        section = i;
        if point.x <= polygon_points[offset + (i + 1u) * stride].x { break; }
    }
    let a = offset + section * stride;
    let b = a + stride;
    let ax = polygon_points[a].x;
    let bx = polygon_points[b].x;
    var t = clamp((point.x - ax) / max(bx - ax, 0.0001), 0.0, 1.0);
    t = t * t * (3.0 - 2.0 * t);
    let center = mix(vec2(polygon_points[a].y, polygon_points[a + 1u].x),
        vec2(polygon_points[b].y, polygon_points[b + 1u].x), t);
    let radii = max(mix(vec2(polygon_points[a + 1u].y, polygon_points[a + 2u].x),
        vec2(polygon_points[b + 1u].y, polygon_points[b + 2u].x), t), vec2(0.001));
    var radial = (length((point.yz - center) / radii) - 1.0) * min(radii.x, radii.y);
    if profile_count >= 3u {
        var distance_squared = 1e20;
        var inside = false;
        for (var index = 0u; index < 32u; index++) {
            if index >= profile_count { break; }
            let next = select(index + 1u, 0u, index + 1u == profile_count);
            let left = center + mix(polygon_points[a + 3u + index], polygon_points[b + 3u + index], t) * radii;
            let right = center + mix(polygon_points[a + 3u + next], polygon_points[b + 3u + next], t) * radii;
            let edge = right - left;
            let relative = point.yz - left;
            let length_squared = dot(edge, edge);
            if length_squared > 1e-7 {
                let closest = left + edge * clamp(dot(relative, edge) / length_squared, 0.0, 1.0);
                let delta = point.yz - closest;
                distance_squared = min(distance_squared, dot(delta, delta));
            }
            if (left.y > point.z) != (right.y > point.z) {
                let crossing = left.x + (point.z - left.y) * (right.x - left.x) / (right.y - left.y);
                if point.y < crossing { inside = !inside; }
            }
        }
        radial = sqrt(distance_squared) * select(1.0, -1.0, inside);
    }
    let cap = max(first_x - point.x, point.x - last_x);
    let q = vec2(radial, cap);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
}


fn primitive_distance(local: vec3<f32>, object: Object) -> f32 {
    var distance = 100.0;
    if object.state.y == 1 {
        distance = length(local) - object.params.x;
    } else if object.state.y == 2 {
        let radius = clamp(object.scale.w, 0.0, min(object.params.x, min(object.params.y, object.params.z)));
        let q = abs(local) - (object.params.xyz - vec3(radius));
        distance = length(max(q, vec3(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - radius;
    } else if object.state.y == 3 {
        let q = vec2(length(local.xz) - object.params.x, abs(local.y) - object.params.y);
        distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
    } else if object.state.y == 4 {
        let q = vec2(length(local.xz) - object.params.x, local.y);
        distance = length(q) - object.params.y;
    } else if HAS_POLYGON_PRISMS && object.state.y == 5 {
        let polygon = polygon_distance(
            local.xy,
            bitcast<u32>(object.params.y),
            bitcast<u32>(object.params.z)
        );
        let softness = clamp(object.scale.w, 0.0, object.params.x);
        let depth = abs(local.z) - (object.params.x - softness);
        let outside = length(max(vec2(polygon, depth), vec2(0.0)));
        distance = outside + min(max(polygon, depth), 0.0) - softness;
    } else if HAS_BEZIER_CURVES && object.state.y == 6 {
        distance = bezier_extrusion_distance(local, object);
    } else if HAS_LOFTS && object.state.y == 7 {
        distance = loft_distance(local, object);
    } else if HAS_TEXT && object.state.y == 11 {
        distance = text_distance(local, object);
    }
    return distance;
}


fn base_object_distance_at(sample_point: vec3<f32>, object: Object) -> f32 {
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
    let distance = primitive_distance(local, object);
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
    if (object.state.y == 1 || object.state.y == 2) && fabric_geometry_visible(object) {
        let header = material_headers[object.component.w];
        let depth = material_params[header.offset + FABRIC_PITCH].z;
        if world_distance < max(depth * 2.0, 0.04) {
            world_distance += fabric_geometry_cut(local, object, header.offset);
        }
    }
    return world_distance;
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


fn poisson_owner_supported(object: Object) -> bool {
    let kind = object.state.y;
    // A surface inlay clips against another component and needs a separate
    // host query. Keep its sampled triangle owner until that path is present.
    return (kind >= 1 && kind <= 7 || kind == 11)
        && (kind == 6 || object.mirror_axes.w == 0u);
}

fn poisson_operand_distance(point: vec3<f32>, object: Object) -> f32 {
    return base_object_distance_at(point, object);
}

fn poisson_flat_union_owner(point: vec3<f32>, start: u32, root: u32,
    triangle_owner: u32) -> u32 {
    let parent = objects[root];
    let mirrored = mirror_point(point, parent);
    let repeated = parent.repeat_count.w != 0;
    let group_point = select(mirrored, group_repeat_point(mirrored, parent), repeated);
    let parent_point = modifier_point(group_point, parent);
    var parent_shape = parent;
    if repeated { parent_shape.repeat_count.w = 0; }
    if !poisson_owner_supported(parent_shape) { return triangle_owner; }
    var closest = vec2(poisson_operand_distance(parent_point, parent_shape), f32(root));
    let tree = parent.operand_tree.y > parent.operand_tree.x;
    for (var i = start; i < root; i++) {
        let child = objects[i];
        if tree && child.modifier.x == parent.modifier.x && child.distance_bound.w > 0.0 {
            continue;
        }
        var child_point = parent_point;
        if child.modifier.x != parent.modifier.x {
            child_point = modifier_point(group_point, child);
        }
        if !poisson_owner_supported(child) { return triangle_owner; }
        let distance_to_child = poisson_operand_distance(child_point, child);
        if distance_to_child < closest.x { closest = vec2(distance_to_child, f32(i)); }
    }
    if tree {
        var node_index = parent.operand_tree.x;
        while node_index < parent.operand_tree.y {
            let node = bvh[node_index];
            let lower_bound = distance(parent_point, node.center_radius.xyz) - node.center_radius.w;
            if lower_bound > closest.x { node_index = node.metadata.y; continue; }
            if node.metadata.x != 0xffffffffu {
                let index = node.metadata.x;
                let child = objects[index];
                if !poisson_owner_supported(child) { return triangle_owner; }
                let distance_to_child = poisson_operand_distance(parent_point, child);
                if distance_to_child < closest.x ||
                    (distance_to_child == closest.x && closest.y != f32(root)
                        && index < u32(closest.y)) {
                    closest = vec2(distance_to_child, f32(index));
                }
            }
            node_index += 1u;
        }
    }
    return u32(closest.y);
}

fn poisson_flat_component_owner(point: vec3<f32>, start: u32, root: u32,
    triangle_owner: u32) -> u32 {
    let parent = objects[root];
    let mirrored = mirror_point(point, parent);
    let repeated = parent.repeat_count.w != 0;
    let group_point = select(mirrored, group_repeat_point(mirrored, parent), repeated);
    let parent_point = modifier_point(group_point, parent);
    var parent_shape = parent;
    if repeated { parent_shape.repeat_count.w = 0; }
    if !poisson_owner_supported(parent_shape) { return triangle_owner; }
    var value = vec2(poisson_operand_distance(parent_point, parent_shape), f32(root));
    for (var i = start; i < root; i++) {
        let child = objects[i];
        if !poisson_owner_supported(child) { return triangle_owner; }
        var child_point = parent_point;
        if child.modifier.x != parent.modifier.x {
            child_point = modifier_point(group_point, child);
        }
        let next = vec2(poisson_operand_distance(child_point, child), f32(i));
        value = combine_operand(value, next, child, parent);
    }
    return u32(value.y);
}

fn poisson_component_owner(point: vec3<f32>, triangle_owner: u32) -> u32 {
    let component = objects[triangle_owner].component;
    let start = component.x;
    let root = component.y;
    if start >= root { return triangle_owner; }
    let parent = objects[root];
    if parent.state.w == -2 { // FLAT_UNION_ROOT
        return poisson_flat_union_owner(point, start, root, triangle_owner);
    }
    if parent.state.w == -3 { // FLAT_COMPONENT_ROOT
        return poisson_flat_component_owner(point, start, root, triangle_owner);
    }
    // Nested and non-union components use the original postorder Boolean
    // reductions. The bound keeps fragment scratch small for ordinary meshes.
    if root - start >= POISSON_CSG_SIZE { return triangle_owner; }
    let mirrored = mirror_point(point, parent);
    let repeated = parent.repeat_count.w != 0;
    let group_point = select(mirrored, group_repeat_point(mirrored, parent), repeated);
    let parent_point = modifier_point(group_point, parent);
    var values: array<vec2<f32>, POISSON_CSG_SIZE>;
    for (var i = start; i <= root; i++) {
        var object = objects[i];
        if !poisson_owner_supported(object) { return triangle_owner; }
        var sample_point = parent_point;
        if object.modifier.x != parent.modifier.x {
            sample_point = modifier_point(group_point, object);
        }
        if i == root && repeated { object.repeat_count.w = 0; }
        values[i - start] = vec2(poisson_operand_distance(sample_point, object), f32(i));
    }
    for (var i = start; i < root; i++) {
        let parent_index = u32(objects[i].state.w);
        if parent_index < start || parent_index > root { return triangle_owner; }
        let child = values[i - start];
        values[parent_index - start] = combine_operand(values[parent_index - start], child,
            objects[i], objects[parent_index]);
    }
    return u32(values[root - start].y);
}

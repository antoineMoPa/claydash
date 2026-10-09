// SCENE_ABI_MODULE
struct BvhNode { center_radius: vec4<f32>, metadata: vec4<u32>, aabb_min: vec4<f32>, aabb_max: vec4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> objects: array<Object>;
@group(0) @binding(2) var<storage, read> bvh: array<BvhNode>;
@group(0) @binding(5) var<storage, read> polygon_points: array<vec2<f32>>;
@group(0) @binding(13) var<storage, read> box_depth_texels: array<vec4<f32>>;
@group(0) @binding(11) var image_atlas: texture_2d_array<f32>;
@group(0) @binding(12) var image_sampler: sampler;
override USE_BVH: bool = true;
override HAS_BOOLEANS: bool = false;
override TRANSPARENT_BACKGROUND: bool = false;
override FAST_PREVIEW: bool = false;
override HAS_POLYGON_PRISMS: bool = true;
override HAS_BEZIER_CURVES: bool = true;
override HAS_LOFTS: bool = true;
override HAS_TEXT: bool = true;
override HAS_NEURAL_SDF: bool = true;
override HAS_LATTICE_MODIFIERS: bool = true;
override HAS_MIRRORS: bool = true;
override HAS_REPETITION: bool = true;
override HAS_FLAT_UNIONS: bool = true;
override HYBRID_SPLATS: bool = false;

fn stencil_color(point: vec3<f32>, normal: vec3<f32>, object: Object) -> vec4<f32> {
    if object.stencil_meta.x < 0.5 { return vec4(0.0); }
    let front = normalize(object.inverse_rows[2].xyz);
    if object.stencil_meta.z < 0.5 && dot(normal, front) <= 0.0 { return vec4(0.0); }
    let p = vec4(point, 1.0);
    let local = vec2(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p)) - object.stencil_placement.zw;
    let angle = object.stencil_meta.y;
    let rotated = vec2(cos(angle) * local.x + sin(angle) * local.y,
        -sin(angle) * local.x + cos(angle) * local.y);
    let uv = rotated / max(object.stencil_placement.xy, vec2(0.001)) + vec2(0.5);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) { return vec4(0.0); }
    return textureSampleLevel(image_atlas, image_sampler, uv, i32(object.stencil_meta.x) - 1, 0.0);
}
const CSG_SIZE: u32 = 256u;
const FLAT_UNION_ROOT: i32 = -2;
const FLAT_COMPONENT_ROOT: i32 = -3;

struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) clip: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32,
    @builtin(instance_index) instance: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -3.0), vec2(3.0, 1.0), vec2(-1.0, 1.0));
    var out: VertexOutput;
    if (instance & 0x80000000u) != 0u {
        let width = max(camera.world_mode.w, 1u);
        let height = max(camera.count.z, 1u);
        let blocks_per_row = (width + 15u) / 16u;
        let sample = instance & 0x007fffffu;
        let phase = (instance >> 23u) & 255u;
        var offset = vec2<u32>(0u);
        for (var bit = 0u; bit < 4u; bit++) {
            let digit = (phase >> (6u - 2u * bit)) & 3u;
            offset.x |= u32(digit == 1u || digit == 2u) << bit;
            offset.y |= u32(digit == 1u || digit == 3u) << bit;
        }
        let pixel = vec2(sample % blocks_per_row, sample / blocks_per_row) * 16u + offset;
        if pixel.x >= width || pixel.y >= height {
            out.clip = vec2(2.0);
        } else {
            let corner = array<vec2<f32>, 6>(vec2(0.0, 0.0), vec2(1.0, 0.0),
                vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
            let screen = vec2<f32>(pixel) + corner[index];
            out.clip = screen / vec2<f32>(f32(width), f32(height)) * vec2(2.0, -2.0)
                + vec2(-1.0, 1.0);
        }
        out.position = vec4(out.clip, 0.0, 1.0);
        return out;
    }
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

// TEXT_MODULE

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
        if polygon_points[offset + base * 2u + 1u].y != 0.0 {
            let direction = d - a;
            let t = clamp(dot(point - a, direction) / dot(direction, direction), 0.0, 1.0);
            let position = a + direction * t;
            let delta = position - point;
            let distance = dot(delta, delta);
            if distance < best_distance {
                best_distance = distance;
                best_position = position;
                best_tangent = direction;
                best_t = t;
                best_segment = segment;
            }
            continue;
        }
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

// Each box face captures the first surface along its inward axis. The face
// nearest the viewer supplies a depth field; empty texels leave the ray free
// to continue through the box.
struct BoxDepthFaceSample {
    capture: vec4<f32>,
    material_index: u32,
}

const NEURAL_WIDTH: u32 = 1024u;

// Neural payload: architecture/owner offset, step bound/hit distance, packed weights, 32³ owners.
fn neural_activation(value: f32, offset: u32) -> f32 {
    if box_depth_texels[offset + 1u].z < 0.5 { return max(value, 0.0); }
    return max(value, 0.0) + log(1.0 + exp(-10.0 * abs(value))) / 10.0;
}
fn neural_activation_slope(value: f32, offset: u32) -> f32 {
    if box_depth_texels[offset + 1u].z < 0.5 { return select(0.0, 1.0, value > 0.0); }
    let e = exp(-10.0 * abs(value));
    return select(e / (1.0 + e), 1.0 / (1.0 + e), value >= 0.0);
}
fn neural_weight(offset: u32, index: u32) -> f32 {
    return box_depth_texels[offset + 2u + index / 4u][index % 4u];
}
// Per-invocation ray context also reaches generic CSG distance evaluators.
var<private> neural_ray_direction: vec3<f32> = vec3(1.0, 0.0, 0.0);
fn neural_local_ray(object: Object) -> vec3<f32> {
    let ray = vec3(dot(object.inverse_rows[0].xyz, neural_ray_direction),
        dot(object.inverse_rows[1].xyz, neural_ray_direction),
        dot(object.inverse_rows[2].xyz, neural_ray_direction));
    return ray / max(length(ray), 1e-20);
}
fn neural_distance_vector(local: vec3<f32>, ray: vec3<f32>, object: Object) -> vec3<f32> {
    let offset = object.box_depth_meta.x;
    let header = box_depth_texels[offset];
    let layers = u32(header.x);
    let width = u32(header.y);
    let p = local / object.box_depth_max.x;
    var values: array<vec4<f32>, (NEURAL_WIDTH + 3u) / 4u>;
    var next: array<vec4<f32>, (NEURAL_WIDTH + 3u) / 4u>;
    for (var row = 0u; row < width; row++) {
        let first = box_depth_texels[offset + 2u + row * 2u];
        let second = box_depth_texels[offset + 3u + row * 2u];
        let value = dot(first.xyz, p) + first.w * ray.x + dot(second.xy, ray.yz) + second.z;
        values[row / 4u][row % 4u] = neural_activation(value, offset);
    }
    var layer_offset = width * 2u;
    let row_records = (width + 4u) / 4u;
    let blocks = (width + 3u) / 4u;
    for (var layer = 1u; layer < layers; layer++) {
        for (var row = 0u; row < width; row++) {
            let row_offset = layer_offset + row * row_records;
            var value = neural_weight(offset, row_offset * 4u + width);
            for (var block = 0u; block < blocks; block++) {
                value += dot(box_depth_texels[offset + 2u + row_offset + block], values[block]);
            }
            next[row / 4u][row % 4u] = neural_activation(value, offset);
        }
        values = next;
        layer_offset += width * row_records;
    }
    var result = vec3(0.0);
    for (var axis = 0u; axis < 3u; axis++) {
        let output_offset = u32(header.z) + axis * row_records * 4u;
        result[axis] = neural_weight(offset, output_offset + width);
        for (var block = 0u; block < blocks; block++) {
            result[axis] += dot(box_depth_texels[offset + 2u + output_offset / 4u + block], values[block]);
        }
    }
    return result * object.box_depth_max.x;
}
fn neural_value(local: vec3<f32>, object: Object) -> f32 {
    let ray = neural_local_ray(object);
    return dot(neural_distance_vector(local, ray, object), ray);
}
fn neural_gradient(local: vec3<f32>, object: Object) -> vec3<f32> {
    let offset = object.box_depth_meta.x;
    let header = box_depth_texels[offset];
    let layers = u32(header.x);
    let width = u32(header.y);
    let p = local / object.box_depth_max.x;
    let ray = neural_local_ray(object);
    // xyz carries spatial derivatives with the ray held fixed; w is activation.
    var values: array<vec4<f32>, NEURAL_WIDTH>;
    var next: array<vec4<f32>, NEURAL_WIDTH>;
    var layer_offset = 0u;
    for (var layer = 0u; layer < layers; layer++) {
        let inputs = select(width, 6u, layer == 0u);
        for (var row = 0u; row < width; row++) {
            let row_stride = ((inputs + 4u) / 4u) * 4u;
            let row_offset = layer_offset + row * row_stride;
            var value = vec4(0.0, 0.0, 0.0, neural_weight(offset, row_offset + inputs));
            for (var col = 0u; col < inputs; col++) {
                var input = vec4(0.0);
                if layer == 0u {
                    if col < 3u { input.w = p[col]; input[col] = 1.0; }
                    else { input.w = ray[col - 3u]; }
                } else { input = values[col]; }
                value += neural_weight(offset, row_offset + col) * input;
            }
            next[row] = vec4(value.xyz * neural_activation_slope(value.w, offset), neural_activation(value.w, offset));
        }
        values = next;
        layer_offset += width * ((inputs + 4u) / 4u) * 4u;
    }
    var result = vec3(0.0);
    let row_stride = ((width + 4u) / 4u) * 4u;
    for (var axis = 0u; axis < 3u; axis++) {
        for (var i = 0u; i < width; i++) {
            result += ray[axis] * neural_weight(offset, u32(header.z) + axis * row_stride + i) * values[i].xyz;
        }
    }
    return result;
}
fn neural_shape(local: vec3<f32>, object: Object) -> f32 {
    let q = abs(local) - object.box_depth_max.xyz;
    let cube = length(max(q, vec3(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
    return max(cube, neural_value(local, object));
}
fn neural_surface_sample(point: vec3<f32>, object: Object) -> BoxDepthFaceSample {
    let p = vec4(point, 1.0);
    let local = vec3(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p));
    let grid = 32u; // Material ownership is independent of training resolution.
    let last = f32(grid - 1u);
    let cell = vec3<u32>(clamp(round((local / object.box_depth_max.x + vec3(1.0)) * (last * 0.5)), vec3(0.0), vec3(last)));
    let index = cell.x + grid * (cell.y + grid * cell.z);
    let sample = box_depth_texels[object.box_depth_meta.x + u32(box_depth_texels[object.box_depth_meta.x].w) + index];
    return BoxDepthFaceSample(vec4(0.0, sample.yzw), u32(sample.x));
}
fn surface_hit_tolerance(owner: f32, classic: f32) -> f32 {
    if !HAS_NEURAL_SDF || owner < 0.0 { return classic; }
    let object = objects[u32(owner)];
    if object.state.y != 12 { return classic; }
    // Equivalent sample spacing in world-distance units, preserving legacy hit tolerance.
    return max(classic, 2.0 * object.box_depth_max.x * object.params.w * box_depth_texels[object.box_depth_meta.x + 1u].y / (box_depth_texels[object.box_depth_meta.x + 1u].w - 1.0));
}
fn component_has_neural(owner: f32) -> bool {
    if !HAS_NEURAL_SDF || owner < 0.0 { return false; }
    let component = objects[u32(owner)].component;
    for (var i = component.x; i <= component.y; i++) {
        if objects[i].state.y == 12 { return true; }
    }
    return false;
}
// A learned distance can overstep a zero crossing. Recover a bracketed hit
// without dividing every step by a very loose global network slope bound.
fn refine_neural_crossing(origin: vec3<f32>, direction: vec3<f32>, low: f32, high: f32, owner: f32) -> vec3<f32> {
    neural_ray_direction = direction;
    let component = objects[u32(owner)].component;
    var a = low;
    var b = high;
    let negative_at_a = component_distance(origin + direction * a, component.x, component.y).x < 0.0;
    var sample = vec2(0.0, owner);
    var t = (a + b) * 0.5;
    for (var iteration = 0u; iteration < 12u; iteration++) {
        t = (a + b) * 0.5;
        sample = component_distance(origin + direction * t, component.x, component.y);
        if abs(sample.x) <= surface_hit_tolerance(sample.y, 0.0015) { break; }
        if (sample.x < 0.0) == negative_at_a { a = t; } else { b = t; }
    }
    return vec3(t, sample.y, -1.0);
}

fn box_depth_face_uv(local: vec3<f32>, object: Object, face: u32) -> vec2<f32> {
    let minimum = object.box_depth_min.xyz;
    let maximum = object.box_depth_max.xyz;
    if face < 2u {
        return (local.yz - minimum.yz) / (maximum.yz - minimum.yz);
    }
    if face < 4u {
        return (local.xz - minimum.xz) / (maximum.xz - minimum.xz);
    }
    return (local.xy - minimum.xy) / (maximum.xy - minimum.xy);
}

fn box_depth_texel_offset(object: Object, face: u32, pixel: vec2<i32>) -> u32 {
    let resolution = i32(object.box_depth_meta.y);
    let clamped = clamp(pixel, vec2<i32>(0), vec2<i32>(resolution - 1));
    return object.box_depth_meta.x + 2u * (face * object.box_depth_meta.y * object.box_depth_meta.y
        + u32(clamped.y) * object.box_depth_meta.y + u32(clamped.x));
}

fn gaussian_splat_texel_offset(object: Object, face: u32, pixel: vec2<i32>) -> u32 {
    return object.box_depth_meta.x + 3u * (face * object.box_depth_meta.y * object.box_depth_meta.y
        + u32(pixel.y) * object.box_depth_meta.y + u32(pixel.x));
}

fn box_depth_face_sample(local: vec3<f32>, object: Object, face: u32) -> BoxDepthFaceSample {
    let uv = box_depth_face_uv(local, object, face);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return BoxDepthFaceSample(vec4(-1.0, 0.0, 0.0, 0.0), 0u);
    }
    let pixel = vec2<i32>(floor(uv * f32(object.box_depth_meta.y)));
    let offset = box_depth_texel_offset(object, face, pixel);
    return BoxDepthFaceSample(box_depth_texels[offset], u32(box_depth_texels[offset + 1u].x));
}

fn box_depth_face_depth(local: vec3<f32>, object: Object, face: u32) -> f32 {
    let uv = box_depth_face_uv(local, object, face);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) { return -1.0; }
    let resolution = f32(object.box_depth_meta.y);
    let nearest = box_depth_texels[box_depth_texel_offset(object, face, vec2<i32>(floor(uv * resolution)))];
    if nearest.x < 0.0 { return nearest.x; }
    let coordinate = uv * resolution - vec2(0.5);
    let lower = vec2<i32>(floor(coordinate));
    let blend = fract(coordinate);
    let a = box_depth_texels[box_depth_texel_offset(object, face, lower)];
    let b = box_depth_texels[box_depth_texel_offset(object, face, lower + vec2<i32>(1, 0))];
    let c = box_depth_texels[box_depth_texel_offset(object, face, lower + vec2<i32>(0, 1))];
    let d = box_depth_texels[box_depth_texel_offset(object, face, lower + vec2<i32>(1, 1))];
    if min(min(a.x, b.x), min(c.x, d.x)) < 0.0 { return nearest.x; }
    let size = object.box_depth_max.xyz - object.box_depth_min.xyz;
    let depth_limit = max(size.x, max(size.y, size.z)) / resolution * 3.0;
    if max(max(a.x, b.x), max(c.x, d.x)) - min(min(a.x, b.x), min(c.x, d.x)) > depth_limit {
        return nearest.x;
    }
    if any(a.yzw != nearest.yzw) || any(b.yzw != nearest.yzw)
        || any(c.yzw != nearest.yzw) || any(d.yzw != nearest.yzw) {
        return nearest.x;
    }
    return mix(mix(a.x, b.x, blend.x), mix(c.x, d.x, blend.x), blend.y);
}

fn box_depth_face_plane(local: vec3<f32>, object: Object, face: u32, depth: f32) -> f32 {
    if face == 0u { return local.x - object.box_depth_max.x + depth; }
    if face == 1u { return object.box_depth_min.x - local.x + depth; }
    if face == 2u { return local.y - object.box_depth_max.y + depth; }
    if face == 3u { return object.box_depth_min.y - local.y + depth; }
    if face == 4u { return local.z - object.box_depth_max.z + depth; }
    return object.box_depth_min.z - local.z + depth;
}

fn box_depth_view_face(local: vec3<f32>, object: Object) -> u32 {
    let eye = vec4(camera.position.xyz, 1.0);
    let local_eye = vec3(
        dot(object.inverse_rows[0], eye),
        dot(object.inverse_rows[1], eye),
        dot(object.inverse_rows[2], eye)
    );
    let view = local_eye - local;
    let axis = abs(view);
    if axis.x >= axis.y && axis.x >= axis.z {
        return select(1u, 0u, view.x >= 0.0);
    }
    if axis.y >= axis.z {
        return select(3u, 2u, view.y >= 0.0);
    }
    return select(5u, 4u, view.z >= 0.0);
}

fn box_depth_shape(local: vec3<f32>, object: Object) -> f32 {
    let q = max(object.box_depth_min.xyz - local, local - object.box_depth_max.xyz);
    let box_distance = length(max(q, vec3(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
    // Near the cube boundary, sample the capture before the marcher reaches
    // its hit threshold. Otherwise the empty bounding face appears opaque.
    if box_distance > 0.01 { return box_distance; }
    let face = box_depth_view_face(local, object);
    let depth = box_depth_face_depth(local, object, face);
    if depth < 0.0 {
        let box_width = object.box_depth_max.xyz - object.box_depth_min.xyz;
        return max(-depth, min(box_width.x, min(box_width.y, box_width.z)) * 0.05);
    }
    return box_depth_face_plane(local, object, face, depth);
}

fn box_depth_surface_sample(point: vec3<f32>, object: Object) -> BoxDepthFaceSample {
    let homogeneous = vec4(point, 1.0);
    let local = vec3(
        dot(object.inverse_rows[0], homogeneous),
        dot(object.inverse_rows[1], homogeneous),
        dot(object.inverse_rows[2], homogeneous)
    );
    let sample = box_depth_face_sample(local, object, box_depth_view_face(local, object));
    if sample.capture.x >= 0.0 {
        return sample;
    }
    return BoxDepthFaceSample(vec4(0.0, object.color.rgb), object.component.w);
}

// Sphere captures use a seamless azimuth and a clamped polar axis.
fn sphere_depth_uv(local: vec3<f32>) -> vec2<f32> {
    let radius = length(local);
    let direction = select(vec3(0.0, 1.0, 0.0), local / max(radius, 0.000001), radius > 0.000001);
    let azimuth = atan2(direction.z, direction.x);
    return vec2(fract(azimuth / 6.28318530718 + 0.5), acos(clamp(direction.y, -1.0, 1.0)) / 3.14159265359);
}

fn sphere_depth_texel_offset(object: Object, pixel: vec2<i32>) -> u32 {
    let width = i32(object.box_depth_meta.y);
    let height = i32(object.box_depth_meta.z);
    let wrapped_x = ((pixel.x % width) + width) % width;
    let y = clamp(pixel.y, 0, height - 1);
    return object.box_depth_meta.x + 2u * (u32(y) * object.box_depth_meta.y + u32(wrapped_x));
}

fn sphere_depth_sample(local: vec3<f32>, object: Object) -> BoxDepthFaceSample {
    let uv = sphere_depth_uv(local);
    let pixel = vec2<i32>(floor(uv * vec2<f32>(f32(object.box_depth_meta.y), f32(object.box_depth_meta.z))));
    let offset = sphere_depth_texel_offset(object, pixel);
    return BoxDepthFaceSample(box_depth_texels[offset], u32(box_depth_texels[offset + 1u].x));
}

fn sphere_depth_at(local: vec3<f32>, object: Object) -> f32 {
    let uv = sphere_depth_uv(local);
    let dimensions = vec2<f32>(f32(object.box_depth_meta.y), f32(object.box_depth_meta.z));
    let nearest = sphere_depth_sample(local, object).capture;
    if nearest.x < 0.0 { return nearest.x; }
    let coordinate = uv * dimensions - vec2(0.5);
    let lower = vec2<i32>(floor(coordinate));
    let blend = fract(coordinate);
    let a = box_depth_texels[sphere_depth_texel_offset(object, lower)];
    let b = box_depth_texels[sphere_depth_texel_offset(object, lower + vec2<i32>(1, 0))];
    let c = box_depth_texels[sphere_depth_texel_offset(object, lower + vec2<i32>(0, 1))];
    let d = box_depth_texels[sphere_depth_texel_offset(object, lower + vec2<i32>(1, 1))];
    if min(min(a.x, b.x), min(c.x, d.x)) < 0.0 { return nearest.x; }
    let depth_limit = object.box_depth_max.x * 6.0 / dimensions.y;
    if max(max(a.x, b.x), max(c.x, d.x)) - min(min(a.x, b.x), min(c.x, d.x)) > depth_limit {
        return nearest.x;
    }
    if any(a.yzw != nearest.yzw) || any(b.yzw != nearest.yzw)
        || any(c.yzw != nearest.yzw) || any(d.yzw != nearest.yzw) {
        return nearest.x;
    }
    return mix(mix(a.x, b.x, blend.x), mix(c.x, d.x, blend.x), blend.y);
}

fn sphere_depth_shape(local: vec3<f32>, object: Object) -> f32 {
    let radius = object.box_depth_max.x;
    let sphere_distance = length(local) - radius;
    if sphere_distance > 0.01 { return sphere_distance; }
    let depth = sphere_depth_at(local, object);
    if depth < 0.0 { return max(-depth, radius * 0.05); }
    return sphere_distance + depth;
}

fn sphere_depth_surface_sample(point: vec3<f32>, object: Object) -> BoxDepthFaceSample {
    let homogeneous = vec4(point, 1.0);
    let local = vec3(
        dot(object.inverse_rows[0], homogeneous),
        dot(object.inverse_rows[1], homogeneous),
        dot(object.inverse_rows[2], homogeneous)
    );
    let sample = sphere_depth_sample(local, object);
    if sample.capture.x >= 0.0 { return sample; }
    return BoxDepthFaceSample(vec4(0.0, object.color.rgb), object.component.w);
}

// The finite support of nearby splats is an acceleration boundary, not the
// visible shape; the ray compositor below computes projected Gaussian opacity.
fn gaussian_splat_query(local: vec3<f32>, object: Object) -> vec2<f32> {
    let dimensions = vec2<f32>(f32(object.box_depth_meta.y), f32(object.box_depth_meta.z));
    let coordinate = sphere_depth_uv(local) * dimensions - vec2(0.5);
    let nearest = vec2<i32>(round(coordinate));
    var closest = 100.0;
    var closest_offset = -1.0;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let pixel = nearest + vec2<i32>(x, y);
            if pixel.y < 0 || pixel.y >= i32(object.box_depth_meta.z) { continue; }
            let offset = sphere_depth_texel_offset(object, pixel);
            let sample = box_depth_texels[offset];
            if sample.w < 0.0 { continue; }
            let distance = length(local - sample.xyz) - 2.5 * sample.w;
            if distance < closest {
                closest = distance;
                closest_offset = f32(offset);
            }
        }
    }
    return vec2(closest, closest_offset);
}

// Intersect the finite support spheres through a stackless spatial hierarchy.
// This finds splats anywhere in a hollow group without repeated SDF queries.
fn gaussian_splat_ray_entry(origin: vec3<f32>, direction: vec3<f32>, object: Object,
    start: f32, end: f32) -> vec2<f32> {
    var node_index = object.box_depth_meta.w;
    let node_end = u32(object.box_depth_min.w);
    if node_index == 0u || node_index >= node_end { return vec2(100.0, -1.0); }
    let p = vec4(origin, 1.0);
    let local_origin = vec3(dot(object.inverse_rows[0], p),
        dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p));
    let local_direction = vec3(dot(object.inverse_rows[0].xyz, direction),
        dot(object.inverse_rows[1].xyz, direction), dot(object.inverse_rows[2].xyz, direction));
    let direction_squared = dot(local_direction, local_direction);
    let inverse_direction = vec3(1.0) /
        (select(vec3(-1.0), vec3(1.0), local_direction >= vec3(0.0))
            * max(abs(local_direction), vec3(1e-20)));
    var closest = end;
    var found = false;
    var closest_offset = -1.0;
    while node_index < node_end {
        let minimum = box_depth_texels[node_index];
        let maximum = box_depth_texels[node_index + 1u];
        let first = (minimum.xyz - local_origin) * inverse_direction;
        let second = (maximum.xyz - local_origin) * inverse_direction;
        let near = min(first, second);
        let far = max(first, second);
        let entry = max(start, max(near.x, max(near.y, near.z)));
        let exit = min(closest, min(far.x, min(far.y, far.z)));
        if entry > exit {
            node_index = u32(maximum.w);
            continue;
        }
        if minimum.w >= 0.0 {
            let sample = box_depth_texels[u32(minimum.w)];
            let relative = local_origin - sample.xyz;
            let half_b = dot(relative, local_direction);
            let radius = 2.5 * sample.w;
            let discriminant = half_b * half_b
                - direction_squared * (dot(relative, relative) - radius * radius);
            if discriminant >= 0.0 {
                let root = sqrt(discriminant);
                let projected = gaussian_splat_projected_query(local_origin, local_direction, u32(minimum.w));
                let candidate = max(start, projected.y);
                if projected.x < 6.25 && candidate <= closest
                    && (-half_b + root) / direction_squared >= start
                    && (-half_b - root) / direction_squared <= end {
                    closest = candidate;
                    closest_offset = minimum.w;
                    found = true;
                }
            }
        }
        node_index += 2u;
    }
    return select(vec2(100.0, -1.0), vec2(closest, closest_offset), found);
}

fn gaussian_splat_shape(local: vec3<f32>, object: Object) -> f32 {
    let bound = length(local) - object.params.x;
    if bound > 0.01 { return bound; }
    let splat = gaussian_splat_query(local, object);
    if splat.y < 0.0 { return max(bound, object.box_depth_max.x * 0.05); }
    return max(bound, splat.x);
}

fn gaussian_splat_projected_query(local: vec3<f32>, direction: vec3<f32>, offset: u32) -> vec2<f32> {
    let sample = box_depth_texels[offset];
    if sample.w <= 0.0 { return vec2(100.0, 0.0); }
    let normal = box_depth_texels[offset + 2u].xyz;
    let delta = local - sample.xyz;
    let normal_delta = dot(delta, normal);
    let normal_ray = dot(direction, normal);
    let tangent_delta = (delta - normal * normal_delta) / sample.w;
    let tangent_ray = (direction - normal * normal_ray) / sample.w;
    let depth_delta = normal_delta / (sample.w * 0.25);
    let depth_ray = normal_ray / (sample.w * 0.25);
    let a = dot(tangent_ray, tangent_ray) + depth_ray * depth_ray;
    let b = dot(tangent_delta, tangent_ray) + depth_delta * depth_ray;
    let c = dot(tangent_delta, tangent_delta) + depth_delta * depth_delta;
    let squared_radius = max(c - b * b / max(a, 0.000001), 0.0);
    return vec2(squared_radius, -b / max(a, 0.000001));
}

fn gaussian_splat_projected_weight(local: vec3<f32>, direction: vec3<f32>, offset: u32) -> f32 {
    let squared_radius = gaussian_splat_projected_query(local, direction, offset).x;
    return exp(-0.5 * squared_radius)
        * (1.0 - smoothstep(4.0, 6.25, squared_radius));
}

fn gaussian_splat_surface_sample(point: vec3<f32>, direction: vec3<f32>, object: Object) -> BoxDepthFaceSample {
    let homogeneous = vec4(point, 1.0);
    let local = vec3(dot(object.inverse_rows[0], homogeneous),
        dot(object.inverse_rows[1], homogeneous), dot(object.inverse_rows[2], homogeneous));
    let local_direction = normalize(vec3(dot(object.inverse_rows[0].xyz, direction),
        dot(object.inverse_rows[1].xyz, direction), dot(object.inverse_rows[2].xyz, direction)));
    let resolution = i32(object.box_depth_meta.y);
    let face_size = u32(resolution * resolution);
    let sample_index = (u32(object.box_depth_max.w) - object.box_depth_meta.x) / 3u;
    let face = sample_index / face_size;
    let pixel_index = sample_index % face_size;
    let nearest = vec2<i32>(i32(pixel_index % u32(resolution)), i32(pixel_index / u32(resolution)));
    var weighted_color = vec3(0.0);
    var total_weight = 0.0;
    var strongest_weight = 0.0;
    var material_index = object.component.w;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let pixel = nearest + vec2<i32>(x, y);
            if any(pixel < vec2<i32>(0)) || any(pixel >= vec2<i32>(resolution)) { continue; }
            let sample_offset = gaussian_splat_texel_offset(object, face, pixel);
            let weight = gaussian_splat_projected_weight(local, local_direction, sample_offset);
            if weight <= 0.0 { continue; }
            let captured = box_depth_texels[sample_offset + 1u];
            weighted_color += captured.yzw * weight;
            total_weight += weight;
            if weight > strongest_weight {
                strongest_weight = weight;
                material_index = u32(captured.x);
            }
        }
    }
    if total_weight <= 0.0 {
        return BoxDepthFaceSample(vec4(0.0, object.color.rgb), object.component.w);
    }
    return BoxDepthFaceSample(vec4(0.0, weighted_color / total_weight), material_index);
}

// Project each nearby 3D Gaussian onto the ray. Optical depths add, so
// overlapping splats become denser while their edges remain transparent.
fn gaussian_splat_coverage(point: vec3<f32>, direction: vec3<f32>, object: Object) -> f32 {
    let homogeneous = vec4(point, 1.0);
    let local = vec3(dot(object.inverse_rows[0], homogeneous),
        dot(object.inverse_rows[1], homogeneous), dot(object.inverse_rows[2], homogeneous));
    let local_direction = normalize(vec3(dot(object.inverse_rows[0].xyz, direction),
        dot(object.inverse_rows[1].xyz, direction), dot(object.inverse_rows[2].xyz, direction)));
    let resolution = i32(object.box_depth_meta.y);
    let face_size = u32(resolution * resolution);
    let sample_index = (u32(object.box_depth_max.w) - object.box_depth_meta.x) / 3u;
    let face = sample_index / face_size;
    let pixel_index = sample_index % face_size;
    let nearest = vec2<i32>(i32(pixel_index % u32(resolution)), i32(pixel_index / u32(resolution)));
    var optical_depth = 0.0;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let pixel = nearest + vec2<i32>(x, y);
            if any(pixel < vec2<i32>(0)) || any(pixel >= vec2<i32>(resolution)) { continue; }
            let offset = gaussian_splat_texel_offset(object, face, pixel);
            optical_depth += gaussian_splat_projected_weight(local, local_direction, offset);
        }
    }
    return 1.0 - exp(-optical_depth);
}

struct GaussianRayComposite {
    color: vec3<f32>,
    normal: vec3<f32>,
    alpha: f32,
    material_index: u32,
}

// Accumulate all visible capture layers in one hierarchy walk. Eight bins
// preserve front-to-back opacity while samples in one depth slice blend.
fn gaussian_splat_ray_composite(point: vec3<f32>, direction: vec3<f32>, object: Object) -> GaussianRayComposite {
    let p = vec4(point, 1.0);
    let local_origin = vec3(dot(object.inverse_rows[0], p),
        dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p));
    let local_direction = vec3(dot(object.inverse_rows[0].xyz, direction),
        dot(object.inverse_rows[1].xyz, direction), dot(object.inverse_rows[2].xyz, direction));
    let inverse_direction = vec3(1.0) /
        (select(vec3(-1.0), vec3(1.0), local_direction >= vec3(0.0))
            * max(abs(local_direction), vec3(1e-20)));
    let max_distance = length(object.box_depth_max.xyz - object.box_depth_min.xyz)
        / max(length(local_direction), 0.000001) + 1.0;
    var colors: array<vec4<f32>, 8>;
    var normals: array<vec3<f32>, 8>;
    var strongest: array<f32, 8>;
    var materials: array<u32, 8>;
    var node_index = object.box_depth_meta.w;
    let node_end = u32(object.box_depth_min.w);
    while node_index < node_end {
        let minimum = box_depth_texels[node_index];
        let maximum = box_depth_texels[node_index + 1u];
        let first = (minimum.xyz - local_origin) * inverse_direction;
        let second = (maximum.xyz - local_origin) * inverse_direction;
        let near = min(first, second);
        let far = max(first, second);
        if max(near.x, max(near.y, near.z)) > min(max_distance, min(far.x, min(far.y, far.z)))
            || min(far.x, min(far.y, far.z)) < -0.01 {
            node_index = u32(maximum.w);
            continue;
        }
        if minimum.w >= 0.0 {
            let offset = u32(minimum.w);
            let projected = gaussian_splat_projected_query(local_origin, local_direction, offset);
            if projected.x < 6.25 && projected.y >= -0.01 && projected.y <= max_distance {
                let weight = exp(-0.5 * projected.x)
                    * (1.0 - smoothstep(4.0, 6.25, projected.x));
                let bucket = min(u32(max(projected.y, 0.0) / max_distance * 8.0), 7u);
                let captured = box_depth_texels[offset + 1u];
                colors[bucket] += vec4(captured.yzw * weight, weight);
                normals[bucket] += box_depth_texels[offset + 2u].xyz * weight;
                if weight > strongest[bucket] {
                    strongest[bucket] = weight;
                    materials[bucket] = u32(captured.x);
                }
            }
        }
        node_index += 2u;
    }
    var color = vec3(0.0);
    var normal = vec3(0.0);
    var alpha = 0.0;
    var material_index = object.component.w;
    var material_found = false;
    for (var bucket = 0u; bucket < 8u; bucket++) {
        let weight = colors[bucket].w;
        if weight <= 0.0 { continue; }
        let contribution = (1.0 - alpha) * (1.0 - exp(-0.35 * weight));
        color += colors[bucket].xyz / weight * contribution;
        normal += normals[bucket] / weight * contribution;
        alpha += contribution;
        if !material_found {
            material_index = materials[bucket];
            material_found = true;
        }
    }
    if alpha <= 0.0 {
        return GaussianRayComposite(object.color.rgb, vec3(0.0, 1.0, 0.0), 0.0, object.component.w);
    }
    return GaussianRayComposite(color / alpha, normal / alpha, alpha, material_index);
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
    } else if HAS_NEURAL_SDF && object.state.y == 12 {
        distance = neural_shape(local, object);
    } else if object.state.y == 8 {
        distance = box_depth_shape(local, object);
    } else if object.state.y == 9 {
        distance = sphere_depth_shape(local, object);
    } else if object.state.y == 10 {
        distance = gaussian_splat_shape(local, object);
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
    if object.repeat_count.w == 1 {
        local = vec3(
            repeated_axis(local.x, object.repeat_spacing.x, object.repeat_count.x),
            repeated_axis(local.y, object.repeat_spacing.y, object.repeat_count.y),
            repeated_axis(local.z, object.repeat_spacing.z, object.repeat_count.z)
        );
    }
    var distance = primitive_distance(local, object);
    if object.repeat_count.w == 2 {
        let original = local;
        let pivot = object.repeat_spacing.xyz;
        let count = clamp(object.repeat_count.x, 1, 32);
        for (var copy = 1; copy < count; copy += 1) {
            let angle = -6.28318530718 * f32(copy) / f32(count);
            let c = cos(angle);
            let s = sin(angle);
            let p = original - pivot;
            var rotated = p;
            if object.repeat_count.y == 0 {
                rotated = vec3(p.x, c * p.y - s * p.z, s * p.y + c * p.z);
            } else if object.repeat_count.y == 1 {
                rotated = vec3(c * p.x + s * p.z, p.y, -s * p.x + c * p.z);
            } else {
                rotated = vec3(c * p.x - s * p.y, s * p.x + c * p.y, p.z);
            }
            let candidate = primitive_distance(rotated + pivot, object);
            if candidate < distance {
                distance = candidate;
                local = rotated + pivot;
            }
        }
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

// Inlays evaluate a host Boolean component from its native SDF primitives.
// Hosts with spatial modifiers are excluded by the editor and scene validator.
fn inlay_host_distance(point: vec3<f32>, host_index: u32, program: vec2<f32>) -> f32 {
    let packed_start = bitcast<u32>(program.x);
    let start = packed_start & 0x7fffffffu;
    let root = host_index;
    let parents_offset = bitcast<u32>(program.y);
    let parent = objects[root];
    if (packed_start & 0x80000000u) != 0u {
        var value = vec2(base_object_distance_at(point, parent), f32(root));
        for (var i = start; i < root; i++) {
            let child = objects[i];
            let next = vec2(base_object_distance_at(point, child), f32(i));
            value = combine_operand(value, next, child, parent);
        }
        return value.x;
    }
    var values: array<vec2<f32>, CSG_SIZE>;
    for (var i = start; i <= root; i++) {
        values[i - start] = vec2(base_object_distance_at(point, objects[i]), f32(i));
    }
    for (var i = start; i < root; i++) {
        let parent = bitcast<u32>(polygon_points[parents_offset + i].x) - start;
        let child = values[i - start];
        values[parent] = combine_operand(values[parent], child, objects[i], objects[parent + start]);
    }
    return values[root - start].x;
}

fn object_distance_at(sample_point: vec3<f32>, object: Object) -> f32 {
    let distance = base_object_distance_at(sample_point, object);
    if object.state.y == 6 || object.mirror_axes.w == 0u { return distance; }
    if object.mirror_axes.w == 0xffffffffu { return 100.0; }
    let offset = bitcast<u32>(object.repeat_spacing.w);
    let host_distance = inlay_host_distance(sample_point, object.mirror_axes.w - 1u, polygon_points[offset + 1u]);
    let inlay = polygon_points[offset];
    return max(distance, abs(host_distance - inlay.x) - inlay.y);
}

fn object_distance(point: vec3<f32>, object: Object) -> f32 {
    return object_distance_at(modifier_point(point, object), object);
}

// Each leaf is a complete Boolean component in contiguous postorder.
// Direct operands stream through one accumulator; nested components use scratch.
// A radial Boolean group repeats its complete result, including cuts. A small
// explicit traversal stack also supports radial groups nested inside operands.
fn component_has_radial_group(start: u32, root: u32) -> bool {
    for (var i = start; i <= root; i++) {
        if objects[i].repeat_count.w == 3 { return true; }
    }
    return false;
}

fn radial_group_point(point: vec3<f32>, object: Object, copy: u32) -> vec3<f32> {
    let h = vec4(point, 1.0);
    let local = vec3(dot(object.inverse_rows[0], h), dot(object.inverse_rows[1], h), dot(object.inverse_rows[2], h));
    let p = local - object.repeat_spacing.xyz;
    let angle = -6.28318530718 * f32(copy) / f32(clamp(object.repeat_count.x, 1, 32));
    let c = cos(angle);
    let s = sin(angle);
    var rotated = vec3(c*p.x-s*p.y, s*p.x+c*p.y, p.z);
    if object.repeat_count.y == 0 { rotated = vec3(p.x, c*p.y-s*p.z, s*p.y+c*p.z); }
    if object.repeat_count.y == 1 { rotated = vec3(c*p.x+s*p.z, p.y, -s*p.x+c*p.z); }
    let delta = rotated + object.repeat_spacing.xyz - local;
    let x = object.inverse_rows[0].xyz;
    let y = object.inverse_rows[1].xyz;
    let z = object.inverse_rows[2].xyz;
    let determinant = dot(x, cross(y, z));
    if abs(determinant) < 1e-20 { return point; }
    return point + (cross(y,z)*delta.x + cross(z,x)*delta.y + cross(x,y)*delta.z) / determinant;
}

fn radial_component_distance(point: vec3<f32>, start: u32, root: u32) -> vec2<f32> {
    var nodes: array<u32, CSG_SIZE>;
    var sources: array<vec3<f32>, CSG_SIZE>;
    var samples: array<vec3<f32>, CSG_SIZE>;
    var cursors: array<u32, CSG_SIZE>;
    var copies: array<u32, CSG_SIZE>;
    var counts: array<u32, CSG_SIZE>;
    var values: array<vec2<f32>, CSG_SIZE>;
    var best: array<vec2<f32>, CSG_SIZE>;
    var depth = 0u;
    var entering = true;
    nodes[0] = root;
    sources[0] = point;
    loop {
        let index = nodes[depth];
        let object = objects[index];
        if entering {
            copies[depth] = 0u;
            counts[depth] = select(1u, u32(clamp(object.repeat_count.x, 1, 32)), object.repeat_count.w == 3);
            best[depth] = vec2(1e30, f32(index));
            cursors[depth] = 0xffffffffu;
            entering = false;
        }
        if cursors[depth] == 0xffffffffu {
            var sample = sources[depth];
            if object.repeat_count.w == 3 { sample = radial_group_point(sample, object, copies[depth]); }
            sample = mirror_point(sample, object);
            var has_children = false;
            for (var child = start; child < index; child++) {
                if objects[child].state.w == i32(index) { has_children = true; break; }
            }
            var shape = object;
            if has_children && object.repeat_count.w == 1 { sample = group_repeat_point(sample, object); }
            if has_children { shape.repeat_count.w = 0; }
            samples[depth] = sample;
            values[depth] = vec2(object_distance_at(modifier_point(sample, object), shape), f32(index));
            cursors[depth] = start;
        }
        var child = cursors[depth];
        while child < index && objects[child].state.w != i32(index) { child += 1u; }
        if child < index {
            cursors[depth] = child + 1u;
            let sample = samples[depth];
            depth += 1u;
            nodes[depth] = child;
            sources[depth] = sample;
            entering = true;
            continue;
        }
        if values[depth].x < best[depth].x { best[depth] = values[depth]; }
        copies[depth] += 1u;
        if copies[depth] < counts[depth] {
            cursors[depth] = 0xffffffffu;
            continue;
        }
        let result = best[depth];
        if depth == 0u { return result; }
        depth -= 1u;
        values[depth] = combine_operand(values[depth], result, object, objects[nodes[depth]]);
    }
    return vec2(1e30, f32(root));
}

fn component_distance(point: vec3<f32>, start: u32, root: u32) -> vec2<f32> {
    if HAS_REPETITION && component_has_radial_group(start, root) {
        return radial_component_distance(point, start, root);
    }
    let mirrored_point = mirror_point(point, objects[root]);
    if !HAS_BOOLEANS || start == root {
        return vec2(object_distance(mirrored_point, objects[root]), f32(root));
    }
    if CSG_SIZE == 2u && root == start + 1u {
        let parent = objects[root];
        let operand = objects[start];
        let group_repeated = HAS_REPETITION && parent.repeat_count.w == 1;
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
    let parent = objects[root];
    if HAS_FLAT_UNIONS && parent.state.w == FLAT_UNION_ROOT {
        let group_repeated = HAS_REPETITION && parent.repeat_count.w == 1;
        let group_point = select(mirrored_point, group_repeat_point(mirrored_point, parent), group_repeated);
        let parent_point = modifier_point(group_point, parent);
        var parent_shape = parent;
        if group_repeated { parent_shape.repeat_count.w = 0; }
        var closest = vec2(object_distance_at(parent_point, parent_shape), f32(root));
        if parent.operand_tree.y > parent.operand_tree.x {
            // Exceptional operands (different cage, nonuniform scale, loft,
            // etc.) retain their full evaluator alongside the accelerated set.
            for (var i = start; i < root; i++) {
                let child = objects[i];
                if child.modifier.x == parent.modifier.x && child.distance_bound.w > 0.0 { continue; }
                var child_point = parent_point;
                if child.modifier.x != parent.modifier.x {
                    child_point = modifier_point(group_point, child);
                }
                let child_distance = object_distance_at(child_point, child);
                if child_distance < closest.x { closest = vec2(child_distance, f32(i)); }
            }
            var node_index = parent.operand_tree.x;
            while node_index < parent.operand_tree.y {
                let node = bvh[node_index];
                let lower_bound = distance(parent_point, node.center_radius.xyz) - node.center_radius.w;
                if lower_bound > closest.x {
                    node_index = node.metadata.y;
                    continue;
                }
                if node.metadata.x != 0xffffffffu {
                    let child = objects[node.metadata.x];
                    let distance_to_child = object_distance_at(parent_point, child);
                    // The linear path checks the root first, then children in
                    // object order. Preserve that owner when distances tie.
                    if distance_to_child < closest.x
                        || (distance_to_child == closest.x && closest.y != f32(root)
                            && node.metadata.x < u32(closest.y)) {
                        closest = vec2(distance_to_child, f32(node.metadata.x));
                    }
                }
                node_index += 1u;
            }
            return closest;
        }
        for (var i = start; i < root; i++) {
            let child = objects[i];
            if child.modifier.x == parent.modifier.x && child.distance_bound.w > 0.0 {
                let reach = child.distance_bound.w + closest.x;
                let offset = parent_point - child.distance_bound.xyz;
                if reach <= 0.0 || dot(offset, offset) > reach * reach { continue; }
            }
            var child_point = parent_point;
            if child.modifier.x != parent.modifier.x {
                child_point = modifier_point(group_point, child);
            }
            let distance = object_distance_at(child_point, child);
            if distance < closest.x { closest = vec2(distance, f32(i)); }
        }
        return closest;
    }
    if parent.state.w == FLAT_COMPONENT_ROOT {
        let group_repeated = HAS_REPETITION && parent.repeat_count.w == 1;
        let group_point = select(mirrored_point, group_repeat_point(mirrored_point, parent), group_repeated);
        let parent_point = modifier_point(group_point, parent);
        var parent_shape = parent;
        if group_repeated { parent_shape.repeat_count.w = 0; }
        var value = vec2(object_distance_at(parent_point, parent_shape), f32(root));
        for (var i = start; i < root; i++) {
            let child = objects[i];
            var child_point = parent_point;
            if child.modifier.x != parent.modifier.x {
                child_point = modifier_point(group_point, child);
            }
            let next = vec2(object_distance_at(child_point, child), f32(i));
            value = combine_operand(value, next, child, parent);
        }
        return value;
    }
    var values: array<vec2<f32>, CSG_SIZE>;
    let group_repeated = HAS_REPETITION && parent.repeat_count.w == 1;
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

fn scene_distance_limit(point: vec3<f32>, limit: f32, stop_on_inside: bool, skip_splats: bool) -> vec2<f32> {
    var closest = vec2(limit, 0.0);
    var node_index = 0u;
    while node_index < camera.count.y {
        let node = bvh[node_index];
        if skip_splats && node.metadata.x != 0xffffffffu
            && node.metadata.z == node.metadata.x
            && objects[node.metadata.x].state.y == 10 {
            node_index += 1u;
            continue;
        }
        var lower_bound: f32;
        if node.aabb_min.w > 0.0 {
            let delta = max(node.aabb_min.xyz - point, point - node.aabb_max.xyz);
            lower_bound = length(max(delta, vec3(0.0))) * node.aabb_min.w
                + min(max(delta.x, max(delta.y, delta.z)), 0.0)
                - node.aabb_max.w;
        } else {
            lower_bound = distance(point, node.center_radius.xyz) - node.center_radius.w;
        }
        if !USE_BVH || lower_bound < closest.x {
            if node.metadata.x != 0xffffffffu {
                var candidate: vec2<f32>;
                if HAS_BOOLEANS && analytic_subtraction(node.metadata.z, node.metadata.x)
                    && !hard_subtraction(node.metadata.z, node.metadata.x) {
                    candidate = smooth_subtraction_distance(point, node.metadata.z, node.metadata.x);
                } else {
                    candidate = component_distance(point, node.metadata.z, node.metadata.x);
                }
                if candidate.x < closest.x { closest = candidate; }
                if stop_on_inside && closest.x <= 0.0 { return closest; }
            }
            node_index += 1u;
        } else { node_index = node.metadata.y; }
    }
    return closest;
}

fn scene_distance(point: vec3<f32>) -> vec2<f32> {
    return scene_distance_limit(point, 100.0, false, false);
}

fn convex_primitive_normal(point: vec3<f32>, object: Object) -> vec3<f32> {
    let p = vec4(point, 1.0);
    let local = vec3(dot(object.inverse_rows[0], p),
        dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p));
    var gradient = vec3(0.0);
    if object.state.y == 1 {
        gradient = local / max(length(local), 0.000001);
    } else if object.state.y == 2 {
        let radius = clamp(object.scale.w, 0.0,
            min(object.params.x, min(object.params.y, object.params.z)));
        let q = abs(local) - (object.params.xyz - vec3(radius));
        let outside = max(q, vec3(0.0));
        if dot(outside, outside) > 0.00000001 {
            gradient = normalize(outside * sign(local));
        } else {
            let major = max(q.x, max(q.y, q.z));
            let axes = select(vec3(0.0), sign(local), q >= vec3(major - 0.00001));
            gradient = axes / max(length(axes), 0.000001);
        }
    } else {
        let radial = length(local.xz);
        let q = vec2(radial - object.params.x, abs(local.y) - object.params.y);
        let radial_gradient = vec3(local.x / max(radial, 0.000001), 0.0,
            local.z / max(radial, 0.000001));
        let cap_gradient = vec3(0.0, sign(local.y), 0.0);
        if q.x > 0.0 && q.y > 0.0 {
            gradient = normalize(radial_gradient * q.x + cap_gradient * q.y);
        } else {
            gradient = select(cap_gradient, radial_gradient, q.x >= q.y);
        }
    }
    let world = vec3(
        dot(gradient, vec3(object.inverse_rows[0].x, object.inverse_rows[1].x, object.inverse_rows[2].x)),
        dot(gradient, vec3(object.inverse_rows[0].y, object.inverse_rows[1].y, object.inverse_rows[2].y)),
        dot(gradient, vec3(object.inverse_rows[0].z, object.inverse_rows[1].z, object.inverse_rows[2].z)));
    return world / max(length(world), 0.000001);
}

fn scene_normal(point: vec3<f32>, index: u32) -> vec3<f32> {
    let component = objects[index].component;
    let neural_object = objects[index];
    if bitcast<f32>(objects[component.y].operand_tree.w) > 0.0 {
        let e = 0.003;
        let a = vec3(1.0, -1.0, -1.0);
        let b = vec3(-1.0, -1.0, 1.0);
        let c = vec3(-1.0, 1.0, -1.0);
        let d = vec3(1.0, 1.0, 1.0);
        let gradient = a * depth_accelerator_source_distance(point + a * e, index)
            + b * depth_accelerator_source_distance(point + b * e, index)
            + c * depth_accelerator_source_distance(point + c * e, index)
            + d * depth_accelerator_source_distance(point + d * e, index);
        return gradient / max(length(gradient), 0.000001);
    }
    if HAS_NEURAL_SDF && neural_object.state.y == 12 && component.x == component.y
        && neural_object.modifier.x == 0u && all(neural_object.mirror_axes == vec4<u32>(0u)) {
        let p = vec4(point, 1.0);
        let local = vec3(dot(neural_object.inverse_rows[0], p), dot(neural_object.inverse_rows[1], p), dot(neural_object.inverse_rows[2], p));
        let gradient = neural_gradient(local, neural_object);
        let world = vec3(
            dot(gradient, vec3(neural_object.inverse_rows[0].x, neural_object.inverse_rows[1].x, neural_object.inverse_rows[2].x)),
            dot(gradient, vec3(neural_object.inverse_rows[0].y, neural_object.inverse_rows[1].y, neural_object.inverse_rows[2].y)),
            dot(gradient, vec3(neural_object.inverse_rows[0].z, neural_object.inverse_rows[1].z, neural_object.inverse_rows[2].z)));
        // Cube clipping needs the ordinary finite-difference normal at its boundary.
        if all(abs(local) < neural_object.box_depth_max.xyz - vec3(0.003)) && dot(world, world) > 1e-12 {
            return world / length(world);
        }
    }
    if HAS_BOOLEANS && analytic_subtraction(component.x, component.y) {
        let parent = objects[component.y];
        let child = objects[component.x];
        let parent_distance = convex_distance_at(point, parent);
        let child_distance = convex_distance_at(point, child);
        let softness = bitcast<f32>(parent.component.z);
        var parent_weight = select(0.0, 1.0, parent_distance >= -child_distance);
        if softness > 0.0 {
            parent_weight = clamp(0.5 + (parent_distance + child_distance) / (2.0 * softness), 0.0, 1.0);
        }
        let gradient = parent_weight * convex_primitive_normal(point, parent)
            - (1.0 - parent_weight) * convex_primitive_normal(point, child);
        if dot(gradient, gradient) > 0.00000001 { return normalize(gradient); }
    }
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

// ENVIRONMENT_MODULE

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

// Clip a local ray against the convex polygon's supporting planes. Expanding
// each plane by the edge radius encloses the rounded SDF without changing it.
fn polygon_prism_interval(origin: vec3<f32>, direction: vec3<f32>, object: Object) -> vec2<f32> {
    let p = vec4(origin, 1.0);
    let v = vec4(direction, 0.0);
    let o = vec3(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p));
    let d = vec3(dot(object.inverse_rows[0], v), dot(object.inverse_rows[1], v), dot(object.inverse_rows[2], v));
    let offset = bitcast<u32>(object.params.y);
    let count = bitcast<u32>(object.params.z);
    let softness = clamp(object.scale.w, 0.0, object.params.x);
    let winding = select(-1.0, 1.0, object.box_depth_meta.w == 1u);
    var interval = vec2(-1e20, 1e20);
    for (var index = 0u; index < 65u; index++) {
        if index >= count { break; }
        let a = polygon_points[offset + index];
        let b = polygon_points[offset + select(index + 1u, 0u, index + 1u == count)];
        let edge = b - a;
        let outward = winding * vec2(edge.y, -edge.x);
        let plane = dot(outward, o.xy - a) - softness * length(edge);
        let slope = dot(outward, d.xy);
        if abs(slope) < 1e-20 {
            if plane > 0.0 { return vec2(1.0, -1.0); }
            continue;
        }
        let crossing = -plane / slope;
        if slope > 0.0 { interval.y = min(interval.y, crossing); }
        else { interval.x = max(interval.x, crossing); }
        if interval.x > interval.y { return vec2(1.0, -1.0); }
    }
    let safe_z = select(-1e-20, 1e-20, d.z >= 0.0);
    let z_direction = select(safe_z, d.z, abs(d.z) >= 1e-20);
    let first = (-object.params.x - o.z) / z_direction;
    let second = (object.params.x - o.z) / z_direction;
    return vec2(max(interval.x, min(first, second)), min(interval.y, max(first, second)));
}

fn has_polygon_prism_interval(object: Object) -> bool {
    return object.state.y == 5 && object.box_depth_meta.y == 0u
        && (object.box_depth_meta.w == 1u || object.box_depth_meta.w == 2u)
        && object.repeat_count.w == 0 && object.modifier.x == 0u
        && object.mirror_axes.w == 0u
        && all(object.mirror_axes.xyz == vec3<u32>(0u))
        && !brick_geometry_visible(object) && !fabric_geometry_visible(object);
}

fn has_analytic_interval(object: Object) -> bool {
    return object.repeat_count.w == 0 && object.state.y <= 3 && object.modifier.x == 0u
        && object.mirror_axes.w == 0u
        && all(object.mirror_axes.xyz == vec3<u32>(0u))
        && !(object.state.y == 2 && object.scale.w > 0.0)
        && !(object.state.y == 2 && brick_geometry_visible(object))
        && !fabric_geometry_visible(object);
}

fn analytic_subtraction(start: u32, root: u32) -> bool {
    return root == start + 1u && objects[start].state.z == 1
        && has_analytic_interval(objects[start]) && has_analytic_interval(objects[root]);
}

fn hard_subtraction(start: u32, root: u32) -> bool {
    return analytic_subtraction(start, root)
        && bitcast<f32>(objects[root].component.z) == 0.0;
}

// The interval path establishes that these primitives have no spatial
// modifiers, repetition, inlays, or geometry-changing materials.
fn convex_distance_at(point: vec3<f32>, object: Object) -> f32 {
    let p = vec4(point, 1.0);
    let local = vec3(dot(object.inverse_rows[0], p),
        dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p));
    return primitive_distance(local, object) * object.params.w;
}

fn smooth_subtraction_distance(point: vec3<f32>, start: u32, root: u32) -> vec2<f32> {
    let outer = objects[root];
    let inner = objects[start];
    let outer_distance = convex_distance_at(point, outer);
    let carved_distance = -convex_distance_at(point, inner);
    let softness = bitcast<f32>(outer.component.z);
    let h = max(softness - abs(outer_distance - carved_distance), 0.0) / softness;
    let distance = max(outer_distance, carved_distance) + softness * h * h * 0.25;
    return vec2(distance, f32(root));
}

// A convex solid minus another convex solid has at most two intervals on a ray.
// The first boundary of each interval comes from the outer entry or inner exit.
fn subtraction_entry(origin: vec3<f32>, direction: vec3<f32>, start: u32, root: u32) -> vec2<f32> {
    let outer = primitive_interval(origin, direction, objects[root]);
    if outer.x > outer.y { return vec2(100.0, -1.0); }
    let inner = primitive_interval(origin, direction, objects[start]);
    let first_end = min(outer.y, inner.x);
    if first_end >= max(outer.x, 0.0) {
        return vec2(max(outer.x, 0.0), f32(root));
    }
    let second_start = max(outer.x, inner.y);
    if outer.y >= max(second_start, 0.0) {
        return vec2(max(second_start, 0.0), f32(root));
    }
    return vec2(100.0, -1.0);
}

fn subtraction_exit(origin: vec3<f32>, direction: vec3<f32>, start: u32, root: u32) -> vec3<f32> {
    let outer = primitive_interval(origin, direction, objects[root]);
    let inner = primitive_interval(origin, direction, objects[start]);
    let overlapping = select(0.0, 1.0,
        inner.x <= inner.y && inner.x < outer.y && inner.y > outer.x);
    if outer.x <= 0.0 && min(outer.y, inner.x) > 0.0 {
        return vec3(min(outer.y, inner.x), f32(root), overlapping);
    }
    if max(outer.x, inner.y) <= 0.0 && outer.y > 0.0 {
        return vec3(outer.y, f32(root), overlapping);
    }
    return vec3(100.0, -1.0, 0.0);
}

// Traverse bounds once per ray, then march only the intersected primitives.
// The nearest boundary of a union is the nearest primitive boundary for rays
// starting outside. Interior rays retain the union marcher to cross overlaps.
fn empty_splat_exclusions() -> array<u32, 12> {
    var excluded: array<u32, 12>;
    for (var i = 0u; i < 12u; i++) { excluded[i] = 0xffffffffu; }
    return excluded;
}

// The atlas only guides approach. Exact source objects stay in the GPU array.
fn depth_accelerator_sample(point: vec3<f32>, direction: vec3<f32>, object: Object) -> vec2<f32> {
    let transform = object.box_depth_meta.w - 1u;
    let p = vec4(point, 1.0);
    let local = vec3(dot(box_depth_texels[transform], p),
        dot(box_depth_texels[transform + 1u], p), dot(box_depth_texels[transform + 2u], p));
    let scale = box_depth_texels[transform + 3u].x;
    if box_depth_texels[transform + 3u].y == 12.0 {
        let last = 31.0;
        let cell = vec3<u32>(clamp(round((local / object.box_depth_max.x + vec3(1.0)) * (last * 0.5)),
            vec3(0.0), vec3(last)));
        let index = cell.x + 32u * (cell.y + 32u * cell.z);
        let owner_offset = object.box_depth_meta.x
            + u32(box_depth_texels[object.box_depth_meta.x].w) + 2u * index + 1u;
        let ray = vec3(dot(box_depth_texels[transform].xyz, direction),
            dot(box_depth_texels[transform + 1u].xyz, direction), dot(box_depth_texels[transform + 2u].xyz, direction));
        let unit_ray = ray / max(length(ray), 1e-20);
        let q = abs(local) - object.box_depth_max.xyz;
        let cube = length(max(q, vec3(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
        let value = dot(neural_distance_vector(local, unit_ray, object), unit_ray);
        return vec2(max(cube, value) * scale, box_depth_texels[owner_offset].x - 1.0);
    }
    if box_depth_texels[transform + 3u].y == 8.0 {
        // Choose the box capture face facing this ray, including secondary rays.
        let view = -vec3(dot(box_depth_texels[transform].xyz, direction),
            dot(box_depth_texels[transform + 1u].xyz, direction),
            dot(box_depth_texels[transform + 2u].xyz, direction));
        let axis = abs(view);
        var face = select(5u, 4u, view.z >= 0.0);
        if axis.x >= axis.y && axis.x >= axis.z { face = select(1u, 0u, view.x >= 0.0); }
        else if axis.y >= axis.z { face = select(3u, 2u, view.y >= 0.0); }
        let depth = box_depth_face_depth(local, object, face);
        if depth < 0.0 {
            let q = max(object.box_depth_min.xyz - local, local - object.box_depth_max.xyz);
            let bound = length(max(q, vec3(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
            let width = object.box_depth_max.xyz - object.box_depth_min.xyz;
            return vec2(max(bound, max(-depth, min(width.x, min(width.y, width.z)) * 0.05)) * scale, -1.0);
        }
        let uv = box_depth_face_uv(local, object, face);
        let pixel = vec2<i32>(floor(uv * f32(object.box_depth_meta.y)));
        let offset = box_depth_texel_offset(object, face, pixel);
        return vec2(box_depth_face_plane(local, object, face, depth) * scale,
            box_depth_texels[offset + 1u].y - 1.0);
    }
    // Unlike the display-only sphere path, read radial depth even outside
    // the capture sphere: the refinement band belongs to the captured surface.
    let radius = object.box_depth_max.x;
    let sphere_distance = length(local) - radius;
    let depth = sphere_depth_at(local, object);
    if depth < 0.0 { return vec2(max(sphere_distance, max(-depth, radius * 0.05)) * scale, -1.0); }
    let uv = sphere_depth_uv(local);
    let pixel = vec2<i32>(floor(uv * vec2<f32>(f32(object.box_depth_meta.y), f32(object.box_depth_meta.z))));
    let offset = sphere_depth_texel_offset(object, pixel);
    let source_owner = box_depth_texels[offset + 1u].y - 1.0;
    return vec2((sphere_distance + depth) * scale, source_owner);
}

fn depth_accelerator_distance(point: vec3<f32>, direction: vec3<f32>, object: Object) -> f32 {
    return depth_accelerator_sample(point, direction, object).x;
}

fn depth_accelerator_texture_normal(point: vec3<f32>, direction: vec3<f32>, owner: u32) -> vec3<f32> {
    let root = objects[owner].operand_tree.z - 1u;
    let capture = objects[root];
    let e = 0.01;
    let a = vec3(1.0, -1.0, -1.0);
    let b = vec3(-1.0, -1.0, 1.0);
    let c = vec3(-1.0, 1.0, -1.0);
    let d = vec3(1.0, 1.0, 1.0);
    let gradient = a * depth_accelerator_distance(point + a * e, direction, capture)
        + b * depth_accelerator_distance(point + b * e, direction, capture)
        + c * depth_accelerator_distance(point + c * e, direction, capture)
        + d * depth_accelerator_distance(point + d * e, direction, capture);
    return gradient / max(length(gradient), 0.000001);
}

// Evaluate only the locked map owner, in the same modifier frame as its source.
fn depth_accelerator_source_distance(point: vec3<f32>, owner: u32) -> f32 {
    var object = objects[owner];
    let root = object.component.y;
    let parent = objects[root];
    if HAS_REPETITION && component_has_radial_group(parent.component.x, root) {
        return radial_component_distance(point, parent.component.x, root).x;
    }
    let mirrored_point = mirror_point(point, parent);
    let group_repeated = HAS_REPETITION && parent.repeat_count.w == 1;
    let group_point = select(mirrored_point, group_repeat_point(mirrored_point, parent), group_repeated);
    if owner == root && group_repeated { object.repeat_count.w = 0; }
    return object_distance_at(modifier_point(group_point, object), object);
}

// A depth texel can name a surface behind or beside the camera ray's first
// hit. Rank cheap analytic intervals in the same baked union once, then still
// march only the selected source.
fn depth_accelerator_alternative_owner(origin: vec3<f32>, direction: vec3<f32>,
    start: u32, root: u32, selected: u32, exclude_selected: bool,
    ray_start: f32, limit: f32) -> vec2<f32> {
    let captured_root = objects[selected].operand_tree.z;
    if captured_root == 0u || bitcast<f32>(objects[captured_root - 1u].component.z) > 0.0 {
        return vec2(-1.0, limit);
    }
    var best = vec2(-1.0, limit);
    for (var index = start; index <= root; index++) {
        let object = objects[index];
        if object.operand_tree.z != captured_root { continue; }
        if index != captured_root - 1u && object.state.z != 0 { return vec2(-1.0, limit); }
        if (exclude_selected && index == selected) || !has_analytic_interval(object) { continue; }
        let interval = primitive_interval(origin, direction, object);
        let entry = max(ray_start, interval.x);
        if interval.y >= entry && entry < best.y {
            best = vec2(f32(index), entry);
        }
    }
    return best;
}

// Keep an occupied depth-atlas texel visible if its chosen native primitive
// misses the camera ray. A learned field has no measured surface to preserve.
fn depth_accelerator_texture_hit(origin: vec3<f32>, direction: vec3<f32>,
    start: f32, end: f32, selected: u32, epsilon: f32) -> vec2<f32> {
    let capture_root = objects[selected].operand_tree.z;
    if capture_root == 0u { return vec2(end, -1.0); }
    let capture = objects[capture_root - 1u];
    let transform = capture.box_depth_meta.w - 1u;
    if box_depth_texels[transform + 3u].y == 12.0 { return vec2(end, -1.0); }
    var travel = start;
    for (var step = 0u; step < 192u; step++) {
        let sample = depth_accelerator_sample(origin + direction * travel, direction, capture);
        if sample.y >= 0.0 && sample.x <= epsilon { return vec2(travel, sample.y); }
        travel += max(sample.x * 0.35, epsilon * 0.5);
        if travel > end { break; }
    }
    return vec2(end, -1.0);
}

fn refine_depth_accelerator_crossing(origin: vec3<f32>, direction: vec3<f32>,
    low: f32, high: f32, owner: u32, epsilon: f32) -> vec3<f32> {
    var a = low;
    var b = high;
    let negative_at_a = depth_accelerator_source_distance(origin + direction * a, owner) < 0.0;
    var travel = (a + b) * 0.5;
    for (var iteration = 0u; iteration < 12u; iteration++) {
        travel = (a + b) * 0.5;
        let value = depth_accelerator_source_distance(origin + direction * travel, owner);
        if abs(value) <= epsilon { break; }
        if (value < 0.0) == negative_at_a { a = travel; } else { b = travel; }
    }
    return vec3(travel, f32(owner), -1.0);
}

// Captured subtrees contribute one texture query. Uncaptured siblings retain
// their native SDFs and the enclosing Boolean operations remain in effect.
fn depth_accelerator_component_distance(point: vec3<f32>, direction: vec3<f32>, start: u32, root: u32) -> vec2<f32> {
    if HAS_REPETITION && component_has_radial_group(start, root) {
        return radial_component_distance(point, start, root);
    }
    let parent = objects[root];
    let mirrored_point = mirror_point(point, parent);
    let group_repeated = HAS_REPETITION && parent.repeat_count.w == 1;
    let group_point = select(mirrored_point, group_repeat_point(mirrored_point, parent), group_repeated);
    let parent_point = modifier_point(group_point, parent);
    var values: array<vec2<f32>, CSG_SIZE>;
    for (var i = start; i <= root; i++) {
        var object = objects[i];
        let captured_root = object.operand_tree.z;
        if captured_root != 0u && captured_root != i + 1u { continue; }
        if captured_root == i + 1u {
            values[i - start] = depth_accelerator_sample(point, direction, object);
        } else {
            var sample_point = parent_point;
            if object.modifier.x != parent.modifier.x { sample_point = modifier_point(group_point, object); }
            if i == root && group_repeated { object.repeat_count.w = 0; }
            values[i - start] = vec2(object_distance_at(sample_point, object), f32(i));
        }
    }
    for (var i = start; i < root; i++) {
        let object = objects[i];
        if object.operand_tree.z != 0u && object.operand_tree.z != i + 1u { continue; }
        let parent_index = u32(object.state.w);
        values[parent_index - start] = combine_operand(values[parent_index - start], values[i - start], object, objects[parent_index]);
    }
    return values[root - start];
}

fn trace_objects(origin: vec3<f32>, direction: vec3<f32>, epsilon: f32,
    excluded_splats: array<u32, 12>, skip_splats: bool) -> vec3<f32> {
    neural_ray_direction = direction;
    var closest = 100.0;
    var owner = -1.0;
    var splat_offset = -1.0;
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
            if skip_splats && object.state.y == 10
                && node.metadata.z == node.metadata.x {
                node_index += 1u;
                continue;
            }
            if object.state.y == 10 {
                var excluded = false;
                for (var i = 0u; i < 12u; i++) {
                    if excluded_splats[i] == node.metadata.x { excluded = true; break; }
                }
                if excluded { node_index += 1u; continue; }
            }
            if object.state.y == 10 && node.metadata.z == node.metadata.x
                && object.box_depth_meta.w != 0u {
                let splat_entry = gaussian_splat_ray_entry(origin, direction, object, start, end);
                if splat_entry.x < closest {
                    closest = splat_entry.x;
                    owner = f32(node.metadata.x);
                    splat_offset = splat_entry.y;
                }
                node_index += 1u;
                continue;
            }
            var travel = start;
            let refinement_distance = bitcast<f32>(object.operand_tree.w);
            let accelerated = refinement_distance > 0.0;
            if !accelerated && HAS_BOOLEANS && analytic_subtraction(node.metadata.z, node.metadata.x) {
                let interval = subtraction_entry(origin, direction, node.metadata.z, node.metadata.x);
                if interval.y < 0.0 || interval.x > end {
                    node_index += 1u;
                    continue;
                }
                if hard_subtraction(node.metadata.z, node.metadata.x) {
                    if interval.x < closest {
                        closest = interval.x;
                        owner = interval.y;
                        splat_offset = -1.0;
                    }
                    node_index += 1u;
                    continue;
                }
                // Smooth subtraction only removes points from the hard result.
                // Its first hit cannot precede the exact hard interval entry;
                // start the original SDF marcher there to preserve the blend.
                travel = max(travel, interval.x - epsilon);
            }
            if !accelerated && node.metadata.z == node.metadata.x && has_analytic_interval(object) {
                let interval = primitive_interval(origin, direction, object);
                let candidate = max(0.0, interval.x);
                if interval.y >= candidate && candidate < closest {
                    closest = candidate;
                    owner = f32(node.metadata.x);
                    splat_offset = -1.0;
                }
                node_index += 1u;
                continue;
            }
            if !accelerated && node.metadata.z == node.metadata.x && has_polygon_prism_interval(object) {
                let interval = polygon_prism_interval(origin, direction, object);
                if interval.y < 0.0 || interval.x > end || interval.x > interval.y {
                    node_index += 1u;
                    continue;
                }
                travel = max(travel, interval.x - max(epsilon, 0.001));
            }
            let smooth_pair = HAS_BOOLEANS && analytic_subtraction(node.metadata.z, node.metadata.x);
            var neural_component = false;
            if !accelerated { neural_component = component_has_neural(f32(node.metadata.x)); }
            var selected_owner = -1.0;
            var recovered_owner = false;
            var hit_leaf = false;
            var previous_travel = travel;
            var previous_value = 0.0;
            var refining = !accelerated;
            var have_exact_sample = false;
            let step_limit = select(128u, 192u, accelerated);
            for (var step = 0u; step < step_limit; step++) {
                if !refining {
                    let coarse = depth_accelerator_component_distance(origin + direction * travel, direction,
                        node.metadata.z, node.metadata.x);
                    let band = max(refinement_distance, epsilon);
                    if coarse.x <= band && coarse.y >= 0.0 {
                        selected_owner = coarse.y;
                        // A sphere texel may belong to a nearby eye while
                        // the camera ray first crosses the duck's body.
                        if has_analytic_interval(objects[u32(selected_owner)]) {
                            let alternative = depth_accelerator_alternative_owner(origin, direction,
                                node.metadata.z, node.metadata.x, u32(selected_owner), false, start, end);
                            if alternative.x >= 0.0 && alternative.x != selected_owner {
                                selected_owner = alternative.x;
                                recovered_owner = true;
                                travel = max(start, alternative.y - epsilon);
                            }
                        }
                        refining = true;
                        if !recovered_owner { travel = max(start, travel - band); }
                        continue;
                    }
                    travel += max((coarse.x - band) * 0.35, epsilon * 0.5);
                    if travel > end { break; }
                    continue;
                }
                let point = origin + direction * travel;
                var sample: vec2<f32>;
                if accelerated {
                    sample = vec2(depth_accelerator_source_distance(point, u32(selected_owner)), selected_owner);
                } else if smooth_pair {
                    sample = smooth_subtraction_distance(point, node.metadata.z, node.metadata.x);
                } else {
                    sample = component_distance(point, node.metadata.z, node.metadata.x);
                }
                let value = sample.x;
                // If capture error put the handoff inside the source, restart
                // this candidate's exact march at its bound rather than shade
                // the texture or accept an interior point as a surface.
                if accelerated && !have_exact_sample && value < -epsilon && travel > start + epsilon {
                    travel = start;
                    continue;
                }
                if (neural_component || accelerated) && have_exact_sample && (value < 0.0) != (previous_value < 0.0) {
                    var refined: vec3<f32>;
                    if accelerated {
                        refined = refine_depth_accelerator_crossing(origin, direction,
                            previous_travel, travel, u32(selected_owner), epsilon);
                    } else {
                        refined = refine_neural_crossing(origin, direction, previous_travel, travel, sample.y);
                    }
                    closest = refined.x;
                    owner = refined.y;
                    splat_offset = -1.0;
                    hit_leaf = true;
                    break;
                }
                let tolerance = surface_hit_tolerance(sample.y, epsilon);
                let neural = HAS_NEURAL_SDF && sample.y >= 0.0 && objects[u32(max(sample.y, 0.0))].state.y == 12;
                if select(value < epsilon, abs(value) <= tolerance, neural || accelerated) {
                    closest = travel;
                    owner = sample.y;
                    splat_offset = -1.0;
                    hit_leaf = true;
                    break;
                }
                have_exact_sample = true;
                previous_travel = travel;
                previous_value = value;
                var march_factor = bitcast<f32>(object.modifier.y);
                if accelerated { march_factor = bitcast<f32>(objects[u32(selected_owner)].modifier.y); }
                travel += select(value, abs(value), neural_component || accelerated) * march_factor;
                if travel > end {
                    if accelerated && !recovered_owner {
                        let alternative = depth_accelerator_alternative_owner(origin, direction,
                            node.metadata.z, node.metadata.x, u32(selected_owner), true, start, end);
                        if alternative.x >= 0.0 {
                            selected_owner = alternative.x;
                            recovered_owner = true;
                            have_exact_sample = false;
                            travel = max(start, alternative.y - epsilon);
                            continue;
                        }
                    }
                    break;
                }
            }
            if accelerated && !hit_leaf && selected_owner >= 0.0 {
                let fallback = depth_accelerator_texture_hit(origin, direction, start, end,
                    u32(selected_owner), epsilon);
                if fallback.y >= 0.0 && fallback.x < closest {
                    closest = fallback.x;
                    owner = fallback.y;
                    splat_offset = -2.0;
                }
            }
        }
        node_index += 1u;
    }
    return vec3(closest, owner, splat_offset);
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
            var sample: vec2<f32>;
            if HAS_BOOLEANS && analytic_subtraction(node.metadata.z, node.metadata.x)
                && !hard_subtraction(node.metadata.z, node.metadata.x) {
                sample = smooth_subtraction_distance(point, node.metadata.z, node.metadata.x);
            } else {
                sample = component_distance(point, node.metadata.z, node.metadata.x);
            }
            if sample.x < -surface_hit_tolerance(sample.y, 0.0015) { return sample; }
        }
        node_index += 1u;
    }
    return vec2(0.0, -1.0);
}

// Reflections stay on the incident side; transmission crosses the surface.
// The caller already knows which side contains the ray, avoiding a scene query.
fn trace(origin: vec3<f32>, direction: vec3<f32>, inside: bool, initial_owner: u32) -> vec3<f32> {
    neural_ray_direction = direction;
    if USE_BVH {
        if !inside { return trace_objects(origin, direction, 0.0015, empty_splat_exclusions(), false); }
        var owner = initial_owner;
        var travel = 0.0;
        var previous_travel = 0.0;
        var previous_value = 0.0;
        for (var step = 0u; step < 128u; step++) {
            let component = objects[owner].component;
            var sample = vec2(0.0, f32(owner));
            if HAS_BOOLEANS && hard_subtraction(component.x, component.y) {
                let exit = subtraction_exit(origin + direction * travel, direction, component.x, component.y);
                if exit.y >= 0.0 {
                    travel += exit.x;
                    sample.y = exit.y;
                } else {
                    sample = component_distance(origin + direction * travel, component.x, component.y);
                }
            } else if component.x == component.y && has_analytic_interval(objects[owner]) {
                let interval = primitive_interval(origin, direction, objects[owner]);
                travel = max(travel, interval.y);
            } else {
                if HAS_BOOLEANS && analytic_subtraction(component.x, component.y) {
                    sample = smooth_subtraction_distance(origin + direction * travel,
                        component.x, component.y);
                } else {
                    sample = component_distance(origin + direction * travel, component.x, component.y);
                }
                if HAS_BOOLEANS && step == 0 && analytic_subtraction(component.x, component.y) {
                    // Most inside rays leave quickly through the smooth SDF.
                    // For a slow ray, the exact convex exit is an outside
                    // bracket for the smooth exit. Refine it with secants.
                    let exit = subtraction_exit(origin + direction * travel,
                        direction, component.x, component.y);
                    if exit.y >= 0.0 && exit.z > 0.0 && sample.x < 0.0 {
                        var low_t = travel;
                        var low_value = sample.x;
                        var high_t = travel + exit.x;
                        var high_value = smooth_subtraction_distance(origin + direction * high_t,
                            component.x, component.y).x;
                        if high_value >= 0.0 {
                            for (var refine = 0; refine < 6; refine++) {
                                let candidate = clamp(
                                    (low_t * high_value - high_t * low_value)
                                        / max(high_value - low_value, 0.000001),
                                    low_t, high_t);
                                let value = smooth_subtraction_distance(origin + direction * candidate,
                                    component.x, component.y).x;
                                if abs(value) < 0.0015 {
                                    travel = candidate;
                                    sample = vec2(0.0, exit.y);
                                    break;
                                }
                                if value < 0.0 {
                                    low_t = candidate;
                                    low_value = value;
                                } else {
                                    high_t = candidate;
                                    high_value = value;
                                }
                            }
                        }
                    }
                }
            }
            if step > 0u && component_has_neural(sample.y) && (sample.x < 0.0) != (previous_value < 0.0) {
                let refined = refine_neural_crossing(origin, direction, previous_travel, travel, sample.y);
                travel = refined.x;
                sample = vec2(0.0, refined.y);
            }
            if abs(sample.x) <= surface_hit_tolerance(sample.y, 0.0015) {
                let other = containing_component(origin + direction * travel);
                if other.y < 0.0 { return vec3(travel, sample.y, -1.0); }
                owner = u32(other.y);
                sample = other;
            }
            previous_travel = travel;
            previous_value = sample.x;
            travel += max(abs(sample.x) * 0.8, 0.0008);
            if travel > 100.0 { break; }
        }
        return vec3(100.0, -1.0, -1.0);
    }
    var travel = 0.0;
    var previous_travel = 0.0;
    var previous_sample = vec2(0.0, -1.0);
    for (var step = 0u; step < 128u; step++) {
        let sample = scene_distance(origin + direction * travel);
        let crossing_owner = select(sample.y, previous_sample.y, previous_sample.x < 0.0);
        if step > 0u && component_has_neural(crossing_owner) && (sample.x < 0.0) != (previous_sample.x < 0.0) {
            return refine_neural_crossing(origin, direction, previous_travel, travel, crossing_owner);
        }
        if abs(sample.x) <= surface_hit_tolerance(sample.y, 0.0015) { return vec3(travel, sample.y, -1.0); }
        previous_travel = travel;
        previous_sample = sample;
        travel += max(abs(sample.x) * 0.8, 0.0008);
        if travel > 100.0 { break; }
    }
    return vec3(100.0, -1.0, -1.0);
}

// MODIFIER_MODULES
// MATERIAL_MODULES

@fragment fn fs_depth(input: VertexOutput) -> @builtin(frag_depth) f32 {
    let far = camera.inverse_view_projection * vec4(input.clip, 1.0, 1.0);
    let near = camera.inverse_view_projection * vec4(input.clip, 0.0, 1.0);
    let far_point = far.xyz / far.w;
    let near_point = near.xyz / near.w;
    let ray = select(normalize(far_point - camera.position.xyz),
        normalize(far_point - near_point), camera.count.w != 0u);
    let origin = select(camera.position.xyz, near_point, camera.count.w != 0u);
    let hit = trace_objects(origin, ray, 0.003, empty_splat_exclusions(), HYBRID_SPLATS);
    if hit.y < 0.0 { return 1.0; }
    let clip = camera.view_projection * vec4(origin + ray * hit.x, 1.0);
    return clamp(clip.z / clip.w, 0.0, 1.0);
}

// DEFERRED_GEOMETRY_MODULE

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let far = camera.inverse_view_projection * vec4(input.clip, 1.0, 1.0);
    let near = camera.inverse_view_projection * vec4(input.clip, 0.0, 1.0);
    let far_point = far.xyz / far.w;
    let near_point = near.xyz / near.w;
    let ray = select(normalize(far_point - camera.position.xyz), normalize(far_point - near_point), camera.count.w != 0u);
    neural_ray_direction = ray;
    let ray_origin = select(camera.position.xyz, near_point, camera.count.w != 0u);
    var point = ray_origin;
    var hit = false;
    var index = 0u;
    var splat_offset = -1.0;
    var excluded_splats = empty_splat_exclusions();
    var excluded_splat_count = 0u;
    if USE_BVH {
        let sample = trace_objects(ray_origin, ray, 0.003, excluded_splats, HYBRID_SPLATS);
        hit = sample.y >= 0.0;
        index = u32(max(sample.y, 0.0));
        splat_offset = sample.z;
        point += ray * sample.x;
    } else {
        var travel = 0.0;
        var previous_travel = 0.0;
        var previous_sample = vec2(0.0, -1.0);
        for (var step = 0u; step < 96u; step++) {
            let scene = scene_distance(point);
            let crossing_owner = select(scene.y, previous_sample.y, previous_sample.x < 0.0);
            if step > 0u && component_has_neural(crossing_owner) && (scene.x < 0.0) != (previous_sample.x < 0.0) {
                let refined = refine_neural_crossing(ray_origin, ray, previous_travel, travel, crossing_owner);
                hit = true; index = u32(refined.y); point = ray_origin + ray * refined.x; break;
            }
            let neural = HAS_NEURAL_SDF && scene.y >= 0.0 && objects[u32(max(scene.y, 0.0))].state.y == 12;
            if select(scene.x < 0.003, abs(scene.x) <= surface_hit_tolerance(scene.y, 0.003), neural) {
                hit = true; index = u32(scene.y); break;
            }
            previous_travel = travel;
            previous_sample = scene;
            travel += select(scene.x, abs(scene.x), neural) * 0.8;
            if travel > 100.0 { break; }
            point = ray_origin + ray * travel;
        }
    }
    if !hit {
        if TRANSPARENT_BACKGROUND || camera.world_mode.x == 3u { return vec4(0.0); }
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
                var reflected_normal: vec3<f32>;
                if splat_offset == -2.0 {
                    reflected_normal = depth_accelerator_texture_normal(point, direction, index);
                } else {
                    reflected_normal = scene_normal(point, index);
                }
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
                if !TRANSPARENT_BACKGROUND && camera.world_mode.x != 3u {
                    radiance += throughput * background(direction);
                    throughput = vec3(0.0);
                }
                break;
            }
            // A habitat may contain several translucent shells (clouds and
            // canopy) before the landscape. Each shell has two crossings.
            if bounce >= 12 { break; }
            var object = objects[index];
            if object.state.y == 10 {
                object.box_depth_max.w = max(splat_offset, f32(object.box_depth_meta.x));
            }
            if object.state.y == 10 && object.component.x == object.component.y {
                let composite = gaussian_splat_ray_composite(point, direction, object);
                object.color = vec4(composite.color, object.color.a);
                object.component.w = composite.material_index;
                object.box_depth_max.w = -1.0;
                let local_normal = normalize(composite.normal);
                let normal = normalize(vec3(
                    dot(local_normal, vec3(object.inverse_rows[0].x, object.inverse_rows[1].x, object.inverse_rows[2].x)),
                    dot(local_normal, vec3(object.inverse_rows[0].y, object.inverse_rows[1].y, object.inverse_rows[2].y)),
                    dot(local_normal, vec3(object.inverse_rows[0].z, object.inverse_rows[1].z, object.inverse_rows[2].z))));
                let surface = material_surface(point, normal, -direction, object);
                let alpha = composite.alpha
                    * clamp(surface.opacity, 0.0, 1.0);
                if FAST_PREVIEW {
                    let light = select(normalize(vec3(2.0, 3.0, 2.0) - point),
                        normalize(camera.sun_direction.xyz), camera.world_mode.x == 1u);
                    let header = material_headers[material_capture(point, -direction, object).index];
                    let ambient = diffuse_ambient_fill(surface.normal,
                        select(0.13, 0.23, header.kind == MATERIAL_WOOD));
                    let shade = ambient + 0.78 * max(dot(normal, light), 0.0);
                    radiance += throughput * surface.color * shade * alpha;
                } else {
                    radiance += throughput * surface_light(point, normal, -direction,
                        object, surface, false) * alpha;
                }
                throughput *= 1.0 - alpha;
                if max(throughput.x, max(throughput.y, throughput.z)) < 0.01 { break; }
                if excluded_splat_count < 12u {
                    excluded_splats[excluded_splat_count] = index;
                    excluded_splat_count += 1u;
                }
                let next_origin = point + direction * 0.0005;
                let next = trace_objects(next_origin, direction, 0.0015, excluded_splats, false);
                hit = next.y >= 0.0;
                point = next_origin + direction * next.x;
                index = u32(max(next.y, 0.0));
                splat_offset = next.z;
                continue;
            }
            bounce += 1;
            var outward: vec3<f32>;
            if splat_offset == -2.0 {
                outward = depth_accelerator_texture_normal(point, direction, index);
            } else {
                outward = scene_normal(point, index);
            }
            let entering = dot(direction, outward) < 0.0;
            let normal = select(-outward, outward, entering);
            if FAST_PREVIEW {
                let light = select(normalize(vec3(2.0, 3.0, 2.0) - point),
                    normalize(camera.sun_direction.xyz), camera.world_mode.x == 1u);
                var surface = material_surface(point, normal, -direction, object);
                let decal = material_stencil_color(point, normal, object);
                let header = material_headers[material_capture(point, -direction, object).index];
                let ambient = diffuse_ambient_fill(surface.normal,
                    select(0.13, 0.23, header.kind == MATERIAL_WOOD));
                let shade = ambient + 0.78 * max(dot(normal, light), 0.0);
                if object.stencil_meta.w > 0.5 && object.stencil_meta.x > 0.5 && !metal_image_detail_enabled(object) {
                    surface.color = decal.rgb;
                    surface.opacity = decal.a;
                } else if decal.a > 0.0 {
                    surface.color = mix(surface.color, decal.rgb, decal.a);
                }
                let opacity = clamp(surface.opacity, 0.0, 1.0);
                var preview_color = surface.color * shade;
                if HAS_METAL_MATERIAL && surface.metal_response {
                    preview_color = surface_light(point, normal, -direction, object, surface, false);
                }
                radiance += throughput * preview_color * opacity;
                throughput *= 1.0 - opacity;
                if opacity >= 0.999 || max(throughput.x, max(throughput.y, throughput.z)) < 0.01 {
                    break;
                }
                let preview_origin = point + direction * max(0.007, 2.0 * surface_hit_tolerance(f32(index), 0.0));
                let next = trace(preview_origin, direction, entering, index);
                hit = next.y >= 0.0;
                point = preview_origin + direction * next.x;
                index = u32(max(next.y, 0.0));
                splat_offset = next.z;
                continue;
            }
            var surface = material_surface(point, normal, -direction, object);
            let decal = material_stencil_color(point, normal, object);
            surface.color = mix(surface.color, decal.rgb, decal.a);
            if object.stencil_meta.w > 0.5 && object.stencil_meta.x > 0.5 && !metal_image_detail_enabled(object) {
                surface.opacity = decal.a;
                surface.reflectivity = 0.0;
                surface.ior = 1.0;
                surface.metallic = 0.0;
            }
            if HAS_METAL_MATERIAL && surface.metal_response {
                let opacity = clamp(surface.opacity, 0.0, 1.0);
                var metal_ao = 1.0;
                if bounce == 1 { metal_ao = ambient_occlusion(point, normal); }
                var metal_color = metal_light(point, -direction, surface, metal_ao);
                if bounce == 1 && opacity > 0.0 {
                    metal_color += metal_scene_reflection(point, -direction, surface, metal_ao);
                }
                radiance += throughput * max(metal_color, vec3(0.0)) * opacity;
                throughput *= 1.0 - opacity;
                opaque = opacity >= 0.999;
                if opaque || max(throughput.x, max(throughput.y, throughput.z)) < 0.01 { break; }
                let metal_origin = point + direction * max(0.007, 2.0 * surface_hit_tolerance(f32(index), 0.0));
                let next = trace(metal_origin, direction, entering, index);
                hit = next.y >= 0.0;
                point = metal_origin + direction * next.x;
                index = u32(max(next.y, 0.0));
                splat_offset = next.z;
                continue;
            }
            let ior = max(surface.ior, 1.0);
            let f0 = pow((ior - 1.0) / (ior + 1.0), 2.0);
            // Matched refractive indices have zero interface reflectance at
            // every angle; Schlick's approximation alone misses this case.
            let fresnel = select(
                f0 + (1.0 - f0) * pow(1.0 - max(dot(-direction, normal), 0.0), 5.0),
                0.0, ior == 1.0);
            let reflection = reflect(direction, normal);
            let opacity = clamp(surface.opacity, 0.0, 1.0);
            let metallic = clamp(surface.metallic, 0.0, 1.0);
            let eta = select(ior, 1.0 / ior, entering);
            let transmitted = refract(direction, normal, eta);
            next_origin = point + normal * max(0.007, 2.0 * surface_hit_tolerance(f32(index), 0.0));
            next_direction = reflection;
            next_inside = !entering;
            if !(opacity < 0.999 && metallic < 0.999 && dot(transmitted, transmitted) < 0.001) {
                let roughness = clamp(surface.roughness, 0.0, 1.0);
                let weight = clamp(max(fresnel, surface.reflectivity), 0.0, 1.0);
                let tint = mix(vec3(1.0), object.color.rgb, metallic);
                reflection_weight = throughput * tint * weight * (1.0 - roughness * roughness);
                var lit_surface = vec3(0.0);
                if opacity > 0.0 {
                    lit_surface = surface_light(point, normal, -direction, object, surface, bounce == 1);
                }
                radiance += throughput * (vec3(0.22, 0.27, 0.35) * roughness * roughness * tint * weight
                    + lit_surface * opacity * (1.0 - weight));
                transmission_direction = transmitted;
                transmission_origin = point - normal * max(0.007, 2.0 * surface_hit_tolerance(f32(index), 0.0));
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
        splat_offset = next.z;
    }
    // The final transmission miss contributes the environment even at the
    // bounce cap, just as it does after each earlier transmitted ray.
    if !reflecting && !hit && !opaque { radiance += throughput * background(direction); }
    let transparent = TRANSPARENT_BACKGROUND || camera.world_mode.x == 3u;
    let alpha = select(1.0, clamp(1.0 - dot(throughput, vec3(0.33333334)), 0.0, 1.0), transparent);
    return vec4(radiance * camera.position.w, alpha);
}

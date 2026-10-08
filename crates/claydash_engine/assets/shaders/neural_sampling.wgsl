@group(1) @binding(0) var<uniform> training_params: TrainParams;
@group(1) @binding(1) var<storage, read_write> training_samples: array<TrainSample>;
@group(1) @binding(2) var<storage, read> training_indices: array<u32>;
// The exact component path deliberately excludes the viewport BVH and captures,
// keeping source sampling within WebGPU's eight-storage-buffer minimum.
fn train_exact_component_distance(point: vec3<f32>, start: u32, root: u32) -> vec2<f32> {
    let parent = objects[root];
    let mirrored = mirror_point(point, parent);
    if start == root { return vec2(object_distance(mirrored, parent), f32(root)); }
    let repeated = HAS_REPETITION && parent.repeat_count.w != 0;
    let group_point = select(mirrored, group_repeat_point(mirrored, parent), repeated);
    let parent_point = modifier_point(group_point, parent);
    var parent_shape = parent;
    if repeated { parent_shape.repeat_count.w = 0; }
    if parent.state.w == FLAT_UNION_ROOT || parent.state.w == FLAT_COMPONENT_ROOT {
        var result = vec2(object_distance_at(parent_point, parent_shape), f32(root));
        for (var i = start; i < root; i++) {
            let child = objects[i];
            var child_point = parent_point;
            if child.modifier.x != parent.modifier.x { child_point = modifier_point(group_point, child); }
            result = combine_operand(result, vec2(object_distance_at(child_point, child), f32(i)), child, parent);
        }
        return result;
    }
    var values: array<vec2<f32>, CSG_SIZE>;
    for (var i = start; i <= root; i++) {
        var object = objects[i];
        var sample_point = parent_point;
        if object.modifier.x != parent.modifier.x { sample_point = modifier_point(group_point, object); }
        if i == root && repeated { object.repeat_count.w = 0; }
        values[i - start] = vec2(object_distance_at(sample_point, object), f32(i));
    }
    for (var i = start; i < root; i++) {
        let parent_index = u32(objects[i].state.w) - start;
        values[parent_index] = combine_operand(values[parent_index], values[i - start], objects[i], objects[parent_index + start]);
    }
    return values[root - start];
}

fn train_sample_hash(input: u32) -> u32 {
    var value = input;
    value ^= value >> 16u;
    value *= 0x7feb352du;
    value ^= value >> 15u;
    value *= 0x846ca68bu;
    return value ^ (value >> 16u);
}
fn train_random_position(index: u32, seed: u32) -> vec3<f32> {
    let base = (index * 3u) ^ seed;
    return vec3<f32>(f32(train_sample_hash(base) >> 8u),
        f32(train_sample_hash(base + 1u) >> 8u),
        f32(train_sample_hash(base + 2u) >> 8u)) * (2.0 / 16777216.0) - vec3(1.0);
}
// Uniform sphere sampling produces a unit direction directly.
fn train_random_ray(index: u32, seed: u32) -> vec3<f32> {
    let base = index ^ seed ^ 0x68bc21ebu;
    let z = f32(train_sample_hash(base) >> 8u) * (2.0 / 16777216.0) - 1.0;
    let angle = f32(train_sample_hash(base ^ 0x02e5be93u) >> 8u) * (6.28318530718 / 16777216.0);
    let radius = sqrt(max(0.0, 1.0 - z * z));
    return vec3(radius * cos(angle), radius * sin(angle), z);
}
// Misses use the cube exit as a finite target. Sign follows the starting SDF
// so the renderer can retain its inside/outside crossing and CSG behavior.
fn train_ray_hit(p: vec3<f32>, ray: vec3<f32>, initial: f32) -> f32 {
    let world_step = (training_params.world * vec4(ray * training_params.cube.w, 0.0)).xyz;
    let speed = length(world_step);
    let edge = select(vec3(-1.0), vec3(1.0), ray >= vec3(0.0));
    let exits = (edge - p) / select(vec3(1e-20), ray, abs(ray) > vec3(1e-20));
    let limit = max(0.0, min(exits.x, min(exits.y, exits.z)));
    let epsilon = max(1e-5, training_params.options.x * 0.0001);
    var travel = 0.0;
    var distance = initial;
    var previous_travel = 0.0;
    var previous_distance = initial;
    for (var step = 0u; step < 256u; step++) {
        if abs(distance) <= epsilon { return select(travel, -travel, initial < 0.0); }
        if (distance < 0.0) != (previous_distance < 0.0) {
            var low = previous_travel;
            var high = travel;
            for (var refine = 0u; refine < 12u; refine++) {
                let mid = (low + high) * 0.5;
                let point = (training_params.world * vec4(training_params.cube.xyz + (p + ray * mid) * training_params.cube.w, 1.0)).xyz;
                let value = train_exact_component_distance(point, training_params.component.x, training_params.component.y).x - training_params.distance_target.x;
                if (value < 0.0) == (initial < 0.0) { low = mid; } else { high = mid; }
            }
            return select((low + high) * 0.5, -(low + high) * 0.5, initial < 0.0);
        }
        previous_distance = distance;
        previous_travel = travel;
        travel += max(abs(distance) * 0.8, epsilon * 0.5) / speed;
        if travel > limit { break; }
        let point = (training_params.world * vec4(training_params.cube.xyz + (p + ray * travel) * training_params.cube.w, 1.0)).xyz;
        distance = train_exact_component_distance(point, training_params.component.x, training_params.component.y).x - training_params.distance_target.x;
    }
    return select(limit, -limit, initial < 0.0);
}
@compute @workgroup_size(64)
fn train_sample_sdf(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= training_params.control.x { return; }
    let index = training_indices[id.x];
    let grid = training_params.control.y;
    var p = train_random_position(index, training_params.component.z);
    // Material ownership retains its fixed spatial lookup grid.
    if grid != 0u {
        p = vec3(f32(index % grid), f32((index / grid) % grid), f32(index / (grid * grid)))
            * (2.0 / f32(grid - 1u)) - vec3(1.0);
    }
    let point = (training_params.world * vec4(training_params.cube.xyz + p * training_params.cube.w, 1.0)).xyz;
    let sample = train_exact_component_distance(point, training_params.component.x, training_params.component.y);
    let drop = training_params.component.w != 0u && sample.x > 0.4
        && (train_sample_hash(index ^ training_params.component.z ^ 0xa511e9b3u) & 1u) != 0u;
    let ray = train_random_ray(index, training_params.component.z);
    let signed_distance = (sample.x - training_params.distance_target.x) / training_params.options.x;
    var distance = signed_distance;
    if grid == 0u && training_params.distance_target.y > 0.5 {
        distance = train_ray_hit(p, ray, sample.x - training_params.distance_target.x);
    }
    training_samples[id.x] = TrainSample(vec4(p, (sample.x - training_params.distance_target.x) / training_params.options.x),
        vec4<u32>(u32(sample.y), u32(drop), 0u, 0u), vec4(ray, 0.0), vec4(ray * distance, 0.0));
}

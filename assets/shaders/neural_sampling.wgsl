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
    training_samples[id.x] = TrainSample(vec4(p, sample.x / training_params.options.x),
        vec4<u32>(u32(sample.y), u32(drop), 0u, 0u));
}

// Architecture constants are supplied by the training pipeline builder.
override TRAIN_LAYER: u32 = 0u;
@group(0) @binding(0) var<uniform> training_params: TrainParams;
// x: weight, y/z: Adam moments, w: best validated checkpoint.
@group(0) @binding(1) var<storage, read_write> training_weights: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> training_samples: array<TrainSample>;
// x: activation, y: backpropagated delta. One row per sample/layer/neuron.
@group(0) @binding(3) var<storage, read_write> training_scratch: array<vec2<f32>>;
// 0: epoch statistics, 1: best statistics, 2: reserved, 3+: per-sample errors.
@group(0) @binding(4) var<storage, read_write> training_stats: array<vec4<f32>>;
fn train_activation(value: f32) -> f32 {
    if TRAIN_ACTIVATION == 0u { return max(value, 0.0); }
    return max(value, 0.0) + log(1.0 + exp(-10.0 * abs(value))) / 10.0;
}
fn train_slope(output: f32) -> f32 {
    if TRAIN_ACTIVATION == 0u { return select(0.0, 1.0, output > 0.0); }
    return 1.0 - exp(-10.0 * output);
}
fn train_layer_offset(layer: u32) -> u32 {
    if layer == 0u { return 0u; }
    return 4u * TRAIN_WIDTH + (layer - 1u) * TRAIN_WIDTH * (TRAIN_WIDTH + 1u);
}
fn train_output_offset() -> u32 { return train_layer_offset(TRAIN_LAYERS); }
fn train_scratch_index(sample: u32, layer: u32, neuron: u32) -> u32 {
    return (sample * TRAIN_LAYERS + layer) * TRAIN_WIDTH + neuron;
}
fn train_prediction(sample: u32) -> f32 {
    let offset = train_output_offset();
    var value = training_weights[offset + TRAIN_WIDTH].x;
    for (var i = 0u; i < TRAIN_WIDTH; i++) {
        value += training_weights[offset + i].x * training_scratch[train_scratch_index(sample, TRAIN_LAYERS - 1u, i)].x;
    }
    return value;
}
@compute @workgroup_size(64)
fn train_forward(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x / TRAIN_WIDTH;
    let row = id.x % TRAIN_WIDTH;
    if sample >= training_params.control.x { return; }
    if training_samples[sample].owner.y != 0u { return; }
    let inputs = select(TRAIN_WIDTH, 3u, TRAIN_LAYER == 0u);
    let offset = train_layer_offset(TRAIN_LAYER) + row * (inputs + 1u);
    var value = training_weights[offset + inputs].x;
    for (var col = 0u; col < inputs; col++) {
        var input = 0.0;
        if TRAIN_LAYER == 0u { input = training_samples[sample].point[col]; }
        else { input = training_scratch[train_scratch_index(sample, TRAIN_LAYER - 1u, col)].x; }
        value += training_weights[offset + col].x * input;
    }
    training_scratch[train_scratch_index(sample, TRAIN_LAYER, row)].x = train_activation(value);
}
@compute @workgroup_size(64)
fn train_output_delta(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x;
    if sample >= training_params.control.x { return; }
    if training_samples[sample].owner.y != 0u { return; }
    let error = 2.0 * (train_prediction(sample) - training_samples[sample].point.w);
    training_stats[3u + sample].x = error;
    for (var row = 0u; row < TRAIN_WIDTH; row++) {
        let index = train_scratch_index(sample, TRAIN_LAYERS - 1u, row);
        training_scratch[index].y = error * training_weights[train_output_offset() + row].x * train_slope(training_scratch[index].x);
    }
}
@compute @workgroup_size(64)
fn train_backward(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x / TRAIN_WIDTH;
    let row = id.x % TRAIN_WIDTH;
    if sample >= training_params.control.x { return; }
    if training_samples[sample].owner.y != 0u { return; }
    var delta = 0.0;
    for (var next = 0u; next < TRAIN_WIDTH; next++) {
        delta += training_weights[train_layer_offset(TRAIN_LAYER + 1u) + next * (TRAIN_WIDTH + 1u) + row].x
            * training_scratch[train_scratch_index(sample, TRAIN_LAYER + 1u, next)].y;
    }
    let index = train_scratch_index(sample, TRAIN_LAYER, row);
    training_scratch[index].y = delta * train_slope(training_scratch[index].x);
}
@compute @workgroup_size(64)
fn train_adam(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if index >= TRAIN_PARAMETERS { return; }
    let output = index >= train_output_offset();
    var layer = 0u;
    var local = index;
    if output { local = index - train_output_offset(); }
    else if index >= 4u * TRAIN_WIDTH {
        layer = 1u + (index - 4u * TRAIN_WIDTH) / (TRAIN_WIDTH * (TRAIN_WIDTH + 1u));
        local = index - train_layer_offset(layer);
    }
    let inputs = select(select(TRAIN_WIDTH, 3u, layer == 0u), TRAIN_WIDTH, output);
    let row = local / (inputs + 1u);
    let col = select(local % (inputs + 1u), local, output);
    var gradient = 0.0;
    var kept = 0u;
    for (var sample = 0u; sample < training_params.control.x; sample++) {
        if training_samples[sample].owner.y != 0u { continue; }
        kept += 1u;
        var input = 1.0;
        if col < inputs {
            if output { input = training_scratch[train_scratch_index(sample, TRAIN_LAYERS - 1u, col)].x; }
            else if layer == 0u { input = training_samples[sample].point[col]; }
            else { input = training_scratch[train_scratch_index(sample, layer - 1u, col)].x; }
        }
        var delta = training_stats[3u + sample].x;
        if !output { delta = training_scratch[train_scratch_index(sample, layer, row)].y; }
        gradient += input * delta;
    }
    if kept == 0u { return; }
    gradient /= f32(kept);
    var state = training_weights[index];
    state.y = 0.9 * state.y + 0.1 * gradient;
    state.z = 0.999 * state.z + 0.001 * gradient * gradient;
    state.x -= training_params.options.y * (state.y / training_params.options.z)
        / (sqrt(state.z / training_params.options.w) + 1e-8);
    training_weights[index] = state;
}
@compute @workgroup_size(64)
fn train_validate(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x;
    if sample >= training_params.control.x { return; }
    if training_samples[sample].owner.y != 0u { return; }
    let value = train_prediction(sample);
    let expected_distance = training_samples[sample].point.w;
    let error = value - expected_distance;
    var stats = vec4(error * error, abs(error), value, value);
    if !(abs(value) < 1e29 && abs(expected_distance) < 1e29) { stats = vec4(1e30, 1e30, 0.0, 0.0); }
    training_stats[3u + sample] = stats;
}
@compute @workgroup_size(1)
fn train_reduce() {
    var stats = training_stats[0];
    if training_params.control.w != 0u { stats = vec4(0.0, 0.0, 1e30, -1e30); }
    for (var sample = 0u; sample < training_params.control.x; sample++) {
        let value = training_stats[3u + sample];
        stats = vec4(stats.x + value.x, max(stats.y, value.y), min(stats.z, value.z), max(stats.w, value.w));
    }
    training_stats[0] = stats;
}
@compute @workgroup_size(64)
fn train_checkpoint(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < TRAIN_PARAMETERS && training_stats[0].x < training_stats[1].x {
        training_weights[id.x].w = training_weights[id.x].x;
    }
}
@compute @workgroup_size(1)
fn train_commit_stats() {
    if training_stats[0].x < training_stats[1].x { training_stats[1] = training_stats[0]; }
}

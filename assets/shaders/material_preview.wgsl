// Preview binding ABI reuses uploaded sphere and material parameters.
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> objects: array<Object>;
@group(0) @binding(11) var image_atlas: texture_2d_array<f32>;
@group(0) @binding(12) var image_sampler: sampler;
struct MaterialCapture { color: vec3<f32>, index: u32 }
fn material_capture(point: vec3<f32>, view: vec3<f32>, object: Object) -> MaterialCapture {
    return MaterialCapture(object.color.rgb, object.component.w);
}
fn stencil_color(point: vec3<f32>, normal: vec3<f32>, object: Object) -> vec4<f32> {
    // Sphere previews have no stencil placement; custom materials can still call this contract.
    return vec4(0.0);
}
struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) clip: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -3.0), vec2(3.0, 1.0), vec2(-1.0, 1.0));
    var out: VertexOutput;
    out.clip = positions[index];
    out.position = vec4(out.clip, 0.0, 1.0);
    return out;
}
fn preview_distance(point: vec3<f32>) -> f32 {
    let object = objects[0];
    var distance = length(point) - object.params.x;
    // PREVIEW_FABRIC_DISTANCE
    return distance;
}
fn preview_trace(origin: vec3<f32>, direction: vec3<f32>, inside: bool, epsilon: f32) -> vec2<f32> {
    let radius = objects[0].params.x;
    let b = dot(origin, direction);
    let discriminant = b * b - dot(origin, origin) + radius * radius;
    if discriminant < 0.0 { return vec2(100.0, -1.0); }
    let chord = sqrt(discriminant);
    let entry = max(-b - chord, 0.0);
    let exit = -b + chord;
    if exit < 0.0 { return vec2(100.0, -1.0); }
    // PREVIEW_FABRIC_TRACE
    return vec2(select(entry, exit, inside), 0.0);
}
fn preview_normal(point: vec3<f32>) -> vec3<f32> {
    // Preserve the viewport's finite difference normal, including woven relief.
    let e = 0.003;
    let a = vec3(1.0, -1.0, -1.0);
    let b = vec3(-1.0, -1.0, 1.0);
    let c = vec3(-1.0, 1.0, -1.0);
    let d = vec3(1.0, 1.0, 1.0);
    return normalize(a * preview_distance(point + a * e) + b * preview_distance(point + b * e)
        + c * preview_distance(point + c * e) + d * preview_distance(point + d * e));
}
fn scene_distance_limit(point: vec3<f32>, limit: f32, solids: bool, ignore_splats: bool) -> vec2<f32> {
    return vec2(min(preview_distance(point), limit), 0.0);
}
// PREVIEW_METAL_REFLECTION

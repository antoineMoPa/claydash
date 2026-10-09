struct Camera {
    view_projection: mat4x4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    light: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec4<f32>,
    @location(1) normal: vec4<f32>,
}
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
}
@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = camera.view_projection * vec4(input.position.xyz, 1.0);
    output.normal = input.normal.xyz;
    return output;
}
@fragment fn fs_main(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let normal = normalize(select(-input.normal, input.normal, front));
    let view = normalize(cross(camera.right.xyz, camera.up.xyz));
    let fill = normalize(view + camera.up.xyz * 0.65 + camera.right.xyz * 0.35);
    let key = max(dot(normal, camera.light.xyz), 0.0);
    let bounce = max(dot(normal, fill), 0.0);
    let horizon = normal.y * 0.5 + 0.5;
    // Mesh preview fill follows the ambient control, including a true zero.
    let fill_strength = camera.right.w / 0.13;
    let shade = camera.right.w + camera.light.w * key * 0.8
        + fill_strength * (0.32 * bounce + 0.16 * horizon);
    return vec4(vec3(0.76, 0.78, 0.81) * shade * camera.up.w, 1.0);
}

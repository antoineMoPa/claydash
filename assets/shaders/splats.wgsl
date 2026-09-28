struct Camera {
    view_projection: mat4x4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    light: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;

struct Instance {
    @location(0) center_radius: vec4<f32>,
    @location(1) color_opacity: vec4<f32>,
    @location(2) normal: vec4<f32>,
}
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) offset: vec2<f32>,
    @location(1) color_opacity: vec4<f32>,
    @location(2) normal: vec3<f32>,
}

@vertex fn vs_main(@builtin(vertex_index) vertex: u32, instance: Instance) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(-2.5, -2.5), vec2(2.5, -2.5), vec2(-2.5, 2.5),
        vec2(-2.5, 2.5), vec2(2.5, -2.5), vec2(2.5, 2.5));
    let offset = corners[vertex];
    let world = instance.center_radius.xyz
        + (camera.right.xyz * offset.x + camera.up.xyz * offset.y) * instance.center_radius.w;
    var out: VertexOutput;
    out.position = camera.view_projection * vec4(world, 1.0);
    out.offset = offset;
    out.color_opacity = instance.color_opacity;
    out.normal = instance.normal.xyz;
    return out;
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let radius2 = dot(input.offset, input.offset);
    let alpha = exp(-0.5 * radius2)
        * (1.0 - smoothstep(4.0, 6.25, radius2)) * input.color_opacity.a;
    if alpha < 0.002 { discard; }
    let shade = camera.right.w
        + camera.light.w * max(dot(normalize(input.normal), camera.light.xyz), 0.0);
    return vec4(input.color_opacity.rgb * shade * alpha * camera.up.w, alpha);
}

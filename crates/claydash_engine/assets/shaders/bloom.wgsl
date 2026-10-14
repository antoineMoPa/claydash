struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let p = array<vec2<f32>, 3>(vec2(-1.0, -3.0), vec2(3.0, 1.0), vec2(-1.0, 1.0));
    return VertexOutput(vec4(p[index], 0.0, 1.0), p[index] * vec2(0.5, -0.5) + vec2(0.5));
}
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var detail: texture_2d<f32>;
@group(0) @binding(2) var filtering: sampler;
@group(0) @binding(3) var<uniform> settings: vec4<f32>;
fn blur(uv: vec2<f32>) -> vec3<f32> {
    let step = vec2(settings.x) / vec2<f32>(textureDimensions(image));
    var color = textureSampleLevel(image, filtering, uv, 0.0).rgb * 0.5;
    color += textureSampleLevel(image, filtering, uv + step, 0.0).rgb * 0.125;
    color += textureSampleLevel(image, filtering, uv - step, 0.0).rgb * 0.125;
    color += textureSampleLevel(image, filtering, uv + step * vec2(1.0, -1.0), 0.0).rgb * 0.125;
    color += textureSampleLevel(image, filtering, uv + step * vec2(-1.0, 1.0), 0.0).rgb * 0.125;
    return color;
}
@fragment fn fs_down(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(blur(input.uv), 1.0);
}
@fragment fn fs_up(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(blur(input.uv) * 0.65 + textureSampleLevel(detail, filtering, input.uv, 0.0).rgb * 0.35, 1.0);
}

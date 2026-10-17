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
    @location(3) axis_x: vec4<f32>,
    @location(4) axis_y: vec4<f32>,
    @location(5) axis_z: vec4<f32>,
}
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) offset: vec2<f32>,
    @location(1) color_opacity: vec4<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) imported: f32,
}

@vertex fn vs_main(@builtin(vertex_index) vertex: u32, instance: Instance) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(-2.5, -2.5), vec2(2.5, -2.5), vec2(-2.5, 2.5),
        vec2(-2.5, 2.5), vec2(2.5, -2.5), vec2(2.5, 2.5));
    let offset = corners[vertex];
    // Project a surface-aligned Gaussian with sigma along the tangent plane
    // and 0.25 * sigma along its normal, matching the ray compositor.
    let screen_normal = vec2(dot(instance.normal.xyz, camera.right.xyz),
        dot(instance.normal.xyz, camera.up.xyz));
    let normal_length = length(screen_normal);
    var minor_axis = vec2(1.0, 0.0);
    if normal_length > 0.000001 { minor_axis = screen_normal / normal_length; }
    let major_axis = vec2(-minor_axis.y, minor_axis.x);
    let minor_scale = sqrt(max(1.0 - 0.9375 * dot(screen_normal, screen_normal), 0.0625));
    let projected = major_axis * offset.x + minor_axis * offset.y * minor_scale;
    let world = instance.center_radius.xyz
        + (camera.right.xyz * projected.x + camera.up.xyz * projected.y) * instance.center_radius.w;
    var out: VertexOutput;
    out.position = camera.view_projection * vec4(world, 1.0);
    out.imported = instance.normal.w;
    if instance.normal.w > 0.5 {
        let center = camera.view_projection * vec4(instance.center_radius.xyz, 1.0);
        let cx = camera.view_projection * instance.axis_x;
        let cy = camera.view_projection * instance.axis_y;
        let cz = camera.view_projection * instance.axis_z;
        let w = max(center.w, 0.00001);
        let ndc = center.xy / w;
        let x = (cx.xy - ndc * cx.w) / w;
        let y = (cy.xy - ndc * cy.w) / w;
        let z = (cz.xy - ndc * cz.w) / w;
        let a = x.x*x.x + y.x*y.x + z.x*z.x;
        let b = x.x*x.y + y.x*y.y + z.x*z.y;
        let d = x.y*x.y + y.y*y.y + z.y*z.y;
        let delta = sqrt(max((a-d)*(a-d) + 4.0*b*b, 0.0));
        let l1 = max((a+d+delta)*0.5, 0.000000000001);
        let l2 = max((a+d-delta)*0.5, 0.000000000001);
        let angle = 0.5 * atan2(2.0*b, a-d);
        let major = vec2(cos(angle), sin(angle));
        let minor = vec2(-major.y, major.x);
        out.position = center;
        out.position = vec4(center.xy + (major * sqrt(l1) * offset.x + minor * sqrt(l2) * offset.y) * center.w, center.zw);
        if center.w <= 0.0 { out.position = vec4(2.0, 2.0, 2.0, 1.0); }
    }
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
    if input.imported > 0.5 { return vec4(input.color_opacity.rgb * alpha, alpha); }
    let ambient_hemisphere = mix(0.60, 1.0, clamp(input.normal.y * 0.5 + 0.5, 0.0, 1.0));
    let shade = camera.right.w * ambient_hemisphere
        + camera.light.w * max(dot(normalize(input.normal), camera.light.xyz), 0.0);
    return vec4(input.color_opacity.rgb * shade * alpha * camera.up.w, alpha);
}

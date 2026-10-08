struct PoissonBakeVertex {
    @builtin(position) clip: vec4<f32>,
    @location(0) tile: vec2<f32>,
    @location(1) @interpolate(flat) a: vec4<f32>,
    @location(2) @interpolate(flat) b: vec3<f32>,
    @location(3) @interpolate(flat) c: vec3<f32>,
    @location(4) @interpolate(flat) na: vec3<f32>,
    @location(5) @interpolate(flat) nb: vec3<f32>,
    @location(6) @interpolate(flat) nc: vec3<f32>,
}
@vertex fn vs_poisson_bake(@builtin(vertex_index) vertex: u32,
    @location(0) a: vec4<f32>, @location(1) b: vec4<f32>,
    @location(2) c: vec4<f32>, @location(3) tile: vec4<f32>,
    @location(4) na: vec4<f32>, @location(5) nb: vec4<f32>,
    @location(6) nc: vec4<f32>) -> PoissonBakeVertex {
    let corners = array<vec2<f32>, 6>(vec2(0.0,0.0), vec2(1.0,0.0), vec2(0.0,1.0),
        vec2(0.0,1.0), vec2(1.0,0.0), vec2(1.0,1.0));
    let uv = corners[vertex];
    let position = (tile.xy + uv * 16.0) / tile.z;
    var output: PoissonBakeVertex;
    output.clip = vec4(position.x * 2.0 - 1.0, 1.0 - position.y * 2.0, 0.0, 1.0);
    output.tile = uv * 16.0;
    output.a = a; output.b = b.xyz; output.c = c.xyz;
    output.na = na.xyz; output.nb = nb.xyz; output.nc = nc.xyz;
    return output;
}
@fragment fn fs_poisson_bake(input: PoissonBakeVertex) -> @location(0) vec4<f32> {
    // Two-pixel gutter keeps bilinear filtering inside each triangle tile.
    var uv = max((input.tile - vec2(2.0)) / 12.0, vec2(0.0));
    if uv.x + uv.y > 1.0 {
        uv = clamp(uv - vec2((uv.x + uv.y - 1.0) * 0.5), vec2(0.0), vec2(1.0));
    }
    let point = input.a.xyz * (1.0 - uv.x - uv.y) + input.b * uv.x + input.c * uv.y;
    let normal = normalize(input.na * (1.0 - uv.x - uv.y)
        + input.nb * uv.x + input.nc * uv.y);
    // Export base colour, not viewport illumination or camera exposure. The
    // importing renderer supplies directional lights and specular highlights.
    let sampled = poisson_surface(point, normal, u32(input.a.w + 0.5));
    return vec4(max(sampled.surface.color, vec3(0.0)), clamp(sampled.surface.opacity, 0.0, 1.0));
}

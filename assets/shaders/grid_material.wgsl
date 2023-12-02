struct VertexOutput {
    // This is `clip position` when the struct is used as a vertex stage output
    // and `frag coord` when used as a fragment stage input
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
#ifdef VERTEX_UVS
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(3) world_tangent: vec4<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(4) color: vec4<f32>,
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(5) @interpolate(flat) instance_index: u32,
#endif
}

fn gen_grid(p: vec3<f32>, grid_line_width: f32) -> f32 {
    // Each square in this grid is one unit
    var grid_x: f32 = max(cos(p.x * 3.1415 * 2.0) - (1.0 - grid_line_width), 0.0) * (1.0 / grid_line_width);
    var grid_z: f32 = max(cos(p.z * 3.1415 * 2.0) - (1.0 - grid_line_width), 0.0) * (1.0 / grid_line_width);

    var grid: f32 = max(grid_x, grid_z);

    return clamp(grid, 0.0, 1.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = in.world_position.xyz;

    var main_grid: f32 = gen_grid(p, 0.001);
    var secondary_grid: f32 = gen_grid(p * 10.0, 0.01);

    main_grid *= 1.0 / pow(length(p), 2.0);

    var col = vec4<f32>(main_grid, main_grid, main_grid, main_grid);
    secondary_grid *= 0.3;
    secondary_grid *= 1.0 / pow(length(p), 2.0);
    col += vec4<f32>(secondary_grid, secondary_grid, secondary_grid, 1.0);

    return col;
}

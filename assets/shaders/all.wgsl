// The time since startup data is in the globals binding which is part of the mesh_view_bindings import
#import bevy_pbr::mesh_view_bindings globals

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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t_1 = sin(globals.time) * 0.5 + 0.5;

    let red = vec3<f32>(0.627955, 0.224863, 0.125846);

    return vec4<f32>(red, 1.0);
}

// The time since startup data is in the globals binding which is part of the mesh_view_bindings import
#import bevy_pbr::mesh_view_bindings globals

// TODO: see if using AssetLoader fixes import
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

    var position = in.world_position.xyz;

    let direction = vec3(0.0, 0.0, -1.0);


    let sphere_r = 0.2;
    let sphere_position = vec3(0.0);
    var d_sphere = 0.0;

    for (var i: i32 = 0; i < 10; i++) {
        d_sphere = length(position - sphere_position) - sphere_r;
        position += direction * d_sphere;
    }

    return vec4<f32>(in.position.x / 2000.0, in.position.y / 2000.0, 1.0/d_sphere, 1.0);
}

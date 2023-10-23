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

struct CustomMaterial {
    mouse: vec2<f32>,
};

@group(1) @binding(0) var<uniform> material: CustomMaterial;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var position = in.world_position.xyz;

    let direction = vec3(0.0, 0.0, -1.0);

    let sphere_r = 0.02 + cos(position.y * 40.0 + globals.time) * 0.002;
    let sphere_position = vec3(material.mouse.x, material.mouse.y, 0.0);

    var box_position = vec3(0.0, 0.0, 0.0);
    let box_parameters = vec3(0.03, 0.03, 0.03);

    var d_sphere = 0.0;
    var d_box = 0.0;
    var box_q = vec3(0.0);
    var max_box_q = vec3(0.0);
    var min_d = 0.0;

    for (var i: i32 = 0; i < 30; i++) {
        d_sphere = length(position - sphere_position) - sphere_r;

        box_q = abs(position - box_position) - box_parameters;
        max_box_q = vec3(max(box_q.x, 0.0), max(box_q.y, 0.0), max(box_q.z, 0.0));
        d_box = length(max_box_q + min(max(box_q.x, max(box_q.y, box_q.z)), 0.0));

        min_d = min(abs(d_sphere), abs(d_box));

        position += direction * min_d;
    }

    return vec4<f32>(0.2, 0.0, 1.0/min_d, 1.0);
}

#define TYPE_SPHERE: i32: 0;
#define TYPE_RECTANGLE: i32: 1;

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
    mouse: vec4<f32>,
};

@group(1) @binding(0)
var<uniform> material: CustomMaterial;

@group(1) @binding(1)
var<storage> sdf_types: array<i32>;

@group(1) @binding(2)
var<storage> sdf_positions: array<vec4<f32>>;

const MAX_ITERATIONS = 64;

fn sdf_union(d1: f32, d2: f32) -> f32 {
    return min(d1, d2);
}

const TYPE_END: i32 = #{TYPE_END};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var position = in.world_position.xyz;

    let direction = vec3(0.0, 0.0, -1.0);

    let sphere_r = 0.2;
    var box_position = vec3(0.3 * cos(globals.time * 0.3), 0.0, -0.3 + 0.3 * cos(globals.time * 0.3));
    let box_parameters = vec3(0.3, 0.3, 0.3);

    var d_sphere = 0.0;
    var d_box = 0.0;
    var box_q = vec3(0.0);
    var max_box_q = vec3(0.0);
    var d = 0.0;
    var i: i32 = 0;
    var d_object = 0.0;

    for (; i < MAX_ITERATIONS; i++) {
        box_q = abs(position - box_position) - box_parameters;
        max_box_q = vec3(max(box_q.x, 0.0), max(box_q.y, 0.0), max(box_q.z, 0.0));
        d_box = length(max_box_q + min(max(box_q.x, max(box_q.y, box_q.z)), 0.0));
        d = d_box;

        for (var sdf_index: i32 = 0; sdf_index < #{MAX_SDFS_PER_ENTITY}; sdf_index++) {
            if (sdf_types[sdf_index] == TYPE_END) {
                break;
            }
            let p = sdf_positions[sdf_index].xyz;

            d_sphere = length(position - p) - sphere_r;
            d = sdf_union(d_sphere, d);
        }

        position += direction * d;

        if(d < 0.001){
            break;
        }
    }

    if(d < 0.001){
        let AOLight: f32 = 1.0 / (f32(i)/f32(MAX_ITERATIONS));

        return vec4<f32>(0.2, 0.1, 1.0/d, 1.0) + AOLight * vec4(0.01);
    }

    return vec4<f32>(0.8, 0.0, 0.04, 1.0);
}

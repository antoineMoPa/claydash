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

@group(1) @binding(0)
var<uniform> camera: vec4<f32>;

@group(1) @binding(1)
var<uniform> sdf_types: array<vec4<i32>, #{MAX_SDFS_PER_ENTITY}>;

@group(1) @binding(2)
var<uniform> sdf_positions: array<vec4<f32>, #{MAX_SDFS_PER_ENTITY}>;

const MAX_ITERATIONS = 64;

fn sdf_union(d1: f32, d2: f32) -> f32 {
    return min(d1, d2);
}

const TYPE_END: i32 = #{TYPE_END};
const TYPE_SPHERE: i32 = #{TYPE_SPHERE};
const TYPE_CUBE: i32 = #{TYPE_CUBE};
const FAR_DIST = 2000.0;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var position = in.world_position.xyz;
    var ray = normalize(position - camera.xyz);
    let direction = ray;

    let sphere_r = 0.2;
    var box_position = vec3(0.3 * cos(globals.time * 0.3), 0.0, 0.2 * cos(globals.time * 0.3));
    let box_parameters = vec3(0.3, 0.3, 0.3);

    var d_box = 0.0;
    var box_q = vec3(0.0);
    var max_box_q = vec3(0.0);
    var d = 10000.0;
    var i: i32 = 0;
    var d_current_object = 0.0;

    // Walk the ray through the scene
    for (; i < MAX_ITERATIONS; i++) {
        // Loop through all objects
        for (var sdf_index: i32 = 0; sdf_index < #{MAX_SDFS_PER_ENTITY}; sdf_index++) {
            if (sdf_types[sdf_index].w == TYPE_END) {
                break;
            }
            let p = sdf_positions[sdf_index].xyz;

            // Find distance based on object type
            if (sdf_types[sdf_index].w == TYPE_SPHERE) {
                d_current_object = length(position - p) - sphere_r;
            }
            else if (sdf_types[sdf_index].w == TYPE_CUBE) {
                box_q = abs(position - box_position) - box_parameters;
                max_box_q = vec3(max(box_q.x, 0.0), max(box_q.y, 0.0), max(box_q.z, 0.0));
                d_current_object = length(max_box_q + min(max(box_q.x, max(box_q.y, box_q.z)), 0.0));
            }

            d = sdf_union(d_current_object, d);
        }

        position += direction * d * 0.8;

        if (d < 0.001){
            break;
        }

        if (d > FAR_DIST) {
            // We are probably past the object.
            // Note that this will not always be true: ex.: for ground objects.
            // But for now it's a valuable optimization.
            break;
        }
    }

    if(d < 0.001){
        let AOLight: f32 = 2.0 / (f32(i)/f32(MAX_ITERATIONS));

        return vec4<f32>(0.2, 0.1, 1.0/d, 1.0) + AOLight * vec4(0.01);
    }

    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}

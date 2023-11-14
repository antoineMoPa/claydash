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

@group(1) @binding(3)
var<uniform> sdf_colors: array<vec4<f32>, #{MAX_SDFS_PER_ENTITY}>;

const MAX_ITERATIONS = 64;

fn sdf_union(d1: f32, d2: f32) -> f32 {
    return min(d1, d2);
}

const TYPE_END: i32 = #{TYPE_END};
const TYPE_SPHERE: i32 = #{TYPE_SPHERE};
const TYPE_CUBE: i32 = #{TYPE_CUBE};
const FAR_DIST = 100.0;
const CLOSE_DIST = 0.01;
const BLEND_DIST = 0.03;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var position = in.world_position.xyz;
    var ray = normalize(position - camera.xyz);
    // Make objects out of the domain visible for a certain range
    // (mostly to have a nicer default view)
    position -= normalize(ray);
    let direction = ray;

    // TODO un-hardcode
    let sphere_r = 0.2;
    let box_parameters = vec3(0.3, 0.3, 0.3);

    var d_box = 0.0;
    var box_q = vec3(0.0);
    var max_box_q = vec3(0.0);
    var d = 10000.0;
    var i: i32 = 0;
    var d_current_object = 0.0;
    var color = vec4(0.0, 0.0, 0.0, 1.0);

    // Walk the ray through the scene
    for (; i < MAX_ITERATIONS; i++) {
        // Loop through all objects
        for (var sdf_index: i32 = 0; sdf_index < #{MAX_SDFS_PER_ENTITY}; sdf_index++) {
            let t = sdf_types[sdf_index].w;
            if (t == TYPE_END) {
                break;
            }
            let p = sdf_positions[sdf_index].xyz;

            // Find distance based on object type
            if (t == TYPE_SPHERE) {
                d_current_object = length(position - p) - sphere_r;
            }
            else if (t == TYPE_CUBE) {
                box_q = abs(position - p) - box_parameters;
                max_box_q = vec3(max(box_q.x, 0.0), max(box_q.y, 0.0), max(box_q.z, 0.0));
                d_current_object = length(max_box_q + min(max(box_q.x, max(box_q.y, box_q.z)), 0.0));
            }

            d = sdf_union(d_current_object, d);
            color = mix(color, sdf_colors[sdf_index], clamp(1.0 - pow(d_current_object / BLEND_DIST, 4.0), 0.0, 1.0));

            if (d < CLOSE_DIST) {
                break;
            }
        }

        position += direction * d;

        if (d < CLOSE_DIST) {
            break;
        }

        if (d > FAR_DIST) {
            // We are probably past the object.
            // Note that this will not always be true: ex.: for ground objects.
            // But for now it's a valuable optimization.
            break;
        }
    }

    if (d < CLOSE_DIST) {
        let AOLight: f32 = 2.0 / (f32(i)/f32(MAX_ITERATIONS));
        return vec4(color.rgb, 1.0) - AOLight * vec4(0.01);
    }

    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

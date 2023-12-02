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
var<uniform> sdf_meta: array<vec4<i32>, #{MAX_SDFS_PER_ENTITY}>;

@group(1) @binding(2)
var<uniform> sdf_colors: array<vec4<f32>, #{MAX_SDFS_PER_ENTITY}>;

@group(1) @binding(3)
var<uniform> sdf_inverse_transforms: array<mat4x4<f32>, #{MAX_SDFS_PER_ENTITY}>;

const MAX_ITERATIONS = 64;

fn sdf_union(d1: f32, d2: f32) -> f32 {
    return min(d1, d2);
}

const TYPE_END: i32 = #{TYPE_END};
const TYPE_SPHERE: i32 = #{TYPE_SPHERE};
const TYPE_BOX: i32 = #{TYPE_BOX};
const FAR_DIST = 100.0;
const CLOSE_DIST = 0.003;
const BLEND_DIST = 0.03;

fn sphere_sdf(p: vec3<f32>, r: f32) -> f32 {
    return length(p) - r;
}

fn max_vec3(p: vec3<f32>, value: f32) -> vec3<f32> {
    return vec3(max(p.x, value), max(p.y, value), max(p.z, value));
}

fn min_vec3(p: vec3<f32>, value: f32) -> vec3<f32> {
    return vec3(min(p.x, value), min(p.y, value), min(p.z, value));
}

fn box_sdf(p: vec3<f32>, b: vec3<f32>) -> f32 {
    let q: vec3<f32> = abs(p) - b;
    return length(max_vec3(q, 0.0)) + min(max(q.x,max(q.y, q.z)), 0.0);
}

fn object_distance(p: vec3<f32>, sdf_index: i32) -> f32 {
    // TODO un-hardcode
    let sphere_r = 0.2;
    let box_parameters = vec3(0.3, 0.3, 0.3);
    var d_current_object: f32 = FAR_DIST;
    let t = sdf_meta[sdf_index].w;
    let inverse_transform = sdf_inverse_transforms[sdf_index];
    let transformed_position = (inverse_transform * vec4(p, 1.0)).xyz;

    // Find distance based on object type
    if (t == TYPE_SPHERE) {
        d_current_object = sphere_sdf(transformed_position, sphere_r);
    }
    else if (t == TYPE_BOX) {
        d_current_object = box_sdf(transformed_position, box_parameters);
    }

    // Correct the returned distance to account for the scale
    // Note that this is not perfect yet and only seems to work when scaling
    // uniformly (e.g. not scaling on axis).
    // TODO find scale from transform matrix and correct for it again.
    // previous factor was:
    // length(sdf_scale) / length(vec3(1.0))
    // Or find jacobian?
    return d_current_object;
}

// Shortcut for object_distance to make next function more readable
fn od(p: vec3<f32>, sdf_index: i32) -> f32{
    return object_distance(p, sdf_index);
}

fn object_normal(p: vec3<f32>, sdf_index: i32) -> vec3<f32> {
    let e = CLOSE_DIST;
    let i = sdf_index;
    return normalize(vec3(od(vec3(p.x + e, p.y, p.z), i) - od(vec3(p.x - e, p.y, p.z), i),
                          od(vec3(p.x, p.y + e, p.z), i) - od(vec3(p.x, p.y - e, p.z), i),
                          od(vec3(p.x, p.y, p.z  + e), i) - od(vec3(p.x, p.y, p.z - e), i)));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = in.world_position.xyz;
    var camera_ray = normalize(p - camera.xyz);
    // Make objects out of the domain visible for a certain range
    // (mostly to have a nicer default view)
    p -= normalize(camera_ray);

    var d = 10000.0;
    var i: i32 = 0;
    var d_current_object = 0.0;
    var object_color = vec4(0.0, 0.0, 0.0, 1.0);
    var found = false;
    var closest = 0;

    // Walk the camera_ray through the scene
    for (; i < MAX_ITERATIONS && !found; i++) {
        // Loop through all objects
        for (var sdf_index: i32 = 0; sdf_index < #{MAX_SDFS_PER_ENTITY}; sdf_index++) {
            if (sdf_meta[sdf_index].w == TYPE_END) {
                break;
            }
            d_current_object = object_distance(p, sdf_index);

            if (abs(d_current_object) < abs(d)) {
                closest = sdf_index;
            }

            d = sdf_union(d_current_object, d);
            object_color = mix(object_color, sdf_colors[sdf_index], clamp(1.0 - pow(abs(d_current_object) / BLEND_DIST, 4.0), 0.0, 1.0));

            if (d < 0.0) {
                //p -= camera_ray * d * 1.1;
            }

            if (d < CLOSE_DIST) {
                found = true;
                break;
            }
        }

        p += camera_ray * d * 0.3;

        if (abs(d) > FAR_DIST) {
            // We are probably past the object.
            // Note that this will not always be true: ex.: for big landscape ground objects.
            // But for now it's a valuable optimization.
            break;
        }
    }

    var col = vec4<f32>(0.0, 0.0, 0.0, 1.0);

    if (found) {
        // Ambiant occlusion light
        let ao_light: f32 = 2.0 / (f32(i)/f32(MAX_ITERATIONS));
        let normal = object_normal(p, closest);
        let selected: bool = sdf_meta[closest].x == 1;

        let light_position = vec3(2.0, 2.0, 2.0);
        let diffuse_light_color = vec4(0.8);
        let diffuse_light_intensity = pow(0.3 * max(dot(normal, light_position - p), 0.0), 4.0);
        let diffuse_light = diffuse_light_intensity * diffuse_light_color;

        col += diffuse_light * object_color;

        let specular_light_color = vec4(0.8);
        let specular_reflection = reflect(light_position - p, normal);
        let specular_light_intensity = pow(0.3 * max(dot(camera_ray, specular_reflection), 0.0), 4.0);
        let specular_light = specular_light_intensity * specular_light_color;

        col += specular_light * object_color;

        if (selected) {
            col += vec4(0.2);
        }

        let ambiant_light = 0.3;
        col += ambiant_light * vec4(object_color.rgb, 1.0);// - ao_light * vec4(0.01);
    }

    return col;
}

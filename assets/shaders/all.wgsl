#import bevy_pbr::forward_io::VertexOutput;
#import bevy_pbr::mesh_view_bindings globals

@group(1) @binding(0)
var<uniform> camera: vec4<f32>;
@group(1) @binding(1)
var<uniform> camera_right: vec4<f32>;
@group(1) @binding(2)
var<uniform> camera_up: vec4<f32>;


@group(1) @binding(3)
var<uniform> sdf_meta: array<vec4<i32>, #{MAX_SDFS_PER_ENTITY}>;

@group(1) @binding(4)
var<uniform> sdf_colors: array<vec4<f32>, #{MAX_SDFS_PER_ENTITY}>;

@group(1) @binding(5)
var<uniform> sdf_inverse_transforms: array<mat4x4<f32>, #{MAX_SDFS_PER_ENTITY}>;

@group(1) @binding(6)
var<uniform> sdf_params: array<mat4x4<f32>, #{MAX_SDFS_PER_ENTITY}>;

@group(1) @binding(7)
var<uniform> control_point_positions: array<vec4<f32>, #{MAX_CONTROL_POINTS}>;

@group(1) @binding(8)
var<uniform> num_control_points: vec4<i32>; // padded for alignment. number is stored in first position.

const MAX_ITERATIONS = 32;

fn sdf_union(d1: f32, d2: f32) -> f32 {
    return min(d1, d2);
}

const TYPE_END: i32 = #{TYPE_END};
const TYPE_SPHERE: i32 = #{TYPE_SPHERE};
const TYPE_BOX: i32 = #{TYPE_BOX};
const FAR_DIST = 100.0;
const CLOSE_DIST = 0.003;

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
    let params = sdf_params[sdf_index];
    let sphere_r = params[0].x;
    let box_parameters = params[0].xyz;
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
    // Note that this is not perfect yet.
    let scale = vec3<f32>(length(inverse_transform[0].xyz),
                          length(inverse_transform[1].xyz),
                          length(inverse_transform[2].xyz));

    return d_current_object / length(scale);
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

fn render_control_points(mesh: VertexOutput) -> vec4<f32> {
    var world_position = mesh.world_position.xyz;
    var col: vec4<f32> = vec4(0.0);
    var camera_ray = normalize(world_position - camera.xyz);

    for (var i: i32 = 0; i < num_control_points.x; i++) {
        var control_point_position = control_point_positions[i].xyz;
        var camera_to_control_point_dist = length(control_point_position - camera.xyz);
        var position_near_control_point = camera.xyz + camera_ray * camera_to_control_point_dist;
        var l = length(position_near_control_point - control_point_position);

        if (l < 0.018) {
            // Red fill
            col.r += 1.0;
            col.g += 0.0;
            col.b += 0.0;
            col.a += 1.0;
        } else if (l < 0.01) {
            // White border
            col.r += 1.0;
            col.g += 1.0;
            col.b += 1.0;
            col.a += 1.0;
        }
    }

    return clamp(col, vec4(0.0), vec4(1.0));
}


@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    var p = mesh.world_position.xyz;
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
            object_color = sdf_colors[sdf_index];

            if (d < 0.0) {
                found = true;
                p -= camera_ray * d;
                break;
            }

            if (d < CLOSE_DIST) {
                found = true;
                break;
            }
        }

        p += camera_ray * d * 0.95;

        if (abs(d) > FAR_DIST) {
            // We are probably past the object.
            // Note that this will not always be true: ex.: for big landscape ground objects.
            // But for now it's a valuable optimization.
            return vec4<f32>(0.0, 0.0, 0.0, 0.0);
        }
    }

    var col = vec4<f32>(0.0, 0.0, 0.0, 0.0);

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
        col.a = 1.0;
    }

    let control_points = render_control_points(mesh);

    col = control_points.a * control_points + (1.0 - control_points.a) * col;

    return col;
}

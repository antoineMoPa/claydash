#import bevy_pbr::forward_io::VertexOutput;
#import bevy_pbr::mesh_view_bindings globals

@group(2) @binding(0)
var<uniform> camera: vec4<f32>;
@group(2) @binding(1)
var<uniform> camera_right: vec4<f32>;
@group(2) @binding(2)
var<uniform> camera_up: vec4<f32>;


@group(2) @binding(3)
var<uniform> sdf_meta: array<vec4<i32>, #{MAX_SDFS_PER_ENTITY}>;

@group(2) @binding(4)
var<uniform> sdf_colors: array<vec4<f32>, #{MAX_SDFS_PER_ENTITY}>;

@group(2) @binding(5)
var<uniform> sdf_inverse_transforms: array<mat4x4<f32>, #{MAX_SDFS_PER_ENTITY}>;

@group(2) @binding(6)
var<uniform> sdf_params: array<mat4x4<f32>, #{MAX_SDFS_PER_ENTITY}>;

/// w: operation type
/// x: lhs index
/// y: rhs index
/// z: unused
@group(2) @binding(7)
var<uniform> sdf_operations: array<vec4<i32>, #{MAX_OPERATION_RESULTS}>;

@group(2) @binding(8)
var<uniform> control_point_positions: array<vec4<f32>, #{MAX_SDFS_PER_ENTITY}>;

/// w: union
/// x: subtraction
/// y: intersection
/// z: use lhs as is
@group(2) @binding(9)
var<uniform> sdf_operations_1: array<vec4<f32>, #{MAX_OPERATION_RESULTS}>;
/// w: use rhs as is
/// x: unused
/// y: unused
/// z: unused
@group(2) @binding(10)
var<uniform> sdf_operations_2: array<vec4<f32>, #{MAX_OPERATION_RESULTS}>;
const MAX_ITERATIONS = 32;

@group(2) @binding(11)
var<uniform> info: array<vec4<i32>, 1>;

fn smin(a: f32, b: f32, input_k: f32 ) -> f32 {
    let k = input_k * 2.0;
    let x = b - a;
    return 0.5 * ( a + b - sqrt(x * x + k * k));
}

const TYPE_END: i32 = #{TYPE_END};
const TYPE_SPHERE: i32 = #{TYPE_SPHERE};
const TYPE_BOX: i32 = #{TYPE_BOX};

const OPERATION_UNION: i32 = #{OPERATION_UNION};
const OPERATION_SUBTRACTION: i32 = #{OPERATION_SUBTRACTION};
const OPERATION_INTERSECTION: i32 = #{OPERATION_INTERSECTION};
const OPERATION_USE_LHS_AS_IS: i32 = #{OPERATION_USE_LHS_AS_IS};
const OPERATION_USE_RHS_AS_IS: i32 = #{OPERATION_USE_LHS_AS_IS};
const OPERATION_USE_LHS_RELATIVE_INDEX: i32 = #{OPERATION_USE_LHS_RELATIVE_INDEX};
const OPERATION_USE_RHS_RELATIVE_INDEX: i32 = #{OPERATION_USE_RHS_RELATIVE_INDEX};
const OPERATION_END: i32 = #{OPERATION_END};

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
    let num_control_points = info[0].y;
    var world_position = mesh.world_position.xyz;
    var col: vec4<f32> = vec4(0.0);
    var camera_ray = normalize(world_position - camera.xyz);

    for (var i: i32 = 0; i < num_control_points; i++) {
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
    var ghost = false;

    // Array of previous distances
    var operations_results = array<f32, #{MAX_OPERATION_RESULTS}>();

    let sdf_object_num: i32 = info[0].w;
    let sdf_operation_num: i32 = info[0].x;

    // Walk the camera_ray through the scene
    while (i < MAX_ITERATIONS) {
        var op_index: i32 = 0;

        // Loop through all objects
        for (;op_index < sdf_object_num; op_index++) {
            d_current_object = object_distance(p, op_index);

            if (abs(d_current_object) < abs(d)) {
                closest = op_index;
                object_color = sdf_colors[op_index];
            }

            operations_results[op_index] = d_current_object;

            if (d_current_object < CLOSE_DIST) {
                ghost = true;
                //break;
            }
        }

        // Loop through all operations (starting at the last object index + 1)
        var result = 1e10;

        while (op_index < sdf_operation_num) {
            let op_entry = sdf_operations[op_index];
            let op = op_entry.w;
            var lhs_relative_index = op_entry.x;
            var rhs_relative_index = op_entry.y;

            var lhs_distance = operations_results[op_index + lhs_relative_index];
            var rhs_distance = operations_results[op_index + rhs_relative_index];

            let op_1 = sdf_operations_1[op_index];
            let op_2 = sdf_operations_2[op_index];

            result = min(lhs_distance, rhs_distance) * op_1.w;
            result += max(lhs_distance, -rhs_distance) * op_1.x;
            result += max(lhs_distance, rhs_distance) * op_1.y;
            result += lhs_distance * op_1.z;
            result += rhs_distance * op_2.w;

            operations_results[op_index] = result;

            op_index += 1;
        }

        d = result;

        p += camera_ray * d * 0.99;

        if (abs(d) > FAR_DIST) {
            // We are probably past the object.
            // Note that this will not always be true: ex.: for big landscape ground objects.
            // But for now it's a valuable optimization.
            return vec4<f32>(0.0, 0.0, 0.0, 0.0);
        }

        if (d < CLOSE_DIST) {
            found = true;
            break;
        }

        i++;
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

    if (ghost) {
        col += vec4(1.0) * 0.2;
    }

    let control_points = render_control_points(mesh);

    col = control_points.a * control_points + (1.0 - control_points.a) * col;

    return col;
}

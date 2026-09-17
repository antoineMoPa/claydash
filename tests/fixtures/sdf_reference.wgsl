// Pre-optimization material renderer, retained for visual comparisons (up to 256 objects).
// Only the storage-buffer layout was extended to match the current upload ABI.
struct Camera { inverse_view_projection: mat4x4<f32>, position: vec4<f32>, count: vec4<u32> }
struct Object {
    state: vec4<i32>,
    color: vec4<f32>,
    inverse_rows: array<vec4<f32>, 3>,
    params: vec4<f32>,
    material: vec4<f32>,
    repeat_spacing: vec4<f32>,
    repeat_count: vec4<i32>,
    component: vec4<u32>,
}
struct BvhNode { center_radius: vec4<f32>, metadata: vec4<u32>, aabb_min: vec4<f32>, aabb_max: vec4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> objects: array<Object>;
@group(0) @binding(2) var<storage, read> bvh: array<BvhNode>;
override USE_BVH: bool = true;
override HAS_BOOLEANS: bool = false;

struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) clip: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -3.0), vec2(3.0, 1.0), vec2(-1.0, 1.0));
    var out: VertexOutput;
    out.clip = positions[index];
    out.position = vec4(out.clip, 0.0, 1.0);
    return out;
}

fn repeated_axis(value: f32, spacing: f32, count: i32) -> f32 {
    if count <= 1 { return value; }
    let safe_spacing = max(spacing, 0.001);
    let half = f32(count - 1) * 0.5;
    let cell = clamp(round(value / safe_spacing), -half, half);
    return value - cell * safe_spacing;
}

fn object_distance(point: vec3<f32>, object: Object) -> f32 {
    let homogeneous = vec4(point, 1.0);
    var local = vec3(
        dot(object.inverse_rows[0], homogeneous),
        dot(object.inverse_rows[1], homogeneous),
        dot(object.inverse_rows[2], homogeneous)
    );
    if object.repeat_count.w != 0 {
        local = vec3(
            repeated_axis(local.x, object.repeat_spacing.x, object.repeat_count.x),
            repeated_axis(local.y, object.repeat_spacing.y, object.repeat_count.y),
            repeated_axis(local.z, object.repeat_spacing.z, object.repeat_count.z)
        );
    }
    var distance = 100.0;
    if object.state.y == 1 {
        distance = length(local) - object.params.x;
    } else if object.state.y == 2 {
        let q = abs(local) - object.params.xyz;
        distance = length(max(q, vec3(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
    } else if object.state.y == 3 {
        let q = vec2(length(local.xz) - object.params.x, abs(local.y) - object.params.y);
        distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
    } else if object.state.y == 4 {
        let q = vec2(length(local.xz) - object.params.x, local.y);
        distance = length(q) - object.params.y;
    }
    return distance * object.params.w;
}

fn scene_distance(point: vec3<f32>) -> vec2<f32> {
    var closest = 100.0;
    var object_index = 0u;
    if camera.count.z != 0u {
        var values: array<vec2<f32>, 256>;
        for (var i = 0u; i < camera.count.x; i++) {
            values[i] = vec2(object_distance(point, objects[i]), f32(i));
        }
        // Upload order is postorder: every subtree is complete before its
        // root is combined with the parent. Siblings keep their scene order.
        for (var i = 0u; i < camera.count.x; i++) {
            let parent = objects[i].state.w;
            let child = values[i];
            if parent < 0 {
                if child.x < closest {
                    closest = child.x;
                    object_index = u32(child.y);
                }
            } else {
                var value = values[u32(parent)];
                if objects[i].state.z == 0 {
                    if child.x < value.x { value = child; }
                } else if objects[i].state.z == 1 {
                    value.x = max(value.x, -child.x);
                } else if child.x > value.x { value = child; }
                values[u32(parent)] = value;
            }
        }
    } else if USE_BVH {
        var node_index = 0u;
        while node_index < camera.count.y {
            let node = bvh[node_index];
            let lower_bound = distance(point, node.center_radius.xyz) - node.center_radius.w;
            if lower_bound < closest {
                if node.metadata.x != 0xffffffffu {
                    let candidate = object_distance(point, objects[node.metadata.x]);
                    if candidate < closest { closest = candidate; object_index = node.metadata.x; }
                }
                node_index += 1u;
            } else {
                node_index = node.metadata.y;
            }
        }
    } else {
        for (var i = 0u; i < camera.count.x; i++) {
            let candidate = object_distance(point, objects[i]);
            if candidate < closest { closest = candidate; object_index = i; }
        }
    }
    return vec2(closest, f32(object_index));
}

fn scene_normal(point: vec3<f32>) -> vec3<f32> {
    let e = 0.003;
    let a = vec3(1.0, -1.0, -1.0);
    let b = vec3(-1.0, -1.0, 1.0);
    let c = vec3(-1.0, 1.0, -1.0);
    let d = vec3(1.0, 1.0, 1.0);
    return normalize(
        a * scene_distance(point + a * e).x +
        b * scene_distance(point + b * e).x +
        c * scene_distance(point + c * e).x +
        d * scene_distance(point + d * e).x
    );
}

fn background(ray: vec3<f32>) -> vec3<f32> {
    let horizon = clamp(ray.y * 0.5 + 0.5, 0.0, 1.0);
    let sky = mix(vec3(0.055, 0.065, 0.085), vec3(0.38, 0.47, 0.62), horizon);
    let softbox = pow(max(dot(ray, normalize(vec3(-0.5, 0.8, 0.4))), 0.0), 36.0);
    let strip = pow(max(dot(ray, normalize(vec3(0.8, 0.3, -0.5))), 0.0), 90.0);
    return sky + vec3(2.8, 2.65, 2.4) * softbox + vec3(1.2, 1.5, 2.0) * strip;
}

// Returns travel and the surface owner. Absolute distances also trace the exit
// surface when a transmitted ray starts inside a solid.
fn trace(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    var travel = 0.0;
    for (var step = 0; step < 128; step++) {
        let sample = scene_distance(origin + direction * travel);
        if abs(sample.x) < 0.0015 { return vec2(travel, sample.y); }
        travel += max(abs(sample.x) * 0.8, 0.0008);
        if travel > 100.0 { break; }
    }
    return vec2(100.0, -1.0);
}

fn surface_light(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, object: Object) -> vec3<f32> {
    let light = normalize(vec3(2.0, 3.0, 2.0) - point);
    let halfway = normalize(light + view);
    let roughness = clamp(object.material.x, 0.03, 1.0);
    let metallic = object.material.y;
    let diffuse = max(dot(normal, light), 0.0);
    let specular = pow(max(dot(normal, halfway), 0.0), mix(256.0, 3.0, roughness * roughness));
    let specular_color = mix(vec3(object.material.z), object.color.rgb, metallic);
    return object.color.rgb * (0.13 + diffuse * 0.75) * (1.0 - metallic)
        + specular_color * specular * (1.0 - roughness * 0.5);
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let far = camera.inverse_view_projection * vec4(input.clip, 1.0, 1.0);
    let near = camera.inverse_view_projection * vec4(input.clip, 0.0, 1.0);
    let far_point = far.xyz / far.w;
    let near_point = near.xyz / near.w;
    let ray = select(normalize(far_point - camera.position.xyz), normalize(far_point - near_point), camera.count.w != 0u);
    var travel = 0.0;
    let ray_origin = select(camera.position.xyz, near_point, camera.count.w != 0u);
    var point = ray_origin;
    var hit = false;
    var index = 0u;
    for (var step = 0; step < 96; step++) {
        let scene = scene_distance(point);
        if scene.x < 0.003 { hit = true; index = u32(scene.y); break; }
        travel += scene.x * 0.8;
        if travel > 100.0 { break; }
        point = ray_origin + ray * travel;
    }
    let sky = background(ray);
    if !hit { return vec4(sky, 1.0); }

    var direction = ray;
    var throughput = vec3(1.0);
    var radiance = vec3(0.0);
    let selected = f32(objects[index].state.x) * 0.12;
    for (var bounce = 0; bounce < 6; bounce++) {
        let object = objects[index];
        let outward = scene_normal(point);
        let entering = dot(direction, outward) < 0.0;
        let normal = select(-outward, outward, entering);
        let ior = max(object.repeat_spacing.w, 1.0);
        let f0 = pow((ior - 1.0) / (ior + 1.0), 2.0);
        let fresnel = f0 + (1.0 - f0) * pow(1.0 - max(dot(-direction, normal), 0.0), 5.0);
        let reflection = reflect(direction, normal);
        let opacity = clamp(object.material.w, 0.0, 1.0);
        let metallic = clamp(object.material.y, 0.0, 1.0);
        let eta = select(ior, 1.0 / ior, entering);
        let transmitted = refract(direction, normal, eta);
        // Total internal reflection continues the path without adding the same
        // reflected energy a second time through the direct reflection sample.
        if opacity < 0.999 && metallic < 0.999 && dot(transmitted, transmitted) < 0.001 {
            direction = reflection;
            point += normal * 0.007;
            let next = trace(point, direction);
            if next.y < 0.0 {
                radiance += throughput * background(direction);
                break;
            }
            point += direction * next.x;
            index = u32(next.y);
            continue;
        }
        let reflected_hit = trace(point + normal * 0.007, reflection);
        var reflected_color = background(reflection);
        if reflected_hit.y >= 0.0 {
            let reflected_point = point + normal * 0.007 + reflection * reflected_hit.x;
            reflected_color = surface_light(reflected_point, scene_normal(reflected_point), -reflection, objects[u32(reflected_hit.y)]);
        }
        let roughness = clamp(object.material.x, 0.0, 1.0);
        reflected_color = mix(reflected_color, vec3(0.22, 0.27, 0.35), roughness * roughness);
        let reflection_weight = clamp(max(fresnel, object.material.z), 0.0, 1.0);
        let tint = mix(vec3(1.0), object.color.rgb, metallic);
        radiance += throughput * (reflected_color * tint * reflection_weight
            + surface_light(point, normal, -direction, object) * opacity * (1.0 - reflection_weight));
        if opacity >= 0.999 || metallic >= 0.999 { break; }
        direction = transmitted;
        point -= normal * 0.007;
        throughput *= (1.0 - reflection_weight) * (1.0 - opacity);
        throughput *= mix(vec3(1.0), object.color.rgb, 0.12);
        let next = trace(point, direction);
        if next.y < 0.0 {
            radiance += throughput * background(direction);
            break;
        }
        point += direction * next.x;
        index = u32(next.y);
    }
    return vec4(radiance + selected, 1.0);
}

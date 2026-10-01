// Shared surface contract and explicit material dispatch.
const MATERIAL_WOOD: u32 = 1u;
const MATERIAL_DIAGNOSTIC: u32 = 4u;
const MATERIAL_BRICK: u32 = 5u;
const MATERIAL_CUSTOM: u32 = 6u;
const MATERIAL_FABRIC: u32 = 7u;
const MATERIAL_METAL: u32 = 8u;
override HAS_METAL_MATERIAL: bool = true;
override HAS_WOOD_MATERIAL: bool = true;
override HAS_BRICK_MATERIAL: bool = true;
override HAS_FABRIC_MATERIAL: bool = true;
override HAS_DIAGNOSTIC_MATERIAL: bool = true;
struct MaterialHeader { kind: u32, offset: u32, length: u32, reserved: u32 }
@group(0) @binding(3) var<storage, read> material_headers: array<MaterialHeader>;
@group(0) @binding(4) var<storage, read> material_params: array<vec4<f32>>;
struct Surface {
    color: vec3<f32>, normal: vec3<f32>, roughness: f32, metallic: f32,
    reflectivity: f32, opacity: f32, ior: f32, coat: f32,
    sheen: f32, fiber: vec3<f32>, figure: f32,
    tangent: vec3<f32>, anisotropy: f32, metal_response: bool,
}
fn material_surface(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, object: Object) -> Surface {
    var material_object = object;
    var captured_color = object.color.rgb;
    if object.state.y == 12 {
        let capture = neural_surface_sample(point, object);
        captured_color = capture.capture.yzw;
        material_object.component.w = capture.material_index;
    } else if object.state.y == 8 {
        let capture = box_depth_surface_sample(point, object);
        captured_color = capture.capture.yzw;
        material_object.component.w = capture.material_index;
    } else if object.state.y == 9 {
        let capture = sphere_depth_surface_sample(point, object);
        captured_color = capture.capture.yzw;
        material_object.component.w = capture.material_index;
    } else if object.state.y == 10 && object.box_depth_max.w >= 0.0 {
        let capture = gaussian_splat_surface_sample(point, -view, object);
        captured_color = capture.capture.yzw;
        material_object.component.w = capture.material_index;
    }
    let header = material_headers[material_object.component.w];
    let common_values = material_params[header.offset];
    let optics = material_params[header.offset + 1u];
    let base = Surface(captured_color, normal, clamp(common_values.x, 0.03, 1.0), common_values.y,
        common_values.z, common_values.w, optics.x, 0.0, 0.0, vec3(0.0, 1.0, 0.0), 0.0, vec3(1.0, 0.0, 0.0), 0.0, false);
    switch header.kind {
        case MATERIAL_WOOD: {
            if HAS_WOOD_MATERIAL { return evaluate_wood(point, normal, material_object, base, header.offset); }
            return base;
        }
        case MATERIAL_DIAGNOSTIC: {
            if HAS_DIAGNOSTIC_MATERIAL { return evaluate_diagnostic(point, material_object, base, header.offset); }
            return base;
        }
        case MATERIAL_BRICK: {
            if HAS_BRICK_MATERIAL { return evaluate_brick(point, normal, view, material_object, base, header.offset); }
            return base;
        }
        case MATERIAL_FABRIC: {
            if HAS_FABRIC_MATERIAL { return evaluate_fabric(point, normal, material_object, base, header.offset); }
            return base;
        }
        case MATERIAL_METAL: {
            if HAS_METAL_MATERIAL { return evaluate_metal(point, normal, material_object, base, header.offset); }
            return base;
        }
        case MATERIAL_CUSTOM: {
            switch header.reserved {
                // CUSTOM_MATERIAL_CASES
                default: { return base; }
            }
        }
        default: { return base; }
    }
}
fn surface_light(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, object: Object, surface: Surface, use_ao: bool) -> vec3<f32> {
    if HAS_METAL_MATERIAL && surface.metal_response {
        var metal_ao = 1.0;
        if use_ao { metal_ao = ambient_occlusion(point, normal); }
        return metal_light(point, view, surface, metal_ao);
    }
    var material_index = object.component.w;
    if object.state.y == 12 {
        material_index = neural_surface_sample(point, object).material_index;
    } else if object.state.y == 8 {
        material_index = box_depth_surface_sample(point, object).material_index;
    } else if object.state.y == 9 {
        material_index = sphere_depth_surface_sample(point, object).material_index;
    } else if object.state.y == 10 && object.box_depth_max.w >= 0.0 {
        material_index = gaussian_splat_surface_sample(point, -view, object).material_index;
    }
    let header = material_headers[material_index];
    let color = surface.color;
    let shade_normal = surface.normal;
    let roughness = surface.roughness;
    let coat = surface.coat;
    let sheen = surface.sheen;
    let fiber = surface.fiber;
    let light = select(normalize(vec3(2.0, 3.0, 2.0) - point),
        normalize(camera.sun_direction.xyz), camera.world_mode.x == 1u);
    let halfway = normalize(light + view);
    let metallic = surface.metallic;
    let diffuse = max(dot(normalize(mix(shade_normal, normal, 0.35 * coat)), light), 0.0);
    let specular = pow(max(dot(shade_normal, halfway), 0.0), mix(256.0, 3.0, roughness * roughness));
    let specular_color = mix(vec3(surface.reflectivity), color, metallic);
    var fiber_light = 0.0;
    var coat_light = 0.0;
    if HAS_WOOD_MATERIAL && header.kind == MATERIAL_WOOD {
        fiber_light = pow(max(1.0 - abs(dot(halfway, fiber)), 0.0), 18.0) * surface.figure * 0.11 * diffuse;
        let coat_normal = normalize(mix(shade_normal, normal, 0.58 * coat));
        let reflection = reflect(-view, coat_normal);
        let strip_width = mix(0.14, 0.035, sheen);
        let strip = exp(-pow((reflection.x - 0.20) / strip_width, 2.0));
        coat_light = coat * (pow(max(dot(coat_normal, halfway), 0.0), mix(15.0, 145.0, sheen))
            * mix(0.22, 0.43, sheen) + 0.12 * strip);
    }
    var ao = 1.0;
    if use_ao { ao = ambient_occlusion(point, normal); }
    let sky_daylight = smoothstep(-0.18, 0.16, camera.sun_direction.y);
    let sky_ambient = mix(0.035, 0.20, sky_daylight);
    var ambient = select(select(0.13, 0.23, header.kind == MATERIAL_WOOD), sky_ambient,
        camera.world_mode.x == 1u) * ao;
    var direct_strength = select(0.75, camera.sun_direction.w * sky_daylight,
        camera.world_mode.x == 1u);
    if camera.world_mode.x == 4u {
        ambient = 0.08 * ao;
        direct_strength = 0.12;
    }
    ambient *= camera.lighting_params.x;
    return color * (ambient + diffuse * direct_strength * mix(0.55, 1.0, ao)) * (1.0 - metallic)
        + specular_color * specular * (1.0 - roughness * 0.5) * (1.0 - 0.72 * coat)
        + color * fiber_light + vec3(1.0, 0.98, 0.93) * coat_light
        + select(vec3(0.0), fabric_extra_light(surface, light, view), HAS_FABRIC_MATERIAL && header.kind == MATERIAL_FABRIC);
}

// A metal height image uses the existing object atlas and placement; other stencils keep their color semantics.
fn metal_image_detail_enabled(object: Object) -> bool {
    let header = material_headers[object.component.w];
    if HAS_METAL_MATERIAL && header.kind == MATERIAL_METAL {
        return material_params[header.offset + METAL_PAINT].z > 0.0;
    }
    return false;
}
fn material_stencil_color(point: vec3<f32>, normal: vec3<f32>, object: Object) -> vec4<f32> {
    if metal_image_detail_enabled(object) { return vec4(0.0); }
    return stencil_color(point, normal, object);
}

// Shared surface contract and explicit material dispatch.
const MATERIAL_WOOD: u32 = 1u;
const MATERIAL_DIAGNOSTIC: u32 = 4u;
const MATERIAL_BRICK: u32 = 5u;
struct MaterialHeader { kind: u32, offset: u32, length: u32, reserved: u32 }
@group(0) @binding(3) var<storage, read> material_headers: array<MaterialHeader>;
@group(0) @binding(4) var<storage, read> material_params: array<vec4<f32>>;
struct Surface {
    color: vec3<f32>, normal: vec3<f32>, roughness: f32, metallic: f32,
    reflectivity: f32, opacity: f32, ior: f32, coat: f32,
    sheen: f32, fiber: vec3<f32>, figure: f32,
}
fn material_surface(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, object: Object) -> Surface {
    let header = material_headers[object.component.w];
    let common_values = material_params[header.offset];
    let optics = material_params[header.offset + 1u];
    let base = Surface(object.color.rgb, normal, clamp(common_values.x, 0.03, 1.0), common_values.y,
        common_values.z, common_values.w, optics.x, 0.0, 0.0, vec3(0.0, 1.0, 0.0), 0.0);
    switch header.kind {
        case MATERIAL_WOOD: { return evaluate_wood(point, normal, object, base, header.offset); }
        case MATERIAL_DIAGNOSTIC: { return evaluate_diagnostic(point, object, base, header.offset); }
        case MATERIAL_BRICK: { return evaluate_brick(point, normal, view, object, base, header.offset); }
        default: { return base; }
    }
}
fn surface_light(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, object: Object, surface: Surface, use_ao: bool) -> vec3<f32> {
    let header = material_headers[object.component.w];
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
    if header.kind == MATERIAL_WOOD {
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
    let ambient = select(select(0.13, 0.23, header.kind == MATERIAL_WOOD), sky_ambient,
        camera.world_mode.x == 1u) * ao;
    let direct_strength = select(0.75, camera.sun_direction.w * sky_daylight,
        camera.world_mode.x == 1u);
    return color * (ambient + diffuse * direct_strength * mix(0.55, 1.0, ao)) * (1.0 - metallic)
        + specular_color * specular * (1.0 - roughness * 0.5) * (1.0 - 0.72 * coat)
        + color * fiber_light + vec3(1.0, 0.98, 0.93) * coat_light;
}

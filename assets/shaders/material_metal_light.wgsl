// Anisotropic GGX in the actual Claydash world, shared by scene and library previews.
fn metal_fresnel(f0: vec3<f32>, cosine: f32) -> vec3<f32> {
    return f0 + (vec3(1.0) - f0) * pow(1.0 - clamp(cosine, 0.0, 1.0), 5.0);
}
fn metal_lambda(w: vec3<f32>, alpha: vec2<f32>) -> f32 {
    return 0.5 * (sqrt(1.0 + dot(alpha * alpha, w.xy * w.xy) / max(w.z * w.z, 0.00001)) - 1.0);
}
fn metal_distribution(h: vec3<f32>, alpha: vec2<f32>) -> f32 {
    let d = dot(h.xy / alpha, h.xy / alpha) + h.z * h.z;
    return 1.0 / max(3.14159265 * alpha.x * alpha.y * d * d, 0.000001);
}
fn metal_inverse_bits(value: u32) -> f32 {
    var bits = (value << 16u) | (value >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xaaaaaaaau) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xccccccccu) >> 2u);
    bits = ((bits & 0x0f0f0f0fu) << 4u) | ((bits & 0xf0f0f0f0u) >> 4u);
    bits = ((bits & 0x00ff00ffu) << 8u) | ((bits & 0xff00ff00u) >> 8u);
    return f32(bits) * 2.3283064365386963e-10;
}
fn metal_light(point: vec3<f32>, view: vec3<f32>, surface: Surface, ao: f32) -> vec3<f32> {
    let n = surface.normal;
    let projected = surface.tangent - n * dot(surface.tangent, n);
    let t = normalize(projected);
    let basis = mat3x3(t, cross(n, t), n);
    let v = transpose(basis) * view;
    let nv = max(v.z, 0.001);
    let aspect = sqrt(1.0 - 0.9 * surface.anisotropy);
    let alpha = max(0.002, surface.roughness * surface.roughness) * vec2(1.0 / aspect, aspect);
    let f0 = mix(vec3(0.04), surface.color, surface.metallic);
    var specular = vec3(0.0);
    var diffuse = vec3(0.0);
    // Fixed Hammersley sampling keeps interactive previews deterministic; no fake highlight.
    for (var i = 0u; i < 16u; i += 1u) {
        let u = (f32(i) + 0.5) / 16.0;
        let phi = 6.2831853 * metal_inverse_bits(i);
        let r = sqrt(u / (1.0 - u));
        let h = normalize(vec3(alpha.x * r * cos(phi), alpha.y * r * sin(phi), 1.0));
        let vh = dot(v, h);
        let l = reflect(-v, h);
        if vh > 0.0 && l.z > 0.0 {
            let g = 1.0 / (1.0 + metal_lambda(v, alpha) + metal_lambda(l, alpha));
            specular += background(basis * l) * metal_fresnel(f0, vh) * (g * vh / max(nv * h.z, 0.001));
        }
        let d = vec3(sqrt(u) * cos(phi), sqrt(u) * sin(phi), sqrt(1.0 - u));
        diffuse += background(basis * d);
    }
    let light = select(normalize(vec3(2.0, 3.0, 2.0) - point), normalize(camera.sun_direction.xyz), camera.world_mode.x == 1u);
    let l = transpose(basis) * light;
    let h = normalize(v + l);
    let daylight = smoothstep(-0.18, 0.16, camera.sun_direction.y);
    var strength = select(0.75, camera.sun_direction.w * daylight, camera.world_mode.x == 1u);
    if camera.world_mode.x == 4u { strength = 0.12; }
    let g = 1.0 / (1.0 + metal_lambda(v, alpha) + metal_lambda(l, alpha));
    let direct_spec = metal_fresnel(f0, max(dot(v, h), 0.0)) * metal_distribution(h, alpha) * g
        / max(4.0 * nv, 0.001) * select(0.0, 1.0, l.z > 0.0 && h.z > 0.0);
    let direct_diffuse = surface.color * (1.0 - surface.metallic) * max(l.z, 0.0) / 3.14159265;
    return (specular / 16.0 + diffuse / 16.0 * surface.color * (1.0 - surface.metallic) * 0.96)
        * ao * camera.lighting_params.x + (direct_spec + direct_diffuse) * strength * mix(0.55, 1.0, ao);
}

// One traced scene reflection corrects the integrated world for smooth surfaces.
// Broad rough lobes keep the sampled environment; secondary reflections do not recurse.
fn metal_scene_reflection(point: vec3<f32>, view: vec3<f32>, surface: Surface, ao: f32) -> vec3<f32> {
    let rough_weight = pow(1.0 - surface.roughness, 4.0);
    if rough_weight < 0.01 { return vec3(0.0); }
    let ray = reflect(-view, surface.normal);
    let origin = point + surface.normal * 0.009;
    let hit = trace_objects(origin, ray, 0.003, empty_splat_exclusions(), HYBRID_SPLATS);
    if hit.y < 0.0 { return vec3(0.0); }
    let reflected_point = origin + ray * hit.x;
    let object = objects[u32(hit.y)];
    let normal = scene_normal(reflected_point, u32(hit.y));
    let reflected_surface = gbuffer_surface(reflected_point, normal, -ray, object);
    // Transmission has its own compositor; leave world lighting for transparent hits.
    if reflected_surface.opacity < 0.999 { return vec3(0.0); }
    let reflected_color = surface_light(reflected_point, normal, -ray, object, reflected_surface, false);
    let f0 = mix(vec3(0.04), surface.color, surface.metallic);
    let weight = metal_fresnel(f0, max(dot(surface.normal, view), 0.0)) * rough_weight;
    return weight * ao * (reflected_color - background(ray) * camera.lighting_params.x);
}

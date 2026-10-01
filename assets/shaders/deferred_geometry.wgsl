struct GeometryOutput {
    @location(0) position_reflectivity: vec4<f32>,
    @location(1) normal_roughness: vec4<f32>,
    @location(2) albedo_metallic: vec4<f32>,
    @location(3) optics: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

fn gbuffer_surface(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, object: Object) -> Surface {
    var surface = material_surface(point, normal, view, object);
    let decal = stencil_color(point, normal, object);
    if object.stencil_meta.w > 0.5 && object.stencil_meta.x > 0.5 {
        surface.color = decal.rgb;
        surface.opacity = decal.a;
    } else if decal.a > 0.0 {
        surface.color = mix(surface.color, decal.rgb, decal.a);
    }
    return surface;
}

// One primary trace supplies every screen-space lighting input. Negative
// reflectivity marks sky pixels without using a second hit-mask texture.
@fragment fn fs_gbuffer(input: VertexOutput) -> GeometryOutput {
    let far = camera.inverse_view_projection * vec4(input.clip, 1.0, 1.0);
    let near = camera.inverse_view_projection * vec4(input.clip, 0.0, 1.0);
    let far_point = far.xyz / far.w;
    let near_point = near.xyz / near.w;
    let ray = select(normalize(far_point - camera.position.xyz),
        normalize(far_point - near_point), camera.count.w != 0u);
    let origin = select(camera.position.xyz, near_point, camera.count.w != 0u);
    let hit = trace_objects(origin, ray, 0.003, empty_splat_exclusions(), HYBRID_SPLATS);
    var output: GeometryOutput;
    output.position_reflectivity = vec4(0.0, 0.0, 0.0, -1.0);
    output.normal_roughness = vec4(0.0);
    output.albedo_metallic = vec4(0.0);
    output.depth = 1.0;
    output.optics = vec4(1.0, 1.0, 0.0, 0.0);
    if hit.y < 0.0 { return output; }
    let point = origin + ray * hit.x;
    let normal = scene_normal(point, u32(hit.y));
    let object = objects[u32(hit.y)];
    let surface = gbuffer_surface(point, normal, -ray, object);
    let clip = camera.view_projection * vec4(point, 1.0);
    output.position_reflectivity = vec4(point, clamp(surface.reflectivity, 0.0, 1.0));
    output.normal_roughness = vec4(normalize(surface.normal), surface.roughness);
    output.albedo_metallic = vec4(surface.color, surface.metallic);
    // A compact sheen payload keeps fabric's broad grazing response in deferred lighting.
    let fabric_sheen = select(0.0, surface.figure,
        material_headers[object.component.w].kind == MATERIAL_FABRIC);
    output.optics = vec4(surface.opacity, surface.ior, f32(object.component.y + 1u), fabric_sheen);
    output.depth = clamp(clip.z / clip.w, 0.0, 1.0);
    return output;
}

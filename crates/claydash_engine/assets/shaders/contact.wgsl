// Same primary ray, geometry normal and eight hemispherical probes as transport.
// No reflection/transmission state is live in this entry point.
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2(f32(camera.world_mode.w), f32(camera.count.z));
    let clip = input.position.xy / size * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    let far = camera.inverse_view_projection * vec4(clip, 1.0, 1.0);
    let near = camera.inverse_view_projection * vec4(clip, 0.0, 1.0);
    let far_point = far.xyz / far.w;
    let near_point = near.xyz / near.w;
    let ray = select(normalize(far_point - camera.position.xyz), normalize(far_point - near_point), camera.count.w != 0u);
    let origin = select(camera.position.xyz, near_point, camera.count.w != 0u);
    neural_ray_direction = ray;
    var sample = trace_objects(origin, ray, 0.003, empty_splat_exclusions(), HYBRID_SPLATS);
    var point = origin + ray * sample.x;
    // Alpha splats do not consume the first material bounce. Find the same
    // first solid surface used by the transport pass's contact probes.
    var excluded = empty_splat_exclusions();
    for (var i = 0u; i < 12u; i++) {
        if sample.y < 0.0 || objects[u32(sample.y)].state.y != 10 { break; }
        excluded[i] = u32(sample.y);
        let next_origin = point + ray * 0.0005;
        sample = trace_objects(next_origin, ray, 0.0015, excluded, false);
        point = next_origin + ray * sample.x;
    }
    if sample.y < 0.0 { return vec4(1.0); }
    let index = u32(sample.y);
    var outward: vec3<f32>;
    if sample.z == -2.0 { outward = depth_accelerator_texture_normal(point, ray, index); }
    else { outward = scene_normal(point, index); }
    let normal = select(-outward, outward, dot(ray, outward) < 0.0);
    return vec4(ambient_occlusion(point, normal));
}

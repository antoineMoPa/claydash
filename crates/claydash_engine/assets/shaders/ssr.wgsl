fn screen_ray_color(center: vec3<f32>, direction: vec3<f32>, roughness: f32, excluded_component: f32) -> vec4<f32> {
    var previous_depth = 0.0;
    for (var step = 1; step <= 28; step++) {
        let traveled = 0.08 + f32(step) * (0.07 + f32(step) * 0.012);
        let position = center + direction * traveled;
        let clip = camera.view_projection * vec4(position, 1.0);
        if clip.w <= 0.0 { break; }
        let uv = clip.xy / clip.w * vec2(0.5, -0.5) + vec2(0.5);
        if any(uv <= vec2(0.005)) || any(uv >= vec2(0.995)) { break; }
        let sample = textureLoad(positions, pixel(uv), 0);
        if sample.w < 0.0 { continue; }
        // Refraction cannot recover the hidden back of its own component.
        if excluded_component > 0.0 && textureLoad(optics, pixel(uv), 0).z == excluded_component { continue; }
        let sampled_clip = camera.view_projection * vec4(sample.xyz, 1.0);
        let delta_depth = clip.z / clip.w - sampled_clip.z / sampled_clip.w;
        if delta_depth >= 0.0 && (previous_depth < 0.0 || step == 1) && distance(sample.xyz, position) < 0.35 {
            let sample_normal = textureLoad(normals, pixel(uv), 0);
            let sample_color = textureLoad(colors, pixel(uv), 0);
            let shaded = lit(sample.xyz, sample_normal.xyz, sample_color.xyz, sample_normal.w, sample_color.w, sample.w, 1.0);
            let edge = min(min(uv.x, uv.y), min(1.0 - uv.x, 1.0 - uv.y));
            let confidence = smoothstep(0.0, 0.08, edge) * (1.0 - f32(step) / 28.0) * (1.0 - roughness / 0.65);
            return vec4(shaded, confidence);
        }
        previous_depth = delta_depth;
    }
    return vec4(0.0);
}


fn reflection_color(center: vec3<f32>, normal: vec3<f32>, roughness: f32) -> vec4<f32> {
    if camera.world_mode.z == 0u || roughness > 0.65 { return vec4(0.0); }
    return screen_ray_color(center, reflect(-view_direction(center), normal), roughness, 0.0);
}

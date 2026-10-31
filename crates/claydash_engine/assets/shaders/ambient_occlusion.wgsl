// Short hemispherical SDF probe for contact shading on the primary hit.
// Distances are in world units. Compare against the original surface's
// expected distance to avoid treating the hit object itself as an occluder.
fn ambient_occlusion(point: vec3<f32>, normal: vec3<f32>) -> f32 {
    if !all(abs(normal) < vec3(2.0)) || dot(normal, normal) < 0.000001 { return 1.0; }
    let reference = select(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), abs(normal.y) > 0.8);
    let tangent = normalize(cross(reference, normal));
    let bitangent = cross(normal, tangent);
    var occlusion = 0.0;
    // Blend close contact and a broader falloff instead of sampling a single
    // shell. Stagger the outer ring to avoid reinforcing eight radial spokes.
    // Deterministic samples keep MCP captures repeatable without temporal noise.
    for (var ring = 0; ring < 2; ring++) {
        let reach = select(0.04, 0.16, ring == 1);
        for (var direction_index = 0; direction_index < 8; direction_index++) {
            let angle = f32(direction_index) * 0.78539816 + f32(ring) * 0.39269908;
            let direction = normalize(normal * 0.75 + tangent * cos(angle) + bitangent * sin(angle));
            let expected = max(reach * dot(direction, normal), 0.000001);
            // Splats are alpha-composited, so they are not solid occluders.
            let distance_to_scene = scene_distance_limit(
                point + normal * 0.008 + direction * reach, expected, true, true).x;
            occlusion += smoothstep(0.0, 1.0, (expected - distance_to_scene) / expected);
        }
    }
    return select(1.0, 1.0 - occlusion * 0.025, occlusion >= 0.0 && occlusion <= 16.0);
}

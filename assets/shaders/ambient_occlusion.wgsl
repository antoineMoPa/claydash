// Short hemispherical SDF probe for contact shading on the primary hit.
// Distances are in world units. Compare against the original surface's
// expected distance to avoid treating the hit object itself as an occluder.
fn ambient_occlusion(point: vec3<f32>, normal: vec3<f32>) -> f32 {
    let reference = select(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), abs(normal.y) > 0.8);
    let tangent = normalize(cross(reference, normal));
    let bitangent = cross(normal, tangent);
    var occlusion = 0.0;
    for (var direction_index = 0; direction_index < 4; direction_index++) {
        let angle = f32(direction_index) * 1.5707963;
        let direction = normalize(normal * 0.75 + tangent * cos(angle) + bitangent * sin(angle));
        let reach = 0.12;
        let expected = reach * dot(direction, normal);
        let distance_to_scene = scene_distance(point + normal * 0.008 + direction * reach).x;
        occlusion += clamp((expected - distance_to_scene) / expected, 0.0, 1.0);
    }
    return clamp(1.0 - occlusion * 0.52, 0.25, 1.0);
}

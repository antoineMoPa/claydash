fn occlusion(center: vec3<f32>, normal: vec3<f32>, coordinates: vec2<i32>) -> f32 {
    if camera.world_mode.y == 0u { return 1.0; }
    let size = vec2<i32>(textureDimensions(positions));
    var blocked = 0.0;
    var valid = 0.0;
    // Four directions and three radii in screen space. World-space distance
    // and hemisphere checks reject silhouettes and unrelated background.
    for (var radius = 1; radius <= 3; radius++) {
        for (var direction = 0; direction < 4; direction++) {
            let offset = select(select(select(vec2<i32>(radius * 3, 0), vec2<i32>(-radius * 3, 0), direction == 1), vec2<i32>(0, radius * 3), direction == 2), vec2<i32>(0, -radius * 3), direction == 3);
            let neighbor = coordinates + offset;
            if any(neighbor < vec2<i32>(0)) || any(neighbor >= size) { continue; }
            let sample = textureLoad(positions, neighbor, 0);
            if sample.w < 0.0 { continue; }
            let delta = sample.xyz - center;
            let distance = length(delta);
            if distance < 0.005 || distance > 1.25 { continue; }
            blocked += max(dot(normal, delta / distance) - 0.08, 0.0) * (1.0 - distance / 1.25);
            valid += 1.0;
        }
    }
    return clamp(1.0 - blocked / max(valid, 1.0) * 2.0, 0.42, 1.0);
}


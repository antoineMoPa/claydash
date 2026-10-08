fn text_distance(point: vec3<f32>, object: Object) -> f32 {
    let offset = bitcast<u32>(object.params.y);
    let header = polygon_points[offset];
    let count = bitcast<u32>(header.x);
    let half_depth = header.y;
    var nearest = 100.0;
    for (var glyph = 0u; glyph < 256u; glyph++) {
        if glyph >= count { break; }
        let base = offset + 1u + glyph * 9u;
        let a = polygon_points[base];
        let b = polygon_points[base + 1u];
        let c = polygon_points[base + 2u];
        let d = polygon_points[base + 3u];
        let e = polygon_points[base + 4u];
        let f = polygon_points[base + 5u];
        let origin = vec3(a, b.x);
        let x_axis = vec3(b.y, c);
        let y_axis = vec3(d, e.x);
        let z_axis = vec3(e.y, f);
        let delta = point - origin;
        let flat = vec2(dot(delta, x_axis), dot(delta, y_axis));
        let depth = abs(dot(delta, z_axis)) - half_depth;
        let lower = polygon_points[base + 6u];
        let upper = polygon_points[base + 7u];
        let bbox_distance = length(max(max(lower - flat, flat - upper), vec2(0.0)));
        if length(vec2(bbox_distance, max(depth, 0.0))) > max(nearest, 0.0) { continue; }
        let record = polygon_points[base + 8u];
        let edge_offset = bitcast<u32>(record.x);
        let edge_count = bitcast<u32>(record.y);
        var edge_distance = 1e20;
        var inside = false;
        for (var edge_index = 0u; edge_index < 1024u; edge_index++) {
            if edge_index >= edge_count { break; }
            let start = polygon_points[edge_offset + edge_index * 2u];
            let end = polygon_points[edge_offset + edge_index * 2u + 1u];
            let edge = end - start;
            let closest = start + edge * clamp(dot(flat - start, edge) / max(dot(edge, edge), 1e-10), 0.0, 1.0);
            edge_distance = min(edge_distance, distance(flat, closest));
            if (start.y > flat.y) != (end.y > flat.y) {
                let crossing = start.x + (flat.y - start.y) * (end.x - start.x) / (end.y - start.y);
                if flat.x < crossing { inside = !inside; }
            }
        }
        let planar = select(edge_distance, -edge_distance, inside);
        let q = vec2(planar, depth);
        nearest = min(nearest, length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0));
    }
    return nearest;
}


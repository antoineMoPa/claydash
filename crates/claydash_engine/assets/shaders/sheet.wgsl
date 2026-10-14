fn sheet_cubic_basis(t: f32) -> vec4<f32> {
    let a = 1.0-t;
    return vec4(a*a*a, 3.0*t*a*a, 3.0*t*t*a, t*t*t);
}
fn sheet_taper(t: f32) -> f32 {
    let v = clamp(t, 0.0, 1.0);
    return v*v*(3.0-2.0*v);
}
// Material-space height graph and relief; records follow the cut records.
fn sheet_emboss_height(uv: vec2<f32>, object: Object) -> f32 {
    let offset = bitcast<u32>(object.params.x);
    let count = bitcast<u32>(object.params.y);
    let header = polygon_points[offset + 1u];
    let relief = polygon_points[offset + 2u];
    let emboss_offset = offset + 4u + (count + u32(header.y)) * 3u;
    var height = 0.0;
    for (var i = 0u; i < u32(relief.x); i++) {
        let base = emboss_offset + i * 3u;
        let center = polygon_points[base];
        let circle = polygon_points[base + 1u];
        let transition = polygon_points[base + 2u].x;
        let distance = length(uv - center);
        if distance <= circle.x { height += circle.y; }
        else if distance < circle.x + transition {
            let t = (distance - circle.x) / transition;
            height += circle.y * 0.5 * (1.0 + cos(3.141592653589793 * t));
        }
    }
    var extension = emboss_offset + u32(relief.x)*3u;
    let feature = polygon_points[extension];
    extension += 1u;
    if feature.x != 0.0 {
        let t = clamp(uv/polygon_points[offset]+vec2(0.5), vec2(0.0), vec2(1.0));
        let x = sheet_cubic_basis(t.x);
        let y = sheet_cubic_basis(t.y);
        for (var j=0u; j<4u; j++) { for (var i=0u; i<4u; i++) {
            let index = j*4u+i;
            let pair = polygon_points[extension+index/2u];
            height += pair[index%2u]*x[i]*y[j];
        } }
        extension += 8u;
    }
    for (var path=0u; path<u32(feature.y); path++) {
        let header = polygon_points[extension];
        let taper = polygon_points[extension+1u];
        let stations = extension+2u;
        var along = 0.0;
        var path_height = 0.0;
        for (var i=0u; i+1u<u32(header.x); i++) {
            let a = stations+i*3u;
            let b = a+3u;
            let start = polygon_points[a];
            let e = polygon_points[b]-start;
            let segment_length = length(e);
            let t = clamp(dot(uv-start,e)/dot(e,e), 0.0, 1.0);
            let shape = mix(polygon_points[a+1u],polygon_points[b+1u],t);
            let blend = mix(polygon_points[a+2u].x,polygon_points[b+2u].x,t);
            let r = min(length(uv-start-e*t)/shape.x,1.0);
            let rounded = 0.5*(1.0+cos(3.141592653589793*r));
            let crease = (1.0-r)*(1.0-r);
            let arclength = along+t*segment_length;
            let value = shape.y*mix(rounded,crease,blend)*sheet_taper(arclength/taper.x)*sheet_taper((header.y-arclength)/taper.y);
            if abs(value)>abs(path_height) { path_height=value; }
            along += segment_length;
        }
        height += path_height;
        extension = stations+u32(header.x)*3u;
    }
    return height;
}

// Shared analytic sheet evaluator; packed strips are compiled once per scene upload.
fn sheet_strip_distance(point: vec3<f32>, object: Object) -> f32 {
    let offset = bitcast<u32>(object.params.x);
    let count = bitcast<u32>(object.params.y);
    let size = polygon_points[offset];
    let header = polygon_points[offset + 1u];
    let half = header.x;
    let cut_offset = offset + 4u + count * 3u;
    var result = 1e10;
    for (var i = 0u; i < count; i++) {
        let base = offset + 4u + i * 3u;
        let range = polygon_points[base];
        let origin = polygon_points[base + 1u];
        let bend = polygon_points[base + 2u];
        let delta = point.xz - origin;
        let s = sin(bend.x);
        let c = cos(bend.x);
        let p = vec2(c * delta.x + s * delta.y, -s * delta.x + c * delta.y);
        var u = p.x;
        var depth = p.y;
        var factor = 1.0;
        if bend.y != 0.0 {
            let r = bend.y;
            u = r * atan2(sign(r) * p.x, sign(r) * (r - p.y));
            depth = sign(r) * (abs(r) - length(vec2(p.x, p.y - r)));
            let range = polygon_points[offset + 3u];
            let clearance = select(abs(r) + range.x, r - range.y, r > 0.0);
            factor = max(min(clearance / abs(r), 1.0), 0.01);
        }
        let x = range.x + u;
        let uv = vec2(x, point.y);
        let box = abs(uv) - size * 0.5;
        var planar = length(max(box, vec2(0.0))) + min(max(box.x, box.y), 0.0);
        for (var j = 0u; j < u32(header.y); j++) {
            let cut = cut_offset + j * 3u;
            let kind = polygon_points[cut];
            let a = polygon_points[cut + 1u];
            var distance = kind.y - length(uv - a);
            if kind.x == 0.0 {
                let e = polygon_points[cut + 2u] - a;
                let v = uv - a;
                distance = (e.y * v.x - e.x * v.y) / length(e);
            }
            planar = max(planar, distance);
        }
        planar = max(max(planar, range.x - x), x - range.y) * factor;
        // The signed extrusion distance is at least max(planar, -half),
        // regardless of relief height. Avoid evaluating crowns and embosses
        // in charts that cannot improve the current union distance.
        if max(planar, -half) >= result { continue; }
        let q = vec2(planar, abs(depth - sheet_emboss_height(uv, object)) - half);
        result = min(result, length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0));
    }
    return result * polygon_points[offset + 2u].y;
}

// Ordered spatial forming, with bounded DFS (four bends, three inverse charts).
// This field represents the union of all preimages, including self overlaps.
fn sheet_directional_distance(point: vec3<f32>, object: Object) -> f32 {
    let offset = bitcast<u32>(object.params.x);
    let count = bitcast<u32>(object.params.y);
    let size = polygon_points[offset];
    let header = polygon_points[offset + 1u];
    let cut_offset = offset + 4u + count * 3u;
    var positions: array<vec3<f32>, 9>;
    var remaining: array<u32, 9>;
    var constraints: array<f32, 9>;
    var scales: array<f32, 9>;
    positions[0] = point;
    remaining[0] = count;
    constraints[0] = -1e10;
    scales[0] = 1.0;
    var size_stack = 1u;
    var result = 1e10;
    loop {
        if size_stack == 0u { break; }
        size_stack -= 1u;
        let p = positions[size_stack];
        let left = remaining[size_stack];
        let constraint = constraints[size_stack];
        let scale = scales[size_stack];
        // Each preimage contributes max(domain constraint, slab distance).
        // A rejected chart also rejects every descendant of that chart.
        if constraint >= result { continue; }
        if left == 0u {
            let uv = p.xy;
            let box = abs(uv) - size * 0.5;
            var planar = length(max(box, vec2(0.0))) + min(max(box.x, box.y), 0.0);
            for (var j = 0u; j < u32(header.y); j++) {
                let cut = cut_offset + j * 3u;
                let kind = polygon_points[cut];
                let a = polygon_points[cut + 1u];
                var distance = kind.y - length(uv - a);
                if kind.x == 0.0 {
                    let e = polygon_points[cut + 2u] - a;
                    let v = uv - a;
                    distance = (e.y * v.x - e.x * v.y) / length(e);
                }
                planar = max(planar, distance);
            }
            if max(constraint, max(planar, -header.x) * scale) >= result { continue; }
            let q = vec2(planar, abs(p.z - sheet_emboss_height(uv, object)) - header.x) * scale;
            result = min(result, max(constraint, length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0)));
        } else {
            let base = offset + 4u + (left - 1u) * 3u;
            let line = polygon_points[base];
            let bend = polygon_points[base + 1u];
            let s = sin(line.y);
            let c = cos(line.y);
            let x = c * p.x + s * p.y - line.x;
            let y = -s * p.x + c * p.y;
            let r = bend.x * sign(bend.y);
            let zone_length = bend.x * abs(bend.y);
            let st = sin(bend.y);
            let ct = cos(bend.y);
            let dx = x - r * st;
            let dz = p.z - r * (1.0 - ct);
            let tail_u = zone_length + ct * dx + st * dz;
            let rho = length(vec2(x, r - p.z));
            for (var branch = 0u; branch < 3u; branch++) {
                var u = x;
                var z = p.z;
                var domain = x;
                let factor = polygon_points[base + 2u].x;
                if branch == 1u {
                    u = tail_u;
                    z = -st * dx + ct * dz;
                    domain = zone_length - u;
                } else if branch >= 2u {
                    let sign_r = sign(r);
                    let t = atan2(sign_r * x, sign_r * (r - p.z));
                    u = r * t;
                    z = r - sign_r * rho;
                    domain = max(-u, u - zone_length);

                }
                let mapped_x = line.x + u;
                positions[size_stack] = vec3(c * mapped_x - s * y, s * mapped_x + c * y, z);
                remaining[size_stack] = left - 1u;
                constraints[size_stack] = max(constraint, domain * scale * factor);
                scales[size_stack] = scale * factor;
                size_stack += 1u;
            }
        }
    }
    return result * polygon_points[offset + 2u].y;
}

fn sheet_distance(point: vec3<f32>, object: Object) -> f32 {
    if object.params.z == 1.0 { return sheet_directional_distance(point, object); }
    return sheet_strip_distance(point, object);
}

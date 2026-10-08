// Object-local woven and knit appearance. Slots mirror material_gpu/fabric.rs.
const FABRIC_KIND: u32 = 2u;
const FABRIC_PITCH: u32 = 3u;
const FABRIC_FIBER: u32 = 4u;
const FABRIC_SHEEN: u32 = 5u;
const FABRIC_LIGHT_YARN: u32 = 6u;
fn fabric_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
fn fabric_frame(point: vec3<f32>, normal: vec3<f32>, object: Object, angle: f32) -> vec3<f32> {
    let p = vec4(point, 1.0);
    let local = vec3(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p)) * object.scale.xyz;
    let n = abs(vec3(dot(object.inverse_rows[0].xyz, normal), dot(object.inverse_rows[1].xyz, normal), dot(object.inverse_rows[2].xyz, normal)));
    var uv = local.xy;
    if n.x > n.y && n.x > n.z { uv = local.zy; }
    else if n.y > n.z { uv = local.xz; }
    let c = cos(angle);
    let s = sin(angle);
    return vec3(c * uv.x - s * uv.y, s * uv.x + c * uv.y, 0.0);
}
fn fabric_tangent(normal: vec3<f32>, object: Object, angle: f32) -> vec3<f32> {
    let n = abs(vec3(dot(object.inverse_rows[0].xyz, normal), dot(object.inverse_rows[1].xyz, normal), dot(object.inverse_rows[2].xyz, normal)));
    var u = vec3(1.0, 0.0, 0.0);
    var v = vec3(0.0, 1.0, 0.0);
    if n.x > n.y && n.x > n.z { u = vec3(0.0, 0.0, 1.0); }
    else if n.y > n.z { v = vec3(0.0, 0.0, 1.0); }
    let local = cos(angle) * u - sin(angle) * v;
    let world = object.inverse_rows[0].xyz * local.x + object.inverse_rows[1].xyz * local.y + object.inverse_rows[2].xyz * local.z;
    return normalize(world - normal * dot(world, normal));
}
fn fabric_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(fabric_hash(cell), fabric_hash(cell + vec2(1.0, 0.0)), w.x),
        mix(fabric_hash(cell + vec2(0.0, 1.0)), fabric_hash(cell + vec2(1.0, 1.0)), w.x), w.y);
}
fn fabric_strand(distance: f32, width: f32) -> f32 {
    return exp(-pow(distance / width, 2.0));
}
fn fabric_knit_loop(uv: vec2<f32>) -> f32 {
    let row = floor(uv.y);
    let x = fract(uv.x + 0.5 * (row - 2.0 * floor(row * 0.5))) - 0.5;
    let y = fract(uv.y);
    let width = 0.085 + 0.025 * fabric_hash(vec2(row, floor(uv.x)));
    let left = fabric_strand(x + 0.17 + 0.14 * sin(y * 3.14159), width);
    let right = fabric_strand(x - 0.17 - 0.14 * sin(y * 3.14159), width);
    let head = fabric_strand(y - 0.76 + 0.14 * cos(x * 5.0), width * 1.3)
        * fabric_strand(x, 0.36);
    return max(max(left, right), head);
}
fn fabric_height(uv: vec2<f32>, kind: vec4<f32>, fiber: vec4<f32>) -> f32 {
    let wander = vec2(fabric_noise(vec2(uv.y * 0.18, uv.x * 0.05)),
        fabric_noise(vec2(uv.x * 0.18, uv.y * 0.05) + vec2(11.0)));
    let q = uv + 0.11 * (wander - 0.5);
    let f = fract(q);
    let noise = fabric_noise(uv * 1.6);
    let mid = fabric_noise(vec2(uv.x * 0.48, uv.y * 1.35));
    var height = 0.5;
    if kind.z > 0.5 && kind.x < 0.5 {
        height = 0.40 + 0.12 * mid + 0.05 * fabric_noise(uv * 4.2);
    } else if kind.x < 0.5 {
        height = 0.34 + 0.38 * fabric_knit_loop(q) + 0.08 * noise;
    } else if kind.x < 1.5 {
        height = 0.35 + 0.23 * fabric_noise(uv * 2.2)
            + 0.14 * fabric_noise(uv * 6.3) + 0.08 * fabric_noise(uv * 10.0);
    } else if kind.x < 2.5 {
        let ribs = max(fabric_strand(f.x - 0.5, 0.13), fabric_strand(f.y - 0.5, 0.13));
        height = 0.17 + 0.72 * ribs + 0.08 * noise;
    } else {
        let warp = fabric_strand(f.x - 0.5, 0.23);
        let weft = fabric_strand(f.y - 0.5, 0.23);
        let parity = floor(q.x) + floor(q.y);
        let crossing = parity - 2.0 * floor(parity * 0.5);
        height = 0.24 + 0.54 * mix(warp, weft, crossing) + 0.10 * noise;
    }
    return clamp(height + fiber.x * 0.045 * (mid - 0.5), 0.0, 1.0);
}
fn fabric_heather_layer(uv: vec2<f32>, shift: vec2<f32>, strand_length: f32) -> f32 {
    let p = uv * vec2(0.72 / max(strand_length, 0.25), 1.45) + shift;
    let cell = floor(p);
    let local = fract(p);
    let jitter = 0.48 * (fabric_hash(cell + vec2(2.7, 8.1)) - 0.5);
    let end_mask = smoothstep(0.0, 0.24, local.x)
        * (1.0 - smoothstep(0.72, 1.0, local.x));
    let width = 0.15 + 0.09 * fabric_hash(cell + vec2(13.0, 4.3));
    let opacity = smoothstep(0.16, 0.90, fabric_hash(cell + vec2(19.1, 11.6)));
    return fabric_strand(local.y - 0.5 - jitter, width) * end_mask * opacity;
}
fn fabric_heather_yarn(uv: vec2<f32>, strand_length: f32) -> f32 {
    return clamp(2.1 * fabric_heather_layer(uv, vec2(0.0), strand_length)
        + 1.7 * fabric_heather_layer(uv, vec2(0.53, 0.37), strand_length), 0.0, 1.0);
}
fn fabric_filtered_heather(uv: vec2<f32>, strand_length: f32, footprint: f32) -> f32 {
    let dx = vec2(0.18 * max(footprint, 0.3), 0.0);
    let dy = vec2(0.0, 0.10 * max(footprint, 0.3));
    let sample = 0.25 * (fabric_heather_yarn(uv - dx - dy, strand_length)
        + fabric_heather_yarn(uv + dx - dy, strand_length)
        + fabric_heather_yarn(uv - dx + dy, strand_length)
        + fabric_heather_yarn(uv + dx + dy, strand_length));
    return mix(sample, 0.22, 0.65 * smoothstep(0.5, 1.7, footprint));
}
fn fabric_geometry_visible(object: Object) -> bool {
    if !HAS_FABRIC_MATERIAL || (object.state.y != 1 && object.state.y != 2) { return false; }
    let header = material_headers[object.component.w];
    if header.kind != MATERIAL_FABRIC { return false; }
    let pitch = material_params[header.offset + FABRIC_PITCH];
    if pitch.z < 0.0005 { return false; }
    let eye = vec4(camera.position.xyz, 1.0);
    let local_eye = vec3(dot(object.inverse_rows[0], eye), dot(object.inverse_rows[1], eye), dot(object.inverse_rows[2], eye));
    let distance_to_shape = max(length(local_eye) - length(object.params.xyz), 0.1) * object.params.w;
    let pixel = 0.83 * distance_to_shape / f32(max(camera.count.z, 1u));
    return min(pitch.x, pitch.y) > 3.0 * pixel;
}
fn fabric_geometry_cut(local: vec3<f32>, object: Object, offset: u32) -> f32 {
    let pitch = material_params[offset + FABRIC_PITCH];
    let fiber = material_params[offset + FABRIC_FIBER];
    let kind = material_params[offset + FABRIC_KIND];
    var local_normal = normalize(local);
    if object.state.y == 2 {
        let box_q = abs(local) - object.params.xyz;
        local_normal = vec3(0.0, 0.0, sign(local.z));
        if box_q.x > box_q.y && box_q.x > box_q.z {
            local_normal = vec3(sign(local.x), 0.0, 0.0);
        } else if box_q.y > box_q.z {
            local_normal = vec3(0.0, sign(local.y), 0.0);
        }
    }
    // Use the same local face frame as surface shading without applying inverse transform twice.
    var face = (local * object.scale.xyz).xy;
    let n = abs(local_normal);
    if n.x > n.y && n.x > n.z { face = (local * object.scale.xyz).zy; }
    else if n.y > n.z { face = (local * object.scale.xyz).xz; }
    let uv = vec2(cos(fiber.w) * face.x - sin(fiber.w) * face.y,
        sin(fiber.w) * face.x + cos(fiber.w) * face.y) / max(pitch.xy, vec2(0.002));
    let height = fabric_height(uv, kind, fiber);
    return pitch.z * (1.0 - height);
}
fn evaluate_fabric(point: vec3<f32>, normal: vec3<f32>, object: Object, base: Surface, offset: u32) -> Surface {
    let kind = material_params[offset + FABRIC_KIND];
    let pitch = material_params[offset + FABRIC_PITCH];
    let fiber = material_params[offset + FABRIC_FIBER];
    let sheen = material_params[offset + FABRIC_SHEEN];
    let light_yarn = material_params[offset + FABRIC_LIGHT_YARN];
    let frame = fabric_frame(point, normal, object, fiber.w);
    let uv = frame.xy / max(pitch.xy, vec2(0.002));
    let footprint = max(length(point - camera.position.xyz) * 0.0012, 0.0005);
    let visibility = 1.0 - smoothstep(0.35, 1.3, footprint / min(pitch.x, pitch.y));
    let detail_visibility = 1.0 - smoothstep(0.08, 0.42, footprint / min(pitch.x, pitch.y));
    let height = fabric_height(uv, kind, fiber);
    var surface = base;
    surface.color *= 1.0 + visibility * (height - 0.5) * 0.14;
    if kind.y > 0.5 && kind.y < 2.5 {
        let nap = fabric_noise(uv * 4.0) - 0.5;
        surface.color *= 1.0 + nap * fiber.z * detail_visibility * 0.08;
    }
    if kind.y > 2.5 {
        let yarn = fabric_noise(uv * 1.5) - 0.5;
        surface.color *= 1.0 + yarn * fiber.x * visibility * 0.06;
    }
    // Heather light yarns are interrupted along the yarn axis; dye recoloring leaves them alone.
    if kind.z > 0.5 {
        let strand = fabric_filtered_heather(uv, fiber.y, footprint / min(pitch.x, pitch.y));
        surface.color = mix(surface.color, light_yarn.rgb, strand * sheen.w * visibility * 1.9);
    }
    let filament = fabric_noise(vec2(uv.x * 6.4, uv.y * 2.1)) - 0.5;
    surface.color *= 1.0 + filament * fiber.x * detail_visibility * 0.09;
    let du = fabric_height(uv + vec2(0.04, 0.0), kind, fiber) - fabric_height(uv - vec2(0.04, 0.0), kind, fiber);
    let dv = fabric_height(uv + vec2(0.0, 0.04), kind, fiber) - fabric_height(uv - vec2(0.0, 0.04), kind, fiber);
    let bump_gain = select(4.0, 1.2, fabric_geometry_visible(object));
    let slope = vec2(du / max(pitch.x, 0.002), dv / max(pitch.y, 0.002)) * pitch.z * pitch.w * visibility * bump_gain;
    let tangent = fabric_tangent(normal, object, fiber.w);
    let bitangent = normalize(cross(normal, tangent));
    surface.normal = normalize(normal - tangent * slope.x - bitangent * slope.y);
    surface.roughness = clamp(base.roughness + fiber.z * 0.08 - sheen.x * 0.06, 0.1, 1.0);
    surface.fiber = tangent * sheen.z;
    surface.figure = sheen.x;
    surface.sheen = sheen.y;
    return surface;
}
fn fabric_extra_light(surface: Surface, light: vec3<f32>, view: vec3<f32>) -> vec3<f32> {
    let halfway = normalize(light + view);
    let aligned = pow(max(1.0 - abs(dot(halfway, normalize(surface.fiber + vec3(0.0001)))), 0.0), mix(2.0, 12.0, surface.sheen));
    let directional = mix(0.65, aligned, clamp(length(surface.fiber), 0.0, 1.0));
    let grazing = pow(1.0 - max(dot(surface.normal, view), 0.0), 1.5);
    return surface.color * surface.figure * (0.05 + 0.22 * grazing) * directional * max(dot(surface.normal, light), 0.0);
}

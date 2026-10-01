// Object-local defect fields and conductor/paint/oxide layers. Slots match material_gpu/metal.rs.
const METAL_FINISH: u32 = 2u;
const METAL_DETAIL: u32 = 3u;
const METAL_PAINT: u32 = 4u;
const METAL_PAINT_COLOR: u32 = 5u;
const METAL_F0: u32 = 6u;
const METAL_OXIDE: u32 = 7u;
struct MetalFrame { local: vec3<f32>, uv: vec2<f32>, tangent: vec3<f32>, bitangent: vec3<f32> }
fn metal_hash(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.yzx + vec3(33.33));
    return fract((q.x + q.y) * q.z);
}
fn metal_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(mix(metal_hash(i), metal_hash(i + vec3(1.0, 0.0, 0.0)), w.x),
        mix(metal_hash(i + vec3(0.0, 1.0, 0.0)), metal_hash(i + vec3(1.0, 1.0, 0.0)), w.x), w.y),
        mix(mix(metal_hash(i + vec3(0.0, 0.0, 1.0)), metal_hash(i + vec3(1.0, 0.0, 1.0)), w.x),
        mix(metal_hash(i + vec3(0.0, 1.0, 1.0)), metal_hash(i + vec3(1.0)), w.x), w.y), w.z);
}
fn metal_fbm(p: vec3<f32>) -> f32 {
    return 0.55 * metal_noise(p) + 0.30 * metal_noise(p * 2.13 + vec3(7.1))
        + 0.15 * metal_noise(p * 4.57 + vec3(12.7));
}
fn metal_frame(point: vec3<f32>, normal: vec3<f32>, object: Object, finish: vec4<f32>) -> MetalFrame {
    let p = vec4(point, 1.0);
    let local = vec3(dot(object.inverse_rows[0], p), dot(object.inverse_rows[1], p), dot(object.inverse_rows[2], p)) * object.scale.xyz;
    let axes = mat3x3(normalize(object.inverse_rows[0].xyz), normalize(object.inverse_rows[1].xyz), normalize(object.inverse_rows[2].xyz));
    let n = abs(transpose(axes) * normal);
    var u = vec3(1.0, 0.0, 0.0);
    var v = vec3(0.0, 1.0, 0.0);
    var uv = local.xy;
    if n.x > n.y && n.x > n.z { u = vec3(0.0, 0.0, 1.0); uv = local.zy; }
    else if n.y > n.z { v = vec3(0.0, 0.0, 1.0); uv = local.xz; }
    if finish.y > 0.5 && finish.y < 1.5 {
        // Radial brushing in each dominant face's plane; center uses a stable fallback.
        let radial = uv;
        let radius = length(radial);
        if radius > 0.0001 { u = (-radial.y * u + radial.x * v) / radius; }
        uv = vec2(atan2(radial.y, radial.x) * max(radius, 0.01), radius);
    } else if finish.y > 1.5 {
        // Wire brushing follows local Z with a fixed helical twist, independent of the demo mesh.
        u = vec3(-0.85 * local.y, 0.85 * local.x, 1.0);
        uv = vec2(local.z, atan2(local.y, local.x) * max(length(local.xy), 0.01));
    }
    var tangent = axes * u;
    tangent -= normal * dot(tangent, normal);
    if dot(tangent, tangent) < 0.000001 {
        tangent = axes * v - normal * dot(axes * v, normal);
    }
    tangent = normalize(tangent);
    var bitangent = normalize(cross(normal, tangent));
    let c = cos(finish.w);
    let s = sin(finish.w);
    let rotated = c * tangent + s * bitangent;
    bitangent = -s * tangent + c * bitangent;
    uv = vec2(c * uv.x + s * uv.y, -s * uv.x + c * uv.y);
    return MetalFrame(local, uv, rotated, bitangent);
}
fn metal_scratch(uv: vec2<f32>, footprint: f32) -> f32 {
    let g = uv * vec2(3.0, 100.0);
    let cell = floor(g);
    let f = fract(g);
    let center = 0.2 + 0.6 * metal_hash(vec3(cell, 7.0));
    let width = max(0.013, footprint * 100.0);
    let line = 1.0 - smoothstep(width, width * 2.0, abs(f.y - center));
    let segment = smoothstep(0.02, 0.15, f.x) * (1.0 - smoothstep(0.58, 0.98, f.x));
    return line * segment * step(0.62, metal_hash(vec3(cell, 3.1)));
}
fn metal_height(local: vec3<f32>, uv: vec2<f32>, finish: f32, detail: vec4<f32>, footprint: f32) -> f32 {
    let q = uv * detail.x;
    let fade = 1.0 - smoothstep(0.002, 0.015, footprint * detail.x);
    var h = 0.000035 * (metal_fbm(local * detail.x * 40.0) - 0.5);
    if finish > 0.5 && finish < 2.5 {
        h += 0.000045 * (metal_noise(vec3(q * vec2(2.0, 230.0), 0.0)) - 0.5) * fade;
    } else if finish > 2.5 && finish < 3.5 {
        h += 0.010 * (metal_noise(local * detail.x * 18.0) - 0.5);
    } else if finish > 3.5 {
        h += 0.0025 * (metal_fbm(local * detail.x * 55.0) - 0.5);
    }
    return (h - detail.z * 0.0005 * metal_scratch(q, footprint * detail.x)) * detail.y;
}
fn metal_height_at(point: vec3<f32>, normal: vec3<f32>, object: Object, finish: vec4<f32>, detail: vec4<f32>, footprint: f32) -> f32 {
    let frame = metal_frame(point, normal, object, finish);
    return metal_height(frame.local, frame.uv, finish.x, detail, footprint);
}
fn metal_image_height(point: vec3<f32>, normal: vec3<f32>, object: Object) -> f32 {
    let image = stencil_color(point, normal, object);
    // Transparent texels and absent images contribute zero relief.
    return (dot(image.rgb, vec3(0.2126, 0.7152, 0.0722)) - 0.5) * image.a * 0.004;
}
fn evaluate_metal(point: vec3<f32>, normal: vec3<f32>, object: Object, base: Surface, offset: u32) -> Surface {
    let finish = material_params[offset + METAL_FINISH];
    let detail = material_params[offset + METAL_DETAIL];
    let paint = material_params[offset + METAL_PAINT];
    let frame = metal_frame(point, normal, object, finish);
    let footprint = max(length(point - camera.position.xyz) * 0.0008, 0.0001);
    let e = max(0.0003, footprint * 0.6);
    let du = metal_height_at(point + frame.tangent * e, normal, object, finish, detail, footprint)
        - metal_height_at(point - frame.tangent * e, normal, object, finish, detail, footprint);
    let dv = metal_height_at(point + frame.bitangent * e, normal, object, finish, detail, footprint)
        - metal_height_at(point - frame.bitangent * e, normal, object, finish, detail, footprint);
    var gradient = vec2(du, dv) / (2.0 * e);
    if paint.z > 0.0 && object.stencil_meta.x > 0.5 {
        gradient += paint.z * vec2(
            metal_image_height(point + frame.tangent * e, normal, object) - metal_image_height(point - frame.tangent * e, normal, object),
            metal_image_height(point + frame.bitangent * e, normal, object) - metal_image_height(point - frame.bitangent * e, normal, object)) / (2.0 * e);
    }
    let q = frame.local * detail.x;
    let scratch = metal_scratch(frame.uv * detail.x, footprint * detail.x) * detail.z;
    let patches = metal_fbm(q * 6.1) + 0.12 * metal_noise(q * 53.0);
    let chips = metal_fbm(q * 9.0) + 0.2 * metal_noise(q * 60.0);
    // Exact zero/one endpoints permit clean and fully covered specimens.
    var oxide = smoothstep(1.0 - detail.w - 0.055, 1.0 - detail.w + 0.055, patches);
    oxide = select(oxide, 0.0, detail.w <= 0.0);
    oxide = select(oxide, 1.0, detail.w >= 1.0);
    var coating = smoothstep(1.0 - paint.x - 0.04, 1.0 - paint.x + 0.04, chips);
    coating = select(coating, 0.0, paint.x <= 0.0);
    coating = select(coating, 1.0, paint.x >= 1.0) * (1.0 - scratch);
    let bare = (1.0 - oxide) * (1.0 - coating);
    let micro = metal_noise(q * 48.0);
    var surface = base;
    surface.normal = normalize(normal - frame.tangent * gradient.x - frame.bitangent * gradient.y);
    surface.roughness = clamp(base.roughness + (micro - 0.5) * 0.12 * detail.y + scratch * 0.25, 0.025, 1.0);
    surface.roughness = mix(mix(surface.roughness, 0.83, oxide), paint.y, coating);
    let oxide_color = material_params[offset + METAL_OXIDE].rgb * mix(0.35, 1.35, metal_noise(q * 21.0));
    let conductor = clamp(material_params[offset + METAL_F0].rgb * base.color, vec3(0.0), vec3(1.0));
    surface.color = mix(mix(conductor, oxide_color, oxide), material_params[offset + METAL_PAINT_COLOR].rgb, coating);
    surface.metallic = bare;
    surface.reflectivity = 0.04;
    surface.opacity = 1.0;
    surface.tangent = frame.tangent;
    surface.anisotropy = clamp(finish.z, 0.0, 0.95) * bare;
    surface.metal_response = true;
    return surface;
}

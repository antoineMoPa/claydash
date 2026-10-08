// Eleven vec4 slots: cage transforms, atlas domain, atlas/control indices,
// and the original control-point domain.
const LATTICE_INVERSE: u32 = 0u;
const LATTICE_FORWARD: u32 = 3u;
const LATTICE_MIN: u32 = 6u;
const LATTICE_INVERSE_EXTENT: u32 = 7u;
const LATTICE_INFO: u32 = 8u;
const LATTICE_CONTROL_MIN: u32 = 9u;
const LATTICE_CONTROL_INVERSE_EXTENT: u32 = 10u;
@group(0) @binding(6) var<storage, read> lattice_points: array<vec4<f32>>;
@group(0) @binding(9) var lattice_atlas: texture_3d<f32>;
@group(0) @binding(10) var lattice_sampler: sampler;

fn lattice_displacement(point: vec3<f32>, offset: u32) -> vec3<f32> {
    let info = modifier_params[offset + LATTICE_INFO];
    let n = bitcast<u32>(info.y);
    let base = bitcast<u32>(info.x);
    let coord = clamp((point - modifier_params[offset + LATTICE_MIN].xyz) *
        modifier_params[offset + LATTICE_INVERSE_EXTENT].xyz, vec3(0.0), vec3(1.0)) * f32(n - 1u);
    let tile_origin = vec2<f32>(f32(base % 32u), f32(base / 32u)) * 19.0 + vec2(1.0);
    let atlas_size = vec3<f32>(textureDimensions(lattice_atlas));
    let uvw = vec3((tile_origin + coord.xy + vec2(0.5)) / atlas_size.xy,
        (coord.z + 0.5) / atlas_size.z);
    return textureSampleLevel(lattice_atlas, lattice_sampler, uvw, 0.0).xyz;
}

fn lattice_control_displacement(point: vec3<f32>, offset: u32) -> vec3<f32> {
    let info = modifier_params[offset + LATTICE_INFO];
    let n = bitcast<u32>(info.w);
    if n > 3u { return lattice_displacement(point, offset); }
    let base = bitcast<u32>(info.z);
    let coord = clamp((point - modifier_params[offset + LATTICE_CONTROL_MIN].xyz) *
        modifier_params[offset + LATTICE_CONTROL_INVERSE_EXTENT].xyz,
        vec3(0.0), vec3(1.0)) * f32(n - 1u);
    let low = vec3<u32>(floor(coord));
    let high = min(low + vec3<u32>(1u), vec3<u32>(n - 1u));
    let t = coord - vec3<f32>(low);
    let a = lattice_points[base + low.x + n * (low.y + n * low.z)].xyz;
    let b = lattice_points[base + high.x + n * (low.y + n * low.z)].xyz;
    let c = lattice_points[base + low.x + n * (high.y + n * low.z)].xyz;
    let d = lattice_points[base + high.x + n * (high.y + n * low.z)].xyz;
    let e = lattice_points[base + low.x + n * (low.y + n * high.z)].xyz;
    let f = lattice_points[base + high.x + n * (low.y + n * high.z)].xyz;
    let g = lattice_points[base + low.x + n * (high.y + n * high.z)].xyz;
    let h = lattice_points[base + high.x + n * (high.y + n * high.z)].xyz;
    return mix(mix(mix(a, b, t.x), mix(c, d, t.x), t.y),
        mix(mix(e, f, t.x), mix(g, h, t.x), t.y), t.z);
}

fn lattice_point(point: vec3<f32>, offset: u32) -> vec3<f32> {
    let homogeneous = vec4(point, 1.0);
    let cage_point = vec3(
        dot(modifier_params[offset + LATTICE_INVERSE + 0u], homogeneous),
        dot(modifier_params[offset + LATTICE_INVERSE + 1u], homogeneous),
        dot(modifier_params[offset + LATTICE_INVERSE + 2u], homogeneous));
    let info = modifier_params[offset + LATTICE_INFO];
    var rest = cage_point;
    if bitcast<u32>(info.w) <= 3u {
        rest = cage_point - lattice_displacement(cage_point, offset);
        rest = cage_point - lattice_control_displacement(rest, offset);
    } else {
        rest = cage_point;
        for (var iteration = 0; iteration < 4; iteration++) {
            let next = cage_point - lattice_control_displacement(rest, offset);
            let change = next - rest;
            rest = next;
            if dot(change, change) < 0.00000001 { break; }
        }
    }
    let displacement = cage_point - rest;
    return point - vec3(
        dot(modifier_params[offset + LATTICE_FORWARD + 0u].xyz, displacement),
        dot(modifier_params[offset + LATTICE_FORWARD + 1u].xyz, displacement),
        dot(modifier_params[offset + LATTICE_FORWARD + 2u].xyz, displacement));
}

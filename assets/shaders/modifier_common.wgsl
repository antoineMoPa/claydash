// Each object caches its modifier kind and shared parameter offset.
const MODIFIER_LATTICE: u32 = 1u;
@group(0) @binding(8) var<storage, read> modifier_params: array<vec4<f32>>;

fn modifier_point(point: vec3<f32>, object: Object) -> vec3<f32> {
    if object.modifier.x == 0u { return point; }
    switch object.modifier.w {
        case MODIFIER_LATTICE: { return lattice_point(point, object.modifier.z); }
        default: { return point; }
    }
}

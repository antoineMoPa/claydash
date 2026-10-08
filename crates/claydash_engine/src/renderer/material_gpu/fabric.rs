use crate::model::Material;

// Slots 0-1 are common. Fabric uses slots 2-6; slot 7 remains available.
pub(super) fn pack_fabric(material: Material, params: &mut Vec<[f32; 4]>) {
    let f = material.fabric;
    params.extend_from_slice(&[
        [
            f.construction.gpu_code(),
            f.finish.gpu_code(),
            f.coloration.gpu_code(),
            0.0,
        ],
        [f.pitch_x, f.pitch_y, f.relief_depth, f.normal_gain],
        [f.fiber_detail, f.strand_length, f.nap_length, f.orientation],
        [
            f.sheen_weight,
            f.sheen_spread,
            f.fiber_alignment,
            f.light_yarn_fraction,
        ],
        [
            f.light_yarn_color.x,
            f.light_yarn_color.y,
            f.light_yarn_color.z,
            0.0,
        ],
    ]);
}

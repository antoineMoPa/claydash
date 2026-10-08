use super::*;

// A modifier owns a fixed set of vec4 slots; objects reference shared headers.
pub(super) const MAX_PARAM_SLOTS: usize = 11;
pub(super) const MODIFIER_LATTICE: u32 = 1;
pub(super) const INVERSE_GRID_RESOLUTION: usize = 17;

fn control_displacement(lattice: &crate::model::Lattice, offsets: &[Vec3], point: Vec3) -> Vec3 {
    let n = lattice.resolution as usize;
    let coord = ((point - lattice.min) / (lattice.max - lattice.min).max(Vec3::splat(0.0001)))
        .clamp(Vec3::ZERO, Vec3::ONE)
        * (n - 1) as f32;
    let low = coord.floor().as_uvec3();
    let high = (low + glam::UVec3::ONE).min(glam::UVec3::splat((n - 1) as u32));
    let t = coord - low.as_vec3();
    let sample =
        |x: u32, y: u32, z: u32| offsets[lattice.index(x as usize, y as usize, z as usize)];
    let a = sample(low.x, low.y, low.z).lerp(sample(high.x, low.y, low.z), t.x);
    let b = sample(low.x, high.y, low.z).lerp(sample(high.x, high.y, low.z), t.x);
    let c = sample(low.x, low.y, high.z).lerp(sample(high.x, low.y, high.z), t.x);
    let d = sample(low.x, high.y, high.z).lerp(sample(high.x, high.y, high.z), t.x);
    a.lerp(b, t.y).lerp(c.lerp(d, t.y), t.z)
}

pub(super) fn inverse_lattice_grid(
    lattice: &crate::model::Lattice,
    offsets: &[Vec3],
) -> (Vec<Vec3>, Vec3, Vec3) {
    let extent = offsets
        .iter()
        .fold(Vec3::ZERO, |maximum, offset| maximum.max(offset.abs()));
    let min = lattice.min - extent;
    let max = lattice.max + extent;
    let n = INVERSE_GRID_RESOLUTION;
    let mut points = Vec::with_capacity(n * n * n);
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                let cage_point =
                    min + (max - min) * Vec3::new(x as f32, y as f32, z as f32) / (n - 1) as f32;
                let mut rest = cage_point;
                for _ in 0..12 {
                    let next = cage_point - control_displacement(lattice, offsets, rest);
                    if next.distance_squared(rest) < 0.0000000001 {
                        rest = next;
                        break;
                    }
                    rest = next;
                }
                points.push(cage_point - rest);
            }
        }
    }
    (points, min, max)
}

// The symmetric deformation Jacobian is multiaffine in each cell. Its value
// inside the cell is a convex combination of corner values, so the smallest
// corner Gershgorin bound also bounds the cell's minimum stretch.
pub(super) fn lattice_march_factor(
    lattice: &crate::model::Lattice,
    offsets: &[Vec3],
    forward: glam::Mat4,
) -> f32 {
    let n = lattice.resolution as usize;
    let cell = (lattice.max - lattice.min).max(Vec3::splat(0.0001)) / (n - 1) as f32;
    let mut minimum_stretch = 1.0_f32;
    for z in 0..n - 1 {
        for y in 0..n - 1 {
            for x in 0..n - 1 {
                for corner in 0..8 {
                    let cx = corner & 1;
                    let cy = (corner >> 1) & 1;
                    let cz = (corner >> 2) & 1;
                    let dx = (offsets[lattice.index(x + 1, y + cy, z + cz)]
                        - offsets[lattice.index(x, y + cy, z + cz)])
                        / cell.x;
                    let dy = (offsets[lattice.index(x + cx, y + 1, z + cz)]
                        - offsets[lattice.index(x + cx, y, z + cz)])
                        / cell.y;
                    let dz = (offsets[lattice.index(x + cx, y + cy, z + 1)]
                        - offsets[lattice.index(x + cx, y + cy, z)])
                        / cell.z;
                    let xy = (dx.y + dy.x).abs() * 0.5;
                    let xz = (dx.z + dz.x).abs() * 0.5;
                    let yz = (dy.z + dz.y).abs() * 0.5;
                    minimum_stretch = minimum_stretch
                        .min(1.0 + dx.x - xy - xz)
                        .min(1.0 + dy.y - xy - yz)
                        .min(1.0 + dz.z - xz - yz);
                }
            }
        }
    }
    let matrix_norm = |matrix: glam::Mat4| {
        let x = matrix.x_axis.truncate().abs();
        let y = matrix.y_axis.truncate().abs();
        let z = matrix.z_axis.truncate().abs();
        let one = x.element_sum().max(y.element_sum()).max(z.element_sum());
        let infinity = (x + y + z).max_element();
        (one * infinity).sqrt()
    };
    let condition_bound = matrix_norm(forward) * matrix_norm(forward.inverse());
    let factor = 0.8 * minimum_stretch / condition_bound;
    if factor.is_finite() {
        factor.clamp(0.35, 0.8)
    } else {
        0.35
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct GpuModifierHeader {
    kind: u32,
    offset: u32,
    length: u32,
    reserved: u32,
}

#[derive(Default)]
pub(super) struct PackedModifiers {
    pub headers: Vec<GpuModifierHeader>,
    pub params: Vec<[f32; 4]>,
}

impl PackedModifiers {
    pub fn parameter_offset(&self, index: u32) -> u32 {
        self.headers[index as usize - 1].offset
    }

    pub fn insert_lattice(
        &mut self,
        inverse: [[f32; 4]; 3],
        forward: [[f32; 4]; 3],
        min: Vec3,
        max: Vec3,
        point_offset: u32,
        resolution: u32,
        control_offset: u32,
        control_resolution: u32,
        control_min: Vec3,
        control_max: Vec3,
    ) -> u32 {
        let index = self.headers.len() as u32 + 1; // Zero means no modifier.
        let offset = self.params.len() as u32;
        self.params.extend_from_slice(&inverse);
        self.params.extend_from_slice(&forward);
        self.params.push(min.extend(0.0).to_array());
        self.params.push(
            (Vec3::ONE / (max - min).max(Vec3::splat(0.0001)))
                .extend(0.0)
                .to_array(),
        );
        self.params.push([
            f32::from_bits(point_offset),
            f32::from_bits(resolution),
            f32::from_bits(control_offset),
            f32::from_bits(control_resolution),
        ]);
        self.params.push(control_min.extend(0.0).to_array());
        self.params.push(
            (Vec3::ONE / (control_max - control_min).max(Vec3::splat(0.0001)))
                .extend(0.0)
                .to_array(),
        );
        self.headers.push(GpuModifierHeader {
            kind: MODIFIER_LATTICE,
            offset,
            length: self.params.len() as u32 - offset,
            reserved: 0,
        });
        index
    }
}

pub(super) fn shader_source() -> String {
    include_str!("../../assets/shaders/sdf.wgsl").replace(
        "// MODIFIER_MODULES",
        &[
            include_str!("../../assets/shaders/modifier_common.wgsl"),
            include_str!("../../assets/shaders/modifier_lattice.wgsl"),
        ]
        .join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lattice_uses_named_nine_slot_record() {
        let mut packed = PackedModifiers::default();
        let identity = inverse_affine_rows(glam::Mat4::IDENTITY);
        assert_eq!(
            packed.insert_lattice(
                identity,
                identity,
                Vec3::ZERO,
                Vec3::ONE,
                17,
                3,
                31,
                2,
                Vec3::ZERO,
                Vec3::ONE
            ),
            1
        );
        assert_eq!(packed.headers[0].kind, MODIFIER_LATTICE);
        assert_eq!(packed.headers[0].offset, 0);
        assert_eq!(packed.headers[0].length, 11);
        assert_eq!(packed.params[7][..3], [1.0; 3]);
        assert_eq!(packed.params[8][0].to_bits(), 17);
        assert_eq!(packed.params[8][1].to_bits(), 3);
        assert_eq!(packed.params[8][2].to_bits(), 31);
        assert_eq!(std::mem::size_of::<GpuModifierHeader>(), 16);
    }

    #[test]
    fn mild_lattice_can_march_faster_than_strong_lattice() {
        let mut lattice = crate::model::Lattice::new(Vec3::ZERO, Vec3::ONE, 2);
        let corner = lattice.index(1, 1, 1);
        lattice.offsets[corner].x = 0.05;
        let mild =
            lattice_march_factor(&lattice, &lattice.effective_offsets(), glam::Mat4::IDENTITY);
        lattice.offsets[corner].x = 0.8;
        let strong =
            lattice_march_factor(&lattice, &lattice.effective_offsets(), glam::Mat4::IDENTITY);
        assert!(mild > 0.35);
        assert!(strong < mild);
    }

    #[test]
    fn march_factor_tracks_contracting_and_expanding_cages() {
        let mut lattice = crate::model::Lattice::new(Vec3::ZERO, Vec3::ONE, 2);
        for z in 0..2 {
            for y in 0..2 {
                let index = lattice.index(1, y, z);
                lattice.offsets[index] = Vec3::new(0.2, 0.0, 0.0);
            }
        }
        let expanding =
            lattice_march_factor(&lattice, &lattice.effective_offsets(), glam::Mat4::IDENTITY);
        assert!((expanding - 0.8).abs() < 0.0001);
        for z in 0..2 {
            for y in 0..2 {
                let index = lattice.index(1, y, z);
                lattice.offsets[index] = Vec3::new(-0.5, 0.0, 0.0);
            }
        }
        let contracting =
            lattice_march_factor(&lattice, &lattice.effective_offsets(), glam::Mat4::IDENTITY);
        assert!((contracting - 0.4).abs() < 0.0001);
    }

    #[test]
    fn inverse_grid_seed_refines_to_original_deformation() {
        for n in [2, 3] {
            let mut lattice = crate::model::Lattice::new(Vec3::splat(-1.0), Vec3::ONE, n);
            for z in 0..n as usize {
                for y in 0..n as usize {
                    for x in 0..n as usize {
                        if lattice.is_surface_point(x, y, z) {
                            let index = lattice.index(x, y, z);
                            lattice.offsets[index] = Vec3::new(
                                (x as f32 * 1.7 + y as f32 * 0.3).sin() * 0.2,
                                (y as f32 * 1.3 + z as f32 * 0.5).cos() * 0.15,
                                (z as f32 * 1.9 + x as f32 * 0.2).sin() * 0.1,
                            );
                        }
                    }
                }
            }
            let offsets = lattice.effective_offsets();
            let (grid, min, max) = inverse_lattice_grid(&lattice, &offsets);
            let size = INVERSE_GRID_RESOLUTION;
            let mut worst = 0.0_f32;
            for z in 0..13 {
                for y in 0..13 {
                    for x in 0..13 {
                        let point =
                            min + (max - min) * Vec3::new(x as f32, y as f32, z as f32) / 12.0;
                        let coord = ((point - min) / (max - min) * (size - 1) as f32)
                            .clamp(Vec3::ZERO, Vec3::splat((size - 1) as f32));
                        let lo = coord.floor().as_uvec3();
                        let hi = (lo + glam::UVec3::ONE).min(glam::UVec3::splat((size - 1) as u32));
                        let t = coord - lo.as_vec3();
                        let sample = |x: u32, y: u32, z: u32| {
                            grid[x as usize + size * (y as usize + size * z as usize)]
                        };
                        let a = sample(lo.x, lo.y, lo.z).lerp(sample(hi.x, lo.y, lo.z), t.x);
                        let b = sample(lo.x, hi.y, lo.z).lerp(sample(hi.x, hi.y, lo.z), t.x);
                        let c = sample(lo.x, lo.y, hi.z).lerp(sample(hi.x, lo.y, hi.z), t.x);
                        let d = sample(lo.x, hi.y, hi.z).lerp(sample(hi.x, hi.y, hi.z), t.x);
                        let approx = a.lerp(b, t.y).lerp(c.lerp(d, t.y), t.z);
                        let corrected = control_displacement(&lattice, &offsets, point - approx);
                        let mut rest = point;
                        for _ in 0..4 {
                            rest = point - control_displacement(&lattice, &offsets, rest);
                        }
                        worst = worst.max(corrected.distance(point - rest));
                    }
                }
            }
            assert!(worst < 0.01, "n={n} inverse seed error: {worst}");
        }
    }
}

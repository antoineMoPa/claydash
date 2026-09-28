use std::collections::HashMap;

use glam::Vec3;

use super::*;
use crate::model::{lattice_bounds, lattice_world_matrix, PreparedSubtreeSampler};

pub(super) const BOX_DEPTH_RESOLUTION: u32 = 40;
pub(super) const GAUSSIAN_BOX_RESOLUTION: u32 = 64;
pub(super) const MAX_BOX_DEPTH_TEXELS: usize = 32 * 1024 * 1024 / 16;

#[derive(Clone, Copy)]
pub(super) enum BoxCaptureStart {
    AtBounds,
    OutsideBounds,
}

#[derive(Clone, Debug)]
pub(super) struct BoxDepthAtlas {
    pub local_min: Vec3,
    pub local_max: Vec3,
    pub resolution: u32,
    pub layers: usize,
    /// Six faces, each stored row-major as (depth, red, green, blue).
    /// A negative value means the capture ray missed; its magnitude estimates
    /// the distance to the nearest occupied projection.
    pub texels: Vec<[f32; 4]>,
    pub owners: Vec<Option<uuid::Uuid>>,
    pub normals: Vec<Vec3>,
}

#[derive(Clone, Copy)]
enum BoxFace {
    PositiveX,
    NegativeX,
    PositiveY,
    NegativeY,
    PositiveZ,
    NegativeZ,
}

impl BoxFace {
    const ALL: [Self; 6] = [
        Self::PositiveX,
        Self::NegativeX,
        Self::PositiveY,
        Self::NegativeY,
        Self::PositiveZ,
        Self::NegativeZ,
    ];

    fn axes(self) -> (usize, usize, usize, f32) {
        match self {
            Self::PositiveX => (0, 1, 2, 1.0),
            Self::NegativeX => (0, 1, 2, -1.0),
            Self::PositiveY => (1, 0, 2, 1.0),
            Self::NegativeY => (1, 0, 2, -1.0),
            Self::PositiveZ => (2, 0, 1, 1.0),
            Self::NegativeZ => (2, 0, 1, -1.0),
        }
    }
}

pub(super) fn bake_box_depth_atlas(
    scene: &[SdfObject],
    root: uuid::Uuid,
    resolution: u32,
    start: BoxCaptureStart,
) -> Option<BoxDepthAtlas> {
    if !(8..=128).contains(&resolution) {
        return None;
    }
    let lookup: HashMap<_, _> = scene.iter().map(|object| (object.uuid, object)).collect();
    lookup.get(&root)?;
    let mut sampler = PreparedSubtreeSampler::new(scene, root)?;
    let march_factor = sampler.march_factor(modifier_gpu::lattice_march_factor);
    let (mut minimum, mut maximum) = lattice_bounds(scene, root)?;
    let world = lattice_world_matrix(scene, root);
    let deformation_extent = sampler.deformation_extent(world.inverse());
    minimum -= deformation_extent;
    maximum += deformation_extent;
    let center = (minimum + maximum) * 0.5;
    let half_extent = (maximum - minimum) * 0.5;
    if !center.is_finite() || !half_extent.is_finite() || half_extent.min_element() <= 0.0 {
        return None;
    }
    let world_scale = world
        .x_axis
        .truncate()
        .length()
        .min(world.y_axis.truncate().length())
        .min(world.z_axis.truncate().length());
    if !world_scale.is_finite() || world_scale <= 0.000001 {
        return None;
    }
    let clearance = match start {
        BoxCaptureStart::AtBounds => 0.0,
        BoxCaptureStart::OutsideBounds => 0.5 / world_scale,
    };
    let local_min = -half_extent - Vec3::splat(clearance);
    let local_max = half_extent + Vec3::splat(clearance);
    let layers = if matches!(start, BoxCaptureStart::OutsideBounds) {
        8
    } else {
        1
    };
    let face_size = resolution as usize * resolution as usize;
    let texel_count = 6 * face_size * layers;
    let mut texels = vec![[-1.0, 0.0, 0.0, 0.0]; texel_count];
    let mut owners = vec![None; texel_count];
    let mut normals = vec![Vec3::ZERO; texel_count];
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    for (face_index, face) in BoxFace::ALL.into_iter().enumerate() {
        let (axis, u_axis, v_axis, sign) = face.axes();
        let inward = -axes[axis] * sign;
        let world_step = world.transform_vector3(inward).length();
        if !world_step.is_finite() || world_step <= 0.000001 {
            return None;
        }
        let span = local_max[axis] - local_min[axis];
        for y in 0..resolution {
            for x in 0..resolution {
                let mut origin = center;
                origin[axis] += if sign > 0.0 {
                    local_max[axis]
                } else {
                    local_min[axis]
                };
                origin[u_axis] += local_min[u_axis]
                    + (x as f32 + 0.5) / resolution as f32
                        * (local_max[u_axis] - local_min[u_axis]);
                origin[v_axis] += local_min[v_axis]
                    + (y as f32 + 0.5) / resolution as f32
                        * (local_max[v_axis] - local_min[v_axis]);
                let mut depth = 0.0;
                let mut layer = 0;
                for _ in 0..192 {
                    let point = world.transform_point3(origin + inward * depth);
                    let (distance, owner) = sampler.sample(point);
                    if !distance.is_finite() {
                        return None;
                    }
                    let at_surface = if layers == 1 {
                        distance < 0.001
                    } else {
                        distance.abs() < 0.001
                    };
                    if at_surface {
                        let color = lookup.get(&owner)?.color;
                        let offset =
                            (layer * 6 + face_index) * face_size + (y * resolution + x) as usize;
                        texels[offset] = [depth, color.x, color.y, color.z];
                        owners[offset] = Some(owner);
                        if matches!(start, BoxCaptureStart::OutsideBounds) {
                            let step = 0.002;
                            let mut gradient = Vec3::ZERO;
                            for direction in 0..3 {
                                let mut delta = Vec3::ZERO;
                                delta[direction] = step;
                                let positive = sampler
                                    .sample(world.transform_point3(origin + inward * depth + delta))
                                    .0;
                                let negative = sampler
                                    .sample(world.transform_point3(origin + inward * depth - delta))
                                    .0;
                                gradient[direction] = positive - negative;
                            }
                            normals[offset] = if gradient.length_squared() > 0.00000001 {
                                gradient.normalize()
                            } else {
                                -inward
                            };
                        }
                        layer += 1;
                        if layer == layers {
                            break;
                        }
                        depth += 0.004;
                        continue;
                    }
                    depth += (distance.abs() / world_step * march_factor).max(0.0001);
                    if depth > span {
                        break;
                    }
                }
            }
        }
        if layers > 1 {
            continue;
        }
        let face_start = face_index * resolution as usize * resolution as usize;
        let filled: Vec<_> = (0..resolution)
            .flat_map(|y| (0..resolution).map(move |x| (x, y)))
            .filter(|(x, y)| texels[face_start + (y * resolution + x) as usize][0] >= 0.0)
            .collect();
        if filled.is_empty() {
            return None;
        }
        let texel_width = (local_max[u_axis] - local_min[u_axis]) / resolution as f32;
        let texel_height = (local_max[v_axis] - local_min[v_axis]) / resolution as f32;
        for y in 0..resolution {
            for x in 0..resolution {
                let sample = &mut texels[face_start + (y * resolution + x) as usize];
                if sample[0] >= 0.0 {
                    continue;
                }
                let nearest = filled
                    .iter()
                    .map(|(hit_x, hit_y)| {
                        let dx = (x.abs_diff(*hit_x).saturating_sub(1) as f32) * texel_width;
                        let dy = (y.abs_diff(*hit_y).saturating_sub(1) as f32) * texel_height;
                        dx.hypot(dy)
                    })
                    .fold(f32::INFINITY, f32::min);
                sample[0] = -nearest.max(0.003);
            }
        }
    }
    if !owners.iter().any(Option::is_some) {
        return None;
    }
    Some(BoxDepthAtlas {
        local_min,
        local_max,
        resolution,
        layers,
        texels,
        owners,
        normals,
    })
}

use std::collections::HashMap;

use glam::Vec3;

use super::*;
use crate::model::{lattice_bounds, lattice_world_matrix, PreparedSubtreeSampler};

pub(super) const SPHERE_DEPTH_WIDTH: u32 = 80;
pub(super) const SPHERE_DEPTH_HEIGHT: u32 = 40;

pub(super) use crate::model::SavedSphereDepthAtlas as SphereDepthAtlas;

pub(super) fn bake_sphere_depth_atlas(
    scene: &[SdfObject],
    root: uuid::Uuid,
    width: u32,
    height: u32,
) -> Option<SphereDepthAtlas> {
    if !(16..=256).contains(&width) || !(8..=128).contains(&height) {
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
    let radius = ((maximum - minimum) * 0.5).length();
    if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let mut texels = vec![[-0.003, 0.0, 0.0, 0.0]; (width * height) as usize];
    let mut owners = vec![None; texels.len()];
    let mut hits = 0;
    for y in 0..height {
        let polar = std::f32::consts::PI * (y as f32 + 0.5) / height as f32;
        for x in 0..width {
            let azimuth = std::f32::consts::TAU * ((x as f32 + 0.5) / width as f32 - 0.5);
            let direction = Vec3::new(
                polar.sin() * azimuth.cos(),
                polar.cos(),
                polar.sin() * azimuth.sin(),
            );
            let inward = -direction;
            let world_step = world.transform_vector3(inward).length();
            if !world_step.is_finite() || world_step <= 0.000001 {
                return None;
            }
            let mut depth = 0.0;
            for _ in 0..96 {
                let point = world.transform_point3(center + direction * (radius - depth));
                let (distance, owner) = sampler.sample(point);
                if !distance.is_finite() {
                    return None;
                }
                if distance < 0.001 {
                    let color = lookup.get(&owner)?.color;
                    let offset = (y * width + x) as usize;
                    texels[offset] = [depth, color.x, color.y, color.z];
                    owners[offset] = Some(owner);
                    hits += 1;
                    break;
                }
                depth += (distance / world_step * march_factor).max(0.0001);
                if depth > radius {
                    break;
                }
            }
        }
    }
    if hits == 0 {
        return None;
    }
    // Empty rays carry a conservative angular distance to the nearest hit.
    let filled: Vec<_> = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|(x, y)| owners[(y * width + x) as usize].is_some())
        .collect();
    for y in 0..height {
        for x in 0..width {
            let sample = &mut texels[(y * width + x) as usize];
            if sample[0] >= 0.0 {
                continue;
            }
            let nearest = filled
                .iter()
                .map(|(hit_x, hit_y)| {
                    let dx = x
                        .abs_diff(*hit_x)
                        .min(width - x.abs_diff(*hit_x))
                        .saturating_sub(1) as f32;
                    let dy = y.abs_diff(*hit_y).saturating_sub(1) as f32;
                    let latitude = std::f32::consts::PI * (y as f32 + 0.5) / height as f32;
                    radius
                        * ((dx * std::f32::consts::TAU / width as f32 * latitude.sin())
                            .hypot(dy * std::f32::consts::PI / height as f32))
                })
                .fold(f32::INFINITY, f32::min);
            sample[0] = -nearest.max(0.003);
        }
    }
    Some(SphereDepthAtlas {
        radius,
        width,
        height,
        texels,
        owners,
    })
}

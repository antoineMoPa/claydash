use glam::Vec3;

#[cfg(test)]
use super::MAX_BOX_DEPTH_TEXELS;

#[derive(Clone, Copy)]
pub(super) struct SplatBound {
    pub center: Vec3,
    pub radius: f32,
    pub texel_offset: u32,
}

#[derive(Clone, Copy)]
struct SplatNode {
    minimum: Vec3,
    maximum: Vec3,
    texel_offset: Option<u32>,
    skip: usize,
}

fn build_nodes(bounds: &mut [SplatBound], nodes: &mut Vec<SplatNode>) {
    let minimum = bounds.iter().fold(Vec3::splat(f32::INFINITY), |lo, bound| {
        lo.min(bound.center - Vec3::splat(bound.radius))
    });
    let maximum = bounds
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |hi, bound| {
            hi.max(bound.center + Vec3::splat(bound.radius))
        });
    let index = nodes.len();
    nodes.push(SplatNode {
        minimum,
        maximum,
        texel_offset: None,
        skip: 0,
    });
    if bounds.len() == 1 {
        nodes[index].texel_offset = Some(bounds[0].texel_offset);
    } else {
        let extent = maximum - minimum;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        let middle = bounds.len() / 2;
        bounds.select_nth_unstable_by(middle, |a, b| a.center[axis].total_cmp(&b.center[axis]));
        let (left, right) = bounds.split_at_mut(middle);
        build_nodes(left, nodes);
        build_nodes(right, nodes);
    }
    nodes[index].skip = nodes.len();
}

/// Appends stackless nodes as two vec4 records per node. Offsets point into
/// the shared capture buffer, including its preceding splat records.
#[cfg(test)]
pub(super) fn append_splat_bvh(
    texels: &mut Vec<[f32; 4]>,
    bounds: &mut [SplatBound],
) -> Option<(u32, u32)> {
    append_splat_bvh_with_budget(texels, bounds, MAX_BOX_DEPTH_TEXELS)
}

pub(super) fn append_splat_bvh_with_budget(
    texels: &mut Vec<[f32; 4]>,
    bounds: &mut [SplatBound],
    budget: usize,
) -> Option<(u32, u32)> {
    if bounds.is_empty() || texels.len() + (bounds.len() * 2 - 1) * 2 > budget {
        return None;
    }
    let start = texels.len() as u32;
    let mut nodes = Vec::with_capacity(bounds.len() * 2 - 1);
    build_nodes(bounds, &mut nodes);
    for node in nodes {
        texels.push([
            node.minimum.x,
            node.minimum.y,
            node.minimum.z,
            node.texel_offset.map_or(-1.0, |offset| offset as f32),
        ]);
        texels.push([
            node.maximum.x,
            node.maximum.y,
            node.maximum.z,
            (start + node.skip as u32 * 2) as f32,
        ]);
    }
    Some((start, texels.len() as u32))
}

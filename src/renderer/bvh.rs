use super::*;

pub(super) fn build_bvh(bounds: &mut [ObjectBound]) -> Vec<GpuBvhNode> {
    let mut nodes = Vec::with_capacity(bounds.len().saturating_mul(2).saturating_sub(1));
    if !bounds.is_empty() {
        build_bvh_subtree(bounds, &mut nodes);
    }
    nodes
}

// A top-level BVH leaf covers a whole Boolean component. Flat hard unions can
// also use a point-query tree over their operands, avoiding a linear scan for
// every ray-march and contact-shading sample. Only use operands whose uploaded
// sphere is a conservative distance bound; other groups keep the linear path.
pub(super) fn append_operand_bvhs(
    nodes: &mut Vec<GpuBvhNode>,
    objects: &mut [GpuObject],
    starts: &[u32],
) {
    for root in 0..objects.len() {
        let parent = objects[root];
        if parent.meta[3] != FLAT_UNION_ROOT
            || parent.repeat_count[3] != 0
            || parent.mirror_axes[..3].iter().any(|&axis| axis != 0)
        {
            continue;
        }
        let start = starts[root] as usize;
        let children = &objects[start..root];
        // A handful of operands is cheaper to scan directly than to traverse
        // another tree at every distance sample.
        let mut bounds: Vec<_> = children
            .iter()
            .enumerate()
            // Bounds and queries share the parent's undeformed world space.
            // Other cages and primitives without a distance bound are scanned
            // separately; one such operand must not disable the entire tree.
            .filter(|(_, child)| {
                child.modifier[0] == parent.modifier[0]
                    && child.distance_bound[3].is_finite()
                    && child.distance_bound[3] > 0.0
            })
            .map(|(offset, child)| {
                let radius = child.distance_bound[3];
                ObjectBound {
                    center: Vec3::new(
                        child.distance_bound[0],
                        child.distance_bound[1],
                        child.distance_bound[2],
                    ),
                    radius,
                    object_index: (start + offset) as u32,
                    half_extent: Vec3::splat(radius),
                }
            })
            .collect();
        if bounds.len() < 8 {
            continue;
        }
        // For cheap undeformed primitives the extra exception scan costs more
        // than it saves. Keep their existing linear path when the tree cannot
        // cover every operand; shared cage evaluations justify a partial tree.
        if parent.modifier[0] == 0 && bounds.len() != children.len() {
            continue;
        }
        let offset = nodes.len() as u32;
        let mut tree = build_bvh(&mut bounds);
        for node in &mut tree {
            node.metadata[1] += offset;
        }
        nodes.extend(tree);
        objects[root].operand_tree = [offset, nodes.len() as u32, 0, 0];
    }
}

pub(super) fn boolean_postorder(objects: &[SdfObject]) -> Vec<&SdfObject> {
    let indices: std::collections::HashMap<_, _> = objects
        .iter()
        .enumerate()
        .map(|(index, object)| (object.uuid, index))
        .collect();
    let mut children = vec![Vec::new(); objects.len()];
    let mut roots = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        if let Some(parent) = object
            .boolean_parent
            .and_then(|id| indices.get(&id).copied())
        {
            children[parent].push(index);
        } else {
            roots.push(index);
        }
    }
    let mut visited = vec![false; objects.len()];
    let mut result = Vec::with_capacity(objects.len());
    let mut pending = Vec::new();
    for root in roots.into_iter().chain(0..objects.len()) {
        pending.push((root, false));
        while let Some((index, complete)) = pending.pop() {
            if complete {
                result.push(&objects[index]);
                continue;
            }
            if visited[index] {
                continue;
            }
            visited[index] = true;
            pending.push((index, true));
            pending.extend(children[index].iter().rev().map(|&child| (child, false)));
        }
    }
    result
}

// World transforms are already baked into each operand. A component made only
// of hard unions has the same distance as the minimum of its primitive SDFs,
// regardless of how the editable Boolean groups are nested. Reparenting the
// GPU copy lets the existing flat-union BVH skip distant operands. Keep the
// source tree intact for editing and serialization.
pub(super) fn flatten_nested_hard_unions(objects: &mut [GpuObject]) {
    let mut has_children = vec![false; objects.len()];
    for object in objects.iter() {
        if object.meta[3] >= 0 {
            has_children[object.meta[3] as usize] = true;
        }
    }
    let mut start = 0;
    for root in 0..objects.len() {
        if objects[root].meta[3] >= 0 {
            continue;
        }
        if root > start
            && objects[start..=root]
                .iter()
                .enumerate()
                .all(|(offset, object)| {
                    let index = start + offset;
                    object.meta[2] == 0
                        && (!has_children[index] || object.component[2] == 0)
                        && object.repeat_count[3] == 0
                        && object.mirror_axes[..3].iter().all(|&axis| axis == 0)
                })
        {
            for object in &mut objects[start..root] {
                object.meta[3] = root as i32;
            }
        }
        start = root + 1;
    }
}

pub(super) fn inverse_affine_rows(inverse: glam::Mat4) -> [[f32; 4]; 3] {
    let columns = inverse.to_cols_array_2d();
    [
        [columns[0][0], columns[1][0], columns[2][0], columns[3][0]],
        [columns[0][1], columns[1][1], columns[2][1], columns[3][1]],
        [columns[0][2], columns[1][2], columns[2][2], columns[3][2]],
    ]
}

fn build_bvh_subtree(bounds: &mut [ObjectBound], nodes: &mut Vec<GpuBvhNode>) {
    let node_index = nodes.len();
    let (center, radius) = enclosing_bound(bounds);
    let (minimum, maximum) = enclosing_aabb(bounds);
    nodes.push(GpuBvhNode {
        center_radius: center.extend(radius).to_array(),
        metadata: [BVH_LEAF, 0, 0, 0],
        aabb_min: minimum.extend(0.0).to_array(),
        aabb_max: maximum.extend(0.0).to_array(),
    });

    if bounds.len() == 1 {
        nodes[node_index].metadata = [bounds[0].object_index, (node_index + 1) as u32, 0, 0];
        return;
    }

    let centroid_min = bounds
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |value, bound| {
            value.min(bound.center)
        });
    let centroid_max = bounds
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |value, bound| {
            value.max(bound.center)
        });
    let extent = centroid_max - centroid_min;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    bounds.sort_unstable_by(|left, right| left.center[axis].total_cmp(&right.center[axis]));
    let middle = bounds.len() / 2;
    let (left, right) = bounds.split_at_mut(middle);
    build_bvh_subtree(left, nodes);
    build_bvh_subtree(right, nodes);
    nodes[node_index].metadata[1] = nodes.len() as u32;
}

pub(super) fn enclosing_aabb(bounds: &[ObjectBound]) -> (Vec3, Vec3) {
    bounds.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), bound| {
            (
                lo.min(bound.center - bound.half_extent),
                hi.max(bound.center + bound.half_extent),
            )
        },
    )
}

fn enclosing_bound(bounds: &[ObjectBound]) -> (Vec3, f32) {
    let minimum = bounds
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |value, bound| {
            value.min(bound.center - Vec3::splat(bound.radius))
        });
    let maximum = bounds
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |value, bound| {
            value.max(bound.center + Vec3::splat(bound.radius))
        });
    let center = (minimum + maximum) * 0.5;
    let radius = bounds
        .iter()
        .map(|bound| center.distance(bound.center) + bound.radius)
        .fold(0.0, f32::max);
    (center, radius)
}

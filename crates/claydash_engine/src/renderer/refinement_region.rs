//! Project conservative ray-traversal bounds so empty screen space is cheap.
use super::*;

pub(super) fn projected_region(camera: &Camera, bounds: Option<([f32;3],[f32;3])>, size: [u32;2]) -> [u32;4] {
    let full = [0,0,size[0],size[1]];
    let Some((low, high)) = bounds else { return full; };
    let low = Vec3::from(low) - Vec3::splat(0.01);
    let high = Vec3::from(high) + Vec3::splat(0.01);
    let matrix = camera.projection() * camera.view();
    let mut minimum = glam::Vec2::splat(f32::INFINITY);
    let mut maximum = glam::Vec2::splat(f32::NEG_INFINITY);
    for x in [low.x,high.x] { for y in [low.y,high.y] { for z in [low.z,high.z] {
        let clip = matrix * Vec3::new(x,y,z).extend(1.0);
        // A bound crossing the near plane can project outside its corners.
        if !clip.is_finite() || clip.w <= 0.0 || clip.z <= 0.0 { return full; }
        let pixel = (glam::Vec2::new(clip.x,-clip.y) / clip.w * 0.5 + glam::Vec2::splat(0.5)) * glam::Vec2::new(size[0] as f32,size[1] as f32);
        minimum = minimum.min(pixel); maximum = maximum.max(pixel);
    } } }
    let start = (minimum.floor() - glam::Vec2::splat(2.0)).max(glam::Vec2::ZERO);
    let end = (maximum.ceil() + glam::Vec2::splat(2.0)).min(glam::Vec2::new(size[0] as f32,size[1] as f32));
    // An object entirely outside the view has no SDF pixels to refine. Keep
    // one harmless sample because zero extent means "unbounded" to the GPU.
    if end.x <= start.x || end.y <= start.y { return [0, 0, 1, 1]; }
    [start.x as u32,start.y as u32,(end.x-start.x) as u32,(end.y-start.y) as u32]
}

pub(super) fn retained_scene_bounds(objects: &[&SdfObject], scene: &[SdfObject], nodes: &[GpuBvhNode], starts: &[u32], gpu: &[GpuObject]) -> Option<([f32;3],[f32;3])> {
    let mut minimum=Vec3::splat(f32::INFINITY); let mut maximum=Vec3::splat(f32::NEG_INFINITY);
    for node in nodes.iter().filter(|node| node.metadata[0]!=BVH_LEAF) {
        let index=node.metadata[0] as usize;
        let mut low=Vec3::new(node.aabb_min[0],node.aabb_min[1],node.aabb_min[2]);
        let mut high=Vec3::new(node.aabb_max[0],node.aabb_max[1],node.aabb_max[2]);
        let object=objects[index];
        if starts[index] == index as u32 && gpu[index].meta[1]==13 && gpu[index].modifier[0]==0
            && object.mirror.is_none() && !object.repetition.enabled && object.path_extrusion.is_none() {
            if let SdfParams::SheetPanelParams(panel)=&object.params {
                if let Some((local_low,local_high))=panel.clipped_local_bounds() {
                    let matrix=crate::model::object_world_matrix(scene,object.uuid);
                    let margin=Vec3::splat(0.015/panel.emboss_metric_scale().max(0.05));
                    low=Vec3::splat(f32::INFINITY); high=Vec3::splat(f32::NEG_INFINITY);
                    for x in [local_low.x-margin.x,local_high.x+margin.x] {
                        for y in [local_low.y-margin.y,local_high.y+margin.y] {
                            for z in [local_low.z-margin.z,local_high.z+margin.z] {
                                let p=matrix.transform_point3(Vec3::new(x,y,z)); low=low.min(p); high=high.max(p);
                            }
                        }
                    }
                }
            }
        }
        minimum=minimum.min(low); maximum=maximum.max(high);
    }
    minimum.is_finite().then_some((minimum.to_array(),maximum.to_array()))
}


/// A tighter ray rejection tree is independent of the conservative distance
/// bounds. In particular, contact probes must keep querying the original field.
pub(super) fn populate_ray_bounds(
    objects: &[&SdfObject], scene: &[SdfObject], nodes: &mut [GpuBvhNode],
    starts: &[u32], gpu: &[GpuObject],
) {
    for index in (0..nodes.len()).rev() {
        let node = nodes[index];
        let mut low = Vec3::from_array(node.aabb_min[..3].try_into().unwrap()) - Vec3::splat(0.01);
        let mut high = Vec3::from_array(node.aabb_max[..3].try_into().unwrap()) + Vec3::splat(0.01);
        let mut tight = false;
        if node.metadata[0] == BVH_LEAF {
            let left = index + 1;
            let right = nodes[left].metadata[1] as usize;
            low = Vec3::from_array(nodes[left].ray_min[..3].try_into().unwrap())
                .min(Vec3::from_array(nodes[right].ray_min[..3].try_into().unwrap()));
            high = Vec3::from_array(nodes[left].ray_max[..3].try_into().unwrap())
                .max(Vec3::from_array(nodes[right].ray_max[..3].try_into().unwrap()));
            tight = nodes[left].ray_min[3] > 0.0 || nodes[right].ray_min[3] > 0.0;
        } else {
            let owner = node.metadata[0] as usize;
            let object = objects[owner];
            if starts[owner] == owner as u32 && gpu[owner].meta[1] == 13
                && gpu[owner].modifier[0] == 0 && gpu[owner].operand_tree[2] == 0
                && object.mirror.is_none() && !object.repetition.enabled
                && object.surface_inlay.is_none() && object.path_extrusion.is_none() {
                if let SdfParams::SheetPanelParams(panel) = &object.params {
                    let depth = panel.embossed_depth_range();
                    let mut metric = 1.0_f32;
                    for strip in panel.strips() {
                        let r = strip.radius;
                        if r != 0.0 {
                            let gap = if r > 0.0 { r - depth.y } else { r.abs() + depth.x };
                            metric = metric.min((gap / r.abs()).clamp(0.01, 1.0));
                        }
                    }
                    let margin = 0.015 / (gpu[owner].params[3] * panel.emboss_metric_scale() * metric).max(0.000001);
                    if let Some((a,b)) = (margin <= panel.size.max_element()).then(|| panel.clipped_local_bounds_with_margin(margin)).flatten() {
                        let matrix = crate::model::object_world_matrix(scene,object.uuid);
                        let mut aabb_low = Vec3::splat(f32::INFINITY);
                        let mut aabb_high = Vec3::splat(f32::NEG_INFINITY);
                        for x in [a.x,b.x] { for y in [a.y,b.y] { for z in [a.z,b.z] {
                            let p = matrix.transform_point3(Vec3::new(x,y,z));
                            aabb_low = aabb_low.min(p); aabb_high = aabb_high.max(p);
                        } } }
                        if aabb_low.is_finite() && aabb_high.is_finite() {
                            low = low.max(aabb_low - Vec3::splat(0.001));
                            high = high.min(aabb_high + Vec3::splat(0.001));
                            tight = low.cmple(high).all();
                            if !tight {
                                low = Vec3::from_array(node.aabb_min[..3].try_into().unwrap()) - Vec3::splat(0.01);
                                high = Vec3::from_array(node.aabb_max[..3].try_into().unwrap()) + Vec3::splat(0.01);
                            }
                        }
                    }
                }
            }
        }
        nodes[index].ray_min = low.extend(if tight { 1.0 } else { 0.0 }).to_array();
        nodes[index].ray_max = high.extend(0.0).to_array();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offscreen_source_does_not_request_full_viewport_raymarching() {
        let mut camera = Camera::new();
        camera.position = Vec3::new(0.0, 0.0, 3.0);
        camera.target = Vec3::ZERO;
        let rect = projected_region(&camera,
            Some(([1000.0, -0.1, -0.1], [1001.0, 0.1, 0.1])), [160, 100]);
        assert_eq!(rect, [0, 0, 1, 1]);
    }
    #[test]
    fn trimmed_ray_bounds_do_not_replace_distance_bounds_or_modified_components() {
        let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::SheetPanel);
        if let SdfParams::SheetPanelParams(panel) = &mut object.params {
            panel.size = Vec2::splat(2.0);
            panel.stage = crate::model::SheetStage::Cut;
            panel.cuts = vec![
                crate::model::SheetCut::Straight { from: Vec2::new(-2.0,-0.02), to: Vec2::new(2.0,-0.02) },
                crate::model::SheetCut::Straight { from: Vec2::new(2.0,0.02), to: Vec2::new(-2.0,0.02) },
            ];
        }
        let bounds = ObjectBound { center: Vec3::ZERO, half_extent: Vec3::ONE,
            radius: 2.0, object_index: 0 };
        let mut nodes = build_bvh(&mut [bounds]);
        nodes[0].aabb_min[3] = 0.42;
        nodes[0].aabb_max[3] = 0.11;
        let original = nodes[0];
        let mut gpu = GpuObject::zeroed();
        gpu.meta[1] = 13; gpu.params[3] = 1.0;
        populate_ray_bounds(&[&object], &[object.clone()], &mut nodes, &[0], &[gpu]);
        assert_eq!(nodes[0].aabb_min, original.aabb_min);
        assert_eq!(nodes[0].aabb_max, original.aabb_max);
        assert_eq!(nodes[0].ray_min[3], 1.0);
        assert!(nodes[0].ray_min[1] > -0.1 && nodes[0].ray_max[1] < 0.1,
            "thin trimmed gaskets must stop tracing the uncut blank");
        object.repetition.enabled = true;
        populate_ray_bounds(&[&object], &[object.clone()], &mut nodes, &[0], &[gpu]);
        assert_eq!(nodes[0].ray_min[3], 0.0, "unsupported modifiers keep their original bounds");
    }
}

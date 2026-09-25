use super::*;

pub(super) fn mirrored_bound(
    bound: ObjectBound,
    group: glam::Mat4,
    mirror: crate::model::Mirror,
) -> ObjectBound {
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let inverse = group.inverse();
    for corner in 0..8 {
        let position = bound.center
            + bound.half_extent
                * Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
        let local = inverse.transform_point3(position);
        for reflected in 0..8 {
            let mut copy = local;
            for axis in 0..3 {
                if mirror.axes[axis] && reflected & (1 << axis) != 0 {
                    copy[axis] = -copy[axis];
                }
            }
            let world = group.transform_point3(copy);
            minimum = minimum.min(world);
            maximum = maximum.max(world);
        }
    }
    let half_extent = (maximum - minimum) * 0.5;
    ObjectBound {
        center: (minimum + maximum) * 0.5,
        half_extent,
        radius: half_extent.length(),
        ..bound
    }
}

// Smooth unions lower distance by at most k/4 per operand. Softness belongs to
// the parent group, so every union edge reads the same value as evaluation.
pub(super) fn expand_soft_bounds(objects: &[GpuObject], bounds: &mut [ObjectBound]) {
    let mut start = 0;
    for (root, object) in objects.iter().enumerate() {
        if object.meta[3] >= 0 {
            continue;
        }
        let group = &objects[start..=root];
        let blend: f32 = group
            .iter()
            .filter_map(|o| {
                (o.meta[3] >= 0 && o.meta[2] == 0).then(|| {
                    f32::from_bits(objects[o.meta[3] as usize].component[2]).max(0.0) * 0.25
                })
            })
            .sum();
        if blend > 0.0 {
            let ratio = group
                .iter()
                .map(|o| {
                    let smallest_inverse_scale = o
                        .inverse_rows
                        .iter()
                        .map(|row| Vec3::new(row[0], row[1], row[2]).length())
                        .fold(f32::INFINITY, f32::min);
                    1.0 / (smallest_inverse_scale * o.params[3]).max(0.000001)
                })
                .fold(1.0_f32, f32::max);
            for bound in &mut bounds[start..=root] {
                bound.half_extent += Vec3::splat(blend * ratio);
                bound.radius = bound.half_extent.length();
            }
        }
        start = root + 1;
    }
}

pub(super) fn boolean_component_bounds(
    objects: &[GpuObject],
    bounds: &[ObjectBound],
) -> Vec<Option<ObjectBound>> {
    let mut result: Vec<_> = bounds.iter().copied().map(Some).collect();
    for (index, object) in objects.iter().enumerate() {
        let parent = object.meta[3];
        if parent < 0 {
            continue;
        }
        let parent = parent as usize;
        let child = result[index];
        let target = result[parent];
        result[parent] = match object.meta[2] {
            0 => match (target, child) {
                (Some(a), Some(b)) => {
                    let (minimum, maximum) = enclosing_aabb(&[a, b]);
                    let half_extent = (maximum - minimum) * 0.5;
                    Some(ObjectBound {
                        center: (minimum + maximum) * 0.5,
                        half_extent,
                        radius: half_extent.length(),
                        object_index: parent as u32,
                    })
                }
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(ObjectBound {
                    object_index: parent as u32,
                    ..b
                }),
                (None, None) => None,
            },
            1 => target, // Subtraction cannot extend the target's occupied volume.
            _ => match (target, child) {
                (Some(a), Some(b)) => {
                    let minimum = (a.center - a.half_extent).max(b.center - b.half_extent);
                    let maximum = (a.center + a.half_extent).min(b.center + b.half_extent);
                    let half_extent = (maximum - minimum) * 0.5;
                    (minimum.cmple(maximum).all()).then_some(ObjectBound {
                        center: (minimum + maximum) * 0.5,
                        half_extent,
                        radius: half_extent.length(),
                        object_index: parent as u32,
                    })
                }
                _ => None,
            },
        };
    }
    result
}

pub(super) fn specialized_shader_source(source: &str, capacity: u32) -> String {
    source.replace(
        "const CSG_SIZE: u32 = 256u;",
        &format!("const CSG_SIZE: u32 = {capacity}u;"),
    )
}

pub(super) fn create_scene_pipeline(
    device: &wgpu::Device,
    shader_source: &str,
    pipeline_layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    use_bvh: bool,
    capacity: u32,
    transparent_background: bool,
) -> wgpu::RenderPipeline {
    let source = specialized_shader_source(shader_source, capacity);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("specialized SDF shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sdf pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[
                    ("USE_BVH", f64::from(use_bvh)),
                    ("HAS_BOOLEANS", f64::from(capacity > 1)),
                    ("TRANSPARENT_BACKGROUND", f64::from(transparent_background)),
                ],
                ..Default::default()
            },
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

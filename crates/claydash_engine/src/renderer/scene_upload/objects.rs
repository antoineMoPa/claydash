//! Pack editable objects into GPU geometry, material references and modifier buffers.
use super::*;

pub(super) struct PackedObjects {
    pub gpu_objects: Vec<GpuObject>,
    pub bounds: Vec<ObjectBound>,
    pub distance_bound_factors: Vec<f32>,
    pub polygon_points: Vec<GpuPolygonPoint>,
    pub host_capacity: u32,
    pub lattice_points: Vec<[f32; 4]>,
    pub lattice_atlas_uploads: Vec<(u32, u32, Vec<Vec3>)>,
    pub modifiers: modifier_gpu::PackedModifiers,
}

impl Renderer {
    pub(super) fn pack_scene_objects(
        &self,
        objects: &[&SdfObject],
        scene_objects: &[SdfObject],
        source_objects: &[SdfObject],
        selected: &[uuid::Uuid],
        prepared_scene: Option<&PreparedGroupScene>,
        atlases: &mut super::super::atlas_upload::UploadedAtlases,
        training_only: bool,
    ) -> PackedObjects {
        let super::super::atlas_upload::UploadedAtlases {
            box_depth_metadata,
            splat_bvh_metadata,
            depth_accelerator_transforms,
            materials,
            stencil_layers,
            ..
        } = atlases;
        let selected_ids = visible_selection(source_objects, scene_objects, selected);
        let scene_lookup: std::collections::HashMap<_, _> = scene_objects
            .iter()
            .map(|object| (object.uuid, object))
            .collect();
        let object_indices: std::collections::HashMap<_, _> = objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.uuid, index as i32))
            .collect();

        let mut bounds = Vec::with_capacity(objects.len().min(MAX_OBJECTS));
        let mut distance_bound_factors = Vec::with_capacity(objects.len().min(MAX_OBJECTS));
        let mut polygon_points = Vec::new();
        let inlay_hosts = super::super::inlay_hosts::pack(objects, &mut polygon_points);
        let mut text_points_used = 0usize;
        let mut lattice_points: Vec<[f32; 4]> = Vec::new();
        let mut lattice_atlas_uploads = Vec::new();
        let mut modifiers = modifier_gpu::PackedModifiers::default();
        let mut lattice_slots = std::collections::HashMap::new();
        let gpu_objects: Vec<GpuObject> = objects
            .iter()
            .take(MAX_OBJECTS)
            .enumerate()
            .map(|(index, object)| {
                let mut ancestor = Some(object.uuid);
                let mut cage = None;
                for _ in 0..scene_objects.len() {
                    let Some(id) = ancestor else {
                        break;
                    };
                    let Some(candidate) = scene_lookup.get(&id).copied() else {
                        break;
                    };
                    if let Some(lattice) = &candidate.lattice {
                        let n = lattice.resolution as usize;
                        if (2..=9).contains(&n) && lattice.offsets.len() == n * n * n {
                            cage = Some((id, lattice));
                            break;
                        }
                    }
                    ancestor = candidate.boolean_parent;
                }
                let (modifier_index, march_factor, cage_extent) =
                    if let Some((root, lattice)) = cage {
                        *lattice_slots.entry(root).or_insert_with(|| {
                            if lattice.offsets.iter().all(|offset| *offset == Vec3::ZERO) {
                                return (0, 0.8, Vec3::ZERO);
                            }
                            let effective_offsets = lattice.effective_offsets();
                            let control_offset = lattice_points.len() as u32;
                            lattice_points.extend(
                                effective_offsets
                                    .iter()
                                    .map(|point| point.extend(0.0).to_array()),
                            );
                            let (tile, inverse_min, inverse_max) = if lattice.resolution <= 3 {
                                let tile = lattice_atlas_uploads.len() as u32;
                                let (inverse_points, inverse_min, inverse_max) =
                                    modifier_gpu::inverse_lattice_grid(lattice, &effective_offsets);
                                lattice_atlas_uploads.push((
                                    tile,
                                    modifier_gpu::INVERSE_GRID_RESOLUTION as u32,
                                    inverse_points,
                                ));
                                (tile, inverse_min, inverse_max)
                            } else {
                                let tile = lattice_atlas_uploads.len() as u32;
                                lattice_atlas_uploads.push((
                                    tile,
                                    lattice.resolution as u32,
                                    effective_offsets.clone(),
                                ));
                                (tile, lattice.min, lattice.max)
                            };
                            let forward = crate::model::lattice_world_matrix(scene_objects, root);
                            let march_factor = modifier_gpu::lattice_march_factor(
                                lattice,
                                &effective_offsets,
                                forward,
                            );
                            let maximum = lattice
                                .offsets
                                .iter()
                                .fold(Vec3::ZERO, |maximum, offset| maximum.max(offset.abs()));
                            let world_extent = forward.x_axis.truncate().abs() * maximum.x
                                + forward.y_axis.truncate().abs() * maximum.y
                                + forward.z_axis.truncate().abs() * maximum.z;
                            let index = modifiers.insert_lattice(
                                inverse_affine_rows(forward.inverse()),
                                inverse_affine_rows(forward),
                                inverse_min,
                                inverse_max,
                                tile,
                                if lattice.resolution <= 3 {
                                    modifier_gpu::INVERSE_GRID_RESOLUTION as u32
                                } else {
                                    lattice.resolution as u32
                                },
                                control_offset,
                                lattice.resolution as u32,
                                lattice.min,
                                lattice.max,
                            );
                            (index, march_factor, world_extent)
                        })
                    } else {
                        (0, 0.8, Vec3::ZERO)
                    };
                let matrix = crate::model::object_world_matrix(scene_objects, object.uuid);
                let group_matrix = crate::model::group_world_matrix(scene_objects, object.uuid);
                let repeats_group = object.repetition.enabled
                    && scene_objects
                        .iter()
                        .any(|child| child.boolean_parent == Some(object.uuid));
                let abs_scale = Vec3::new(
                    matrix.x_axis.truncate().length(),
                    matrix.y_axis.truncate().length(),
                    matrix.z_axis.truncate().length(),
                );
                let distance_scale = abs_scale.min_element().max(0.000_001);
                let super::super::primitive_upload::PackedPrimitive {
                    params,
                    radius,
                    path_profile_extent,
                    path_radius,
                    path_profile_count,
                    text_geometry,
                } = super::super::primitive_upload::pack_primitive(
                    object,
                    scene_objects,
                    source_objects,
                    abs_scale,
                    distance_scale,
                    &mut polygon_points,
                    &mut text_points_used,
                );
                let inlay_host = object.surface_inlay.and_then(|inlay| {
                    object_indices
                        .get(&inlay.host)
                        .copied()
                        .map(|host| (host, inlay))
                });
                let inlay_parameters = inlay_host.map(|(host, inlay)| {
                    let offset = polygon_points.len() as u32;
                    polygon_points.push(GpuPolygonPoint {
                        position: [inlay.offset, inlay.thickness],
                    });
                    polygon_points.push(GpuPolygonPoint {
                        position: [
                            f32::from_bits(inlay_hosts.starts[host as usize]),
                            f32::from_bits(inlay_hosts.parents_offset),
                        ],
                    });
                    offset
                });
                let radial = match object.repetition.mode {
                    crate::model::RepetitionMode::Radial { axis, count, pivot }
                        if object.repetition.enabled =>
                    {
                        Some((axis, count.clamp(1, 32), pivot))
                    }
                    _ => None,
                };
                let mut repeated_radius =
                    if object.repetition.enabled && !repeats_group && radial.is_none() {
                        let extent = Vec3::from_array([
                            if object.repetition.count[0] > 1 {
                                (object.repetition.count[0].saturating_sub(1)) as f32
                                    * object.repetition.spacing.x
                                    * 0.5
                            } else {
                                0.0
                            },
                            if object.repetition.count[1] > 1 {
                                (object.repetition.count[1].saturating_sub(1)) as f32
                                    * object.repetition.spacing.y
                                    * 0.5
                            } else {
                                0.0
                            },
                            if object.repetition.count[2] > 1 {
                                (object.repetition.count[2].saturating_sub(1)) as f32
                                    * object.repetition.spacing.z
                                    * 0.5
                            } else {
                                0.0
                            },
                        ]);
                        radius + extent.length() * abs_scale.max_element()
                    } else {
                        radius
                    };
                let local_extent = match object.params {
                    SdfParams::SphereParams(ref p) => Vec3::splat(p.radius),
                    SdfParams::BoxParams(ref p) => p.box_q,
                    SdfParams::CylinderParams {
                        radius,
                        half_height,
                    } => Vec3::new(radius, half_height, radius),
                    SdfParams::TorusParams {
                        major_radius,
                        minor_radius,
                    } => Vec3::new(
                        major_radius + minor_radius,
                        minor_radius,
                        major_radius + minor_radius,
                    ),
                    SdfParams::PolygonPrismParams(ref polygon) => {
                        let planar = polygon
                            .vertices
                            .iter()
                            .fold(Vec2::ZERO, |extent, point| extent.max(point.abs()));
                        Vec3::new(
                            planar.x + polygon.edge_softness.max(0.0).min(polygon.half_depth),
                            planar.y + polygon.edge_softness.max(0.0).min(polygon.half_depth),
                            polygon.half_depth,
                        )
                    }
                    SdfParams::LoftParams(ref loft) => loft.local_extent(),
                    SdfParams::BezierCurveParams(ref curve) => {
                        curve.local_extent(path_radius * path_profile_extent)
                    }
                    SdfParams::TextParams(_) => text_geometry
                        .as_ref()
                        .map_or(Vec3::ZERO, |text| text.local_extent()),
                };
                if let Some((_, _, pivot)) = radial.filter(|_| !repeats_group) {
                    repeated_radius = radial_bound_radius(local_extent, pivot, abs_scale);
                }
                let mut repeated_extent = local_extent;
                if let Some((_, _, pivot)) = radial.filter(|_| !repeats_group) {
                    repeated_extent = Vec3::splat(local_extent.length() + 2.0 * pivot.length());
                } else if object.repetition.enabled && !repeats_group {
                    for axis in 0..3 {
                        if object.repetition.count[axis] > 1 {
                            repeated_extent[axis] += object.repetition.count[axis].saturating_sub(1)
                                as f32
                                * object.repetition.spacing[axis].max(0.001)
                                * 0.5;
                        }
                    }
                }
                let half_extent = matrix.x_axis.truncate().abs() * repeated_extent.x
                    + matrix.y_axis.truncate().abs() * repeated_extent.y
                    + matrix.z_axis.truncate().abs() * repeated_extent.z
                    + cage_extent;
                let bound = ObjectBound {
                    half_extent,
                    center: matrix.transform_point3(Vec3::ZERO),
                    radius: repeated_radius + cage_extent.length(),
                    object_index: index as u32,
                };
                bounds.push(bound);
                // A nonuniform transform scales the local SDF by its shortest
                // axis. Relative to a world-space box, its outside distance
                // can therefore be smaller than the geometric distance.
                let matrix_frobenius = (matrix.x_axis.truncate().length_squared()
                    + matrix.y_axis.truncate().length_squared()
                    + matrix.z_axis.truncate().length_squared())
                .sqrt();
                let simple_distance = matches!(
                    object.params,
                    SdfParams::SphereParams(_)
                        | SdfParams::BoxParams(_)
                        | SdfParams::CylinderParams { .. }
                        | SdfParams::TorusParams { .. }
                        | SdfParams::PolygonPrismParams(_)
                        | SdfParams::TextParams(_)
                ) && cage.is_none()
                    && object.render_representation
                        == crate::model::GroupRenderRepresentation::ExactSdf
                    && !matches!(
                        object.material.kind,
                        crate::model::MaterialKind::Brick | crate::model::MaterialKind::Fabric
                    );
                distance_bound_factors.push(if simple_distance {
                    (distance_scale / matrix_frobenius.max(0.000_001)).min(1.0)
                } else {
                    0.0
                });
                let uniform_scale = (abs_scale.max_element() - abs_scale.min_element())
                    <= abs_scale.max_element() * 0.00001;
                let box_depth = box_depth_metadata.get(&object.uuid).copied();
                let neural_accelerator_offset =
                    if !training_only && depth_accelerator_transforms.contains_key(&object.uuid) {
                        prepared_scene
                            .as_ref()
                            .and_then(|prepared| prepared.neural_fields.get(&object.uuid))
                            .filter(|field| field.raymarch_last_segment)
                            .map(|field| field.distance_offset)
                    } else {
                        None
                    };
                let splat_bvh = splat_bvh_metadata.get(&object.uuid).copied();
                let safe_distance_bound = box_depth.is_none()
                    && uniform_scale
                    && matches!(
                        &object.params,
                        SdfParams::SphereParams(_)
                            | SdfParams::BoxParams(_)
                            | SdfParams::CylinderParams { .. }
                            | SdfParams::TorusParams { .. }
                            | SdfParams::PolygonPrismParams(_)
                    );
                let custom_index = if object.material.kind == MaterialKind::Custom {
                    object
                        .material_id
                        .and_then(|id| {
                            self.custom_material_sources
                                .iter()
                                .position(|(asset_id, _)| *asset_id == id)
                        })
                        .map_or(0, |index| index as u32 + 1)
                } else {
                    0
                };
                let material_index = materials.insert_custom(object.material, custom_index);
                GpuObject {
                    distance_bound: bound
                        .center
                        .extend(if safe_distance_bound {
                            repeated_radius + 0.01
                        } else {
                            -1.0
                        })
                        .to_array(),
                    operand_tree: [
                        0,
                        0,
                        0,
                        if let Some(offset) = neural_accelerator_offset {
                            offset.max(0.1).to_bits()
                        } else if !training_only
                            && depth_accelerator_transforms.contains_key(&object.uuid)
                        {
                            object
                                .depth_accelerator_settings()
                                .filter(|settings| settings.is_valid())
                                .map_or(0, |settings| settings.configurable_epsilon.to_bits())
                        } else {
                            0
                        },
                    ],
                    box_depth_meta: box_depth.map_or_else(
                        || {
                            let winding = match &object.params {
                                SdfParams::PolygonPrismParams(polygon) => {
                                    convex_polygon_winding(&polygon.vertices)
                                }
                                _ => 0,
                            };
                            [0, 0, 0, winding]
                        },
                        |(offset, width, height, _, _, _)| {
                            [
                                offset,
                                width,
                                height,
                                depth_accelerator_transforms
                                    .get(&object.uuid)
                                    .copied()
                                    .unwrap_or_else(|| splat_bvh.map_or(0, |(start, _)| start)),
                            ]
                        },
                    ),
                    box_depth_min: box_depth.map_or([0.0; 4], |(_, _, _, minimum, _, _)| {
                        minimum
                            .extend(splat_bvh.map_or(0.0, |(_, end)| end as f32))
                            .to_array()
                    }),
                    box_depth_max: box_depth.map_or([0.0; 4], |(_, _, _, _, maximum, _)| {
                        maximum.extend(0.0).to_array()
                    }),
                    // Spare component lanes carry blend width and material index.
                    component: [0, 0, object.softness.to_bits(), material_index],
                    scale: abs_scale
                        .extend(match &object.params {
                            SdfParams::BoxParams(box_params) => box_params.corner_radius,
                            SdfParams::PolygonPrismParams(polygon) => polygon.edge_softness,
                            _ => path_profile_count,
                        })
                        .to_array(),
                    meta: [
                        i32::from(selected_ids.contains(&object.uuid)),
                        if object.render_representation.is_depth_accelerator()
                            || neural_accelerator_offset.is_some()
                        {
                            object.object_type
                        } else {
                            box_depth.map_or(object.object_type, |(_, _, _, _, _, kind)| kind)
                        },
                        object.operation.gpu_code(),
                        object
                            .boolean_parent
                            .and_then(|parent| object_indices.get(&parent).copied())
                            .unwrap_or(-1),
                    ],
                    color: object.color.to_array(),
                    stencil_placement: object.image_stencil.as_ref().map_or([0.0; 4], |stencil| {
                        [
                            stencil.size.x,
                            stencil.size.y,
                            stencil.offset.x,
                            stencil.offset.y,
                        ]
                    }),
                    stencil_meta: object.image_stencil.as_ref().map_or([0.0; 4], |stencil| {
                        [
                            stencil_layers
                                .get(&object.uuid)
                                .map_or(0.0, |layer| *layer as f32 + 1.0),
                            stencil.rotation.to_radians(),
                            f32::from(stencil.both_sides),
                            f32::from(stencil.image_plane),
                        ]
                    }),
                    inverse_rows: inverse_affine_rows(matrix.inverse()),
                    group_inverse_rows: inverse_affine_rows(group_matrix.inverse()),
                    params,
                    repeat_spacing: radial
                        .map_or(object.repetition.spacing, |(_, _, pivot)| pivot)
                        .extend(inlay_parameters.map_or(0.0, f32::from_bits))
                        .to_array(),
                    repeat_count: if let Some((axis, count, _)) = radial {
                        [
                            count as i32,
                            axis.gpu_code(),
                            1,
                            if repeats_group { 3 } else { 2 },
                        ]
                    } else if object.repetition.enabled
                        && object.repetition.mode == crate::model::RepetitionMode::Linear
                    {
                        [
                            object.repetition.count[0] as i32,
                            object.repetition.count[1] as i32,
                            object.repetition.count[2] as i32,
                            1,
                        ]
                    } else {
                        [1, 1, 1, 0]
                    },
                    modifier: [
                        modifier_index,
                        (if matches!(object.params, SdfParams::BezierCurveParams(_))
                            && path_profile_count == 0.0
                            && modifier_index == 0
                        {
                            1.0
                        } else if matches!(object.params, SdfParams::BezierCurveParams(_))
                            && path_profile_count != 0.0
                        {
                            march_factor.min(0.5)
                        } else if let SdfParams::LoftParams(loft) = &object.params {
                            march_factor.min(loft.march_factor())
                        } else {
                            if box_depth.is_some() {
                                march_factor.min(0.35)
                            } else {
                                march_factor
                            }
                        })
                        .to_bits(),
                        if modifier_index == 0 {
                            0
                        } else {
                            modifiers.parameter_offset(modifier_index)
                        },
                        if modifier_index == 0 {
                            0
                        } else {
                            modifier_gpu::MODIFIER_LATTICE
                        },
                    ],
                    mirror_axes: {
                        let mut axes = object.mirror.map_or([0; 4], |mirror| {
                            [
                                u32::from(mirror.axes[0]),
                                u32::from(mirror.axes[1]),
                                u32::from(mirror.axes[2]),
                                0,
                            ]
                        });
                        if let SdfParams::BezierCurveParams(curve) = &object.params {
                            axes[3] = curve
                                .segment_count()
                                .min(crate::model::BezierCurveParams::MAX_SEGMENTS)
                                as u32
                                | if curve.closed { 0x8000_0000 } else { 0 };
                        }
                        if !matches!(object.params, SdfParams::BezierCurveParams(_)) {
                            axes[3] = inlay_host.map_or(
                                if object.surface_inlay.is_some() {
                                    u32::MAX
                                } else {
                                    0
                                },
                                |(host, _)| host as u32 + 1,
                            );
                        }
                        axes
                    },
                }
            })
            .collect();
        PackedObjects {
            gpu_objects,
            bounds,
            distance_bound_factors,
            polygon_points,
            host_capacity: inlay_hosts.capacity,
            lattice_points,
            lattice_atlas_uploads,
            modifiers,
        }
    }
}

// The prism interval only applies when every edge is a supporting plane.
// Zero means that the polygon needs the ordinary SDF traversal.
fn convex_polygon_winding(vertices: &[Vec2]) -> u32 {
    if vertices.len() < 3 || vertices.len() > crate::model::MAX_POLYGON_PRISM_VERTICES {
        return 0;
    }
    let mut winding = 0;
    for index in 0..vertices.len() {
        let first = vertices[(index + 1) % vertices.len()] - vertices[index];
        let second =
            vertices[(index + 2) % vertices.len()] - vertices[(index + 1) % vertices.len()];
        if first.length_squared() < 1e-12 || second.length_squared() < 1e-12 {
            return 0;
        }
        let cross = first.perp_dot(second);
        if cross.abs() < 1e-7 * first.length() * second.length() {
            return 0;
        }
        let edge_winding = if cross > 0.0 { 1 } else { 2 };
        if winding != 0 && winding != edge_winding {
            return 0;
        }
        for point in vertices {
            let side = first.perp_dot(*point - vertices[index]);
            if (edge_winding == 1 && side < -1e-6 * first.length())
                || (edge_winding == 2 && side > 1e-6 * first.length())
            {
                return 0;
            }
        }
        winding = edge_winding;
    }
    winding
}

// Frobenius norm bounds the transform in every rotated direction, including shear.
fn radial_bound_radius(extent: Vec3, pivot: Vec3, column_lengths: Vec3) -> f32 {
    (extent.length() + 2.0 * pivot.length()) * column_lengths.length()
}

#[cfg(test)]
mod radial_bound_tests {
    use super::*;

    #[test]
    fn radial_sphere_contains_rotated_spokes_with_nonuniform_scale_and_shear() {
        let extent = Vec3::new(4.0, 0.02, 0.02);
        let pivot = Vec3::new(-4.2, 0.0, 0.0);
        for matrix in [
            glam::Mat4::from_scale(Vec3::new(0.1, 8.0, 0.5)),
            glam::Mat4::from_cols(
                Vec3::new(0.1, 0.0, 0.0).extend(0.0),
                Vec3::new(5.0, 8.0, 0.0).extend(0.0),
                Vec3::new(0.0, 0.0, 0.5).extend(0.0),
                Vec3::ZERO.extend(1.0),
            ),
        ] {
            let columns = Vec3::new(
                matrix.x_axis.truncate().length(),
                matrix.y_axis.truncate().length(),
                matrix.z_axis.truncate().length(),
            );
            let radius = radial_bound_radius(extent, pivot, columns);
            for copy in 0..32 {
                let rotation =
                    glam::Quat::from_rotation_z(std::f32::consts::TAU * copy as f32 / 32.0);
                for x in [-1.0, 1.0] {
                    for y in [-1.0, 1.0] {
                        for z in [-1.0, 1.0] {
                            let corner = extent * Vec3::new(x, y, z);
                            let point =
                                matrix.transform_vector3(pivot + rotation * (corner - pivot));
                            assert!(point.length() <= radius);
                        }
                    }
                }
            }
        }
    }
}

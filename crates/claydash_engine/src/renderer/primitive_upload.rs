use super::group_capture::prepared_render_text;
use super::*;

pub(super) struct PackedPrimitive {
    pub params: [f32; 4],
    pub radius: f32,
    pub path_profile_extent: f32,
    pub path_radius: f32,
    pub path_profile_count: f32,
    pub text_geometry: Option<std::sync::Arc<crate::model::PreparedText>>,
}

pub(super) fn pack_primitive(
    object: &SdfObject,
    scene_objects: &[SdfObject],
    source_objects: &[SdfObject],
    abs_scale: Vec3,
    distance_scale: f32,
    polygon_points: &mut Vec<GpuPolygonPoint>,
    text_points_used: &mut usize,
) -> PackedPrimitive {
    let path_profile = object
        .path_extrusion
        .and_then(|modifier| modifier.profile_curve)
        .and_then(|id| crate::model::profile_curve_vertices(scene_objects, id));
    let path_profile_extent = path_profile.as_ref().map_or(1.0_f32, |vertices| {
        vertices
            .iter()
            .map(|point| point.length())
            .fold(1.0_f32, f32::max)
    });
    let path_radius = object
        .path_extrusion
        .map_or(0.0, |modifier| modifier.radius);
    let path_profile_count = if object
        .path_extrusion
        .and_then(|modifier| modifier.profile_curve)
        .is_some()
    {
        path_profile
            .as_ref()
            .map_or(2.0, |vertices| vertices.len().min(65) as f32)
    } else if object
        .path_extrusion
        .is_some_and(|modifier| modifier.profile == crate::model::BezierProfile::Square)
    {
        1.0
    } else {
        0.0
    };
    let text_geometry = if matches!(object.params, SdfParams::TextParams(_)) {
        prepared_render_text(source_objects, object)
    } else {
        None
    };
    let (params, radius) = match object.params {
        SdfParams::SphereParams(ref params) => (
            [params.radius, 0.0, 0.0, distance_scale],
            params.radius * abs_scale.max_element(),
        ),
        SdfParams::BoxParams(ref params) => (
            [
                params.box_q.x,
                params.box_q.y,
                params.box_q.z,
                distance_scale,
            ],
            (params.box_q * abs_scale).length(),
        ),
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => (
            [radius, half_height, 0.0, distance_scale],
            Vec2::new(radius, half_height).length() * abs_scale.max_element(),
        ),
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => (
            [major_radius, minor_radius, 0.0, distance_scale],
            (major_radius + minor_radius) * abs_scale.max_element(),
        ),
        SdfParams::PolygonPrismParams(ref polygon) => {
            let offset = polygon_points.len() as u32;
            polygon_points.extend(
                polygon
                    .vertices
                    .iter()
                    .take(crate::model::MAX_POLYGON_PRISM_VERTICES)
                    .map(|point| GpuPolygonPoint {
                        position: point.to_array(),
                    }),
            );
            let count = polygon_points.len() as u32 - offset;
            let planar_radius = polygon
                .vertices
                .iter()
                .map(|point| point.length())
                .fold(0.0_f32, f32::max);
            (
                [
                    polygon.half_depth,
                    f32::from_bits(offset),
                    f32::from_bits(count),
                    distance_scale,
                ],
                Vec2::new(
                    planar_radius + polygon.edge_softness.max(0.0).min(polygon.half_depth),
                    polygon.half_depth,
                )
                .length()
                    * abs_scale.max_element(),
            )
        }
        SdfParams::LoftParams(ref loft) => {
            let offset = polygon_points.len() as u32;
            let profile_count = loft.profile_count();
            for section in loft
                .sections
                .iter()
                .take(crate::model::LoftParams::MAX_SECTIONS)
            {
                polygon_points.push(GpuPolygonPoint {
                    position: [section.x, section.center_y],
                });
                polygon_points.push(GpuPolygonPoint {
                    position: [section.center_z, section.half_height],
                });
                polygon_points.push(GpuPolygonPoint {
                    position: [section.half_width, 0.0],
                });
                for index in 0..profile_count {
                    polygon_points.push(GpuPolygonPoint {
                        position: crate::model::LoftParams::profile_point(
                            section,
                            index,
                            profile_count,
                        )
                        .to_array(),
                    });
                }
            }
            let count = loft
                .sections
                .len()
                .min(crate::model::LoftParams::MAX_SECTIONS) as u32;
            (
                [
                    f32::from_bits(offset),
                    f32::from_bits(count),
                    f32::from_bits(profile_count as u32),
                    distance_scale,
                ],
                loft.local_extent().length() * abs_scale.max_element(),
            )
        }
        SdfParams::BezierCurveParams(ref curve) => {
            let offset = polygon_points.len() as u32;
            for &point in curve.points.iter().take(25) {
                polygon_points.push(GpuPolygonPoint {
                    position: [point.x, point.y],
                });
                polygon_points.push(GpuPolygonPoint {
                    position: [point.z, 0.0],
                });
            }
            let profile_offset = polygon_points.len() as u32;
            if let Some(vertices) = &path_profile {
                polygon_points.extend(vertices.iter().take(65).map(|point| GpuPolygonPoint {
                    position: point.to_array(),
                }));
            }
            (
                [
                    path_radius,
                    f32::from_bits(offset),
                    f32::from_bits(profile_offset),
                    distance_scale,
                ],
                curve
                    .local_extent(path_radius * path_profile_extent)
                    .length()
                    * abs_scale.max_element(),
            )
        }
        SdfParams::TextParams(_) => {
            let offset = polygon_points.len() as u32;
            let prepared = text_geometry.as_deref();
            let count = prepared.map_or(0, |text| text.glyphs.len());
            let required = prepared.map_or(1, |text| text.packed_points());
            if *text_points_used + required <= crate::model::MAX_SCENE_TEXT_POINTS {
                *text_points_used += required;
                polygon_points.push(GpuPolygonPoint {
                    position: [
                        f32::from_bits(count as u32),
                        prepared.map_or(0.0, |text| text.half_depth),
                    ],
                });
                if let Some(text) = prepared {
                    let mut edge_offset = offset + 1 + (count * 9) as u32;
                    for glyph in &text.glyphs {
                        for point in [
                            [glyph.origin.x, glyph.origin.y],
                            [glyph.origin.z, glyph.x.x],
                            [glyph.x.y, glyph.x.z],
                            [glyph.y.x, glyph.y.y],
                            [glyph.y.z, glyph.z.x],
                            [glyph.z.y, glyph.z.z],
                            glyph.min.to_array(),
                            glyph.max.to_array(),
                            [
                                f32::from_bits(edge_offset),
                                f32::from_bits(glyph.edges.len() as u32),
                            ],
                        ] {
                            polygon_points.push(GpuPolygonPoint { position: point });
                        }
                        edge_offset += (glyph.edges.len() * 2) as u32;
                    }
                    for glyph in &text.glyphs {
                        for edge in &glyph.edges {
                            polygon_points.push(GpuPolygonPoint {
                                position: edge[0].to_array(),
                            });
                            polygon_points.push(GpuPolygonPoint {
                                position: edge[1].to_array(),
                            });
                        }
                    }
                    debug_assert_eq!(polygon_points.len() - offset as usize, required);
                }
            } else {
                polygon_points.push(GpuPolygonPoint {
                    position: [0.0, 0.0],
                });
            }
            (
                [0.0, f32::from_bits(offset), 0.0, distance_scale],
                prepared.map_or(0.0, |text| {
                    text.local_extent().length() * abs_scale.max_element()
                }),
            )
        }
    };

    PackedPrimitive {
        params,
        radius,
        path_profile_extent,
        path_radius,
        path_profile_count,
        text_geometry,
    }
}

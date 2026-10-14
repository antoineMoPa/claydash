//! Tessellate the material domain first, then apply the native forming map.
//! Top and bottom share one planar triangulation; its boundary supplies the walls.
use super::super::poisson_mesh::geometry::Mesh;
use crate::model::{SheetCut, SheetPanelParams, SheetStage};
use geo::{BooleanOps, Coord, LineString, MultiPolygon, Polygon};
use glam::DVec2;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

fn polygon(points: &[DVec2]) -> Polygon<f64> {
    Polygon::new(
        LineString::new(points.iter().map(|p| Coord { x: p.x, y: p.y }).collect()),
        vec![],
    )
}

fn edge(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("mesh export cancelled".into())
    } else {
        Ok(())
    }
}

pub(super) fn build(
    panel: &SheetPanelParams,
    owner: Uuid,
    resolution: u32,
    cancel: &AtomicBool,
) -> Result<Mesh, String> {
    let mut budget = super::super::cooperative_work::WorkerBudget::new();
    check_cancel(cancel)?;
    panel.validate()?;
    let half = panel.size.as_dvec2() * 0.5;
    let mut outline = vec![
        DVec2::new(-half.x, -half.y),
        DVec2::new(half.x, -half.y),
        DVec2::new(half.x, half.y),
        DVec2::new(-half.x, half.y),
    ];
    // Straight cuts retain the directed line's left half-plane. Clipping a
    // convex polygon keeps these exact, without a sampling grid or margin box.
    if panel.stage != SheetStage::Blank {
        for cut in &panel.cuts {
            budget.checkpoint();
            check_cancel(cancel)?;
            if let SheetCut::Straight { from, to } = cut {
                let from = from.as_dvec2();
                let direction = to.as_dvec2() - from;
                let side = |p: DVec2| direction.perp_dot(p - from);
                let previous = std::mem::take(&mut outline);
                for i in 0..previous.len() {
                    let a = previous[i];
                    let b = previous[(i + 1) % previous.len()];
                    let da = side(a);
                    let db = side(b);
                    if da >= 0.0 {
                        outline.push(a);
                    }
                    if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
                        outline.push(a + (b - a) * (da / (da - db)));
                    }
                }
            }
        }
    }
    if outline.len() < 3 {
        return Ok(Mesh::from_parts(vec![], vec![], vec![]));
    }
    let mut domain = MultiPolygon(vec![polygon(&outline)]);
    // Polygon booleans handle edge-crossing arches, overlapping holes, and
    // disconnected remaining islands with the same topology as interior holes.
    let circle_segments = resolution.clamp(24, 256) as usize;
    if panel.stage != SheetStage::Blank {
        for cut in &panel.cuts {
            budget.checkpoint();
            check_cancel(cancel)?;
            if let SheetCut::Circle { center, radius } = cut {
                let center = center.as_dvec2();
                let ring: Vec<_> = (0..circle_segments)
                    .map(|i| {
                        let angle = std::f64::consts::TAU * i as f64 / circle_segments as f64;
                        center + DVec2::new(angle.cos(), angle.sin()) * *radius as f64
                    })
                    .collect();
                domain = domain.difference(&polygon(&ring));
            }
        }
    }
    let mut uv = Vec::new();
    let mut faces = Vec::new();
    let strips = (!panel.uses_directional_forming()).then(|| panel.strips());
    for island in domain.0 {
        budget.checkpoint();
        check_cancel(cancel)?;
        let base = uv.len() as u32;
        let mut coordinates = Vec::new();
        let mut holes = Vec::new();
        for (index, ring) in std::iter::once(island.exterior())
            .chain(island.interiors())
            .enumerate()
        {
            if index > 0 {
                holes.push(coordinates.len() / 2);
            }
            let points = &ring.0;
            let count = points.len() - usize::from(points.first() == points.last());
            for point in &points[..count] {
                uv.push(DVec2::new(point.x, point.y));
                coordinates.extend([point.x, point.y]);
            }
        }
        let indices = earcutr::earcut(&coordinates, &holes, 2)
            .map_err(|error| format!("sheet triangulation failed: {error:?}"))?;
        for triangle in indices.chunks_exact(3) {
            let mut t = [
                base + triangle[0] as u32,
                base + triangle[1] as u32,
                base + triangle[2] as u32,
            ];
            let [a, b, c] = t.map(|i| uv[i as usize]);
            let area = (b - a).perp_dot(c - a);
            if area == 0.0 {
                continue;
            }
            if area < 0.0 {
                t.swap(1, 2);
            }
            faces.push(t);
        }
    }
    if panel.stage == SheetStage::Formed && (panel.has_relief() || !panel.bends.is_empty()) {
        let mut spacing = panel.size.max_element() as f64 / resolution.clamp(8, 256) as f64;
        for bend in &panel.bends {
            spacing = spacing.min(bend.radius as f64 * 0.2);
        }
        for relief in &panel.embosses {
            let crate::model::SheetEmboss::Circle { transition, .. } = relief;
            spacing = spacing.min(*transition as f64 / 3.0);
        }
        for path in &panel.path_embosses {
            for station in &path.stations {
                spacing = spacing.min(station.width as f64 / 3.0);
            }
        }
        let curved_ranges = if !panel.has_relief() {
            strips.as_ref().map(|strips| {
                strips
                    .iter()
                    .filter(|strip| strip.radius != 0.0)
                    .map(|strip| (strip.start as f64, strip.end as f64))
                    .collect::<Vec<_>>()
            })
        } else {
            None
        };
        refine(
            &mut uv,
            &mut faces,
            spacing,
            curved_ranges.as_deref(),
            cancel,
        )?;
    }
    // Earcut can omit redundant collinear ring points. Compact before creating
    // the extrusion so every stored vertex belongs to its closed surface.
    let mut mapping = vec![u32::MAX; uv.len()];
    let mut material = Vec::new();
    for (face, t) in faces.iter_mut().enumerate() {
        if face % 1024 == 0 {
            budget.checkpoint();
            check_cancel(cancel)?;
        }
        for i in t {
            if mapping[*i as usize] == u32::MAX {
                mapping[*i as usize] = material.len() as u32;
                material.push(uv[*i as usize]);
            }
            *i = mapping[*i as usize];
        }
    }
    let count = material.len() as u32;
    let mut positions = Vec::with_capacity(material.len() * 2);
    for depth in [-panel.thickness * 0.5, panel.thickness * 0.5] {
        for (i, point) in material.iter().enumerate() {
            if i % 1024 == 0 {
                budget.checkpoint();
                check_cancel(cancel)?;
            }
            let material_point = point.as_vec2().extend(depth);
            positions.push(match &strips {
                Some(strips) => panel.form_point_with_strips(material_point, strips),
                None => panel.form_point(material_point),
            });
        }
    }
    let mut triangles = Vec::with_capacity(faces.len() * 2);
    let mut edges = BTreeMap::new();
    for (face, [a, b, c]) in faces.into_iter().enumerate() {
        if face % 1024 == 0 {
            budget.checkpoint();
            check_cancel(cancel)?;
        }
        triangles.push([c, b, a]);
        triangles.push([a + count, b + count, c + count]);
        for (a, b) in [(a, b), (b, c), (c, a)] {
            let entry = edges.entry(edge(a, b)).or_insert((0, a, b));
            entry.0 += 1;
        }
    }
    for (index, (_, (uses, a, b))) in edges.into_iter().enumerate() {
        if index % 1024 == 0 {
            budget.checkpoint();
            check_cancel(cancel)?;
        }
        if uses == 1 {
            triangles.extend([[a, b, b + count], [a, b + count, a + count]]);
        } else if uses != 2 {
            return Err("sheet material triangulation has a non-manifold edge".into());
        }
    }
    check_cancel(cancel)?;
    let owners = vec![owner; triangles.len()];
    Ok(Mesh::from_parts(positions, triangles, owners))
}

/// Split every long edge once per round, sharing the midpoint with all its
/// incident faces. The subdivision templates introduce no T junctions.
fn refine(
    points: &mut Vec<DVec2>,
    faces: &mut Vec<[u32; 3]>,
    spacing: f64,
    curved_ranges: Option<&[(f64, f64)]>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut budget = super::super::cooperative_work::WorkerBudget::new();
    for _ in 0..32 {
        check_cancel(cancel)?;
        if faces.len() > 1_000_000 {
            return Err("sheet tessellation exceeds one million material faces; reduce mesh resolution or relief detail".into());
        }
        let mut midpoints = BTreeMap::new();
        for (i, &[a, b, c]) in faces.iter().enumerate() {
            if i % 1024 == 0 {
                budget.checkpoint();
                check_cancel(cancel)?;
            }
            for (a, b) in [(a, b), (b, c), (c, a)] {
                let pa = points[a as usize];
                let pb = points[b as usize];
                // Parallel bends depend only on material X; Y is affine. Flat
                // portions need no refinement, even when their triangles are long.
                // Checking the entire edge interval catches edges crossing a bend
                // with both endpoints outside it, including bend seam crossings.
                let split = if let Some(ranges) = curved_ranges {
                    (pa.x - pb.x).abs() > spacing
                        && ranges
                            .iter()
                            .any(|&(start, end)| pa.x.max(pb.x) > start && pa.x.min(pb.x) < end)
                } else {
                    pa.distance_squared(pb) > spacing * spacing
                };
                if split {
                    midpoints.entry(edge(a, b)).or_insert(0);
                }
            }
        }
        if midpoints.is_empty() {
            return Ok(());
        }
        for (index, (&(a, b), midpoint)) in midpoints.iter_mut().enumerate() {
            if index % 1024 == 0 {
                budget.checkpoint();
                check_cancel(cancel)?;
            }
            *midpoint = points.len() as u32;
            points.push((points[a as usize] + points[b as usize]) * 0.5);
        }
        let previous = std::mem::take(faces);
        for (index, [a, b, c]) in previous.into_iter().enumerate() {
            if index % 1024 == 0 {
                budget.checkpoint();
                check_cancel(cancel)?;
            }
            let ab = midpoints.get(&edge(a, b)).copied();
            let bc = midpoints.get(&edge(b, c)).copied();
            let ca = midpoints.get(&edge(c, a)).copied();
            match (ab, bc, ca) {
                (None, None, None) => faces.push([a, b, c]),
                (Some(m), None, None) => faces.extend([[a, m, c], [m, b, c]]),
                (None, Some(m), None) => faces.extend([[b, m, a], [m, c, a]]),
                (None, None, Some(m)) => faces.extend([[c, m, b], [m, a, b]]),
                (Some(m), Some(n), None) => faces.extend([[b, n, m], [a, m, c], [m, n, c]]),
                (None, Some(m), Some(n)) => faces.extend([[c, n, m], [b, m, a], [m, n, a]]),
                (Some(n), None, Some(m)) => faces.extend([[a, n, m], [c, m, b], [m, n, b]]),
                (Some(m), Some(n), Some(p)) => {
                    faces.extend([[a, m, p], [m, b, n], [p, n, c], [m, n, p]])
                }
            }
        }
    }
    Err("sheet subdivision did not converge".into())
}

#[cfg(test)]
#[path = "sheet/tests.rs"]
mod tests;

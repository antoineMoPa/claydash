//! Tessellate the material domain first, then apply the native forming map.
//! Top and bottom share one planar triangulation; its boundary supplies the walls.
use super::super::poisson_mesh::geometry::Mesh;
use crate::model::{SheetCut, SheetPanelParams, SheetStage};
use geo::{BooleanOps, Coord, LineString, MultiPolygon, Polygon};
use glam::DVec2;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;
mod triangulation;

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

#[cfg(test)]
pub(super) fn build(
    panel: &SheetPanelParams,
    owner: Uuid,
    resolution: u32,
    cancel: &AtomicBool,
) -> Result<Mesh, String> {
    build_with_quality(panel, owner, resolution, super::TessellationQuality::from_resolution(resolution), cancel)
}

pub(super) fn build_with_quality(panel: &SheetPanelParams, owner: Uuid, _resolution: u32,
    quality: super::TessellationQuality, cancel: &AtomicBool) -> Result<Mesh, String> {
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

    if panel.stage != SheetStage::Blank {
        for cut in &panel.cuts {
            budget.checkpoint();
            check_cancel(cancel)?;
            if let SheetCut::Circle { center, radius } = cut {
                let circle_segments = quality.circle_segments(*radius);
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
    triangulation::improve(&mut uv, &mut faces, true, cancel)?;
    if panel.stage == SheetStage::Formed && (panel.has_relief() || !panel.bends.is_empty()) {
        refine(&mut uv, &mut faces, panel, quality, strips.as_deref(), cancel)?;
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

/// Split edges that miss the formed surface, sharing midpoint samples between
/// incident faces. Reconnect their interiors without introducing T junctions.
fn refine(
    points: &mut Vec<DVec2>,
    faces: &mut Vec<[u32; 3]>,
    panel: &SheetPanelParams,
    quality: super::TessellationQuality,
    strips: Option<&[crate::model::SheetStrip]>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut budget = super::super::cooperative_work::WorkerBudget::new();
    for _ in 0..32 {
        check_cancel(cancel)?;
        if faces.len() > 1_000_000 {
            return Err("sheet tessellation exceeds one million material faces; reduce mesh resolution or relief detail".into());
        }
        // Reconnect adaptive samples instead of preserving the initial earcut fan.
        // This also makes new diagonals participate in the surface-error check.
        triangulation::improve(points, faces, false, cancel)?;
        let mut midpoints = BTreeMap::new();
        for (i, &[a, b, c]) in faces.iter().enumerate() {
            if i % 1024 == 0 {
                budget.checkpoint();
                check_cancel(cancel)?;
            }
            let [pa, pb, pc] = [a,b,c].map(|i| points[i as usize]);
            let feature_spacing=feature_spacing(panel,pa.min(pb).min(pc),pa.max(pb).max(pc));
            let center = (pa + pb + pc) / 3.0;
            let interior_error = [-panel.thickness*0.5, panel.thickness*0.5].into_iter().any(|depth| {
                let map = |p| formed(panel, strips, p, depth);
                map(center).distance((map(pa)+map(pb)+map(pc))/3.0) > quality.position_error
            });
            for (a, b) in [(a, b), (b, c), (c, a)] {
                let pa = points[a as usize];
                let pb = points[b as usize];
                let split = interior_error || feature_spacing.is_some_and(|spacing|pa.distance(pb)>spacing) || edge_needs_refinement(panel, strips, pa, pb, quality);
                if split {
                    midpoints.entry(edge(a, b)).or_insert(0);
                }
            }
        }
        if midpoints.is_empty() {
            return Ok(());
        }
        let mut boundary_uses=BTreeMap::new();
        for &[a,b,c] in faces.iter() { for (a,b) in [(a,b),(b,c),(c,a)] { *boundary_uses.entry(edge(a,b)).or_insert(0)+=1; } }
        for (index, (&(a, b), midpoint)) in midpoints.iter_mut().enumerate() {
            if index % 1024 == 0 {
                budget.checkpoint();
                check_cancel(cancel)?;
            }
            *midpoint = points.len() as u32;
            let pa=points[a as usize];let pb=points[b as usize];let mut point=(pa+pb)*0.5;
            let straight=panel.cuts.iter().any(|cut| match cut {
                SheetCut::Straight{from,to}=>{let from=from.as_dvec2();let d=to.as_dvec2()-from;d.perp_dot(pa-from).abs()<1e-10 && d.perp_dot(pb-from).abs()<1e-10}, _=>false
            });
            if boundary_uses.get(&edge(a,b))==Some(&1) && !straight {
                for cut in &panel.cuts {if let SheetCut::Circle{center,radius}=cut {
                    let center=center.as_dvec2();let r=*radius as f64;
                    if (pa.distance(center)-r).abs()<1e-6 && (pb.distance(center)-r).abs()<1e-6 {
                        let d=point-center;point=center+d*(r/d.length());break;
                    }
                }}
            }
            points.push(point);
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

// Sampling alone can miss a small relief entirely inside a large initial triangle.
// Only triangles intersecting its material support receive the feature-size guard.
fn feature_spacing(panel:&SheetPanelParams,min:DVec2,max:DVec2)->Option<f64> {
    let overlap=|lo:DVec2,hi:DVec2| min.x<=hi.x && max.x>=lo.x && min.y<=hi.y && max.y>=lo.y;
    let mut spacing=f64::INFINITY;
    for relief in &panel.embosses {let crate::model::SheetEmboss::Circle{center,radius,transition,..}=relief;
        let center=center.as_dvec2();let span=DVec2::splat((*radius+*transition) as f64);
        if overlap(center-span,center+span){spacing=spacing.min(*transition as f64/3.0);}
    }
    for path in &panel.path_embosses {for pair in path.stations.windows(2){
        let a=pair[0].point.as_dvec2();let b=pair[1].point.as_dvec2();let width=pair[0].width.max(pair[1].width) as f64;
        if overlap(a.min(b)-DVec2::splat(width),a.max(b)+DVec2::splat(width)) {spacing=spacing.min(pair[0].width.min(pair[1].width) as f64/3.0);}
    }}
    spacing.is_finite().then_some(spacing)
}

fn formed(panel: &SheetPanelParams, strips: Option<&[crate::model::SheetStrip]>, p: DVec2, depth: f32) -> glam::Vec3 {
    let p = p.as_vec2().extend(depth);
    match strips { Some(strips) => panel.form_point_with_strips(p, strips), None => panel.form_point(p) }
}
fn edge_needs_refinement(panel: &SheetPanelParams, strips: Option<&[crate::model::SheetStrip]>,
    a: DVec2, b: DVec2, quality: super::TessellationQuality) -> bool {
    for depth in [-panel.thickness*0.5, panel.thickness*0.5] {
        let start=formed(panel,strips,a,depth); let end=formed(panel,strips,b,depth);
        let mut previous=start; let mut direction=None;
        for t in [0.125,0.25,0.5,0.75,0.875,1.0] {
            let p=formed(panel,strips,a.lerp(b,t),depth);
            if p.distance(start.lerp(end,t as f32)) > quality.position_error { return true; }
            let d=p-previous;
            if let Some(first) = direction {
                let first: glam::Vec3=first;
                let length=(d.length_squared()*first.length_squared()).sqrt();
                if length>1e-12 && d.dot(first)<quality.normal_angle.cos()*length { return true; }
            } else if d.length_squared()>1e-12 { direction=Some(d); }
            previous=p;
        }
        // Include bend boundaries: a narrow bend can lie between uniform samples.
        for bend in &panel.bends {
            let axis=DVec2::new(bend.direction.cos() as f64,bend.direction.sin() as f64);
            let da=a.dot(axis); let db=b.dot(axis);
            for x in [bend.start, bend.start+bend.radius*bend.angle.abs()*0.5, bend.start+bend.radius*bend.angle.abs()] {
                let t=(x as f64-da)/(db-da);
                if t>0.0 && t<1.0 && formed(panel,strips,a.lerp(b,t),depth).distance(start.lerp(end,t as f32))>quality.position_error { return true; }
            }
        }
    }
    false
}

#[cfg(test)]
#[path = "sheet/tests.rs"]
mod tests;

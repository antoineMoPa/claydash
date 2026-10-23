use super::{check_cancel, sheet, Mesh};
use crate::model::{BezierProfile, LoftParams, SdfObject, SdfParams};
use glam::{Vec2, Vec3};
use std::{collections::HashMap, sync::atomic::AtomicBool};
use uuid::Uuid;

#[derive(Default)]
struct Surface {
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
}
impl Surface {
    fn vertex(&mut self, p: Vec3) -> u32 {
        let i = self.positions.len() as u32;
        self.positions.push(p);
        i
    }
    fn face(&mut self, a: u32, b: u32, c: u32) {
        let [p, q, r] = [a, b, c].map(|i| self.positions[i as usize]);
        if (q - p).cross(r - p).length_squared() > 0.0 {
            self.triangles.push([a, b, c]);
        }
    }
    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.face(a, b, c);
        self.face(b, d, c);
    }
    fn finish(self, owner: Uuid) -> Mesh {
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        let mut shared = HashMap::new();
        for p in self.positions {
            let key = p.to_array().map(|v| if v == 0.0 { 0 } else { v.to_bits() });
            let i = *shared.entry(key).or_insert_with(|| {
                let i = positions.len() as u32;
                positions.push(p);
                i
            });
            indices.push(i);
        }
        let triangles: Vec<_> = self
            .triangles
            .into_iter()
            .map(|f| f.map(|i| indices[i as usize]))
            .filter(|f| f[0] != f[1] && f[1] != f[2] && f[2] != f[0])
            .collect();
        let owners = vec![owner; triangles.len()];
        Mesh {
            revision: 0,
            positions,
            triangles,
            owners,
            normals: vec![],
            shading: Default::default(),
            colors: vec![],
            uvs: vec![],
            baked_texture: None, sections: Vec::new(), section_caps: false,
        }
    }
    fn append(&mut self, other: Surface) {
        let offset = self.positions.len() as u32;
        self.positions.extend(other.positions);
        self.triangles.extend(
            other
                .triangles
                .into_iter()
                .map(|face| face.map(|v| v + offset)),
        );
    }
}

pub(super) fn build(
    scene: &[SdfObject],
    object: &SdfObject,
    resolution: u32,
    cancel: &AtomicBool,
) -> Result<Mesh, String> {
    build_with_quality(scene, object, resolution,
        super::TessellationQuality::from_resolution(resolution).local(scene, object), cancel)
}

pub(super) fn build_with_quality(scene: &[SdfObject], object: &SdfObject, resolution: u32,
    quality: super::TessellationQuality, cancel: &AtomicBool) -> Result<Mesh, String> {
    check_cancel(cancel)?;
    let n = resolution.clamp(8, 256) as usize;
    let surface = match &object.params {
        SdfParams::SheetPanelParams(panel) => {
            return sheet::build_with_quality(panel, object.uuid, resolution, quality, cancel)
        }
        SdfParams::SphereParams(p) => {
            positive(p.radius, "Sphere radius")?;
            sphere(p.radius, quality.circle_segments(p.radius))
        }
        SdfParams::BoxParams(p) => {
            if !p.box_q.is_finite() || p.box_q.min_element() <= 0.0 {
                return Err("Box dimensions must be positive and finite".into());
            }
            rounded_box(
                p.box_q,
                p.corner_radius.max(0.0).min(p.box_q.min_element()),
                quality.circle_segments(p.corner_radius * 2.0).div_ceil(8).max(1),
            )
        }
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => {
            positive(*radius, "Cylinder radius")?;
            positive(*half_height, "Cylinder half height")?;
            let outline = circle(*radius, quality.circle_segments(*radius));
            let mut surface = extrude(&outline, *half_height)?;
            for point in &mut surface.positions {
                *point = Vec3::new(point.x, point.z, -point.y);
            }
            surface
        }
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => {
            positive(*minor_radius, "Torus minor radius")?;
            if !major_radius.is_finite() || major_radius <= minor_radius {
                return Err(
                    "Direct mesh requires a ring torus with major radius greater than minor radius"
                        .into(),
                );
            }
            torus(*major_radius, *minor_radius, quality.circle_segments(*major_radius + *minor_radius), quality.circle_segments(*minor_radius))
        }
        SdfParams::PolygonPrismParams(p) => {
            positive(p.half_depth, "Polygon half depth")?;
            if p.edge_softness > 0.0 {
                rounded_prism(
                    &p.vertices,
                    p.half_depth,
                    p.edge_softness.min(p.half_depth),
                    n,
                )?
            } else {
                extrude(&p.vertices, p.half_depth)?
            }
        }
        SdfParams::LoftParams(p) => loft(p, n, cancel)?,
        SdfParams::BezierCurveParams(curve) => {
            let Some(modifier) = object.path_extrusion else {
                return Ok(Surface::default().finish(object.uuid));
            };
            positive(modifier.radius, "Curve extrusion radius")?;
            let outline = if let Some(profile) = modifier.profile_curve {
                crate::model::profile_curve_vertices(scene, profile)
                    .ok_or_else(|| "Curve profile reference is missing or invalid".to_owned())?
                    .into_iter()
                    .map(|point| point * modifier.radius)
                    .collect()
            } else {
                match modifier.profile {
                    BezierProfile::Round => circle(modifier.radius, n),
                    BezierProfile::Square => vec![
                        Vec2::new(-1., -1.),
                        Vec2::new(1., -1.),
                        Vec2::new(1., 1.),
                        Vec2::new(-1., 1.),
                    ]
                    .into_iter()
                    .map(|p| p * modifier.radius)
                    .collect(),
                }
            };
            curve_surface(
                curve,
                &outline,
                modifier.profile == BezierProfile::Round && modifier.profile_curve.is_none(),
                modifier.radius,
                n,
                cancel,
            )?
        }
        SdfParams::TextParams(_) => {
            let prepared = crate::model::prepared_scene_text(scene, object)?;
            let mut all = Surface::default();
            for glyph in &prepared.glyphs {
                check_cancel(cancel)?;
                let mut contours: Vec<Vec<Vec2>> = Vec::new();
                for &[a, b] in &glyph.edges {
                    if contours.last().is_none_or(|ring| ring.last() != Some(&a)) {
                        contours.push(vec![a]);
                    }
                    contours.last_mut().unwrap().push(b);
                }
                for ring in &mut contours {
                    if ring.first() == ring.last() {
                        ring.pop();
                    }
                }
                let mut part = extrude_contours(&contours, prepared.half_depth)?;
                for point in &mut part.positions {
                    *point =
                        glyph.origin + glyph.x * point.x + glyph.y * point.y + glyph.z * point.z;
                }
                all.append(part);
            }
            all
        }
    };
    if surface.positions.iter().any(|p| !p.is_finite()) {
        return Err("Direct primitive tessellation produced non-finite vertices".into());
    }
    check_cancel(cancel)?;
    Ok(surface.finish(object.uuid))
}

fn positive(value: f32, label: &str) -> Result<(), String> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(format!("{label} must be positive and finite"))
    }
}
fn circle(radius: f32, n: usize) -> Vec<Vec2> {
    (0..n)
        .map(|i| {
            let angle = std::f32::consts::TAU * i as f32 / n as f32;
            Vec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect()
}
fn sphere(radius: f32, n: usize) -> Surface {
    let mut s = Surface::default();
    let top = s.vertex(Vec3::Y * radius);
    let latitude = (n / 2).max(4);
    for j in 1..latitude {
        let theta = std::f32::consts::PI * j as f32 / latitude as f32;
        for i in 0..n {
            // A half-segment azimuth keeps common axis-aligned CSG cuts away
            // from simultaneous latitude/longitude grid vertices.
            let phi = std::f32::consts::TAU * (i as f32 + 0.5) / n as f32;
            s.vertex(
                Vec3::new(
                    theta.sin() * phi.cos(),
                    theta.cos(),
                    theta.sin() * phi.sin(),
                ) * radius,
            );
        }
    }
    let bottom = s.vertex(-Vec3::Y * radius);
    for i in 0..n {
        let next = (i + 1) % n;
        s.face(top, 1 + next as u32, 1 + i as u32);
        for j in 0..latitude - 2 {
            let a = 1 + (j * n + i) as u32;
            let b = 1 + (j * n + next) as u32;
            s.quad(a, b, a + n as u32, b + n as u32);
        }
        let last = 1 + ((latitude - 2) * n) as u32;
        s.face(bottom, last + i as u32, last + next as u32);
    }
    s
}
fn torus(major: f32, minor: f32, n: usize, cross: usize) -> Surface {
    let mut s = Surface::default();
    for i in 0..n {
        let u = std::f32::consts::TAU * i as f32 / n as f32;
        for j in 0..cross {
            let v = std::f32::consts::TAU * j as f32 / cross as f32;
            let r = major + minor * v.cos();
            s.vertex(Vec3::new(r * u.cos(), minor * v.sin(), r * u.sin()));
        }
    }
    for i in 0..n {
        for j in 0..cross {
            let a = (i * cross + j) as u32;
            let b = (i * cross + (j + 1) % cross) as u32;
            let c = (((i + 1) % n) * cross + j) as u32;
            let d = (((i + 1) % n) * cross + (j + 1) % cross) as u32;
            s.quad(a, b, c, d);
        }
    }
    s
}
fn rounded_box(extent: Vec3, radius: f32, n: usize) -> Surface {
    // One quad covers each flat face; only the fillet/corner bands are sampled.
    fn coordinates(extent: f32, radius: f32, steps: usize) -> Vec<f32> {
        if radius == 0.0 { return vec![-extent, extent]; }
        let inner = extent - radius;
        let mut out = Vec::new();
        for i in (0..=steps).rev() { out.push(-inner - radius * (std::f32::consts::FRAC_PI_4 * i as f32 / steps as f32).tan()); }
        for i in 0..=steps { out.push(inner + radius * (std::f32::consts::FRAC_PI_4 * i as f32 / steps as f32).tan()); }
        out.dedup(); out
    }
    let coords = [coordinates(extent.x, radius, n), coordinates(extent.y, radius, n), coordinates(extent.z, radius, n)];
    let mut s = Surface::default();
    let mut vertices = HashMap::new();
    for axis in 0..3 {
        for sign in [-1_i32, 1] {
            let u = &coords[(axis + 1) % 3]; let v = &coords[(axis + 2) % 3];
            let mut grid = vec![0; u.len() * v.len()];
            for j in 0..v.len() {
                for i in 0..u.len() {
                    let mut p = Vec3::ZERO; p[axis] = sign as f32 * extent[axis];
                    p[(axis + 1) % 3] = u[i]; p[(axis + 2) % 3] = v[j];
                    let key = p.to_array().map(f32::to_bits);
                    let v = *vertices.entry(key).or_insert_with(|| {
                        let inner = extent - Vec3::splat(radius);
                        let center = p.clamp(-inner, inner);
                        let delta = p - center;
                        s.vertex(if radius > 0.0 {
                            center + delta * (radius / delta.length())
                        } else {
                            p
                        })
                    });
                    grid[j * u.len() + i] = v;
                }
            }
            for j in 0..v.len()-1 {
                for i in 0..u.len()-1 {
                    let a = grid[j * u.len() + i];
                    let b = grid[j * u.len() + i + 1];
                    let c = grid[(j + 1) * u.len() + i];
                    let d = grid[(j + 1) * u.len() + i + 1];
                    if sign > 0 {
                        s.quad(a, b, c, d);
                    } else {
                        s.quad(a, c, b, d);
                    }
                }
            }
        }
    }
    s
}
fn signed_area(ring: &[Vec2]) -> f32 {
    (0..ring.len())
        .map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()]))
        .sum::<f32>()
        * 0.5
}
fn contour(ring: &[Vec2]) -> Result<Vec<Vec2>, String> {
    if ring.len() < 3 || ring.iter().any(|p| !p.is_finite()) {
        return Err("Direct mesh requires a finite polygon with at least three vertices".into());
    }
    let mut result = ring.to_vec();
    if result.first() == result.last() {
        result.pop();
    }
    loop {
        let removable = (0..result.len()).find(|&i| {
            let a = result[(i + result.len() - 1) % result.len()];
            let b = result[i];
            let c = result[(i + 1) % result.len()];
            let left = b - a;
            let right = c - b;
            left.length_squared() == 0.0
                || (left.perp_dot(right).abs() <= 1e-7 * left.length() * right.length()
                    && left.dot(right) >= 0.0)
        });
        if result.len() <= 3 {
            break;
        }
        if let Some(i) = removable {
            result.remove(i);
        } else {
            break;
        }
    }
    if signed_area(&result).abs() < 1e-10 {
        return Err("Direct mesh polygon has zero area".into());
    }
    if signed_area(&result) < 0.0 {
        result.reverse();
    }
    Ok(result)
}
fn extrude(ring: &[Vec2], half_depth: f32) -> Result<Surface, String> {
    extrude_contours(&[contour(ring)?], half_depth)
}
fn cap(
    s: &mut Surface,
    rings: &[Vec<Vec2>],
    indices: &[Vec<u32>],
    reverse: bool,
) -> Result<(), String> {
    let mut coordinates = Vec::new();
    let mut holes = Vec::new();
    let mut map = Vec::new();
    for (i, ring) in rings.iter().enumerate() {
        if i > 0 {
            holes.push(map.len());
        }
        for (&point, &vertex) in ring.iter().zip(&indices[i]) {
            coordinates.extend([point.x as f64, point.y as f64]);
            map.push(vertex);
        }
    }
    let faces = earcutr::earcut(&coordinates, &holes, 2)
        .map_err(|e| format!("Polygon triangulation failed: {e}"))?;
    for face in faces.chunks_exact(3) {
        let a = map[face[0]];
        let mut b = map[face[1]];
        let mut c = map[face[2]];
        let z = (s.positions[b as usize] - s.positions[a as usize])
            .cross(s.positions[c as usize] - s.positions[a as usize])
            .z;
        if (z < 0.0) != reverse {
            std::mem::swap(&mut b, &mut c);
        }
        s.face(a, b, c);
    }
    Ok(())
}
fn contains(ring: &[Vec2], point: Vec2) -> bool {
    let mut inside = false;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        if (a.y > point.y) != (b.y > point.y)
            && point.x < a.x + (point.y - a.y) * (b.x - a.x) / (b.y - a.y)
        {
            inside = !inside;
        }
    }
    inside
}
fn extrude_contours(contours: &[Vec<Vec2>], half_depth: f32) -> Result<Surface, String> {
    let mut s = Surface::default();
    // Font outlines may contain several disjoint islands and nested holes. Classify
    // by containment parity rather than relying on font-specific winding.
    for (outer_index, outer) in contours.iter().enumerate() {
        if outer.len() < 3 {
            continue;
        }
        let depth = contours
            .iter()
            .enumerate()
            .filter(|(i, ring)| *i != outer_index && ring.len() >= 3 && contains(ring, outer[0]))
            .count();
        if depth % 2 == 1 {
            continue;
        }
        let mut rings = vec![contour(outer)?];
        for (i, hole) in contours.iter().enumerate() {
            if i == outer_index || hole.len() < 3 || !contains(outer, hole[0]) {
                continue;
            }
            let ancestors = contours
                .iter()
                .enumerate()
                .filter(|(j, ring)| *j != i && ring.len() >= 3 && contains(ring, hole[0]))
                .count();
            if ancestors == depth + 1 {
                let mut ring = contour(hole)?;
                ring.reverse();
                rings.push(ring);
            }
        }
        let mut bottom = Vec::new();
        let mut top = Vec::new();
        for ring in &rings {
            bottom.push(
                ring.iter()
                    .map(|p| s.vertex(p.extend(-half_depth)))
                    .collect::<Vec<_>>(),
            );
            top.push(
                ring.iter()
                    .map(|p| s.vertex(p.extend(half_depth)))
                    .collect::<Vec<_>>(),
            );
        }
        cap(&mut s, &rings, &bottom, true)?;
        cap(&mut s, &rings, &top, false)?;
        for i in 0..rings.len() {
            for j in 0..rings[i].len() {
                let k = (j + 1) % rings[i].len();
                s.quad(bottom[i][j], bottom[i][k], top[i][j], top[i][k]);
            }
        }
    }
    Ok(s)
}

fn rounded_prism(vertices: &[Vec2], depth: f32, radius: f32, n: usize) -> Result<Surface, String> {
    let ring = contour(vertices)?;
    for i in 0..ring.len() {
        let a = ring[(i + ring.len() - 1) % ring.len()];
        let b = ring[i];
        let c = ring[(i + 1) % ring.len()];
        if (b - a).perp_dot(c - b) < -1e-7 {
            return Err(
                "Direct rounded polygon tessellation currently requires a convex outline".into(),
            );
        }
    }
    let arc = (n / 8).max(2);
    let mut profile = Vec::new();
    let mut base = Vec::new();
    for i in 0..ring.len() {
        let prev = ring[(i + ring.len() - 1) % ring.len()];
        let point = ring[i];
        let next = ring[(i + 1) % ring.len()];
        let a = point - prev;
        let b = next - point;
        let normal_a = Vec2::new(a.y, -a.x) / a.length();
        let normal_b = Vec2::new(b.y, -b.x) / b.length();
        let angle = normal_a.y.atan2(normal_a.x);
        let turn = normal_a.perp_dot(normal_b).atan2(normal_a.dot(normal_b));
        for j in 0..=arc {
            let t = angle + turn * j as f32 / arc as f32;
            profile.push(Vec2::new(t.cos(), t.sin()));
            base.push(point);
        }
    }
    let mut s = Surface::default();
    let mut rings = Vec::new();
    let mut flat = Vec::new();
    for j in 0..=arc * 2 + 1 {
        let (offset, z) = if j <= arc {
            let t = std::f32::consts::FRAC_PI_2 * (arc - j) as f32 / arc as f32;
            (
                if j == 0 { 0.0 } else { radius * t.cos() },
                -(depth - radius) - radius * t.sin(),
            )
        } else {
            let t = std::f32::consts::FRAC_PI_2 * (j - arc - 1) as f32 / arc as f32;
            (
                if j == arc * 2 + 1 {
                    0.0
                } else {
                    radius * t.cos()
                },
                depth - radius + radius * t.sin(),
            )
        };
        let outline: Vec<_> = base
            .iter()
            .zip(&profile)
            .map(|(&p, &dir)| p + dir * offset)
            .collect();
        rings.push(
            outline
                .iter()
                .map(|p| s.vertex(p.extend(z)))
                .collect::<Vec<_>>(),
        );
        flat.push(outline);
    }
    for j in 0..rings.len() - 1 {
        for i in 0..profile.len() {
            let k = (i + 1) % profile.len();
            s.quad(rings[j][i], rings[j][k], rings[j + 1][i], rings[j + 1][k]);
        }
    }
    cap(&mut s, &[flat[0].clone()], &[rings[0].clone()], true)?;
    cap(
        &mut s,
        &[flat.last().unwrap().clone()],
        &[rings.last().unwrap().clone()],
        false,
    )?;
    Ok(s)
}

fn ring_cap(
    s: &mut Surface,
    outline: &[Vec2],
    indices: &[u32],
    reverse: bool,
) -> Result<(), String> {
    let coordinates: Vec<_> = outline
        .iter()
        .flat_map(|p| [p.x as f64, p.y as f64])
        .collect();
    let faces = earcutr::earcut(&coordinates, &[], 2)
        .map_err(|e| format!("Profile triangulation failed: {e}"))?;
    for face in faces.chunks_exact(3) {
        let a = indices[face[0]];
        let mut b = indices[face[1]];
        let mut c = indices[face[2]];
        let cross =
            (outline[face[1]] - outline[face[0]]).perp_dot(outline[face[2]] - outline[face[0]]);
        if (cross < 0.0) != reverse {
            std::mem::swap(&mut b, &mut c);
        }
        s.face(a, b, c);
    }
    Ok(())
}
fn loft(params: &LoftParams, n: usize, cancel: &AtomicBool) -> Result<Surface, String> {
    if params.sections.len() < 2 {
        return Err("Loft requires at least two sections".into());
    }
    if params.sections.windows(2).any(|p| p[1].x <= p[0].x) {
        return Err("Loft section X coordinates must increase".into());
    }
    let count = if params.profile_count() >= 3 {
        params.profile_count()
    } else {
        n
    };
    let mut s = Surface::default();
    let mut rings = Vec::new();
    let mut outlines = Vec::new();
    for (section, pair) in params.sections.windows(2).enumerate() {
        for step in 0..=n {
            if section > 0 && step == 0 {
                continue;
            }
            check_cancel(cancel)?;
            let t = step as f32 / n as f32;
            let a = &pair[0];
            let b = &pair[1];
            let previous = &params.sections[section.saturating_sub(1)];
            let next = &params.sections[(section + 2).min(params.sections.len() - 1)];
            let x = [previous.x, a.x, b.x, next.x];
            let blend = |a, b, p, n| params.blend(a, b, p, n, t, x);
            positive(a.half_height, "Loft section height")?;
            positive(a.half_width, "Loft section width")?;
            positive(b.half_height, "Loft section height")?;
            positive(b.half_width, "Loft section width")?;
            let center = Vec2::new(
                blend(a.center_y, b.center_y, previous.center_y, next.center_y),
                blend(a.center_z, b.center_z, previous.center_z, next.center_z),
            );
            let radii = Vec2::new(
                blend(a.half_height, b.half_height, previous.half_height, next.half_height),
                blend(a.half_width, b.half_width, previous.half_width, next.half_width),
            ).max(Vec2::splat(0.001));
            let outline: Vec<_> = (0..count)
                .map(|i| {
                    let pa = LoftParams::profile_point(a, i, count);
                    let pb = LoftParams::profile_point(b, i, count);
                    let pp = LoftParams::profile_point(previous, i, count);
                    let pn = LoftParams::profile_point(next, i, count);
                    center + Vec2::new(
                        blend(pa.x, pb.x, pp.x, pn.x),
                        blend(pa.y, pb.y, pp.y, pn.y),
                    ) * radii
                })
                .collect();
            if signed_area(&outline) <= 0.0 {
                return Err("Loft profiles must be counterclockwise and have positive area".into());
            }
            let x = a.x + (b.x - a.x) * t;
            rings.push(
                outline
                    .iter()
                    .map(|p| s.vertex(Vec3::new(x, p.x, p.y)))
                    .collect::<Vec<_>>(),
            );
            outlines.push(outline);
        }
    }
    for j in 0..rings.len() - 1 {
        for i in 0..count {
            let k = (i + 1) % count;
            s.quad(rings[j][i], rings[j][k], rings[j + 1][i], rings[j + 1][k]);
        }
    }
    ring_cap(&mut s, &outlines[0], &rings[0], true)?;
    ring_cap(
        &mut s,
        outlines.last().unwrap(),
        rings.last().unwrap(),
        false,
    )?;
    Ok(s)
}
fn curve_frame(
    curve: &crate::model::BezierCurveParams,
    tangent: Vec3,
) -> Result<(Vec3, Vec3, Vec3), String> {
    let start = curve.tangent(0, 0.0);
    if start.length_squared() < 1e-12 || tangent.length_squared() < 1e-12 {
        return Err("Curve tessellation requires nonzero endpoint and sampled tangents".into());
    }
    let start = start / start.length();
    let tangent = tangent / tangent.length();
    let reference = if start.y.abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let side = start.cross(reference);
    let initial = side / side.length();
    let axis = start.cross(tangent);
    let w = 1.0 + start.dot(tangent);
    let denominator = w * w + axis.length_squared();
    let side = if denominator < 1e-6 {
        initial
    } else {
        let v = initial + 2.0 * axis.cross(axis.cross(initial) + w * initial) / denominator;
        v / v.length()
    };
    Ok((side, tangent.cross(side), tangent))
}
fn curve_surface(
    curve: &crate::model::BezierCurveParams,
    outline: &[Vec2],
    round: bool,
    radius: f32,
    n: usize,
    cancel: &AtomicBool,
) -> Result<Surface, String> {
    let outline = contour(outline)?;
    let count = outline.len();
    let segments = curve.segment_count();
    if segments == 0 {
        return Err("Curve has no cubic segments".into());
    }
    if curve.closed
        && curve
            .point(0, 0.0)
            .distance_squared(curve.point(segments - 1, 1.0))
            > 1e-10
    {
        return Err(
            "Direct closed curve tessellation requires coincident first and last endpoints".into(),
        );
    }
    let mut s = Surface::default();
    let mut rings = Vec::new();
    let steps = segments * n;
    for step in 0..=steps {
        if curve.closed && step == steps {
            break;
        }
        check_cancel(cancel)?;
        let segment = (step / n).min(segments - 1);
        let t = (step - segment * n) as f32 / n as f32;
        let center = curve.point(segment, t);
        let (side, up, _) = curve_frame(curve, curve.tangent(segment, t))?;
        rings.push(
            outline
                .iter()
                .map(|p| s.vertex(center + side * p.x + up * p.y))
                .collect::<Vec<_>>(),
        );
    }
    let pairs = if curve.closed {
        rings.len()
    } else {
        rings.len() - 1
    };
    for j in 0..pairs {
        let next = (j + 1) % rings.len();
        for i in 0..count {
            let k = (i + 1) % count;
            s.quad(rings[j][i], rings[j][k], rings[next][i], rings[next][k]);
        }
    }
    if !curve.closed {
        if round {
            for end in [false, true] {
                let segment = if end { segments - 1 } else { 0 };
                let t = if end { 1.0 } else { 0.0 };
                let center = curve.point(segment, t);
                let (side, up, tangent) = curve_frame(curve, curve.tangent(segment, t))?;
                let direction = if end { 1.0 } else { -1.0 };
                let mut previous = if end {
                    rings.last().unwrap().clone()
                } else {
                    rings[0].clone()
                };
                for j in 1..(n / 4).max(2) {
                    let angle = std::f32::consts::FRAC_PI_2 * j as f32 / (n / 4).max(2) as f32;
                    let ring: Vec<_> = outline
                        .iter()
                        .map(|p| {
                            s.vertex(
                                center
                                    + tangent * (direction * radius * angle.sin())
                                    + (side * p.x + up * p.y) * angle.cos(),
                            )
                        })
                        .collect();
                    for i in 0..count {
                        let k = (i + 1) % count;
                        if end {
                            s.quad(previous[i], previous[k], ring[i], ring[k]);
                        } else {
                            s.quad(previous[i], ring[i], previous[k], ring[k]);
                        }
                    }
                    previous = ring;
                }
                let pole = s.vertex(center + tangent * direction * radius);
                for i in 0..count {
                    let k = (i + 1) % count;
                    if end {
                        s.face(previous[i], previous[k], pole);
                    } else {
                        s.face(previous[k], previous[i], pole);
                    }
                }
            }
        } else {
            ring_cap(&mut s, &outline, &rings[0], true)?;
            ring_cap(&mut s, &outline, rings.last().unwrap(), false)?;
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PrimitiveKind;
    #[test]
    fn every_default_solid_has_outward_closed_mesh() {
        let cancel = AtomicBool::new(false);
        for kind in PrimitiveKind::ALL {
            let mut object = SdfObject::create_kind(kind);
            if kind == PrimitiveKind::BezierCurve {
                object.path_extrusion = Some(Default::default());
            }
            let mesh = build(&[object.clone()], &object, 16, &cancel).unwrap();
            assert!(!mesh.triangles.is_empty(), "{}", kind.label());
            let manifold = super::super::manifold(&mesh)
                .unwrap_or_else(|error| panic!("{}: {error}", kind.label()));
            assert!(manifold.is_manifold());
            let volume: f32 = mesh
                .triangles
                .iter()
                .map(|face| {
                    let [a, b, c] = face.map(|v| mesh.positions[v as usize]);
                    a.dot(b.cross(c)) / 6.0
                })
                .sum();
            assert!(volume > 0.0, "{} volume {volume}", kind.label());
        }
    }
    #[test]
    fn rounded_box_stays_on_analytic_surface() {
        let extent = Vec3::new(1., 2., 3.);
        let radius = 0.25;
        let s = rounded_box(extent, radius, 16);
        for p in s.positions {
            let q = p.abs() - (extent - Vec3::splat(radius));
            let distance = q.max(Vec3::ZERO).length() + q.max_element().min(0.) - radius;
            assert!(distance.abs() < 1e-5);
        }
    }
    #[test]
    fn concave_prism_is_closed() {
        let ring = [
            Vec2::new(0., 0.),
            Vec2::new(2., 0.),
            Vec2::new(2., 1.),
            Vec2::new(1., 1.),
            Vec2::new(1., 2.),
            Vec2::new(0., 2.),
        ];
        let mesh = extrude(&ring, 0.5).unwrap().finish(Uuid::new_v4());
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
    }
    fn assert_closed(surface: Surface) {
        let mesh = surface.finish(Uuid::new_v4());
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        let volume: f32 = mesh
            .triangles
            .iter()
            .map(|f| {
                let [a, b, c] = f.map(|i| mesh.positions[i as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum();
        assert!(volume > 0.0);
    }
    #[test]
    fn rounded_convex_prism_and_collinear_boundary_are_closed() {
        let square = [
            Vec2::new(-1., -1.),
            Vec2::new(0., -1.),
            Vec2::new(1., -1.),
            Vec2::new(1., 1.),
            Vec2::new(-1., 1.),
        ];
        assert_closed(extrude(&square, 0.4).unwrap());
        assert_closed(rounded_prism(&square, 0.4, 0.1, 16).unwrap());
        assert_closed(rounded_prism(&square, 0.4, 0.4, 16).unwrap());
    }
    #[test]
    fn text_holes_are_closed_and_y_cylinder_matches_field() {
        let cancel = AtomicBool::new(false);
        let mut text = SdfObject::create_kind(PrimitiveKind::Text);
        if let SdfParams::TextParams(p) = &mut text.params {
            p.text = "BO".into();
        }
        let mesh = build(&[text.clone()], &text, 16, &cancel).unwrap();
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        let mut cylinder = SdfObject::create_kind(PrimitiveKind::Cylinder);
        cylinder.params = SdfParams::CylinderParams {
            radius: 0.7,
            half_height: 1.9,
        };
        let mesh = build(&[cylinder.clone()], &cylinder, 16, &cancel).unwrap();
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        for p in mesh.positions {
            let radial = Vec2::new(p.x, p.z).length() - 0.7;
            let q = Vec2::new(radial, p.y.abs() - 1.9);
            assert!((q.max(Vec2::ZERO).length() + q.max_element().min(0.)).abs() < 1e-5);
        }
    }
    #[test]
    fn loft_custom_profile_and_square_curve_caps_are_closed() {
        let cancel = AtomicBool::new(false);
        let mut object = SdfObject::create_kind(PrimitiveKind::Loft);
        if let SdfParams::LoftParams(p) = &mut object.params {
            for section in &mut p.sections {
                section.profile = Some(vec![
                    Vec2::new(-1., -1.),
                    Vec2::new(1., -1.),
                    Vec2::new(1., 1.),
                    Vec2::new(-1., 1.),
                ]);
            }
        }
        let mesh = build(&[object.clone()], &object, 16, &cancel).unwrap();
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        let mut curve = SdfObject::create_kind(PrimitiveKind::BezierCurve);
        curve.path_extrusion = Some(crate::model::PathExtrusion {
            profile: BezierProfile::Square,
            ..Default::default()
        });
        let mesh = build(&[curve.clone()], &curve, 16, &cancel).unwrap();
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        let mut profile = SdfObject::create_kind(PrimitiveKind::BezierCurve);
        if let SdfParams::BezierCurveParams(p) = &mut profile.params {
            p.closed = true;
        }
        curve.path_extrusion.as_mut().unwrap().profile_curve = Some(profile.uuid);
        let mesh = build(&[curve.clone(), profile], &curve, 16, &cancel).unwrap();
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
    }
}

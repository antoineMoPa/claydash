//! Independent construction polylines and explicit connections to existing tools.
use super::{lattice_world_matrix, object_world_matrix, Lattice, SdfObject, SdfParams, SheetEdge};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SeamPoint {
    World(Vec3),
    Surface {
        object: Uuid,
        triangle: usize,
        barycentric: Vec3,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seam {
    pub id: Uuid,
    pub name: String,
    pub points: Vec<SeamPoint>,
    #[serde(default)]
    pub closed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CageAxis {
    X,
    Y,
    Z,
}
impl CageAxis {
    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}
/// The varying axis plus min/max sides along the other axes in XYZ order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatticeEdge {
    pub axis: CageAxis,
    pub sides: [bool; 2],
}
impl LatticeEdge {
    pub fn control(self, lattice: &Lattice, station: usize) -> usize {
        let mut indices = [0; 3];
        let mut side = 0;
        for axis in 0..3 {
            if axis == self.axis.index() {
                indices[axis] = station;
            } else {
                indices[axis] = if self.sides[side] {
                    lattice.resolution as usize - 1
                } else {
                    0
                };
                side += 1;
            }
        }
        lattice.index(indices[0], indices[1], indices[2])
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeamTarget {
    LatticeEdge(LatticeEdge),
    SheetBoundary(SheetEdge),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeamBinding {
    pub object: Uuid,
    pub seam: Uuid,
    pub target: SeamTarget,
    /// Inclusive point indices; closed seams wrap from start to end.
    #[serde(default)]
    pub span: Option<[usize; 2]>,
    #[serde(default)]
    pub reverse: bool,
    /// World-space maximum boundary error; independent of viewport resolution.
    pub tolerance: f32,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneSeams {
    #[serde(default)]
    pub seams: Vec<Seam>,
    #[serde(default)]
    pub bindings: Vec<SeamBinding>,
}

pub fn seam_point_position(point: &SeamPoint, objects: &[SdfObject]) -> Result<Vec3, String> {
    let p = match point {
        SeamPoint::World(p) => *p,
        SeamPoint::Surface {
            object,
            triangle,
            barycentric,
        } => {
            if !barycentric.is_finite()
                || barycentric.min_element() < 0.0
                || (barycentric.element_sum() - 1.0).abs() > 1e-5
            {
                return Err("surface point weights must be nonnegative and sum to one".into());
            }
            let source = objects
                .iter()
                .find(|o| o.uuid == *object)
                .ok_or("seam source mesh missing")?;
            if source.lattice.is_some() || source.mirror.is_some() || source.repetition.enabled {
                return Err("surface seam attachments require an undeformed mesh".into());
            }
            let mut ancestor = source.boolean_parent;
            let mut seen = std::collections::HashSet::new();
            while let Some(id) = ancestor {
                if !seen.insert(id) {
                    return Err("seam source ancestry cycle".into());
                }
                let parent = objects
                    .iter()
                    .find(|o| o.uuid == id)
                    .ok_or("seam source ancestor missing")?;
                if parent.lattice.is_some() || parent.mirror.is_some() || parent.repetition.enabled
                {
                    return Err("surface seam source ancestors cannot deform the mesh".into());
                }
                ancestor = parent.boolean_parent;
            }
            let mesh = source
                .imported_mesh
                .as_ref()
                .ok_or("surface seam point requires a triangle mesh")?;
            let face = mesh
                .triangles
                .get(*triangle)
                .ok_or("surface seam triangle missing")?;
            let mut p = Vec3::ZERO;
            for (index, weight) in face.iter().zip(barycentric.to_array()) {
                p += *mesh
                    .positions
                    .get(*index as usize)
                    .ok_or("surface seam vertex missing")?
                    * weight;
            }
            object_world_matrix(objects, *object).transform_point3(p)
        }
    };
    if !p.is_finite() {
        return Err("seam positions must be finite".into());
    }
    Ok(p)
}
impl Seam {
    pub fn positions(&self, objects: &[SdfObject]) -> Result<Vec<Vec3>, String> {
        if self.points.len() > 4096 {
            return Err("a seam supports at most 4096 points".into());
        }
        let points = self
            .points
            .iter()
            .map(|p| seam_point_position(p, objects))
            .collect::<Result<Vec<_>, _>>()?;
        if points
            .windows(2)
            .any(|p| p[0].distance_squared(p[1]) < 1e-14)
            || (self.closed
                && (points.len() < 3
                    || points[0].distance_squared(*points.last().unwrap()) < 1e-14))
        {
            return Err("seam segments must be nonzero; closed seams need three distinct stations without a repeated endpoint".into());
        }
        Ok(points)
    }
}
fn span_positions(
    seam: &Seam,
    binding: &SeamBinding,
    objects: &[SdfObject],
) -> Result<Vec<Vec3>, String> {
    let points = seam.positions(objects)?;
    if points.len() < 2 {
        return Err("a bound seam requires at least two points".into());
    }
    let mut span = if let Some([start, end]) = binding.span {
        if start >= points.len()
            || end >= points.len()
            || start == end
            || (!seam.closed && start > end)
        {
            return Err("invalid seam span".into());
        }
        let mut span = vec![points[start]];
        let mut i = start;
        while i != end {
            i = (i + 1) % points.len();
            span.push(points[i]);
        }
        span
    } else {
        if seam.closed {
            return Err("select a point span when connecting an edge to a closed seam".into());
        }
        points
    };
    if binding.reverse {
        span.reverse();
    }
    Ok(span)
}
/// Station parameterization: equally spaced knot parameters, piecewise linear geometry.
pub fn seam_sample(points: &[Vec3], t: f32) -> Vec3 {
    let t = t.clamp(0.0, 1.0) * (points.len() - 1) as f32;
    let i = (t.floor() as usize).min(points.len() - 2);
    points[i].lerp(points[i + 1], t - i as f32)
}
pub fn sheet_edge_material_point(edge: SheetEdge, size: Vec2, t: f32) -> Vec2 {
    match edge {
        SheetEdge::MinX => Vec2::new(-size.x * 0.5, (t - 0.5) * size.y),
        SheetEdge::MaxX => Vec2::new(size.x * 0.5, (t - 0.5) * size.y),
        SheetEdge::MinY => Vec2::new((t - 0.5) * size.x, -size.y * 0.5),
        SheetEdge::MaxY => Vec2::new((t - 0.5) * size.x, size.y * 0.5),
    }
}
fn sheet_edge_survives(panel: &super::SheetPanelParams, edge: SheetEdge) -> bool {
    if panel.stage == super::SheetStage::Blank {
        return true;
    }
    let a = sheet_edge_material_point(edge, panel.size, 0.0);
    let b = sheet_edge_material_point(edge, panel.size, 1.0);
    for cut in &panel.cuts {
        match cut {
            super::SheetCut::Straight { from, to } => {
                let line = *to - *from;
                let inward = match edge {
                    SheetEdge::MinX => Vec2::new(panel.size.x * 1e-4, 0.0),
                    SheetEdge::MaxX => Vec2::new(-panel.size.x * 1e-4, 0.0),
                    SheetEdge::MinY => Vec2::new(0.0, panel.size.y * 1e-4),
                    SheetEdge::MaxY => Vec2::new(0.0, -panel.size.y * 1e-4),
                };
                if [a, b, a + inward, b + inward]
                    .iter()
                    .any(|p| line.perp_dot(*p - *from) < -1e-6)
                {
                    return false;
                }
            }
            super::SheetCut::Circle { center, radius } => {
                let line = b - a;
                let closest =
                    a + line * (((*center - a).dot(line) / line.length_squared()).clamp(0.0, 1.0));
                if center.distance(closest) <= radius + 1e-5 {
                    return false;
                }
            }
        }
    }
    true
}

struct Equation {
    weights: Vec<f32>,
    target: Vec3,
}
fn weights(lattice: &Lattice, controls: &[usize], rest: Vec3) -> Vec<f32> {
    let n = lattice.resolution as usize;
    let last = n - 1;
    let mut result = vec![0.0; controls.len()];
    let mut lookup = vec![0; n * n * n];
    for (slot, index) in controls.iter().enumerate() {
        lookup[*index] = slot;
    }
    let coordinates = ((rest - lattice.min) / (lattice.max - lattice.min).max(Vec3::splat(0.0001)))
        .clamp(Vec3::ZERO, Vec3::ONE)
        * last as f32;
    let low = coordinates
        .floor()
        .as_uvec3()
        .to_array()
        .map(|v| v as usize);
    let high = low.map(|v| (v + 1).min(last));
    let t = coordinates - Vec3::new(low[0] as f32, low[1] as f32, low[2] as f32);
    for z in 0..2 {
        for y in 0..2 {
            for x in 0..2 {
                let flags = [x, y, z];
                let mut indices = [0; 3];
                let mut weight = 1.0;
                for axis in 0..3 {
                    indices[axis] = if flags[axis] == 0 {
                        low[axis]
                    } else {
                        high[axis]
                    };
                    weight *= if flags[axis] == 0 {
                        1.0 - t[axis]
                    } else {
                        t[axis]
                    };
                }
                let index = lattice.index(indices[0], indices[1], indices[2]);
                if lattice.is_surface_point(indices[0], indices[1], indices[2]) {
                    result[lookup[index]] += weight;
                } else {
                    for axis in 0..3 {
                        let f = indices[axis] as f32 / last as f32;
                        let mut a = indices;
                        let mut b = indices;
                        a[axis] = 0;
                        b[axis] = last;
                        result[lookup[lattice.index(a[0], a[1], a[2])]] += weight * (1.0 - f) / 3.0;
                        result[lookup[lattice.index(b[0], b[1], b[2])]] += weight * f / 3.0;
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
fn native_rest(lattice: &Lattice, point: Vec3) -> Vec3 {
    let mut rest = point;
    for _ in 0..4 {
        rest = point - lattice.displacement(rest);
    }
    rest
}

fn check_bound_pose(
    lattice: Option<&Lattice>,
    frame: glam::Mat4,
    edge_checks: &[(LatticeEdge, Vec<Vec3>, f32)],
    sheet_checks: &[(Vec3, Vec3, f32)],
) -> Result<(), String> {
    for (edge, span, tolerance) in edge_checks {
        let cage = lattice.unwrap();
        let n = cage.resolution as usize;
        for station in 0..=(n - 1) * 16 {
            let t = station as f32 / ((n - 1) * 16) as f32;
            let f = t * (n - 1) as f32;
            let i = (f.floor() as usize).min(n - 2);
            let point = |station| {
                let index = edge.control(cage, station);
                let p = cage.position(index % n, (index / n) % n, index / (n * n));
                p + cage.offsets[index]
            };
            let position = frame.transform_point3(point(i).lerp(point(i + 1), f - i as f32));
            if position.distance(seam_sample(span, t)) > *tolerance {
                return Err("incompatible seam connections on lattice edges".into());
            }
        }
    }
    for (rest, target, tolerance) in sheet_checks {
        let deformed = if let Some(cage) = lattice {
            cage.deformed_point(*rest)?
        } else {
            *rest
        };
        let error = frame.transform_vector3(deformed - *target).length();
        if error > *tolerance {
            return Err(format!("sheet boundary cannot follow this seam with the current bend/cage (error {error:.6}, tolerance {tolerance:.6}); adjust sheet bends or lattice density"));
        }
    }
    Ok(())
}

/// Resolve bindings onto the existing authored cages. No replacement geometry is created.
/// Validate complete boundary samples after native sheet forming and cage inversion.
pub fn resolve_seam_bindings(seams: &SceneSeams, objects: &mut [SdfObject]) -> Result<(), String> {
    if seams.seams.is_empty() && seams.bindings.is_empty() {
        return Ok(());
    }
    if seams.seams.len() > 1024 || seams.bindings.len() > 1024 {
        return Err("at most 1024 seams and bindings are supported".into());
    }
    let mut ids = std::collections::HashSet::new();
    for seam in &seams.seams {
        if !ids.insert(seam.id) || objects.iter().any(|o| o.uuid == seam.id) {
            return Err("duplicate seam id".into());
        }
        seam.positions(objects)?;
    }
    for binding in &seams.bindings {
        if !binding.tolerance.is_finite() || binding.tolerance <= 0.0 {
            return Err("seam tolerance must be positive and finite".into());
        }
        if !objects.iter().any(|o| o.uuid == binding.object) {
            return Err("seam binding object missing".into());
        }
        if !ids.contains(&binding.seam) {
            return Err("bound seam missing".into());
        }
    }
    for seam in &seams.seams {
        for point in &seam.points {
            if let SeamPoint::Surface { object, .. } = point {
                if seams.bindings.iter().any(|b| b.object == *object) {
                    return Err(
                        "a surface seam source cannot also be deformed by a seam binding".into(),
                    );
                }
            }
        }
    }
    let mut draft = objects.to_vec();
    for object_index in 0..draft.len() {
        let object = &draft[object_index];
        let bindings = seams
            .bindings
            .iter()
            .filter(|b| b.object == object.uuid)
            .collect::<Vec<_>>();
        if bindings.is_empty() {
            continue;
        }
        if object.boolean_parent.is_some()
            || super::has_boolean_children(&draft, object.uuid)
            || object.repetition.enabled
            || object.surface_inlay.is_some()
        {
            return Err(
                "seam bindings require an independent object without repetition or surface inlay"
                    .into(),
            );
        }
        let frame = lattice_world_matrix(&draft, object.uuid);
        let inverse = frame.inverse();
        if !frame.is_finite() || !inverse.is_finite() || frame.determinant().abs() < 1e-10 {
            return Err("seam binding requires a nonsingular frame".into());
        }
        let mut lattice = object.lattice.clone();
        if let Some(cage) = &lattice {
            let n = cage.resolution as usize;
            if !(2..=9).contains(&n)
                || cage.offsets.len() != n * n * n
                || !cage.min.is_finite()
                || !cage.max.is_finite()
                || (cage.max - cage.min).min_element() <= 0.0
                || cage.offsets.iter().any(|p| !p.is_finite())
            {
                return Err("invalid bound lattice".into());
            }
        }
        let controls = lattice
            .as_ref()
            .map(|cage| {
                let n = cage.resolution as usize;
                let mut controls = Vec::new();
                for z in 0..n {
                    for y in 0..n {
                        for x in 0..n {
                            if cage.is_surface_point(x, y, z) {
                                controls.push(cage.index(x, y, z));
                            }
                        }
                    }
                }
                controls
            })
            .unwrap_or_default();
        let mut equations = Vec::new();
        let mut sheet_checks = Vec::new();
        let mut edge_checks = Vec::new();
        let mut occupied = Vec::new();
        for binding in bindings {
            if occupied.contains(&binding.target) {
                return Err("an edge can have only one seam binding".into());
            }
            occupied.push(binding.target);
            let seam = seams.seams.iter().find(|s| s.id == binding.seam).unwrap();
            let span = span_positions(seam, binding, &draft)?;
            match binding.target {
                SeamTarget::LatticeEdge(edge) => {
                    let cage = lattice
                        .as_ref()
                        .ok_or("add a lattice before connecting its edge to a seam")?;
                    let n = cage.resolution as usize;
                    if (n - 1) % (span.len() - 1) != 0 {
                        return Err("lattice edge stations must include every seam knot; change cage density or select a shorter span".into());
                    }
                    for station in 0..n {
                        let index = edge.control(cage, station);
                        let target = inverse
                            .transform_point3(seam_sample(&span, station as f32 / (n - 1) as f32));
                        let mut coefficients = vec![0.0; controls.len()];
                        coefficients[controls.iter().position(|i| *i == index).unwrap()] = 1.0;
                        let x = index % n;
                        let y = (index / n) % n;
                        let z = index / (n * n);
                        equations.push(Equation {
                            weights: coefficients,
                            target: target - cage.position(x, y, z),
                        });
                    }
                    edge_checks.push((edge, span, binding.tolerance));
                }
                SeamTarget::SheetBoundary(edge) => {
                    let SdfParams::SheetPanelParams(panel) = &object.params else {
                        return Err("sheet boundary connection requires a sheet panel".into());
                    };
                    panel.validate()?;
                    if !sheet_edge_survives(panel, edge) {
                        return Err(
                            "sheet boundary binding requires a surviving full material edge".into(),
                        );
                    }
                    if span.len() > 65 {
                        return Err(
                            "sheet boundary spans support at most 65 seam stations; split the span"
                                .into(),
                        );
                    }
                    let stations = (32usize).max((span.len() - 1) * 8);
                    for station in 0..=stations {
                        let t = station as f32 / stations as f32;
                        let rest =
                            panel.material_point(sheet_edge_material_point(edge, panel.size, t));
                        let rest = inverse.transform_point3(
                            object_world_matrix(&draft, object.uuid).transform_point3(rest),
                        );
                        let target = inverse.transform_point3(seam_sample(&span, t));
                        if let Some(cage) = &lattice {
                            if rest.cmplt(cage.min - Vec3::splat(1e-5)).any()
                                || rest.cmpgt(cage.max + Vec3::splat(1e-5)).any()
                            {
                                return Err(
                                    "bound sheet boundary must be inside its lattice cage".into()
                                );
                            }
                            equations.push(Equation {
                                weights: weights(cage, &controls, rest),
                                target: target - rest,
                            });
                            // Native inverse starts at the displaced location. Constrain
                            // both ends to the same displacement so all four inverse
                            // iterations settle on the intended material point.
                            equations.push(Equation {
                                weights: weights(cage, &controls, target),
                                target: target - rest,
                            });
                        }
                        sheet_checks.push((rest, target, binding.tolerance));
                    }
                }
            }
        }
        // A satisfied pose is already a valid solution. Re-projecting inconsistent
        // auxiliary equations moves free controls on every edit or document reload.
        // Preserve authored controls and shape keys when native boundary checks pass.
        if check_bound_pose(lattice.as_ref(), frame, &edge_checks, &sheet_checks).is_ok() {
            continue;
        }
        if let Some(cage) = &mut lattice {
            // Orthogonal row projections change only controls influencing the constraints.
            // Start at the current pose, preserving independent controls and shape-key intent.
            for _ in 0..256 {
                let mut largest = 0.0_f32;
                for equation in &equations {
                    let mut value = Vec3::ZERO;
                    let mut length = 0.0;
                    for (i, w) in controls.iter().zip(&equation.weights) {
                        value += cage.offsets[*i] * *w;
                        length += w * w;
                    }
                    let error = equation.target - value;
                    largest = largest.max(error.length());
                    if length > 1e-12 {
                        for (i, w) in controls.iter().zip(&equation.weights) {
                            cage.offsets[*i] += error * (*w / length);
                        }
                    }
                }
                if largest < 1e-6 {
                    break;
                }
            }
            if cage.offsets.iter().any(|p| !p.is_finite()) {
                return Err("seam connection produced a nonfinite lattice".into());
            }
            if !equations.is_empty() {
                cage.save_selected_shape_key();
            }
        }
        check_bound_pose(lattice.as_ref(), frame, &edge_checks, &sheet_checks)?;
        draft[object_index].lattice = lattice;
    }
    objects.clone_from_slice(&draft);
    Ok(())
}

/// Ray test against undeformed source triangles; returns a persistent attachment.
pub fn seam_surface_hit(
    objects: &[SdfObject],
    source: Uuid,
    origin: Vec3,
    direction: Vec3,
) -> Option<SeamPoint> {
    let object = objects.iter().find(|o| o.uuid == source)?;
    let mesh = object.imported_mesh.as_ref()?;
    let inverse = object_world_matrix(objects, source).inverse();
    let origin = inverse.transform_point3(origin);
    let direction = inverse.transform_vector3(direction);
    let mut nearest = f32::INFINITY;
    let mut result = None;
    for (triangle, face) in mesh.triangles.iter().enumerate() {
        let [a, b, c] = face.map(|i| mesh.positions.get(i as usize).copied());
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            continue;
        };
        let e = b - a;
        let f = c - a;
        let h = direction.cross(f);
        let det = e.dot(h);
        if det.abs() < 1e-10 {
            continue;
        }
        let s = origin - a;
        let u = s.dot(h) / det;
        let q = s.cross(e);
        let v = direction.dot(q) / det;
        let t = f.dot(q) / det;
        if u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t >= 0.0 && t < nearest {
            nearest = t;
            result = Some(SeamPoint::Surface {
                object: source,
                triangle,
                barycentric: Vec3::new(1.0 - u - v, u, v),
            });
        }
    }
    result
}

#[cfg(test)]
mod tests;

/// Closest world-space triangle point, retaining barycentric attachment data.
pub fn seam_surface_point(
    objects: &[SdfObject],
    source: Uuid,
    position: Vec3,
) -> Result<SeamPoint, String> {
    if !position.is_finite() {
        return Err("surface query must be finite".into());
    }
    let object = objects
        .iter()
        .find(|o| o.uuid == source)
        .ok_or("source mesh missing")?;
    let mesh = object
        .imported_mesh
        .as_ref()
        .ok_or("source must be a triangle mesh")?;
    let frame = object_world_matrix(objects, source);
    let mut nearest = f32::INFINITY;
    let mut result = None;
    for (triangle, face) in mesh.triangles.iter().enumerate() {
        let [a, b, c] = face.map(|i| {
            mesh.positions
                .get(i as usize)
                .map(|p| frame.transform_point3(*p))
        });
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            return Err("source mesh index missing".into());
        };
        let ab = b - a;
        let ac = c - a;
        let ap = position - a;
        let aa = ab.dot(ab);
        let bb = ab.dot(ac);
        let cc = ac.dot(ac);
        let determinant = aa * cc - bb * bb;
        let mut candidates = [Vec3::ZERO; 4];
        let mut count = 0;
        if determinant > 1e-20 {
            let u = (ap.dot(ab) * cc - ap.dot(ac) * bb) / determinant;
            let v = (ap.dot(ac) * aa - ap.dot(ab) * bb) / determinant;
            if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
                candidates[count] = Vec3::new(1.0 - u - v, u, v);
                count += 1;
            }
        }
        for (p, q, wp, wq) in [
            (a, b, Vec3::X, Vec3::Y),
            (b, c, Vec3::Y, Vec3::Z),
            (c, a, Vec3::Z, Vec3::X),
        ] {
            let edge = q - p;
            if edge.length_squared() > 1e-20 {
                let t = ((position - p).dot(edge) / edge.length_squared()).clamp(0.0, 1.0);
                candidates[count] = wp * (1.0 - t) + wq * t;
                count += 1;
            }
        }
        for barycentric in candidates.into_iter().take(count) {
            let p = a * barycentric.x + b * barycentric.y + c * barycentric.z;
            let distance = p.distance_squared(position);
            if distance < nearest {
                nearest = distance;
                result = Some(SeamPoint::Surface {
                    object: source,
                    triangle,
                    barycentric,
                });
            }
        }
    }
    let point = result.ok_or("source mesh has no nondegenerate triangles")?;
    seam_point_position(&point, objects)?;
    Ok(point)
}

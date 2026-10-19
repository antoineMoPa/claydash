//! CPU scene distance and ray intersection queries, shared by selection and future physics.
use super::{BooleanOperation, SdfObject};
use glam::Vec3;
pub fn boolean_distance(a: f32, b: f32, operation: BooleanOperation, softness: f32) -> f32 {
    let b = if operation == BooleanOperation::Subtract {
        -b
    } else {
        b
    };
    let hard = if operation == BooleanOperation::Union {
        a.min(b)
    } else {
        a.max(b)
    };
    if softness <= 0.0 {
        return hard;
    }
    let h = (softness - (a - b).abs()).max(0.0) / softness;
    let blend = softness * h * h * 0.25;
    if operation == BooleanOperation::Union {
        hard - blend
    } else {
        hard + blend
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub object: uuid::Uuid,
    pub position: Vec3,
}

/// Finds a visible scene surface using the editor's selection tolerances.
///
/// `direction` must be a unit vector. The march has a 100 world-unit range and
/// accepts distances below 0.01, including an origin inside a surface.
pub fn raymarch_hit(origin: Vec3, direction: Vec3, objects: &[SdfObject]) -> Option<RayHit> {
    let sdf_hit = raymarch_sdf_hit(origin, direction, objects);
    let mesh_hit = imported_mesh_hit(origin, direction, objects);
    let solid_hit = match (sdf_hit, mesh_hit) {
        (Some(a), Some(b)) => Some(if a.position.distance_squared(origin) <= b.position.distance_squared(origin) { a } else { b }),
        (a, b) => a.or(b),
    };
    let surface_depth = solid_hit.map_or(100., |hit| hit.position.distance(origin));
    let mut samples = Vec::new();
    for object in objects {
        let Some(cloud) = &object.imported_splats else { continue; };
        let matrix = crate::model::object_world_matrix(objects, object.uuid);
        let inverse = matrix.inverse();
        let o = inverse.transform_point3(origin);
        let d = inverse.transform_vector3(direction);
        for g in &cloud.splats {
            // Rasterized Gaussians have center depth, not a solid ellipsoid's
            // front surface. Large support volumes can even contain the camera.
            let depth = (matrix.transform_point3(g.position) - origin).dot(direction);
            if depth <= 0. || depth >= surface_depth { continue; }
            let local_o = (g.rotation.conjugate() * (o - g.position)) / g.scale;
            let local_d = (g.rotation.conjugate() * d) / g.scale;
            let a = local_d.length_squared();
            if !a.is_finite() || a <= 0. { continue; }
            let closest = -local_o.dot(local_d) / a;
            let radius2 = (local_o + local_d * closest).length_squared();
            if !radius2.is_finite() || radius2 >= 6.25 { continue; }
            // Same Gaussian falloff and support fade as splats.wgsl. Faint
            // tails should not steal clicks from visible solid objects.
            let fade = ((radius2 - 4.) / 2.25).clamp(0., 1.);
            let alpha = g.opacity * (-0.5 * radius2).exp()
                * (1. - fade * fade * (3. - 2. * fade));
            if alpha >= 0.002 { samples.push((depth, alpha, object.uuid)); }
        }
    }
    samples.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut transmission = 1.;
    for (depth, alpha, object) in samples {
        transmission *= 1. - alpha;
        if transmission <= 0.8 {
            return Some(RayHit { object, position: origin + direction * depth });
        }
    }
    solid_hit
}

fn imported_mesh_hit(origin: Vec3, direction: Vec3, objects: &[SdfObject]) -> Option<RayHit> {
    let mut best = 100.0_f32;
    let mut hit = None;
    for object in objects {
        let Some(mesh) = &object.imported_mesh else { continue };
        let inverse = crate::model::object_world_matrix(objects, object.uuid).inverse();
        let local_origin = inverse.transform_point3(origin);
        let local_direction = inverse.transform_vector3(direction);
        for triangle in &mesh.triangles {
            let [a, b, c] = triangle.map(|index| mesh.positions[index as usize]);
            let edge1 = b - a;
            let edge2 = c - a;
            let cross = local_direction.cross(edge2);
            let determinant = edge1.dot(cross);
            if determinant.abs() < 1e-8 { continue; }
            let inverse_determinant = determinant.recip();
            let from_a = local_origin - a;
            let u = from_a.dot(cross) * inverse_determinant;
            if !(0.0..=1.0).contains(&u) { continue; }
            let v = local_direction.dot(from_a.cross(edge1)) * inverse_determinant;
            if v < 0.0 || u + v > 1.0 { continue; }
            let depth = edge2.dot(from_a.cross(edge1)) * inverse_determinant;
            if depth > 0.0 && depth < best {
                best = depth;
                hit = Some(RayHit { object: object.uuid, position: origin + direction * depth });
            }
        }
    }
    hit
}

fn raymarch_sdf_hit(origin: Vec3, direction: Vec3, objects: &[SdfObject]) -> Option<RayHit> {
    let mut point = origin;
    let march_factor = objects.iter().fold(0.8_f32, |factor, object| {
        if let crate::model::SdfParams::LoftParams(loft) = &object.params {
            factor.min(loft.march_factor())
        } else if matches!(object.params, crate::model::SdfParams::SheetPanelParams(_)) {
            factor.min(0.35)
        } else {
            factor
        }
    });
    let max_steps = if march_factor < 0.5 { 128 } else { 64 };
    for _ in 0..max_steps {
        let (distance, object) = scene_distance(point, objects)?;
        if !distance.is_finite() { return None; }
        if distance < 0.01 {
            return Some(RayHit {
                object,
                position: point,
            });
        }
        point += direction * distance.max(0.003) * march_factor;
        if point.distance(origin) > 100.0 {
            return None;
        }
    }
    None
}

/// Returns the object owning [`raymarch_hit`]'s visible surface.
pub fn raymarch(origin: Vec3, direction: Vec3, objects: &[SdfObject]) -> Option<uuid::Uuid> {
    raymarch_hit(origin, direction, objects).map(|hit| hit.object)
}

fn scene_distance(point: Vec3, objects: &[SdfObject]) -> Option<(f32, uuid::Uuid)> {
    crate::model::scene_sample(point, objects)
}

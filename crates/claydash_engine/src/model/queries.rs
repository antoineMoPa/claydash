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
    let mut point = origin;
    let march_factor = objects.iter().fold(0.8_f32, |factor, object| {
        if let crate::model::SdfParams::LoftParams(loft) = &object.params {
            factor.min(loft.march_factor())
        } else {
            factor
        }
    });
    let max_steps = if march_factor < 0.5 { 128 } else { 64 };
    for _ in 0..max_steps {
        let (distance, object) = scene_distance(point, objects)?;
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

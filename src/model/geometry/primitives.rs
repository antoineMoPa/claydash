use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::SdfObject;

#[derive(Clone, Serialize, Deserialize)]
pub struct BoxParams {
    pub box_q: Vec3,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SphereParams {
    pub radius: f32,
}

pub const MAX_POLYGON_PRISM_VERTICES: usize = 32;

#[derive(Clone, Serialize, Deserialize)]
pub struct PolygonPrismParams {
    pub vertices: Vec<Vec2>,
    pub half_depth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BezierProfile {
    Round,
    Square,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BezierCurveParams {
    /// Adjacent cubic segments share an endpoint, so each extension adds three points.
    pub points: Vec<Vec3>,
    #[serde(default)]
    pub closed: bool,
}

impl BezierCurveParams {
    pub const MAX_SEGMENTS: usize = 8;

    pub fn segment_count(&self) -> usize {
        self.points.len().saturating_sub(1) / 3
    }

    pub fn point(&self, segment: usize, t: f32) -> Vec3 {
        let Some(points) = self.points.get(segment * 3..segment * 3 + 4) else {
            return Vec3::ZERO;
        };
        let u = 1.0 - t;
        points[0] * (u * u * u)
            + points[1] * (3.0 * u * u * t)
            + points[2] * (3.0 * u * t * t)
            + points[3] * (t * t * t)
    }

    pub fn tangent(&self, segment: usize, t: f32) -> Vec3 {
        let Some(p) = self.points.get(segment * 3..segment * 3 + 4) else {
            return Vec3::X;
        };
        let u = 1.0 - t;
        (p[1] - p[0]) * (3.0 * u * u)
            + (p[2] - p[1]) * (6.0 * u * t)
            + (p[3] - p[2]) * (3.0 * t * t)
    }

    fn second_derivative(&self, segment: usize, t: f32) -> Vec3 {
        let p = &self.points[segment * 3..segment * 3 + 4];
        (p[2] - p[1] * 2.0 + p[0]) * (6.0 * (1.0 - t)) + (p[3] - p[2] * 2.0 + p[1]) * (6.0 * t)
    }

    pub fn closest(&self, point: Vec3) -> Option<(Vec3, Vec3, usize, f32)> {
        let mut best = None;
        let mut best_distance = f32::INFINITY;
        for segment in 0..self.segment_count().min(Self::MAX_SEGMENTS) {
            let mut seed = 0.0;
            let mut seed_distance = f32::INFINITY;
            for step in 0..=12 {
                let t = step as f32 / 12.0;
                let distance = self.point(segment, t).distance_squared(point);
                if distance < seed_distance {
                    seed_distance = distance;
                    seed = t;
                }
            }
            let mut t = seed;
            for _ in 0..4 {
                let position = self.point(segment, t);
                let tangent = self.tangent(segment, t);
                let delta = position - point;
                let denominator =
                    tangent.length_squared() + delta.dot(self.second_derivative(segment, t));
                if denominator.abs() < 1e-6 {
                    break;
                }
                t = (t - delta.dot(tangent) / denominator).clamp(0.0, 1.0);
            }
            for candidate in [0.0, seed, t, 1.0] {
                let position = self.point(segment, candidate);
                let distance = position.distance_squared(point);
                if distance < best_distance {
                    best_distance = distance;
                    best = Some((
                        position,
                        self.tangent(segment, candidate),
                        segment,
                        candidate,
                    ));
                }
            }
        }
        best
    }

    pub fn local_extent(&self, radius: f32) -> Vec3 {
        self.points
            .iter()
            .fold(Vec3::ZERO, |extent, point| extent.max(point.abs()))
            + Vec3::splat(radius.max(0.0) * 1.42)
    }

    pub fn extend_from_end(&mut self) -> Option<usize> {
        if self.closed || self.segment_count() >= Self::MAX_SEGMENTS || self.points.len() < 4 {
            return None;
        }
        let last = *self.points.last()?;
        let direction = last - self.points[self.points.len() - 2];
        self.points.extend([
            last + direction,
            last + direction * 2.0,
            last + direction * 3.0,
        ]);
        Some(self.points.len() - 1)
    }

    pub fn extend_from_start(&mut self) -> Option<usize> {
        if self.closed || self.segment_count() >= Self::MAX_SEGMENTS || self.points.len() < 4 {
            return None;
        }
        let first = self.points[0];
        let direction = first - self.points[1];
        self.points.splice(
            0..0,
            [
                first + direction * 3.0,
                first + direction * 2.0,
                first + direction,
            ],
        );
        Some(0)
    }

    pub fn delete_anchor(&mut self, index: usize) -> Option<usize> {
        if index % 3 != 0 || index >= self.points.len() || self.segment_count() <= 1 {
            return None;
        }
        let last = self.points.len() - 1;
        if self.closed && index == last {
            return None;
        }
        let next_selection = if index == 0 {
            self.points.drain(0..3);
            if self.closed {
                let last = self.points.len() - 1;
                self.points[last] = self.points[0];
                self.points[last - 1] = self.points[0] - (self.points[1] - self.points[0]);
            }
            0
        } else if index == last {
            self.points.truncate(last - 2);
            self.points.len() - 1
        } else {
            self.points.drain(index - 1..=index + 1);
            index - 3
        };
        Some(next_selection)
    }

    pub fn close_from_end(&mut self) -> bool {
        if self.closed {
            return false;
        }
        let Some((&first, &last)) = self.points.first().zip(self.points.last()) else {
            return false;
        };
        if self.points.len() < 4 {
            return false;
        }
        if first.distance_squared(last) < 1e-8 {
            let last_index = self.points.len() - 1;
            self.points[last_index] = first;
            self.points[last_index - 1] = first - (self.points[1] - first);
            self.closed = true;
            return true;
        }
        if self.segment_count() >= Self::MAX_SEGMENTS {
            return false;
        }
        let outgoing = last - self.points[self.points.len() - 2];
        let incoming = first - (self.points[1] - first);
        self.points.extend([last + outgoing, incoming, first]);
        self.closed = true;
        true
    }

    pub fn close_at_current_tip(&mut self) -> bool {
        if self.closed || self.points.len() < 7 {
            return false;
        }
        let first = self.points[0];
        let last = self.points.len() - 1;
        self.points[last] = first;
        self.points[last - 1] = first - (self.points[1] - first);
        self.closed = true;
        true
    }

    pub fn close_at_current_start(&mut self) -> bool {
        if self.closed || self.points.len() < 7 {
            return false;
        }
        let last = *self.points.last().unwrap();
        let previous_handle = self.points[self.points.len() - 2];
        self.points[0] = last;
        self.points[1] = last + (last - previous_handle);
        self.closed = true;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathExtrusion {
    pub radius: f32,
    pub profile: BezierProfile,
    pub profile_curve: Option<uuid::Uuid>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurvePointSelection {
    pub object: uuid::Uuid,
    pub index: usize,
}

impl Default for PathExtrusion {
    fn default() -> Self {
        Self {
            radius: 0.12,
            profile: BezierProfile::Round,
            profile_curve: None,
        }
    }
}

pub(super) fn curve_profile_frame(curve: &BezierCurveParams, tangent: Vec3) -> (Vec3, Vec3) {
    let start = curve.tangent(0, 0.0).normalize_or_zero();
    let start = if start.length_squared() < 1e-8 {
        Vec3::X
    } else {
        start
    };
    let tangent = tangent.normalize_or_zero();
    let tangent = if tangent.length_squared() < 1e-8 {
        start
    } else {
        tangent
    };
    let reference = if start.y.abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let initial_side = start.cross(reference).normalize_or_zero();
    let axis = start.cross(tangent);
    let w = 1.0 + start.dot(tangent);
    let denominator = w * w + axis.length_squared();
    let side = if denominator < 1e-6 {
        initial_side
    } else {
        (initial_side + 2.0 * axis.cross(axis.cross(initial_side) + w * initial_side) / denominator)
            .normalize_or_zero()
    };
    (side, tangent.cross(side))
}

#[derive(Clone, Serialize, Deserialize)]
pub enum SdfParams {
    BoxParams(BoxParams),
    SphereParams(SphereParams),
    CylinderParams {
        radius: f32,
        half_height: f32,
    },
    TorusParams {
        major_radius: f32,
        minor_radius: f32,
    },
    PolygonPrismParams(PolygonPrismParams),
    #[serde(alias = "BezierExtrusionParams")]
    BezierCurveParams(BezierCurveParams),
}

pub fn polygon_distance(point: Vec2, vertices: &[Vec2]) -> f32 {
    if vertices.len() < 3 {
        return f32::INFINITY;
    }
    let mut distance_squared = f32::INFINITY;
    let mut inside = false;
    for index in 0..vertices.len() {
        let a = vertices[index];
        let b = vertices[(index + 1) % vertices.len()];
        let edge = b - a;
        let relative = point - a;
        let edge_length_squared = edge.length_squared();
        if edge_length_squared > 0.000_000_1 {
            let closest = a + edge * (relative.dot(edge) / edge_length_squared).clamp(0.0, 1.0);
            distance_squared = distance_squared.min(point.distance_squared(closest));
        }
        if (a.y > point.y) != (b.y > point.y) {
            let crossing_x = a.x + (point.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if point.x < crossing_x {
                inside = !inside;
            }
        }
    }
    distance_squared.sqrt() * if inside { -1.0 } else { 1.0 }
}

pub fn profile_curve_vertices(scene: &[SdfObject], id: uuid::Uuid) -> Option<Vec<Vec2>> {
    let object = scene.iter().find(|object| object.uuid == id)?;
    let SdfParams::BezierCurveParams(curve) = &object.params else {
        return None;
    };
    let mut vertices = Vec::new();
    for segment in 0..curve.segment_count().min(BezierCurveParams::MAX_SEGMENTS) {
        for step in 0..8 {
            vertices.push(curve.point(segment, step as f32 / 8.0).truncate());
        }
    }
    if curve.segment_count() > 0 {
        vertices.push(
            curve
                .point(
                    curve.segment_count().min(BezierCurveParams::MAX_SEGMENTS) - 1,
                    1.0,
                )
                .truncate(),
        );
    }
    (vertices.len() >= 3).then_some(vertices)
}

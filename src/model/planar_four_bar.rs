//! Fixed-length planar linkage driven by a point's vertical travel.
use super::{VariableSpace, VectorVariable};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircleIntersectionBranch {
    Positive,
    Negative,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanarFourBarConstraint {
    /// World point used as a travel control; only its Y component drives the linkage.
    pub driver: Uuid,
    pub lower_pivot: Uuid,
    pub upper_pivot: Uuid,
    pub lower_joint: Uuid,
    pub upper_joint: Uuid,
    pub lower_length: f32,
    pub upper_length: f32,
    pub upright_length: f32,
    /// Maps authored driver Y into the lower ball joint's world Y coordinate.
    pub driver_y_offset: f32,
    pub travel_min: f32,
    pub travel_max: f32,
    pub branch: CircleIntersectionBranch,
}

#[derive(Clone, Copy, Debug)]
pub struct PlanarFourBarPose {
    pub driver: Vec3,
    pub lower_joint: Vec3,
    pub upper_joint: Vec3,
}

/// The lower arm uses the positive-X arc; the upper joint uses an explicit
/// circle-intersection branch, so dragging never swaps the assembly's topology.
pub fn solve_planar_four_bar(
    constraint: &PlanarFourBarConstraint,
    driver: Vec3,
    lower_pivot: Vec3,
    upper_pivot: Vec3,
) -> Option<PlanarFourBarPose> {
    let c = constraint;
    if !driver.is_finite()
        || !lower_pivot.is_finite()
        || !upper_pivot.is_finite()
        || (lower_pivot.z - upper_pivot.z).abs() > 1e-5
        || ![
            c.lower_length,
            c.upper_length,
            c.upright_length,
            c.driver_y_offset,
            c.travel_min,
            c.travel_max,
        ]
        .iter()
        .all(|v| v.is_finite())
        || c.lower_length <= 0.0
        || c.upper_length <= 0.0
        || c.upright_length <= 0.0
        || c.travel_min > c.travel_max
    {
        return None;
    }
    let mut driver = driver;
    driver.y = driver.y.clamp(c.travel_min, c.travel_max);
    let lower_y = driver.y + c.driver_y_offset;
    let dy = lower_y - lower_pivot.y;
    let lower_x_squared = c.lower_length * c.lower_length - dy * dy;
    if lower_x_squared < 0.0 {
        return None;
    }
    let lower_joint = Vec3::new(
        lower_pivot.x + lower_x_squared.sqrt(),
        lower_y,
        lower_pivot.z,
    );
    let delta = lower_joint - upper_pivot;
    let distance = delta.length();
    if distance <= 1e-6
        || distance > c.upper_length + c.upright_length
        || distance < (c.upper_length - c.upright_length).abs()
    {
        return None;
    }
    let chord = (c.upper_length * c.upper_length - c.upright_length * c.upright_length
        + distance * distance)
        / (2.0 * distance);
    let height_squared = c.upper_length * c.upper_length - chord * chord;
    // Tangent circles can produce a tiny negative residue in floating point.
    if height_squared < -1e-5 * c.upper_length * c.upper_length {
        return None;
    }
    let heading = delta / distance;
    let perpendicular = Vec3::new(-heading.y, heading.x, 0.0);
    let sign = match c.branch {
        CircleIntersectionBranch::Positive => 1.0,
        CircleIntersectionBranch::Negative => -1.0,
    };
    let upper_joint =
        upper_pivot + heading * chord + perpendicular * (sign * height_squared.max(0.0).sqrt());
    upper_joint.is_finite().then_some(PlanarFourBarPose {
        driver,
        lower_joint,
        upper_joint,
    })
}

pub fn apply_four_bar_constraints(
    constraints: &[PlanarFourBarConstraint],
    variables: &mut [VectorVariable],
) {
    for constraint in constraints {
        let point = |id| {
            variables
                .iter()
                .find(|v| v.id == id && v.space == VariableSpace::World)
                .map(|v| v.value)
        };
        let (Some(driver), Some(lower), Some(upper)) = (
            point(constraint.driver),
            point(constraint.lower_pivot),
            point(constraint.upper_pivot),
        ) else {
            continue;
        };
        if ![constraint.lower_joint, constraint.upper_joint]
            .iter()
            .all(|id| {
                variables
                    .iter()
                    .any(|v| v.id == *id && v.space == VariableSpace::World)
            })
        {
            continue;
        }
        let Some(pose) = solve_planar_four_bar(constraint, driver, lower, upper) else {
            continue;
        };
        for (id, value) in [
            (constraint.driver, pose.driver),
            (constraint.lower_joint, pose.lower_joint),
            (constraint.upper_joint, pose.upper_joint),
        ] {
            if let Some(variable) = variables.iter_mut().find(|v| v.id == id) {
                variable.value = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linkage() -> PlanarFourBarConstraint {
        PlanarFourBarConstraint {
            driver: Uuid::new_v4(),
            lower_pivot: Uuid::new_v4(),
            upper_pivot: Uuid::new_v4(),
            lower_joint: Uuid::new_v4(),
            upper_joint: Uuid::new_v4(),
            lower_length: Vec3::new(2.16, 0.07, 0.0).length(),
            upper_length: Vec3::new(2.16, -0.19, 0.0).length(),
            upright_length: 0.91,
            driver_y_offset: -0.48,
            travel_min: -0.65,
            travel_max: 0.65,
            branch: CircleIntersectionBranch::Positive,
        }
    }

    #[test]
    fn suspension_preserves_three_link_lengths_through_travel() {
        let c = linkage();
        let lower_pivot = Vec3::new(-1.4, -0.55, 0.0);
        let upper_pivot = Vec3::new(-1.4, 0.62, 0.0);
        for step in -65..=65 {
            let pose = solve_planar_four_bar(
                &c,
                Vec3::new(1.15, step as f32 / 100.0, 0.0),
                lower_pivot,
                upper_pivot,
            )
            .unwrap();
            assert!((pose.lower_joint.distance(lower_pivot) - c.lower_length).abs() < 2e-6);
            assert!((pose.upper_joint.distance(upper_pivot) - c.upper_length).abs() < 2e-6);
            assert!((pose.upper_joint.distance(pose.lower_joint) - c.upright_length).abs() < 2e-6);
        }
        let neutral =
            solve_planar_four_bar(&c, Vec3::new(1.15, 0.0, 0.0), lower_pivot, upper_pivot).unwrap();
        assert!(neutral.lower_joint.distance(Vec3::new(0.76, -0.48, 0.0)) < 1e-6);
        assert!(neutral.upper_joint.distance(Vec3::new(0.76, 0.43, 0.0)) < 1e-6);
        let bump =
            solve_planar_four_bar(&c, Vec3::new(1.15, 0.4, 0.0), lower_pivot, upper_pivot).unwrap();
        assert!((bump.upper_joint.x - bump.lower_joint.x).abs() > 0.04);
    }

    #[test]
    fn travel_limits_clamp_and_impossible_geometry_is_rejected() {
        let mut c = linkage();
        let lower = Vec3::new(-1.4, -0.55, 0.0);
        let upper = Vec3::new(-1.4, 0.62, 0.0);
        let pose = solve_planar_four_bar(&c, Vec3::new(1.15, 20.0, 0.0), lower, upper).unwrap();
        assert_eq!(pose.driver.y, 0.65);
        c.upright_length = 10.0;
        assert!(solve_planar_four_bar(&c, Vec3::ZERO, lower, upper).is_none());
        c.travel_min = 2.0;
        assert!(solve_planar_four_bar(&c, Vec3::ZERO, lower, upper).is_none());
    }

    #[test]
    fn circle_branch_is_explicit_and_world_plane_is_required() {
        let mut c = linkage();
        let lower = Vec3::new(-1.4, -0.55, 0.0);
        let upper = Vec3::new(-1.4, 0.62, 0.0);
        let positive = solve_planar_four_bar(&c, Vec3::ZERO, lower, upper).unwrap();
        c.branch = CircleIntersectionBranch::Negative;
        let negative = solve_planar_four_bar(&c, Vec3::ZERO, lower, upper).unwrap();
        assert!(positive.upper_joint.distance(negative.upper_joint) > 1.0);
        assert!(
            (negative.upper_joint.distance(negative.lower_joint) - c.upright_length).abs() < 2e-6
        );
        assert!((negative.upper_joint.distance(upper) - c.upper_length).abs() < 2e-6);
        assert!(solve_planar_four_bar(&c, Vec3::ZERO, lower, upper + Vec3::Z).is_none());
        assert!(solve_planar_four_bar(&c, Vec3::splat(f32::NAN), lower, upper).is_none());
    }
}

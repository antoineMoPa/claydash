use glam::{Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::{SdfObject, SdfParams};

/// Radial repetition uses the owning primitive's local coordinate system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepetitionAxis {
    X,
    Y,
    #[default]
    Z,
}

impl RepetitionAxis {
    pub fn rotation(self, angle: f32) -> Quat {
        match self {
            Self::X => Quat::from_rotation_x(angle),
            Self::Y => Quat::from_rotation_y(angle),
            Self::Z => Quat::from_rotation_z(angle),
        }
    }
    pub fn gpu_code(self) -> i32 {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum RepetitionMode {
    #[default]
    Linear,
    Radial {
        axis: RepetitionAxis,
        count: u32,
        pivot: Vec3,
    },
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Repetition {
    pub mode: RepetitionMode,
    pub enabled: bool,
    pub count: [u32; 3],
    pub spacing: Vec3,
}

#[derive(Deserialize)]
struct StoredRepetition {
    #[serde(default)]
    mode: RepetitionMode,
    enabled: bool,
    #[serde(default)]
    axes: Option<[bool; 3]>,
    count: [u32; 3],
    spacing: Vec3,
}

impl<'de> Deserialize<'de> for Repetition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredRepetition::deserialize(deserializer)?;
        let count = stored.axes.map_or(stored.count, |axes| {
            std::array::from_fn(|axis| if axes[axis] { stored.count[axis] } else { 1 })
        });
        Ok(Self {
            mode: stored.mode,
            enabled: stored.enabled,
            count,
            spacing: stored.spacing,
        })
    }
}

impl Default for Repetition {
    fn default() -> Self {
        Self {
            mode: RepetitionMode::Linear,
            enabled: false,
            count: [3, 1, 1],
            spacing: Vec3::splat(0.8),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mirror {
    pub axes: [bool; 3],
}

/// Restricts this object's SDF to a thin shell following another SDF primitive.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceInlay {
    pub host: uuid::Uuid,
    pub offset: f32,
    pub thickness: f32,
}

impl Default for Mirror {
    fn default() -> Self {
        Self {
            axes: [true, false, false],
        }
    }
}

impl Mirror {
    pub fn fold_point(self, point: Vec3, matrix: Mat4) -> Vec3 {
        let mut local = matrix.inverse().transform_point3(point);
        for axis in 0..3 {
            if self.axes[axis] {
                local[axis] = local[axis].abs();
            }
        }
        matrix.transform_point3(local)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// A regular three-dimensional cage in the owning group's local space.
/// Only control-point offsets are stored; undeformed points are implicit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lattice {
    pub resolution: u8,
    pub min: Vec3,
    pub max: Vec3,
    pub offsets: Vec<Vec3>,
    #[serde(default)]
    pub shape_keys: Vec<LatticeShapeKey>,
    #[serde(default)]
    pub current_shape_key: Option<usize>,
    #[serde(default)]
    pub shape_position: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatticeShapeKey {
    pub name: String,
    pub offsets: Vec<Vec3>,
}

impl Lattice {
    pub fn new(min: Vec3, max: Vec3, resolution: u8) -> Self {
        let resolution = resolution.clamp(2, 9);
        Self {
            resolution,
            min,
            max,
            offsets: vec![
                Vec3::ZERO;
                resolution as usize * resolution as usize * resolution as usize
            ],
            shape_keys: Vec::new(),
            current_shape_key: Some(0),
            shape_position: 0.0,
        }
    }

    pub fn select_shape_key(&mut self, index: usize) {
        if index > self.shape_keys.len() {
            return;
        }
        self.current_shape_key = Some(index);
        self.apply_shape_position(index as f32);
    }

    pub fn add_shape_key(&mut self) -> usize {
        let index = self.shape_keys.len() + 1;
        self.shape_keys.push(LatticeShapeKey {
            name: format!("Shape {index}"),
            offsets: self.offsets.clone(),
        });
        self.select_shape_key(index);
        index
    }

    pub fn save_selected_shape_key(&mut self) {
        if let Some(index) = self.current_shape_key.filter(|index| *index > 0) {
            if let Some(key) = self.shape_keys.get_mut(index - 1) {
                key.offsets = self.offsets.clone();
            }
        }
    }

    pub fn apply_shape_position(&mut self, value: f32) {
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(0.0, self.shape_keys.len() as f32);
        let lower = value.floor() as usize;
        let upper = value.ceil() as usize;
        let amount = value - lower as f32;
        let count = self.offsets.len();
        let left = if lower == 0 {
            None
        } else {
            self.shape_keys.get(lower - 1).map(|key| &key.offsets)
        };
        let right = if upper == 0 {
            None
        } else {
            self.shape_keys.get(upper - 1).map(|key| &key.offsets)
        };
        if left.is_some_and(|offsets| offsets.len() != count)
            || right.is_some_and(|offsets| offsets.len() != count)
        {
            return;
        }
        for index in 0..count {
            let a = left.map_or(Vec3::ZERO, |offsets| offsets[index]);
            let b = right.map_or(Vec3::ZERO, |offsets| offsets[index]);
            self.offsets[index] = a.lerp(b, amount);
        }
        self.shape_position = value;
    }

    pub fn index(&self, x: usize, y: usize, z: usize) -> usize {
        let n = self.resolution as usize;
        x + n * (y + n * z)
    }

    pub fn position(&self, x: usize, y: usize, z: usize) -> Vec3 {
        let denominator = (self.resolution - 1) as f32;
        self.min + (self.max - self.min) * Vec3::new(x as f32, y as f32, z as f32) / denominator
    }

    pub fn is_surface_point(&self, x: usize, y: usize, z: usize) -> bool {
        let last = self.resolution as usize - 1;
        x == 0 || y == 0 || z == 0 || x == last || y == last || z == last
    }

    pub fn effective_offsets(&self) -> Vec<Vec3> {
        let n = self.resolution as usize;
        if self.offsets.len() != n * n * n {
            return vec![Vec3::ZERO; n * n * n];
        }
        let mut result = self.offsets.clone();
        let last = n - 1;
        for z in 1..last {
            for y in 1..last {
                for x in 1..last {
                    let tx = x as f32 / last as f32;
                    let ty = y as f32 / last as f32;
                    let tz = z as f32 / last as f32;
                    let along_x = self.offsets[self.index(0, y, z)]
                        .lerp(self.offsets[self.index(last, y, z)], tx);
                    let along_y = self.offsets[self.index(x, 0, z)]
                        .lerp(self.offsets[self.index(x, last, z)], ty);
                    let along_z = self.offsets[self.index(x, y, 0)]
                        .lerp(self.offsets[self.index(x, y, last)], tz);
                    result[self.index(x, y, z)] = (along_x + along_y + along_z) / 3.0;
                }
            }
        }
        result
    }

    pub fn displacement(&self, position: Vec3) -> Vec3 {
        self.displacement_with_offsets(position, &self.effective_offsets())
    }

    pub(crate) fn displacement_with_offsets(&self, position: Vec3, offsets: &[Vec3]) -> Vec3 {
        let n = self.resolution as usize;
        if n < 2 || offsets.len() != n * n * n {
            return Vec3::ZERO;
        }
        let coordinates = ((position - self.min) / (self.max - self.min).max(Vec3::splat(0.0001)))
            .clamp(Vec3::ZERO, Vec3::ONE)
            * (n - 1) as f32;
        let lower = coordinates.floor().as_uvec3();
        let upper = (lower + glam::UVec3::ONE).min(glam::UVec3::splat((n - 1) as u32));
        let t = coordinates - lower.as_vec3();
        let sample =
            |x: u32, y: u32, z: u32| offsets[self.index(x as usize, y as usize, z as usize)];
        let a = sample(lower.x, lower.y, lower.z).lerp(sample(upper.x, lower.y, lower.z), t.x);
        let b = sample(lower.x, upper.y, lower.z).lerp(sample(upper.x, upper.y, lower.z), t.x);
        let c = sample(lower.x, lower.y, upper.z).lerp(sample(upper.x, lower.y, upper.z), t.x);
        let d = sample(lower.x, upper.y, upper.z).lerp(sample(upper.x, upper.y, upper.z), t.x);
        a.lerp(b, t.y).lerp(c.lerp(d, t.y), t.z)
    }

    pub fn resize(&mut self, resolution: u8) {
        let resolution = resolution.clamp(2, 9);
        if resolution == self.resolution {
            return;
        }
        let previous = self.clone();
        *self = Self::new(previous.min, previous.max, resolution);
        self.current_shape_key = previous.current_shape_key;
        self.shape_position = previous.shape_position;
        self.shape_keys = previous
            .shape_keys
            .iter()
            .map(|key| LatticeShapeKey {
                name: key.name.clone(),
                offsets: vec![
                    Vec3::ZERO;
                    resolution as usize * resolution as usize * resolution as usize
                ],
            })
            .collect();
        for z in 0..resolution as usize {
            for y in 0..resolution as usize {
                for x in 0..resolution as usize {
                    let index = self.index(x, y, z);
                    self.offsets[index] = previous.displacement(self.position(x, y, z));
                }
            }
        }
        let min = self.min;
        let max = self.max;
        let denominator = (resolution - 1) as f32;
        for (key, previous_key) in self.shape_keys.iter_mut().zip(&previous.shape_keys) {
            let mut previous_shape = previous.clone();
            previous_shape.offsets = previous_key.offsets.clone();
            for z in 0..resolution as usize {
                for y in 0..resolution as usize {
                    for x in 0..resolution as usize {
                        let index = x + resolution as usize * (y + resolution as usize * z);
                        let position = min
                            + (max - min) * Vec3::new(x as f32, y as f32, z as f32) / denominator;
                        key.offsets[index] = previous_shape.displacement(position);
                    }
                }
            }
        }
    }
}

pub fn has_boolean_children(scene: &[SdfObject], id: uuid::Uuid) -> bool {
    scene.iter().any(|object| object.boolean_parent == Some(id))
}

pub fn group_world_matrix(scene: &[SdfObject], id: uuid::Uuid) -> Mat4 {
    fn visit(
        scene: &[SdfObject],
        id: uuid::Uuid,
        visited: &mut std::collections::HashSet<uuid::Uuid>,
    ) -> Mat4 {
        if !visited.insert(id) {
            return Mat4::IDENTITY;
        }
        let Some(object) = scene.iter().find(|object| object.uuid == id) else {
            return Mat4::IDENTITY;
        };
        let parent = object
            .boolean_parent
            .map(|parent| visit(scene, parent, visited))
            .unwrap_or(Mat4::IDENTITY);
        parent * object.group_transform.matrix()
    }
    visit(scene, id, &mut std::collections::HashSet::new())
}

pub fn parent_group_world_matrix(scene: &[SdfObject], id: uuid::Uuid) -> Mat4 {
    scene
        .iter()
        .find(|object| object.uuid == id)
        .and_then(|object| object.boolean_parent)
        .map(|parent| group_world_matrix(scene, parent))
        .unwrap_or(Mat4::IDENTITY)
}

pub fn object_world_matrix(scene: &[SdfObject], id: uuid::Uuid) -> Mat4 {
    let Some(object) = scene.iter().find(|object| object.uuid == id) else {
        return Mat4::IDENTITY;
    };
    group_world_matrix(scene, id) * object.transform.matrix()
}

pub fn lattice_world_matrix(scene: &[SdfObject], id: uuid::Uuid) -> Mat4 {
    if has_boolean_children(scene, id) {
        group_world_matrix(scene, id)
    } else {
        object_world_matrix(scene, id)
    }
}

pub fn lattice_bounds(scene: &[SdfObject], root: uuid::Uuid) -> Option<(Vec3, Vec3)> {
    subtree_bounds(scene, root, false)
}

/// Capture all reflected copies without changing the editable lattice frame.
pub(crate) fn capture_bounds(scene: &[SdfObject], root: uuid::Uuid) -> Option<(Vec3, Vec3)> {
    subtree_bounds(scene, root, true)
}

fn subtree_bounds(
    scene: &[SdfObject],
    root: uuid::Uuid,
    include_mirrors: bool,
) -> Option<(Vec3, Vec3)> {
    let root_inverse = lattice_world_matrix(scene, root).inverse();
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for object in scene {
        let mut ancestor = Some(object.uuid);
        let mut belongs = false;
        for _ in 0..scene.len() {
            let Some(id) = ancestor else {
                break;
            };
            if id == root {
                belongs = true;
                break;
            }
            ancestor = scene
                .iter()
                .find(|candidate| candidate.uuid == id)
                .and_then(|candidate| candidate.boolean_parent);
        }
        if !belongs {
            continue;
        }
        let extent = match &object.params {
            SdfParams::SphereParams(p) => Vec3::splat(p.radius),
            SdfParams::BoxParams(p) => p.box_q,
            SdfParams::CylinderParams {
                radius,
                half_height,
            } => Vec3::new(*radius, *half_height, *radius),
            SdfParams::TorusParams {
                major_radius,
                minor_radius,
            } => Vec3::new(
                major_radius + minor_radius,
                *minor_radius,
                major_radius + minor_radius,
            ),
            SdfParams::PolygonPrismParams(p) => {
                let planar = p
                    .vertices
                    .iter()
                    .fold(Vec2::ZERO, |size, point| size.max(point.abs()));
                let softness = p.edge_softness.max(0.0).min(p.half_depth);
                Vec3::new(planar.x + softness, planar.y + softness, p.half_depth)
            }
            SdfParams::LoftParams(p) => p.local_extent(),
            SdfParams::BezierCurveParams(p) => p.local_extent(
                object
                    .path_extrusion
                    .map_or(0.0, |modifier| modifier.radius),
            ),
            SdfParams::TextParams(_) => {
                super::prepared_scene_text(scene, object).map_or(Vec3::ZERO, |p| p.local_extent())
            }
        };
        let mut extent = extent;
        if object.repetition.enabled {
            if let RepetitionMode::Radial { pivot, .. } = object.repetition.mode {
                if !scene
                    .iter()
                    .any(|child| child.boolean_parent == Some(object.uuid))
                {
                    extent = Vec3::splat(extent.length() + 2.0 * pivot.length());
                }
            }
            for axis in 0..3 {
                if object.repetition.mode == RepetitionMode::Linear
                    && object.repetition.count[axis] > 1
                {
                    extent[axis] += object.repetition.count[axis].saturating_sub(1) as f32
                        * object.repetition.spacing[axis].max(0.001)
                        * 0.5;
                }
            }
        }
        let matrix = root_inverse * object_world_matrix(scene, object.uuid);
        let center = matrix.transform_point3(Vec3::ZERO);
        let half = matrix.x_axis.truncate().abs() * extent.x
            + matrix.y_axis.truncate().abs() * extent.y
            + matrix.z_axis.truncate().abs() * extent.z;
        let mut part_min = center - half;
        let mut part_max = center + half;
        if include_mirrors {
            let mut ancestor = Some(object.uuid);
            for _ in 0..scene.len() {
                let Some(node) = ancestor.and_then(|id| scene.iter().find(|node| node.uuid == id))
                else {
                    break;
                };
                if let Some(mirror) = node.mirror {
                    let frame = root_inverse * group_world_matrix(scene, node.uuid);
                    let inverse = frame.inverse();
                    let center = (part_min + part_max) * 0.5;
                    let half = (part_max - part_min) * 0.5;
                    for mask in 1..8 {
                        if (0..3).any(|axis| mask & (1 << axis) != 0 && !mirror.axes[axis]) {
                            continue;
                        }
                        let signs = Vec3::from_array(std::array::from_fn(|axis| {
                            if mask & (1 << axis) != 0 {
                                -1.0
                            } else {
                                1.0
                            }
                        }));
                        let reflected = frame * Mat4::from_scale(signs) * inverse;
                        let reflected_center = reflected.transform_point3(center);
                        let reflected_half = reflected.x_axis.truncate().abs() * half.x
                            + reflected.y_axis.truncate().abs() * half.y
                            + reflected.z_axis.truncate().abs() * half.z;
                        part_min = part_min.min(reflected_center - reflected_half);
                        part_max = part_max.max(reflected_center + reflected_half);
                    }
                }
                if node.uuid == root {
                    break;
                }
                ancestor = node.boolean_parent;
            }
        }
        // Repetition belongs to the completed group, so additive operands must
        // rotate with it too. Walk outward to include nested radial groups.
        let mut ancestor = Some(object.uuid);
        for _ in 0..scene.len() {
            let Some(node) = ancestor.and_then(|id| scene.iter().find(|node| node.uuid == id))
            else {
                break;
            };
            if node.repetition.enabled
                && scene
                    .iter()
                    .any(|child| child.boolean_parent == Some(node.uuid))
            {
                if let RepetitionMode::Radial { axis, count, pivot } = node.repetition.mode {
                    let frame = root_inverse * object_world_matrix(scene, node.uuid);
                    let inverse = frame.inverse();
                    let mut radial_min = Vec3::splat(f32::INFINITY);
                    let mut radial_max = Vec3::splat(f32::NEG_INFINITY);
                    for copy in 0..count.clamp(1, 32) {
                        let rotation = axis.rotation(
                            std::f32::consts::TAU * copy as f32 / count.clamp(1, 32) as f32,
                        );
                        for x in [part_min.x, part_max.x] {
                            for y in [part_min.y, part_max.y] {
                                for z in [part_min.z, part_max.z] {
                                    let local = inverse.transform_point3(Vec3::new(x, y, z));
                                    let corner =
                                        frame.transform_point3(pivot + rotation * (local - pivot));
                                    radial_min = radial_min.min(corner);
                                    radial_max = radial_max.max(corner);
                                }
                            }
                        }
                    }
                    part_min = radial_min;
                    part_max = radial_max;
                }
            }
            if node.uuid == root {
                break;
            }
            ancestor = node.boolean_parent;
        }
        minimum = minimum.min(part_min);
        maximum = maximum.max(part_max);
    }
    if !minimum.is_finite() {
        return None;
    }
    let padding = (maximum - minimum).max(Vec3::splat(0.1)) * 0.1;
    Some((minimum - padding, maximum + padding))
}

#[cfg(test)]
mod radial_repetition_tests {
    use super::*;
    use crate::model::{PrimitiveKind, SdfParams};

    fn spoke() -> SdfObject {
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        if let SdfParams::BoxParams(params) = &mut object.params {
            params.box_q = Vec3::new(0.65, 0.035, 0.035);
            params.corner_radius = 0.0;
        }
        object.repetition.enabled = true;
        object.repetition.mode = RepetitionMode::Radial {
            axis: RepetitionAxis::Z,
            count: 12,
            pivot: Vec3::new(-0.8, 0.0, 0.0),
        };
        object
    }

    #[test]
    fn radial_spokes_match_explicit_rotated_primitives() {
        let object = spoke();
        let RepetitionMode::Radial { axis, count, pivot } = object.repetition.mode else {
            unreachable!()
        };
        let mut original = object.clone();
        original.repetition.enabled = false;
        for x in -20..=20 {
            for y in -20..=20 {
                let point = Vec3::new(x as f32 * 0.1, y as f32 * 0.1, 0.01);
                let expected = (0..count)
                    .map(|copy| {
                        let rotation =
                            axis.rotation(-std::f32::consts::TAU * copy as f32 / count as f32);
                        original.distance(pivot + rotation * (point - pivot))
                    })
                    .fold(f32::INFINITY, f32::min);
                assert!((object.distance(point) - expected).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn radial_settings_round_trip_and_legacy_files_stay_linear() {
        let object = spoke();
        let saved = serde_json::to_string(&object.repetition).unwrap();
        let loaded: Repetition = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.mode, object.repetition.mode);
        let legacy: Repetition =
            serde_json::from_str(r#"{"enabled":true,"count":[3,1,1],"spacing":[1,1,1]}"#).unwrap();
        assert_eq!(legacy.mode, RepetitionMode::Linear);
    }

    #[test]
    fn radial_copies_remain_pickable_and_fit_capture_bounds() {
        let object = spoke();
        let id = object.uuid;
        let scene = vec![object];
        let pivot = Vec3::new(-0.8, 0.0, 0.0);
        let (minimum, maximum) = capture_bounds(&scene, id).unwrap();
        for copy in 0..12 {
            let rotation = Quat::from_rotation_z(std::f32::consts::TAU * copy as f32 / 12.0);
            let point = pivot + rotation * (-pivot);
            let (distance, owner) = crate::model::scene_sample(point, &scene).unwrap();
            assert!(distance < 0.0);
            assert_eq!(owner, id);
            assert!(point.cmpge(minimum).all() && point.cmple(maximum).all());
        }
    }

    #[test]
    fn radial_cut_groups_match_explicit_rotated_subtrees_and_prepared_sampler() {
        let mut root = spoke();
        root.transform.scale = Vec3::new(1.1, 0.8, 1.4);
        root.transform.rotation = Quat::from_rotation_y(0.23);
        root.transform.translation = Vec3::new(0.2, -0.15, 0.3);
        let mut cut = SdfObject::create_kind(PrimitiveKind::Box);
        cut.boolean_parent = Some(root.uuid);
        cut.operation = crate::model::BooleanOperation::Subtract;
        cut.transform = root.transform;
        if let SdfParams::BoxParams(params) = &mut cut.params {
            params.box_q = Vec3::new(0.12, 0.2, 0.2);
        }
        let scene = vec![root.clone(), cut];
        let mut original = scene.clone();
        original[0].repetition.enabled = false;
        let matrix = object_world_matrix(&scene, root.uuid);
        let inverse = matrix.inverse();
        let pivot = Vec3::new(-0.8, 0.0, 0.0);
        let mut prepared = crate::model::PreparedSubtreeSampler::new(&scene, root.uuid).unwrap();
        for x in -15..=10 {
            for y in -15..=15 {
                let point = Vec3::new(x as f32 * 0.1, y as f32 * 0.1, 0.3);
                let local = inverse.transform_point3(point);
                let expected = (0..12)
                    .map(|copy| {
                        let rotation =
                            Quat::from_rotation_z(-std::f32::consts::TAU * copy as f32 / 12.0);
                        let sample = matrix.transform_point3(pivot + rotation * (local - pivot));
                        crate::model::scene_sample(sample, &original).unwrap().0
                    })
                    .fold(f32::INFINITY, f32::min);
                let actual = crate::model::scene_sample(point, &scene).unwrap().0;
                assert!((actual - expected).abs() < 1e-5);
                assert!((prepared.sample(point).0 - expected).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn radial_group_capture_bounds_include_distant_additive_operands() {
        let root = spoke();
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        child.transform.translation = Vec3::new(4.0, 0.0, 0.0);
        let scene = vec![root.clone(), child];
        let (minimum, maximum) = capture_bounds(&scene, root.uuid).unwrap();
        let pivot = Vec3::new(-0.8, 0.0, 0.0);
        for copy in 0..12 {
            let rotation = Quat::from_rotation_z(std::f32::consts::TAU * copy as f32 / 12.0);
            let point = pivot + rotation * (Vec3::new(4.0, 0.0, 0.0) - pivot);
            assert!(crate::model::scene_sample(point, &scene).unwrap().0 < 0.0);
            assert!(point.cmpge(minimum).all() && point.cmple(maximum).all());
        }
    }

    #[test]
    fn radial_count_limits_are_finite() {
        let mut object = spoke();
        for count in [0, 1, 32, u32::MAX] {
            object.repetition.mode = RepetitionMode::Radial {
                axis: RepetitionAxis::X,
                count,
                pivot: Vec3::ZERO,
            };
            assert!(object.distance(Vec3::ONE).is_finite());
        }
    }
}

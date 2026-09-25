use glam::{Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::{SdfObject, SdfParams};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Repetition {
    pub enabled: bool,
    pub count: [u32; 3],
    pub spacing: Vec3,
}

#[derive(Deserialize)]
struct StoredRepetition {
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
            enabled: stored.enabled,
            count,
            spacing: stored.spacing,
        })
    }
}

impl Default for Repetition {
    fn default() -> Self {
        Self {
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
        let n = self.resolution as usize;
        if self.offsets.len() != n * n * n {
            return Vec3::ZERO;
        }
        let coordinates = ((position - self.min) / (self.max - self.min).max(Vec3::splat(0.0001)))
            .clamp(Vec3::ZERO, Vec3::ONE)
            * (n - 1) as f32;
        let lower = coordinates.floor().as_uvec3();
        let upper = (lower + glam::UVec3::ONE).min(glam::UVec3::splat((n - 1) as u32));
        let t = coordinates - lower.as_vec3();
        let offsets = self.effective_offsets();
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
                Vec3::new(planar.x, planar.y, p.half_depth)
            }
            SdfParams::BezierCurveParams(p) => p.local_extent(
                object
                    .path_extrusion
                    .map_or(0.0, |modifier| modifier.radius),
            ),
        };
        let mut extent = extent;
        if object.repetition.enabled {
            for axis in 0..3 {
                if object.repetition.count[axis] > 1 {
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
        minimum = minimum.min(center - half);
        maximum = maximum.max(center + half);
    }
    if !minimum.is_finite() {
        return None;
    }
    let padding = (maximum - minimum).max(Vec3::splat(0.1)) * 0.1;
    Some((minimum - padding, maximum + padding))
}

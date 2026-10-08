use glam::{Mat4, Vec2, Vec3};
#[cfg(not(target_arch = "wasm32"))]
use glam::{Quat, Vec4};
#[cfg(not(target_arch = "wasm32"))]
use sdf_consts::{TYPE_BOX, TYPE_SPHERE};
use super::{boolean_distance, object_world_matrix, BooleanOperation, SdfObject, SdfParams};
#[cfg(not(target_arch = "wasm32"))]
use super::{BoxParams, GroupRenderRepresentation, Material, MaterialKind, PolygonPrismParams,
    PrimitiveKind, SphereParams, WoodSpecies};

/// Deterministic visual QA scene: materials above, boolean operations below.
#[cfg(not(target_arch = "wasm32"))]
pub fn ui_preview_scene() -> Vec<SdfObject> {
    let mut scene = Vec::new();
    for (index, kind) in [
        MaterialKind::Solid,
        MaterialKind::Metallic,
        MaterialKind::Transparent,
    ]
    .into_iter()
    .enumerate()
    {
        let mut sphere = SdfObject::create(TYPE_SPHERE);
        sphere.name = kind.label().into();
        sphere.params = SdfParams::SphereParams(SphereParams { radius: 0.48 });
        sphere.transform.translation = Vec3::new((index as f32 - 1.0) * 1.35, 0.7, 0.0);
        sphere.material = Material::preset(kind);
        sphere.color = sphere.material.color;
        scene.push(sphere);
    }
    let mut backdrop = SdfObject::create(TYPE_BOX);
    backdrop.name = "Orange bar behind glass".into();
    backdrop.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::new(2.1, 0.12, 0.12),
        corner_radius: 0.0,
    });
    backdrop.transform.translation = Vec3::new(0.0, 0.7, -0.9);
    backdrop.material.color = Vec4::new(1.0, 0.23, 0.025, 1.0);
    backdrop.color = backdrop.material.color;
    scene.push(backdrop);
    for (index, operation) in [
        BooleanOperation::Union,
        BooleanOperation::Subtract,
        BooleanOperation::Intersect,
    ]
    .into_iter()
    .enumerate()
    {
        let mut target = SdfObject::create(TYPE_BOX);
        target.name = format!("{} target", operation.label());
        target.params = SdfParams::BoxParams(BoxParams {
            box_q: Vec3::splat(0.4),
            corner_radius: 0.0,
        });
        target.transform.translation = Vec3::new((index as f32 - 1.0) * 1.35, -0.65, 0.0);
        target.material.color = Vec4::new(0.15, 0.6, 0.8, 1.0);
        target.color = target.material.color;
        let mut operand = SdfObject::create(TYPE_SPHERE);
        operand.name = format!("{} operand", operation.label());
        operand.params = SdfParams::SphereParams(SphereParams { radius: 0.43 });
        operand.transform.translation = target.transform.translation + Vec3::new(0.18, 0.16, 0.35);
        operand.boolean_parent = Some(target.uuid);
        operand.operation = operation;
        operand.material = target.material;
        operand.color = target.color;
        scene.extend([target, operand]);
    }
    scene
}

/// Evaluate each subtree before combining it with its parent. Parent links are
/// independent of storage order, and root objects always union with one another.
pub fn scene_sample(point: Vec3, scene: &[SdfObject]) -> Option<(f32, uuid::Uuid)> {
    scene
        .iter()
        .enumerate()
        .filter(|(_, object)| object.boolean_parent.is_none())
        .map(|(index, _)| subtree_sample_at(point, scene, index, 0))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// Evaluate one Boolean subtree while retaining its ancestor transforms.
#[cfg(test)]
pub fn scene_subtree_sample(
    point: Vec3,
    scene: &[SdfObject],
    root: uuid::Uuid,
) -> Option<(f32, uuid::Uuid)> {
    scene
        .iter()
        .position(|object| object.uuid == root)
        .map(|index| subtree_sample_at(point, scene, index, 0))
}

fn subtree_sample_at(
    point: Vec3,
    scene: &[SdfObject],
    index: usize,
    depth: usize,
) -> (f32, uuid::Uuid) {
    let object = &scene[index];
    let has_children = scene
        .iter()
        .any(|child| child.boolean_parent == Some(object.uuid));
    let point = if let Some(mirror) = object.mirror {
        mirror.fold_point(point, super::group_world_matrix(scene, object.uuid))
    } else {
        point
    };
    let point = if has_children && object.repetition.enabled {
        let group = super::group_world_matrix(scene, object.uuid);
        group.transform_point3(object.repeated_local_point(group.inverse().transform_point3(point)))
    } else {
        point
    };
    let matrix = object_world_matrix(scene, object.uuid);
    let profile = object
        .path_extrusion
        .and_then(|modifier| modifier.profile_curve)
        .and_then(|id| super::profile_curve_vertices(scene, id));
    let prepared_text = if matches!(object.params, SdfParams::TextParams(_)) {
        super::prepared_scene_text(scene, object).ok()
    } else {
        None
    };
    let mut object_distance = object.distance_with_matrix_with_geometry(
        point,
        matrix,
        !(has_children && object.repetition.enabled),
        profile.as_deref(),
        prepared_text.as_deref(),
    );
    if let Some(inlay) = object.surface_inlay {
        if let Some(host_index) = scene
            .iter()
            .position(|candidate| candidate.uuid == inlay.host)
        {
            let host_distance = subtree_sample_at(point, scene, host_index, depth + 1).0;
            object_distance =
                object_distance.max((host_distance - inlay.offset).abs() - inlay.thickness);
        } else {
            object_distance = 100.0;
        }
    }
    let mut result = (object_distance, object.uuid);
    if depth >= scene.len() {
        return result;
    }
    for (child_index, child) in scene.iter().enumerate() {
        if child.boolean_parent != Some(object.uuid) {
            continue;
        }
        let candidate = subtree_sample_at(point, scene, child_index, depth + 1);
        let distance = boolean_distance(result.0, candidate.0, child.operation, object.softness);
        match child.operation {
            BooleanOperation::Union if candidate.0 < result.0 => result = candidate,
            BooleanOperation::Subtract => result.0 = result.0.max(-candidate.0),
            BooleanOperation::Intersect if candidate.0 > result.0 => result = candidate,
            _ => {}
        }
        result.0 = distance;
    }
    result
}

/// Geometry and tree relationships used repeatedly while baking one capture.
/// A bake evaluates millions of points, so resolving these once matters more
/// than the small setup cost when the scene changes.
pub(crate) struct PreparedSubtreeSampler<'a> {
    scene: &'a [SdfObject],
    root: usize,
    nodes: Vec<PreparedSampleNode>,
    flat_union: bool,
    lattices: Vec<PreparedSampleLattice<'a>>,
    cage_points: Vec<Vec3>,
}

struct PreparedSampleLattice<'a> {
    lattice: &'a super::Lattice,
    offsets: Vec<Vec3>,
    forward: Mat4,
    inverse: Mat4,
}

impl PreparedSampleLattice<'_> {
    fn rest_point(&self, point: Vec3) -> Vec3 {
        let cage_point = self.inverse.transform_point3(point);
        let mut rest = cage_point;
        for _ in 0..4 {
            let next = cage_point - self.lattice.displacement_with_offsets(rest, &self.offsets);
            let change = next - rest;
            rest = next;
            if change.length_squared() < 0.00000001 {
                break;
            }
        }
        self.forward.transform_point3(rest)
    }
}

struct PreparedSampleNode {
    children: Vec<usize>,
    inlay_host: Option<usize>,
    inverse: Mat4,
    distance_scale: f32,
    group: Mat4,
    group_inverse: Mat4,
    profile: Option<Vec<Vec2>>,
    text: Option<std::sync::Arc<super::PreparedText>>,
    sphere_bound: Option<(Vec3, f32, f32)>,
    cage: Option<usize>,
}

impl<'a> PreparedSubtreeSampler<'a> {
    pub fn new(scene: &'a [SdfObject], root: uuid::Uuid) -> Option<Self> {
        let root = scene.iter().position(|object| object.uuid == root)?;
        let indices: std::collections::HashMap<_, _> = scene
            .iter()
            .enumerate()
            .map(|(index, object)| (object.uuid, index))
            .collect();
        let mut children = vec![Vec::new(); scene.len()];
        for (index, object) in scene.iter().enumerate() {
            if let Some(parent) = object.boolean_parent.and_then(|id| indices.get(&id)) {
                children[*parent].push(index);
            }
        }
        let mut lattices = Vec::new();
        let mut lattice_indices = std::collections::HashMap::new();
        if scene[root].boolean_parent.is_none() {
            for object in scene {
                let mut ancestor = Some(object.uuid);
                let mut belongs = false;
                for _ in 0..scene.len() {
                    let Some(id) = ancestor else { break };
                    if id == scene[root].uuid {
                        belongs = true;
                        break;
                    }
                    ancestor = indices
                        .get(&id)
                        .and_then(|&index| scene[index].boolean_parent);
                }
                if !belongs {
                    continue;
                }
                if let Some(lattice) = object.lattice.as_ref().filter(|lattice| {
                    let n = lattice.resolution as usize;
                    (2..=9).contains(&n)
                        && lattice.offsets.len() == n * n * n
                        && lattice.offsets.iter().any(|offset| *offset != Vec3::ZERO)
                }) {
                    let forward = super::lattice_world_matrix(scene, object.uuid);
                    lattice_indices.insert(object.uuid, lattices.len());
                    lattices.push(PreparedSampleLattice {
                        lattice,
                        offsets: lattice.effective_offsets(),
                        forward,
                        inverse: forward.inverse(),
                    });
                }
            }
        }
        let nodes = scene
            .iter()
            .enumerate()
            .map(|(index, object)| {
                let mut ancestor = Some(object.uuid);
                let mut cage = None;
                for _ in 0..scene.len() {
                    let Some(id) = ancestor else { break };
                    if let Some(&slot) = lattice_indices.get(&id) {
                        cage = Some(slot);
                        break;
                    }
                    // A valid zero cage also overrides a parent's cage.
                    let Some(&candidate_index) = indices.get(&id) else {
                        break;
                    };
                    let candidate = &scene[candidate_index];
                    if candidate.lattice.as_ref().is_some_and(|lattice| {
                        let n = lattice.resolution as usize;
                        (2..=9).contains(&n) && lattice.offsets.len() == n * n * n
                    }) {
                        break;
                    }
                    ancestor = candidate.boolean_parent;
                }
                let matrix = object_world_matrix(scene, object.uuid);
                let distance_scale = Vec3::new(
                    matrix.x_axis.truncate().length(),
                    matrix.y_axis.truncate().length(),
                    matrix.z_axis.truncate().length(),
                )
                .min_element();
                let max_scale = Vec3::new(
                    matrix.x_axis.truncate().length(),
                    matrix.y_axis.truncate().length(),
                    matrix.z_axis.truncate().length(),
                )
                .length(); // Frobenius norm also bounds sheared ancestor transforms.
                let local_radius = if object.repetition.enabled
                    || object.surface_inlay.is_some()
                    || object.path_extrusion.is_some()
                    || object.mirror.is_some()
                    || !children[index].is_empty()
                {
                    None
                } else {
                    match &object.params {
                        SdfParams::SphereParams(p) => Some(p.radius),
                        SdfParams::BoxParams(p) => Some(p.box_q.length()),
                        SdfParams::CylinderParams {
                            radius,
                            half_height,
                        } => Some(radius.hypot(*half_height)),
                        SdfParams::TorusParams {
                            major_radius,
                            minor_radius,
                        } => Some(major_radius + 2.0 * minor_radius),
                        _ => None,
                    }
                };
                let group = super::group_world_matrix(scene, object.uuid);
                let profile = object
                    .path_extrusion
                    .and_then(|modifier| modifier.profile_curve)
                    .and_then(|id| super::profile_curve_vertices(scene, id));
                let text = if matches!(object.params, SdfParams::TextParams(_)) {
                    super::prepared_scene_text(scene, object).ok()
                } else {
                    None
                };
                PreparedSampleNode {
                    children: children[index].clone(),
                    inlay_host: object
                        .surface_inlay
                        .and_then(|inlay| indices.get(&inlay.host).copied()),
                    inverse: matrix.inverse(),
                    distance_scale,
                    group,
                    group_inverse: group.inverse(),
                    profile,
                    text,
                    sphere_bound: local_radius
                        .map(|radius| (matrix.transform_point3(Vec3::ZERO), radius, max_scale)),
                    cage,
                }
            })
            .collect();
        let flat_union = scene[root].softness <= 0.0
            && !scene[root].repetition.enabled
            && scene[root].mirror.is_none()
            && children[root].iter().all(|&index| {
                scene[index].operation == BooleanOperation::Union && children[index].is_empty()
                    && scene[index].mirror.is_none()
            });
        let cage_points = vec![Vec3::ZERO; lattices.len()];
        Some(Self {
            scene,
            root,
            nodes,
            flat_union,
            lattices,
            cage_points,
        })
    }

    pub fn sample(&mut self, point: Vec3) -> (f32, uuid::Uuid) {
        if self.flat_union {
            for (target, lattice) in self.cage_points.iter_mut().zip(&self.lattices) {
                *target = lattice.rest_point(point);
            }
        }
        self.sample_at(point, self.root, 0)
    }

    pub(crate) fn deformation_extent(&self, capture_inverse: Mat4) -> Vec3 {
        self.lattices.iter().fold(Vec3::ZERO, |maximum, cage| {
            let offset = cage
                .offsets
                .iter()
                .fold(Vec3::ZERO, |extent, offset| extent.max(offset.abs()));
            let matrix = capture_inverse * cage.forward;
            maximum.max(
                matrix.x_axis.truncate().abs() * offset.x
                    + matrix.y_axis.truncate().abs() * offset.y
                    + matrix.z_axis.truncate().abs() * offset.z,
            )
        })
    }

    pub(crate) fn march_factor(
        &self,
        lattice_factor: impl Fn(&super::Lattice, &[Vec3], Mat4) -> f32,
    ) -> f32 {
        let mut factor = self.lattices.iter().fold(0.8_f32, |factor, cage| {
            factor.min(lattice_factor(cage.lattice, &cage.offsets, cage.forward))
        });
        for object in self.scene {
            let mut ancestor = Some(object.uuid);
            let mut belongs = false;
            for _ in 0..self.scene.len() {
                let Some(id) = ancestor else { break };
                if id == self.scene[self.root].uuid {
                    belongs = true;
                    break;
                }
                ancestor = self
                    .scene
                    .iter()
                    .find(|candidate| candidate.uuid == id)
                    .and_then(|candidate| candidate.boolean_parent);
            }
            if belongs {
                match &object.params {
                    SdfParams::LoftParams(loft) => factor = factor.min(loft.march_factor()),
                    SdfParams::BezierCurveParams(_) => factor = factor.min(0.5),
                    SdfParams::TextParams(_) if object.path_extrusion.is_some() => {
                        factor = factor.min(0.35);
                    }
                    _ => {}
                }
            }
        }
        factor
    }

    fn object_point(&self, point: Vec3, index: usize) -> Vec3 {
        self.nodes[index].cage.map_or(point, |cage| {
            if self.flat_union {
                self.cage_points[cage]
            } else {
                self.lattices[cage].rest_point(point)
            }
        })
    }

    fn sample_at(&self, point: Vec3, index: usize, depth: usize) -> (f32, uuid::Uuid) {
        let object = &self.scene[index];
        let node = &self.nodes[index];
        let point = if let Some(mirror) = object.mirror
        {
            mirror.fold_point(point, node.group)
        } else {
            point
        };
        let group_repeat = !node.children.is_empty() && object.repetition.enabled;
        let point = if group_repeat {
            node.group.transform_point3(
                object.repeated_local_point(node.group_inverse.transform_point3(point)),
            )
        } else {
            point
        };
        let mut distance = object.distance_with_precomputed_inverse(
            self.object_point(point, index),
            node.inverse,
            node.distance_scale,
            !group_repeat,
            node.profile.as_deref(),
            node.text.as_deref(),
        );
        if let Some(inlay) = object.surface_inlay {
            distance = if let Some(host) = node.inlay_host {
                let host_distance = self.sample_at(point, host, depth + 1).0;
                distance.max((host_distance - inlay.offset).abs() - inlay.thickness)
            } else {
                100.0
            };
        }
        let mut result = (distance, object.uuid);
        if depth >= self.scene.len() {
            return result;
        }
        for &child in &node.children {
            if self.flat_union && index == self.root {
                if let Some((center, radius, max_scale)) = self.nodes[child].sphere_bound {
                    let lower = ((self.object_point(point, child) - center).length() / max_scale
                        - radius)
                        * self.nodes[child].distance_scale;
                    if lower >= result.0 {
                        continue;
                    }
                }
            }
            let candidate = self.sample_at(point, child, depth + 1);
            let operation = self.scene[child].operation;
            let distance = boolean_distance(result.0, candidate.0, operation, object.softness);
            match operation {
                BooleanOperation::Union if candidate.0 < result.0 => result = candidate,
                BooleanOperation::Subtract => result.0 = result.0.max(-candidate.0),
                BooleanOperation::Intersect if candidate.0 > result.0 => result = candidate,
                _ => {}
            }
            result.0 = distance;
        }
        result
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn renderer_stress_scene() -> Vec<SdfObject> {
    use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

    let layers = if std::env::args().any(|arg| arg == "--benchmark-1024") {
        16
    } else {
        4
    };
    let mut objects = Vec::with_capacity(layers * 64);
    for z in 0..layers {
        for y in 0..8 {
            for x in 0..8 {
                let object_type = if (x + y + z) % 2 == 0 {
                    TYPE_SPHERE
                } else {
                    TYPE_BOX
                };
                let mut object = SdfObject::create(object_type);
                object.transform.translation = Vec3::new(
                    (x as f32 - 3.5) * 0.42,
                    (y as f32 - 3.5) * 0.42,
                    (z as f32 - (layers as f32 - 1.0) * 0.5) * 0.42,
                );
                object.transform.rotation = Quat::from_euler(
                    glam::EulerRot::XYZ,
                    x as f32 * 0.11,
                    y as f32 * 0.07,
                    z as f32 * 0.17,
                );
                object.transform.scale = Vec3::new(
                    0.8 + (x % 3) as f32 * 0.14,
                    0.8 + (y % 3) as f32 * 0.14,
                    0.8 + (z % 3) as f32 * 0.14,
                );
                object.color = Vec4::new(
                    0.25 + x as f32 * 0.07,
                    0.2 + y as f32 * 0.06,
                    0.35 + z as f32 * 0.14,
                    1.0,
                );
                objects.push(object);
            }
        }
    }
    objects
}

/// Deterministic fixtures for timing and pixel comparisons with every material.
#[cfg(not(target_arch = "wasm32"))]
pub fn renderer_benchmark_scenes() -> Vec<(String, Vec<SdfObject>)> {
    let mut cases = Vec::new();
    // A nested group capture beside exact geometry exercises the baked GPU
    // operand, its material color, and its composition in a larger union.
    let mut outer = SdfObject::create_kind(PrimitiveKind::Sphere);
    outer.params = SdfParams::SphereParams(SphereParams { radius: 0.35 });
    outer.softness = 0.0;
    let mut capture_root = SdfObject::create_kind(PrimitiveKind::Box);
    capture_root.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::new(0.35, 0.55, 0.35),
        corner_radius: 0.0,
    });
    capture_root.boolean_parent = Some(outer.uuid);
    capture_root.group_transform.translation = Vec3::new(-0.7, 0.0, 0.0);
    capture_root.softness = 0.0;
    let mut capture_child = SdfObject::create_kind(PrimitiveKind::Sphere);
    capture_child.boolean_parent = Some(capture_root.uuid);
    capture_child.params = SdfParams::SphereParams(SphereParams { radius: 0.55 });
    capture_child.transform.translation = Vec3::new(0.45, 0.1, 0.0);
    capture_child.color = glam::Vec4::new(0.25, 0.75, 0.95, 1.0);
    let mut right = SdfObject::create_kind(PrimitiveKind::Sphere);
    right.boolean_parent = Some(outer.uuid);
    right.transform.translation = Vec3::new(0.8, 0.0, 0.0);
    right.params = SdfParams::SphereParams(SphereParams { radius: 0.55 });
    right.color = glam::Vec4::new(0.95, 0.4, 0.2, 1.0);
    let mut captured = vec![outer, capture_root, capture_child, right];
    let exact = captured.clone();
    captured[1].render_representation = GroupRenderRepresentation::BoxDepthAtlas;
    cases.push(("box-depth-exact".into(), exact));
    cases.push(("box-depth-atlas".into(), captured));
    let mut shell = SdfObject::create_kind(PrimitiveKind::Cylinder);
    shell.params = SdfParams::CylinderParams {
        radius: 1.0,
        half_height: 1.0,
    };
    shell.transform.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    shell.softness = 0.05;
    let mut cavity = SdfObject::create_kind(PrimitiveKind::Cylinder);
    cavity.params = SdfParams::CylinderParams {
        radius: 0.8,
        half_height: 1.1,
    };
    cavity.transform.rotation = shell.transform.rotation;
    cavity.boolean_parent = Some(shell.uuid);
    cavity.operation = BooleanOperation::Subtract;
    cases.push(("soft-cylinder".into(), vec![shell, cavity]));
    let mut layers = Vec::new();
    for index in 0..4 {
        let radius = 1.0 - index as f32 * 0.11;
        let mut shell = SdfObject::create_kind(PrimitiveKind::Cylinder);
        shell.params = SdfParams::CylinderParams {
            radius,
            half_height: 1.0,
        };
        shell.transform.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        shell.softness = 0.05;
        shell.material = Material::preset(MaterialKind::Transparent);
        shell.material.opacity = 0.3;
        shell.material.refractive_index = 1.0;
        shell.material.reflectivity = 0.0;
        let mut cavity = SdfObject::create_kind(PrimitiveKind::Cylinder);
        cavity.params = SdfParams::CylinderParams {
            radius: radius - 0.06,
            half_height: 1.1,
        };
        cavity.transform.rotation = shell.transform.rotation;
        cavity.boolean_parent = Some(shell.uuid);
        cavity.operation = BooleanOperation::Subtract;
        layers.extend([shell, cavity]);
    }
    cases.push(("soft-cylinder-layers".into(), layers));
    let mut sphere = SdfObject::create_kind(PrimitiveKind::Sphere);
    sphere.params = SdfParams::SphereParams(SphereParams { radius: 1.0 });
    sphere.softness = 0.05;
    let mut sphere_cavity = SdfObject::create_kind(PrimitiveKind::Sphere);
    sphere_cavity.params = SdfParams::SphereParams(SphereParams { radius: 0.8 });
    sphere_cavity.boolean_parent = Some(sphere.uuid);
    sphere_cavity.operation = BooleanOperation::Subtract;
    cases.push(("soft-sphere".into(), vec![sphere, sphere_cavity]));
    let mut block = SdfObject::create_kind(PrimitiveKind::Box);
    block.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(1.0),
        corner_radius: 0.0,
    });
    block.softness = 0.05;
    let mut box_cavity = SdfObject::create_kind(PrimitiveKind::Box);
    box_cavity.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(0.8),
        corner_radius: 0.0,
    });
    box_cavity.boolean_parent = Some(block.uuid);
    box_cavity.operation = BooleanOperation::Subtract;
    let mut rounded_block = block.clone();
    let mut rounded_cavity = box_cavity.clone();
    if let SdfParams::BoxParams(params) = &mut rounded_block.params {
        params.corner_radius = 0.12;
    }
    if let SdfParams::BoxParams(params) = &mut rounded_cavity.params {
        params.corner_radius = 0.1;
    }
    cases.push(("soft-box".into(), vec![block, box_cavity]));
    cases.push((
        "soft-rounded-box".into(),
        vec![rounded_block, rounded_cavity],
    ));
    let mut rotated_shell = SdfObject::create_kind(PrimitiveKind::Box);
    rotated_shell.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(1.0),
        corner_radius: 0.0,
    });
    rotated_shell.transform.rotation = Quat::from_rotation_x(0.32) * Quat::from_rotation_y(-0.26);
    rotated_shell.transform.scale = Vec3::new(1.35, 0.85, 1.1);
    rotated_shell.softness = 0.1;
    let mut angled_cavity = SdfObject::create_kind(PrimitiveKind::Cylinder);
    angled_cavity.params = SdfParams::CylinderParams {
        radius: 0.6,
        half_height: 1.25,
    };
    angled_cavity.transform.translation = Vec3::new(0.2, 0.1, 0.0);
    angled_cavity.transform.rotation = Quat::from_rotation_z(0.42);
    angled_cavity.boolean_parent = Some(rotated_shell.uuid);
    angled_cavity.operation = BooleanOperation::Subtract;
    cases.push((
        "soft-transformed-pair".into(),
        vec![rotated_shell, angled_cavity],
    ));
    let mut mixed_shell = SdfObject::create_kind(PrimitiveKind::Sphere);
    mixed_shell.params = SdfParams::SphereParams(SphereParams { radius: 1.0 });
    mixed_shell.transform.scale = Vec3::new(1.15, 0.9, 1.3);
    mixed_shell.softness = 0.08;
    let mut mixed_cavity = SdfObject::create_kind(PrimitiveKind::Box);
    mixed_cavity.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::new(0.6, 0.8, 0.55),
        corner_radius: 0.0,
    });
    mixed_cavity.transform.translation = Vec3::new(-0.2, 0.12, 0.05);
    mixed_cavity.transform.rotation = Quat::from_rotation_y(0.35);
    mixed_cavity.boolean_parent = Some(mixed_shell.uuid);
    mixed_cavity.operation = BooleanOperation::Subtract;
    cases.push(("soft-mixed-pair".into(), vec![mixed_shell, mixed_cavity]));
    let mut union_root = SdfObject::create_kind(PrimitiveKind::Box);
    union_root.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::new(0.5, 0.4, 0.5),
        corner_radius: 0.0,
    });
    union_root.softness = 0.0;
    let mut union_sphere = SdfObject::create_kind(PrimitiveKind::Sphere);
    union_sphere.params = SdfParams::SphereParams(SphereParams { radius: 0.48 });
    union_sphere.transform.translation = Vec3::new(0.65, 0.2, 0.0);
    union_sphere.boolean_parent = Some(union_root.uuid);
    let mut union_cylinder = SdfObject::create_kind(PrimitiveKind::Cylinder);
    union_cylinder.params = SdfParams::CylinderParams {
        radius: 0.35,
        half_height: 0.7,
    };
    union_cylinder.transform.translation = Vec3::new(-0.6, 0.0, 0.0);
    union_cylinder.boolean_parent = Some(union_root.uuid);
    cases.push((
        "flat-hard-union".into(),
        vec![union_root, union_sphere, union_cylinder],
    ));
    let mut wide_root = SdfObject::create_kind(PrimitiveKind::Box);
    wide_root.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(0.24),
        corner_radius: 0.0,
    });
    wide_root.softness = 0.0;
    let wide_root_id = wide_root.uuid;
    let mut wide_union = vec![wide_root];
    for index in 0..12 {
        let kind = match index % 3 {
            0 => PrimitiveKind::Sphere,
            1 => PrimitiveKind::Box,
            _ => PrimitiveKind::Cylinder,
        };
        let mut operand = SdfObject::create_kind(kind);
        operand.boolean_parent = Some(wide_root_id);
        operand.operation = BooleanOperation::Union;
        operand.transform.translation = Vec3::new(
            (index % 4) as f32 * 0.75 - 1.125,
            (index / 4) as f32 * 0.75 - 0.75,
            0.0,
        );
        wide_union.push(operand);
    }
    cases.push(("flat-hard-union-wide".into(), wide_union));
    let mut polygon_prism = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    polygon_prism.params = SdfParams::PolygonPrismParams(PolygonPrismParams {
        vertices: vec![
            Vec2::new(-0.8, -0.7),
            Vec2::new(0.8, -0.7),
            Vec2::new(0.25, 0.0),
            Vec2::new(0.8, 0.7),
            Vec2::new(-0.8, 0.7),
        ],
        half_depth: 0.35,
        edge_softness: 0.0,
    });
    polygon_prism.transform.rotation = Quat::from_rotation_y(0.35);
    cases.push(("polygon-prism".into(), vec![polygon_prism]));
    let mut mirrored_box = SdfObject::create_kind(PrimitiveKind::Box);
    mirrored_box.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::new(0.8, 0.6, 0.45),
        corner_radius: 0.0,
    });
    mirrored_box.mirror = Some(super::Mirror::default());
    let mut mirrored_cut = SdfObject::create_kind(PrimitiveKind::Sphere);
    mirrored_cut.params = SdfParams::SphereParams(SphereParams { radius: 0.4 });
    mirrored_cut.transform.translation = Vec3::new(0.45, 0.2, 0.0);
    mirrored_cut.boolean_parent = Some(mirrored_box.uuid);
    mirrored_cut.operation = BooleanOperation::Subtract;
    cases.push(("mirrored-boolean".into(), vec![mirrored_box, mirrored_cut]));
    let mut transparent_lattice = SdfObject::create_kind(PrimitiveKind::Box);
    transparent_lattice.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(0.55),
        corner_radius: 0.0,
    });
    let mut transparent_cage = super::Lattice::new(Vec3::splat(-0.55), Vec3::splat(0.55), 3);
    let transparent_corner = transparent_cage.index(2, 2, 2);
    transparent_cage.offsets[transparent_corner] = Vec3::new(0.3, 0.15, 0.0);
    transparent_lattice.lattice = Some(transparent_cage);
    cases.push(("lattice-transparent".into(), vec![transparent_lattice]));
    let mut lattice_reference = SdfObject::create_kind(PrimitiveKind::Box);
    lattice_reference.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(1.0),
        corner_radius: 0.0,
    });
    lattice_reference.material = Material::preset(MaterialKind::Solid);
    lattice_reference.color = lattice_reference.material.color;
    let mut lattice_deformed = lattice_reference.clone();
    let mut lattice = super::Lattice::new(Vec3::splat(-1.0), Vec3::splat(1.0), 3);
    let corner = lattice.index(2, 2, 2);
    lattice.offsets[corner] = Vec3::new(0.5, 0.25, 0.0);
    lattice_deformed.lattice = Some(lattice);
    cases.push(("lattice-reference".into(), vec![lattice_reference]));
    cases.push(("lattice-deformed".into(), vec![lattice_deformed]));
    let mut dense = SdfObject::create_kind(PrimitiveKind::Box);
    dense.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::ONE,
        corner_radius: 0.0,
    });
    dense.material = Material::preset(MaterialKind::Solid);
    dense.color = dense.material.color;
    let mut dense_lattice = super::Lattice::new(Vec3::splat(-1.0), Vec3::ONE, 9);
    let dense_corner = dense_lattice.index(8, 8, 8);
    dense_lattice.offsets[dense_corner] = Vec3::new(0.5, 0.25, 0.0);
    dense.lattice = Some(dense_lattice);
    cases.push(("lattice-dense".into(), vec![dense]));
    let mut group = SdfObject::create_kind(PrimitiveKind::Box);
    group.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::ONE,
        corner_radius: 0.0,
    });
    group.material = Material::preset(MaterialKind::Solid);
    group.color = group.material.color;
    let mut group_lattice = super::Lattice::new(Vec3::splat(-1.0), Vec3::ONE, 3);
    let group_corner = group_lattice.index(2, 2, 2);
    group_lattice.offsets[group_corner] = Vec3::new(0.5, 0.25, 0.0);
    group.lattice = Some(group_lattice);
    let mut cutter = SdfObject::create_kind(PrimitiveKind::Sphere);
    cutter.params = SdfParams::SphereParams(SphereParams { radius: 0.55 });
    cutter.transform.translation = Vec3::new(0.5, 0.3, 0.7);
    cutter.boolean_parent = Some(group.uuid);
    cutter.operation = BooleanOperation::Subtract;
    cases.push(("lattice-boolean".into(), vec![group, cutter]));
    let mut many_cages = Vec::new();
    for index in 0..40 {
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.params = SdfParams::BoxParams(BoxParams {
            box_q: Vec3::splat(0.12),
            corner_radius: 0.0,
        });
        object.transform.translation = Vec3::new(
            (index % 8) as f32 * 0.32 - 1.12,
            (index / 8) as f32 * 0.32 - 0.64,
            0.0,
        );
        object.material = Material::preset(MaterialKind::Solid);
        object.color = object.material.color;
        let mut cage = super::Lattice::new(Vec3::splat(-0.12), Vec3::splat(0.12), 2);
        let corner = cage.index(1, 1, 1);
        cage.offsets[corner] = Vec3::new(0.04, 0.0, 0.0);
        object.lattice = Some(cage);
        many_cages.push(object);
    }
    cases.push(("lattice-many".into(), many_cages));
    let mut wood_gallery = Vec::new();
    for (index, species) in [WoodSpecies::Pine, WoodSpecies::Oak, WoodSpecies::Walnut]
        .into_iter()
        .enumerate()
    {
        let mut block = SdfObject::create_kind(PrimitiveKind::Box);
        block.name = species.label().into();
        block.params = SdfParams::BoxParams(BoxParams {
            box_q: Vec3::splat(0.46),
            corner_radius: 0.0,
        });
        block.transform.translation = Vec3::new((index as f32 - 1.0) * 1.12, 0.0, 0.0);
        block.material = Material::wood_preset(species);
        block.material.wood.cut_angle = (index as f32 - 1.0) * 0.58;
        block.color = block.material.color;
        wood_gallery.push(block);
    }
    cases.push(("wood-gallery".into(), wood_gallery));
    let mut cut_block = SdfObject::create_kind(PrimitiveKind::Box);
    cut_block.name = "Oak with drilled hole".into();
    cut_block.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(0.55),
        corner_radius: 0.0,
    });
    cut_block.material = Material::wood_preset(WoodSpecies::Oak);
    cut_block.color = cut_block.material.color;
    let mut drill = SdfObject::create_kind(PrimitiveKind::Cylinder);
    drill.params = SdfParams::CylinderParams {
        radius: 0.25,
        half_height: 0.75,
    };
    drill.transform.translation = Vec3::new(0.12, 0.04, 0.0);
    drill.transform.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    drill.boolean_parent = Some(cut_block.uuid);
    drill.operation = BooleanOperation::Subtract;
    drill.material = cut_block.material;
    drill.color = cut_block.color;
    cases.push(("wood-cut".into(), vec![cut_block, drill]));
    let mut brick_corner = SdfObject::create_kind(PrimitiveKind::Box);
    brick_corner.name = "Brick corner detail".into();
    brick_corner.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(0.72),
        corner_radius: 0.0,
    });
    brick_corner.material = Material::preset(MaterialKind::Brick);
    brick_corner.color = brick_corner.material.color;
    cases.push(("brick-corner".into(), vec![brick_corner]));
    let mut brick_wall = SdfObject::create_kind(PrimitiveKind::Box);
    brick_wall.name = "Brick wall with opening".into();
    brick_wall.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::new(1.05, 0.75, 0.18),
        corner_radius: 0.0,
    });
    brick_wall.material = Material::preset(MaterialKind::Brick);
    brick_wall.color = brick_wall.material.color;
    let mut opening = SdfObject::create_kind(PrimitiveKind::Box);
    opening.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::new(0.28, 0.37, 0.35),
        corner_radius: 0.0,
    });
    opening.transform.translation = Vec3::new(0.18, -0.04, 0.0);
    opening.boolean_parent = Some(brick_wall.uuid);
    opening.operation = BooleanOperation::Subtract;
    opening.material = brick_wall.material;
    opening.color = brick_wall.color;
    cases.push(("brick-window".into(), vec![brick_wall, opening]));
    for study in crate::model::MetalStudy::ALL {
        let mut sphere = SdfObject::create_kind(PrimitiveKind::Sphere);
        sphere.name = study.label().into();
        sphere.params = SdfParams::SphereParams(SphereParams { radius: 0.95 });
        sphere.material = study.material();
        sphere.color = sphere.material.color;
        cases.push((
            format!("metal-{}", study.label().replace(' ', "-")),
            vec![sphere],
        ));
    }
    for kind in MaterialKind::BUILTINS {
        let mut scene = renderer_stress_scene();
        for object in &mut scene {
            object.material = Material::preset(kind);
            if kind == MaterialKind::Wood
                || kind == MaterialKind::Brick
                || kind == MaterialKind::Diagnostic
                || kind == MaterialKind::Fabric
                || kind == MaterialKind::Metal
            {
                object.color = object.material.color;
            }
        }
        let name = match kind {
            MaterialKind::Transparent => "transparent",
            MaterialKind::Metallic => "metallic",
            MaterialKind::Solid => "solid",
            MaterialKind::Wood => "wood",
            MaterialKind::Brick => "brick",
            MaterialKind::Diagnostic => "diagnostic",
            MaterialKind::Fabric => "fabric",
            MaterialKind::Metal => "metal",
            MaterialKind::Custom => unreachable!(),
        };
        cases.push((name.into(), scene));
    }
    let mut mixed = renderer_stress_scene();
    for (i, object) in mixed.iter_mut().enumerate() {
        let template = SdfObject::create(
            PrimitiveKind::SPAWNABLE[i % PrimitiveKind::SPAWNABLE.len()].object_type(),
        );
        object.object_type = template.object_type;
        object.params = template.params;
        object.material =
            Material::preset(MaterialKind::BUILTINS[i % MaterialKind::BUILTINS.len()]);
    }
    cases.push(("mixed".into(), mixed.clone()));
    for pair in mixed.chunks_mut(2) {
        pair[1].boolean_parent = Some(pair[0].uuid);
        pair[1].operation = BooleanOperation::Subtract;
    }
    cases.push(("booleans".into(), mixed.clone()));
    for group in mixed.chunks_mut(4) {
        group[0].boolean_parent = None;
        for i in 1..group.len() {
            group[i].boolean_parent = Some(group[i - 1].uuid);
            group[i].operation = if i == 2 {
                BooleanOperation::Intersect
            } else {
                BooleanOperation::Subtract
            };
        }
    }
    cases.push(("nested".into(), mixed));
    let mut repeated: Vec<_> = renderer_stress_scene().into_iter().take(64).collect();
    for (i, object) in repeated.iter_mut().enumerate() {
        let template = SdfObject::create(
            PrimitiveKind::SPAWNABLE[i % PrimitiveKind::SPAWNABLE.len()].object_type(),
        );
        object.object_type = template.object_type;
        object.params = template.params;
        object.material =
            Material::preset(MaterialKind::BUILTINS[i % MaterialKind::BUILTINS.len()]);
        object.repetition.enabled = true;
        object.repetition.count = [3; 3];
        object.repetition.spacing = Vec3::splat(1.5);
    }
    cases.push(("repeated".into(), repeated));
    cases.push(("preview".into(), ui_preview_scene()));
    cases
}

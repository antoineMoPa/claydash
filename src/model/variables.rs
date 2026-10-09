//! Shared local/world-space vectors and explicit bindings to authored object points.
use super::{ClaydashValue, DataTree, SdfObject, SdfParams};
use glam::{Mat3, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorVariable {
    pub id: Uuid,
    pub name: String,
    pub value: Vec3,
    #[serde(default)]
    pub space: VariableSpace,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableSpace {
    #[default]
    Local,
    World,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorBindingTarget {
    Position,
    GroupPosition,
    BezierPoint(usize),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorBinding {
    pub object: Uuid,
    pub target: VectorBindingTarget,
    pub variable: Uuid,
    pub offset: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RigidBindingTarget {
    Object,
    Group,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RigidBindingUp {
    Point(Uuid),
    WorldDirection(Vec3),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RigidBinding {
    pub object: Uuid,
    pub target: RigidBindingTarget,
    pub origin: Uuid,
    pub aim: Uuid,
    pub up: RigidBindingUp,
    pub local_origin: Vec3,
    pub local_aim: Vec3,
    pub local_up: Vec3,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneVariables {
    pub vectors: Vec<VectorVariable>,
    pub bindings: Vec<VectorBinding>,
    #[serde(default)]
    pub rigid_bindings: Vec<RigidBinding>,
    #[serde(default)]
    pub four_bar_constraints: Vec<super::PlanarFourBarConstraint>,
}

pub fn scene_variables(tree: &DataTree) -> SceneVariables {
    match tree.get_path("scene.variables") {
        ClaydashValue::SceneVariables(variables) => variables,
        _ => SceneVariables::default(),
    }
}

/// Bindings are constraints: animation and direct edits cannot change a linked point.
/// Values remain materialized in the scene so the engine and exported geometry agree.
pub fn apply_vector_bindings(variables: &SceneVariables, objects: &mut [SdfObject]) {
    // Settle group frames parent-first, then object poses, then deformable points.
    let mut order: Vec<_> = objects.iter().map(|object| object.uuid).collect();
    order.sort_by_key(|id| {
        let mut depth = 0;
        let mut cursor = *id;
        while depth < objects.len() {
            let Some(parent) = objects
                .iter()
                .find(|o| o.uuid == cursor)
                .and_then(|o| o.boolean_parent)
            else {
                break;
            };
            cursor = parent;
            depth += 1;
        }
        depth
    });
    for target in [RigidBindingTarget::Group, RigidBindingTarget::Object] {
        for id in &order {
            let vector_target = match target {
                RigidBindingTarget::Group => VectorBindingTarget::GroupPosition,
                RigidBindingTarget::Object => VectorBindingTarget::Position,
            };
            for binding in variables
                .bindings
                .iter()
                .filter(|b| b.object == *id && b.target == vector_target)
            {
                apply_point_binding(variables, objects, binding);
            }
            for binding in variables
                .rigid_bindings
                .iter()
                .filter(|b| b.object == *id && b.target == target)
            {
                if let Ok(pose) = rigid_binding_pose(variables, objects, binding) {
                    if let Some(object) = objects.iter_mut().find(|o| o.uuid == *id) {
                        match target {
                            RigidBindingTarget::Group => object.group_transform = pose,
                            RigidBindingTarget::Object => object.transform = pose,
                        }
                    }
                }
            }
        }
    }
    for binding in variables
        .bindings
        .iter()
        .filter(|b| matches!(b.target, VectorBindingTarget::BezierPoint(_)))
    {
        apply_point_binding(variables, objects, binding);
    }
}

fn apply_point_binding(
    variables: &SceneVariables,
    objects: &mut [SdfObject],
    binding: &VectorBinding,
) {
    let Some(variable) = variables.vectors.iter().find(|v| v.id == binding.variable) else {
        return;
    };
    let authored = variable.value + binding.offset;
    let value = match variable.space {
        VariableSpace::Local => authored,
        VariableSpace::World => vector_binding_frame(objects, binding.object, binding.target)
            .inverse()
            .transform_point3(authored),
    };
    if !value.is_finite() {
        return;
    }
    let Some(object) = objects.iter_mut().find(|o| o.uuid == binding.object) else {
        return;
    };
    match binding.target {
        VectorBindingTarget::Position => object.transform.translation = value,
        VectorBindingTarget::GroupPosition => object.group_transform.translation = value,
        VectorBindingTarget::BezierPoint(index) => {
            if let SdfParams::BezierCurveParams(params) = &mut object.params {
                if let Some(point) = params.points.get_mut(index) {
                    *point = value;
                }
            }
        }
    }
}

/// Build an explicit right-handed frame from a heading and an independent up direction.
fn rigid_frame(aim: Vec3, up: Vec3) -> Result<Mat3, String> {
    if !aim.is_finite() || !up.is_finite() || aim.length_squared() < 1e-12 {
        return Err("invalid rigid heading".into());
    }
    let x = aim / aim.length();
    let perpendicular = up - x * up.dot(x);
    if perpendicular.length_squared() < 1e-12 {
        return Err("rigid up direction is parallel to heading".into());
    }
    let y = perpendicular / perpendicular.length();
    Ok(Mat3::from_cols(x, y, x.cross(y)))
}

pub fn rigid_binding_pose(
    variables: &SceneVariables,
    objects: &[SdfObject],
    binding: &RigidBinding,
) -> Result<super::Transform, String> {
    let point = |id| {
        variables
            .vectors
            .iter()
            .find(|v| v.id == id && v.space == VariableSpace::World)
            .map(|v| v.value)
            .ok_or_else(|| format!("rigid point {id} must exist in World space"))
    };
    let origin = point(binding.origin)?;
    let aim = point(binding.aim)? - origin;
    let up = match binding.up {
        RigidBindingUp::Point(id) => point(id)? - origin,
        RigidBindingUp::WorldDirection(direction) => direction,
    };
    let object = objects
        .iter()
        .find(|o| o.uuid == binding.object)
        .ok_or_else(|| "rigid object missing".to_string())?;
    let (mut pose, parent) = match binding.target {
        RigidBindingTarget::Object => (
            object.transform,
            super::group_world_matrix(objects, object.uuid),
        ),
        RigidBindingTarget::Group => (
            object.group_transform,
            super::parent_group_world_matrix(objects, object.uuid),
        ),
    };
    validate_rigid_parent_frame(parent)?;
    if !pose.scale.is_finite() || pose.scale.abs().min_element() < 1e-6 {
        return Err("rigid scale must be finite and nonsingular".into());
    }
    let inverse = parent.inverse();
    let authored = rigid_frame(
        pose.scale * (binding.local_aim - binding.local_origin),
        pose.scale * (binding.local_up - binding.local_origin),
    )?;
    let target = rigid_frame(
        inverse.transform_vector3(aim),
        inverse.transform_vector3(up),
    )?;
    pose.rotation = Quat::from_mat3(&(target * authored.transpose()));
    pose.translation =
        inverse.transform_point3(origin) - pose.rotation * (pose.scale * binding.local_origin);
    if !pose.translation.is_finite() || !pose.rotation.is_finite() {
        return Err("nonfinite rigid pose".into());
    }
    Ok(pose)
}

fn validate_rigid_parent_frame(frame: Mat4) -> Result<(), String> {
    let axes = [
        frame.x_axis.truncate(),
        frame.y_axis.truncate(),
        frame.z_axis.truncate(),
    ];
    let lengths = axes.map(|axis| axis.length());
    let scale = lengths[0];
    if !frame.is_finite()
        || !scale.is_finite()
        || scale < 1e-6
        || lengths
            .iter()
            .any(|length| (*length - scale).abs() > 1e-5 * scale)
        || axes[0].dot(axes[1]).abs() > 1e-5 * scale * scale
        || axes[0].dot(axes[2]).abs() > 1e-5 * scale * scale
        || axes[1].dot(axes[2]).abs() > 1e-5 * scale * scale
        || Mat3::from_cols(axes[0], axes[1], axes[2]).determinant() <= 0.0
    {
        return Err("rigid parent frame requires a positive uniform scale without shear".into());
    }
    Ok(())
}

pub fn is_object_rigid_bound(variables: &SceneVariables, id: Uuid) -> bool {
    variables.rigid_bindings.iter().any(|b| b.object == id)
}
pub fn rigid_binding(
    variables: &SceneVariables,
    id: Uuid,
    target: RigidBindingTarget,
) -> Option<&RigidBinding> {
    variables
        .rigid_bindings
        .iter()
        .find(|b| b.object == id && b.target == target)
}

pub fn derived_variable_ids(variables: &SceneVariables) -> Vec<Uuid> {
    variables
        .four_bar_constraints
        .iter()
        .flat_map(|c| [c.lower_joint, c.upper_joint])
        .collect()
}
pub fn is_variable_derived(variables: &SceneVariables, id: Uuid) -> bool {
    derived_variable_ids(variables).contains(&id)
}
pub fn remove_variable_references(variables: &mut SceneVariables, id: Uuid) {
    variables.bindings.retain(|b| b.variable != id);
    variables.rigid_bindings.retain(|b| {
        b.origin != id
            && b.aim != id
            && !matches!(b.up, RigidBindingUp::Point(point) if point == id)
    });
    variables.four_bar_constraints.retain(|c| {
        ![
            c.driver,
            c.lower_pivot,
            c.upper_pivot,
            c.lower_joint,
            c.upper_joint,
        ]
        .contains(&id)
    });
}

pub fn four_bar_constraint_order(variables: &SceneVariables) -> Result<Vec<usize>, String> {
    let mut outputs = std::collections::HashSet::new();
    for c in &variables.four_bar_constraints {
        if !outputs.insert(c.lower_joint) || !outputs.insert(c.upper_joint) {
            return Err("duplicate four-bar output point".into());
        }
    }
    let mut order = Vec::new();
    let mut completed = std::collections::HashSet::new();
    while order.len() < variables.four_bar_constraints.len() {
        let next = variables
            .four_bar_constraints
            .iter()
            .enumerate()
            .find(|(index, c)| {
                !order.contains(index)
                    && [c.driver, c.lower_pivot, c.upper_pivot]
                        .iter()
                        .all(|id| !outputs.contains(id) || completed.contains(id))
            });
        let Some((index, c)) = next else {
            return Err("cyclic four-bar point dependencies".into());
        };
        order.push(index);
        completed.insert(c.lower_joint);
        completed.insert(c.upper_joint);
    }
    Ok(order)
}

pub fn is_variable_constraint_reference(variables: &SceneVariables, id: Uuid) -> bool {
    variables.rigid_bindings.iter().any(|b| {
        b.origin == id || b.aim == id || matches!(b.up,RigidBindingUp::Point(point) if point == id)
    }) || variables.four_bar_constraints.iter().any(|c| {
        [
            c.driver,
            c.lower_pivot,
            c.upper_pivot,
            c.lower_joint,
            c.upper_joint,
        ]
        .contains(&id)
    })
}
pub fn evaluate_scene_variables(variables: &mut SceneVariables) -> Result<(), String> {
    let order = four_bar_constraint_order(variables)?;
    let mut draft = variables.clone();
    for index in order {
        let c = &draft.four_bar_constraints[index];
        let point = |id| {
            draft
                .vectors
                .iter()
                .find(|v| v.id == id && v.space == VariableSpace::World)
                .map(|v| v.value)
                .ok_or_else(|| format!("constraint point {id} must exist in World space"))
        };
        let driver = point(c.driver)?;
        let lower = point(c.lower_pivot)?;
        let upper = point(c.upper_pivot)?;
        point(c.lower_joint)?;
        point(c.upper_joint)?;
        let delta = upper - lower;
        let distance = delta.truncate().length();
        let mut travel_checks = vec![driver.y, c.travel_min, c.travel_max];
        if distance > 1e-6 && delta.x != 0.0 {
            let critical = lower.y + delta.x.signum() * delta.y * c.lower_length / distance
                - c.driver_y_offset;
            if critical >= c.travel_min && critical <= c.travel_max {
                travel_checks.push(critical);
            }
        }
        for y in travel_checks {
            if super::solve_planar_four_bar(c, Vec3::new(driver.x, y, driver.z), lower, upper)
                .is_none()
            {
                return Err("point edit makes the fixed-length linkage travel impossible".into());
            }
        }
        super::apply_four_bar_constraints(std::slice::from_ref(c), &mut draft.vectors);
    }
    *variables = draft;
    Ok(())
}
pub fn try_set_scene_variables(
    tree: &mut DataTree,
    mut variables: SceneVariables,
) -> Result<(), String> {
    evaluate_scene_variables(&mut variables)?;
    let mut objects = super::objects(tree);
    apply_vector_bindings(&variables, &mut objects);
    for binding in &variables.rigid_bindings {
        rigid_binding_pose(&variables, &objects, binding)?;
    }
    set_scene_variables(tree, variables);
    Ok(())
}

pub fn evaluate_derived_variables(variables: &mut SceneVariables) {
    if let Ok(order) = four_bar_constraint_order(variables) {
        for index in order {
            super::apply_four_bar_constraints(
                std::slice::from_ref(&variables.four_bar_constraints[index]),
                &mut variables.vectors,
            );
        }
    }
}

/// Map a binding's authored local coordinates into the shared world frame.
pub fn vector_binding_frame(
    objects: &[SdfObject],
    object: Uuid,
    target: VectorBindingTarget,
) -> glam::Mat4 {
    match target {
        VectorBindingTarget::Position => super::group_world_matrix(objects, object),
        VectorBindingTarget::GroupPosition => super::parent_group_world_matrix(objects, object),
        VectorBindingTarget::BezierPoint(_) => super::object_world_matrix(objects, object),
    }
}

pub fn vector_binding(
    tree: &DataTree,
    object: Uuid,
    target: VectorBindingTarget,
) -> Option<VectorBinding> {
    scene_variables(tree)
        .bindings
        .into_iter()
        .find(|binding| binding.object == object && binding.target == target)
}

pub fn selected_variable(tree: &DataTree) -> Option<Uuid> {
    match tree.get_path("editor.selected_variable") {
        ClaydashValue::Uuid(id)
            if scene_variables(tree)
                .vectors
                .iter()
                .any(|variable| variable.id == id) =>
        {
            Some(id)
        }
        _ => None,
    }
}

pub fn set_selected_variable(tree: &mut DataTree, id: Option<Uuid>) {
    tree.set_transient_path(
        "editor.selected_variable",
        id.map_or(ClaydashValue::None, ClaydashValue::Uuid),
    );
}

/// Local points use a selected linked object's frame, otherwise the first saved
/// binding. Unbound points use identity. World points always use identity.
pub fn variable_viewport_frame(tree: &DataTree, variable: &VectorVariable) -> glam::Mat4 {
    if variable.space == VariableSpace::World {
        return glam::Mat4::IDENTITY;
    }
    let variables = scene_variables(tree);
    let selection = super::selected_ref(tree);
    let binding = variables
        .bindings
        .iter()
        .find(|binding| binding.variable == variable.id && selection.contains(&binding.object))
        .or_else(|| {
            variables
                .bindings
                .iter()
                .find(|binding| binding.variable == variable.id)
        });
    binding.map_or(glam::Mat4::IDENTITY, |binding| {
        vector_binding_frame(super::objects_ref(tree), binding.object, binding.target)
    })
}

pub fn set_scene_variables(tree: &mut DataTree, mut variables: SceneVariables) {
    evaluate_derived_variables(&mut variables);
    let mut objects = super::objects(tree);
    apply_vector_bindings(&variables, &mut objects);
    tree.set_path("scene.variables", ClaydashValue::SceneVariables(variables));
    super::set_objects(tree, objects);
}

#[cfg(test)]
mod tests {
    use super::super::PrimitiveKind;
    use super::*;

    #[test]
    fn deleting_an_object_removes_its_links_and_undo_restores_them() {
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Sphere);
        super::super::set_objects(&mut tree, vec![object.clone()]);
        let variable = VectorVariable {
            id: Uuid::new_v4(),
            name: "Anchor".into(),
            value: Vec3::X,
            space: VariableSpace::World,
        };
        let mut variables = SceneVariables {
            vectors: vec![variable.clone()],
            bindings: vec![VectorBinding {
                object: object.uuid,
                target: VectorBindingTarget::Position,
                variable: variable.id,
                offset: Vec3::ZERO,
            }],
            ..SceneVariables::default()
        };
        // Unknown IDs may be forward references inside an atomic MCP batch.
        let mut future = variables.bindings[0].clone();
        future.object = Uuid::new_v4();
        variables.bindings.push(future.clone());
        set_scene_variables(&mut tree, variables.clone());
        tree.make_undo_redo_snapshot();
        super::super::set_objects(&mut tree, Vec::new());
        assert_eq!(scene_variables(&tree).bindings, vec![future]);
        assert_eq!(scene_variables(&tree).vectors, vec![variable]);
        tree.undo();
        assert_eq!(scene_variables(&tree), variables);
        assert_eq!(super::super::objects(&tree)[0].uuid, object.uuid);
    }

    #[test]
    fn shared_vector_updates_position_and_curve_point_and_survives_save() {
        let mut tree = DataTree::default();
        let mut a = SdfObject::create_kind(PrimitiveKind::Sphere);
        let mut b = SdfObject::create_kind(PrimitiveKind::BezierCurve);
        let variable = VectorVariable {
            id: Uuid::new_v4(),
            name: "Joint".into(),
            space: VariableSpace::Local,
            value: Vec3::new(1.0, 2.0, 3.0),
        };
        let mut variables = SceneVariables {
            bindings: vec![
                VectorBinding {
                    object: a.uuid,
                    target: VectorBindingTarget::Position,
                    variable: variable.id,
                    offset: Vec3::X,
                },
                VectorBinding {
                    object: b.uuid,
                    target: VectorBindingTarget::BezierPoint(0),
                    variable: variable.id,
                    offset: Vec3::ZERO,
                },
            ],
            vectors: vec![variable],
            ..SceneVariables::default()
        };
        super::super::set_objects(&mut tree, vec![a.clone(), b.clone()]);
        set_scene_variables(&mut tree, variables.clone());
        let bytes = crate::document::serialize_scene(&tree).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", crate::document::deserialize_scene(&bytes).unwrap());
        assert_eq!(scene_variables(&restored), variables);
        variables.vectors[0].value = Vec3::splat(5.0);
        set_scene_variables(&mut restored, variables.clone());
        let scene = super::super::objects(&restored);
        assert_eq!(scene[0].transform.translation, Vec3::new(6.0, 5.0, 5.0));
        let SdfParams::BezierCurveParams(params) = &scene[1].params else {
            panic!()
        };
        assert_eq!(params.points[0], Vec3::splat(5.0));
        // An animation/direct transform edit is constrained until the link is removed.
        a.transform.translation = Vec3::ZERO;
        b.transform.translation = Vec3::Y;
        super::super::set_objects_transient(&mut restored, vec![a, b]);
        assert_eq!(
            super::super::objects(&restored)[0].transform.translation,
            Vec3::new(6.0, 5.0, 5.0)
        );
        variables.bindings.clear();
        set_scene_variables(&mut restored, variables);
        assert_eq!(
            super::super::objects(&restored)[0].transform.translation,
            Vec3::new(6.0, 5.0, 5.0)
        );
    }
    #[test]
    fn undo_redo_restores_variable_and_all_linked_coordinates_together() {
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Sphere);
        super::super::set_objects(&mut tree, vec![object.clone()]);
        let variable = VectorVariable {
            id: Uuid::new_v4(),
            name: "Shared".into(),
            value: Vec3::X,
            space: VariableSpace::World,
        };
        let mut variables = SceneVariables {
            vectors: vec![variable.clone()],
            bindings: vec![VectorBinding {
                object: object.uuid,
                target: VectorBindingTarget::Position,
                variable: variable.id,
                offset: Vec3::ZERO,
            }],
            ..SceneVariables::default()
        };
        set_scene_variables(&mut tree, variables.clone());
        tree.make_undo_redo_snapshot();
        // Multiple frames of one drag remain one authored transaction.
        variables.vectors[0].value = Vec3::Y;
        set_scene_variables(&mut tree, variables.clone());
        variables.vectors[0].value = Vec3::Z;
        set_scene_variables(&mut tree, variables);
        tree.undo();
        assert_eq!(scene_variables(&tree).vectors[0].value, Vec3::X);
        assert_eq!(
            super::super::objects(&tree)[0].transform.translation,
            Vec3::X
        );
        tree.redo();
        assert_eq!(scene_variables(&tree).vectors[0].value, Vec3::Z);
        assert_eq!(
            super::super::objects(&tree)[0].transform.translation,
            Vec3::Z
        );
    }

    #[test]
    fn world_points_stay_shared_across_transformed_objects_and_moving_groups() {
        let mut parent = SdfObject::create_kind(PrimitiveKind::Box);
        parent.group_transform.rotation = glam::Quat::from_rotation_z(0.6);
        parent.group_transform.scale = Vec3::new(2.0, 3.0, 1.0);
        let mut a = SdfObject::create_kind(PrimitiveKind::BezierCurve);
        a.boolean_parent = Some(parent.uuid);
        a.transform.rotation = glam::Quat::from_rotation_y(0.3);
        a.transform.scale = Vec3::splat(0.5);
        let mut b = SdfObject::create_kind(PrimitiveKind::BezierCurve);
        b.transform.translation = Vec3::new(-3.0, 2.0, 1.0);
        let anchor = VectorVariable {
            id: Uuid::new_v4(),
            name: "Anchor".into(),
            value: Vec3::new(7.0, -2.0, 3.0),
            space: VariableSpace::World,
        };
        let frame = VectorVariable {
            id: Uuid::new_v4(),
            name: "Frame".into(),
            value: Vec3::new(2.0, 4.0, -1.0),
            space: VariableSpace::World,
        };
        // Intentionally reversed dependency order.
        let mut variables = SceneVariables {
            bindings: vec![
                VectorBinding {
                    object: a.uuid,
                    target: VectorBindingTarget::BezierPoint(0),
                    variable: anchor.id,
                    offset: Vec3::ZERO,
                },
                VectorBinding {
                    object: b.uuid,
                    target: VectorBindingTarget::BezierPoint(0),
                    variable: anchor.id,
                    offset: Vec3::ZERO,
                },
                VectorBinding {
                    object: a.uuid,
                    target: VectorBindingTarget::Position,
                    variable: frame.id,
                    offset: Vec3::ZERO,
                },
                VectorBinding {
                    object: parent.uuid,
                    target: VectorBindingTarget::GroupPosition,
                    variable: frame.id,
                    offset: Vec3::ZERO,
                },
            ],
            vectors: vec![anchor.clone(), frame],
            ..SceneVariables::default()
        };
        let mut scene = vec![a, b, parent];
        for value in [anchor.value, Vec3::new(-4.0, 8.0, 2.0)] {
            variables.vectors[0].value = value;
            variables.vectors[1].value += Vec3::X;
            apply_vector_bindings(&variables, &mut scene);
            for object in &scene[..2] {
                let SdfParams::BezierCurveParams(params) = &object.params else {
                    panic!()
                };
                let world = super::super::object_world_matrix(&scene, object.uuid)
                    .transform_point3(params.points[0]);
                assert!(world.distance(value) < 0.00001, "{world:?} != {value:?}");
            }
        }
    }
}

#[cfg(test)]
mod rigid_tests {
    use super::super::{PrimitiveKind, Transform};
    use super::*;

    fn variable(value: Vec3) -> VectorVariable {
        VectorVariable {
            id: Uuid::new_v4(),
            name: "Point".into(),
            value,
            space: VariableSpace::World,
        }
    }
    #[test]
    fn rigid_frame_preserves_curve_geometry_scale_and_anchors_under_rotated_parent() {
        let mut parent = SdfObject::create_kind(PrimitiveKind::Sphere);
        parent.group_transform = Transform {
            translation: Vec3::new(3., -2., 1.),
            rotation: Quat::from_rotation_z(0.7),
            scale: Vec3::splat(2.),
        };
        let mut curve = SdfObject::create_kind(PrimitiveKind::BezierCurve);
        curve.boolean_parent = Some(parent.uuid);
        curve.transform.scale = Vec3::new(1.5, 0.8, 0.7);
        let params = curve.params.clone();
        let origin = variable(Vec3::new(1., 2., 3.));
        let aim = variable(origin.value + Vec3::Y);
        let binding = RigidBinding {
            object: curve.uuid,
            target: RigidBindingTarget::Object,
            origin: origin.id,
            aim: aim.id,
            up: RigidBindingUp::WorldDirection(Vec3::Z),
            local_origin: Vec3::new(0.2, 0.3, 0.),
            local_aim: Vec3::new(1.2, 0.3, 0.),
            local_up: Vec3::new(0.2, 0.3, 1.),
        };
        let variables = SceneVariables {
            vectors: vec![origin.clone(), aim],
            rigid_bindings: vec![binding.clone()],
            ..SceneVariables::default()
        };
        let mut objects = vec![parent, curve.clone()];
        apply_vector_bindings(&variables, &mut objects);
        let world = super::super::object_world_matrix(&objects, curve.uuid);
        assert!(
            world
                .transform_point3(binding.local_origin)
                .distance(origin.value)
                < 1e-5
        );
        assert!(
            (world.transform_point3(binding.local_aim) - origin.value)
                .cross(Vec3::Y)
                .length()
                < 1e-5
        );
        assert_eq!(
            serde_json::to_value(&objects[1].params).unwrap(),
            serde_json::to_value(&params).unwrap()
        );
        assert_eq!(objects[1].transform.scale, curve.transform.scale);
        let first = objects[1].transform;
        apply_vector_bindings(&variables, &mut objects);
        assert_eq!(first, objects[1].transform);
    }
    #[test]
    fn rigid_group_settles_before_child_and_unlink_retains_pose() {
        let parent = SdfObject::create_kind(PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.boolean_parent = Some(parent.uuid);
        let origin = variable(Vec3::new(2., 3., 0.));
        let aim = variable(origin.value + Vec3::Y);
        let child_origin = variable(Vec3::new(4., 3., 0.));
        let make = |object, target, origin| RigidBinding {
            object,
            target,
            origin,
            aim: aim.id,
            up: RigidBindingUp::WorldDirection(Vec3::Z),
            local_origin: Vec3::ZERO,
            local_aim: Vec3::X,
            local_up: Vec3::Z,
        };
        let mut variables = SceneVariables {
            vectors: vec![origin.clone(), aim.clone(), child_origin.clone()],
            rigid_bindings: vec![
                make(child.uuid, RigidBindingTarget::Object, child_origin.id),
                make(parent.uuid, RigidBindingTarget::Group, origin.id),
            ],
            ..SceneVariables::default()
        };
        let mut tree = DataTree::default();
        super::super::set_objects(&mut tree, vec![child.clone(), parent]);
        set_scene_variables(&mut tree, variables.clone());
        tree.make_undo_redo_snapshot();
        assert!(
            super::super::object_world_matrix(super::super::objects_ref(&tree), child.uuid)
                .transform_point3(Vec3::ZERO)
                .distance(child_origin.value)
                < 1e-5
        );
        let saved = crate::document::serialize_scene(&tree).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", crate::document::deserialize_scene(&saved).unwrap());
        assert_eq!(scene_variables(&restored), variables);
        let pose = super::super::objects_ref(&tree)[0].transform;
        variables.rigid_bindings.clear();
        set_scene_variables(&mut tree, variables);
        assert_eq!(super::super::objects_ref(&tree)[0].transform, pose);
        tree.undo();
        assert_eq!(scene_variables(&tree).rigid_bindings.len(), 2);
        tree.redo();
        assert!(scene_variables(&tree).rigid_bindings.is_empty());
    }
}

#[cfg(test)]
mod dependency_tests {
    use super::*;
    #[test]
    fn four_bar_dependencies_order_before_consumers_and_reject_cycles() {
        let id = || Uuid::new_v4();
        let first = super::super::PlanarFourBarConstraint {
            driver: id(),
            lower_pivot: id(),
            upper_pivot: id(),
            lower_joint: id(),
            upper_joint: id(),
            lower_length: 2.,
            upper_length: 2.,
            upright_length: 1.,
            driver_y_offset: 0.,
            travel_min: -0.5,
            travel_max: 0.5,
            branch: super::super::CircleIntersectionBranch::Positive,
        };
        let mut second = first.clone();
        second.driver = id();
        second.lower_pivot = first.lower_joint;
        second.lower_joint = id();
        second.upper_joint = id();
        let mut variables = SceneVariables {
            four_bar_constraints: vec![second, first],
            ..SceneVariables::default()
        };
        assert_eq!(four_bar_constraint_order(&variables).unwrap(), vec![1, 0]);
        variables.four_bar_constraints[1].upper_pivot =
            variables.four_bar_constraints[0].upper_joint;
        assert!(four_bar_constraint_order(&variables).is_err());
    }
}

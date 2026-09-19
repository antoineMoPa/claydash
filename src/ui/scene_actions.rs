//! Explicit target/operand editing shared by tree menus and drag-and-drop.
use super::*;
use std::collections::HashSet;
use uuid::Uuid;

pub(crate) fn pending_boolean(tree: &DataTree) -> Option<crate::model::BooleanPick> {
    match tree.get_path("editor.boolean_pick") {
        crate::model::ClaydashValue::BooleanPick(pick)
            if objects(tree)
                .iter()
                .any(|object| object.uuid == pick.target) =>
        {
            Some(pick)
        }
        _ => None,
    }
}

pub(crate) fn begin_boolean_pick(tree: &mut DataTree, operation: BooleanOperation) {
    let selection = selected(tree);
    let Some(target) = selection.first().copied() else {
        return;
    };
    if objects(tree).iter().any(|object| object.uuid == target) {
        // An operator ends placement/transform and starts a new editing action.
        tree.set_path(
            "editor.state",
            crate::model::ClaydashValue::EditorState(crate::model::EditorState::Start),
        );
        tree.set_path("editor.spawn_at_cursor", crate::model::ClaydashValue::None);
        cancel_boolean_pick(tree);
        tree.make_undo_redo_snapshot();
        if selection.len() > 1 {
            attach(tree, target, &selection[1..], operation);
            return;
        }
        tree.set_path(
            "editor.boolean_pick",
            crate::model::ClaydashValue::BooleanPick(crate::model::BooleanPick {
                target,
                operation,
            }),
        );
    }
}

pub(crate) fn cancel_boolean_pick(tree: &mut DataTree) {
    if pending_boolean(tree).is_some() {
        tree.set_path("editor.boolean_pick", crate::model::ClaydashValue::None);
    }
}

/// Returns true when an armed operation consumed the click, including invalid
/// self/ancestor picks. Invalid picks leave the target and operation armed.
pub(crate) fn apply_boolean_pick(tree: &mut DataTree, operand: Uuid) -> bool {
    let Some(pick) = pending_boolean(tree) else {
        return false;
    };
    if can_attach(&objects(tree), pick.target, &[operand]) {
        cancel_boolean_pick(tree); // Do not record transient pick mode in undo.
        attach(tree, pick.target, &[operand], pick.operation);
    }
    true
}

pub(crate) fn viewport_group_root(scene: &[SdfObject], hit: Uuid) -> Uuid {
    let mut root = hit;
    let mut visited = HashSet::new();
    while visited.insert(root) {
        let Some(parent) = scene
            .iter()
            .find(|object| object.uuid == root)
            .and_then(|object| object.boolean_parent)
        else {
            break;
        };
        if !scene.iter().any(|object| object.uuid == parent) {
            break;
        }
        root = parent;
    }
    root
}

/// Map a viewport hit to the group root on first click, then descend the
/// clicked branch by one hierarchy level on each repeated click.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ViewportSelectionTarget {
    pub id: Uuid,
    pub scope: crate::model::SelectionScope,
}

pub(crate) fn viewport_selection_target(
    scene: &[SdfObject],
    hit: Uuid,
    selection: &[Uuid],
) -> ViewportSelectionTarget {
    let mut path = vec![hit];
    let mut current = hit;
    let mut visited = HashSet::new();
    while visited.insert(current) {
        let Some(parent) = scene
            .iter()
            .find(|object| object.uuid == current)
            .and_then(|object| object.boolean_parent)
            .filter(|parent| scene.iter().any(|object| object.uuid == *parent))
        else {
            break;
        };
        path.push(parent);
        current = parent;
    }
    path.reverse();
    if selection.len() == 1 {
        if let Some(index) = path.iter().position(|id| *id == selection[0]) {
            if let Some(child) = path.get(index + 1) {
                return ViewportSelectionTarget {
                    id: *child,
                    scope: crate::model::SelectionScope::Group,
                };
            }
            return ViewportSelectionTarget {
                id: hit,
                scope: crate::model::SelectionScope::Exact,
            };
        }
    }
    ViewportSelectionTarget {
        id: path[0],
        scope: crate::model::SelectionScope::Group,
    }
}

#[derive(Clone, Default)]
pub(super) struct DragObjects(pub Vec<Uuid>);

// Keep selected groups intact: selected descendants travel with their selected ancestor.
pub(super) fn operand_roots(scene: &[SdfObject], ids: &[Uuid]) -> Vec<Uuid> {
    let selected: HashSet<_> = ids.iter().copied().collect();
    scene
        .iter()
        .filter(|object| selected.contains(&object.uuid))
        .filter(|object| {
            let mut parent = object.boolean_parent;
            let mut visited = HashSet::new();
            while let Some(id) = parent {
                if !visited.insert(id) || selected.contains(&id) {
                    return false;
                }
                parent = scene
                    .iter()
                    .find(|candidate| candidate.uuid == id)
                    .and_then(|candidate| candidate.boolean_parent);
            }
            true
        })
        .map(|object| object.uuid)
        .collect()
}

pub(crate) fn can_attach(scene: &[SdfObject], target: Uuid, operands: &[Uuid]) -> bool {
    if operands.is_empty() || !scene.iter().any(|object| object.uuid == target) {
        return false;
    }
    let roots = operand_roots(scene, operands);
    if roots.is_empty() {
        return false;
    }
    let mut ancestor = Some(target);
    let mut visited = HashSet::new();
    while let Some(id) = ancestor {
        if !visited.insert(id) || roots.contains(&id) {
            return false;
        }
        ancestor = scene
            .iter()
            .find(|object| object.uuid == id)
            .and_then(|object| object.boolean_parent);
    }
    true
}

pub(crate) fn attach(
    tree: &mut DataTree,
    target: Uuid,
    operands: &[Uuid],
    operation: BooleanOperation,
) -> bool {
    let mut scene = objects(tree);
    if !can_attach(&scene, target, operands) {
        return false;
    }
    let roots = operand_roots(&scene, operands);
    for object in &mut scene {
        if roots.contains(&object.uuid) {
            object.boolean_parent = Some(target);
            object.operation = operation;
        }
    }
    set_objects(tree, scene);
    set_selected(tree, vec![target]);
    tree.make_undo_redo_snapshot();
    true
}

pub(super) fn add_operand(
    tree: &mut DataTree,
    target: Uuid,
    kind: PrimitiveKind,
    operation: BooleanOperation,
) {
    let mut scene = objects(tree);
    let Some(parent) = scene.iter().find(|object| object.uuid == target) else {
        return;
    };
    let mut operand = SdfObject::create_kind(kind);
    operand.material = crate::model::picked_material(tree);
    operand.color = operand.material.color;
    operand.transform = parent.transform.clone();
    operand.transform.scale *= 0.65;
    operand.boolean_parent = Some(target);
    operand.operation = operation;
    let id = operand.uuid;
    scene.push(operand);
    set_objects(tree, scene);
    set_selected(tree, vec![id]);
    tree.make_undo_redo_snapshot();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_selection_descends_the_clicked_boolean_branch() {
        let root = SdfObject::create_kind(PrimitiveKind::Box);
        let mut group = SdfObject::create_kind(PrimitiveKind::Sphere);
        group.boolean_parent = Some(root.uuid);
        let mut primitive = SdfObject::create_kind(PrimitiveKind::Cylinder);
        primitive.boolean_parent = Some(group.uuid);
        let scene = vec![root.clone(), group.clone(), primitive.clone()];

        assert_eq!(
            viewport_selection_target(&scene, primitive.uuid, &[]),
            ViewportSelectionTarget {
                id: root.uuid,
                scope: crate::model::SelectionScope::Group,
            }
        );
        assert_eq!(
            viewport_selection_target(&scene, primitive.uuid, &[root.uuid]),
            ViewportSelectionTarget {
                id: group.uuid,
                scope: crate::model::SelectionScope::Group,
            }
        );
        assert_eq!(
            viewport_selection_target(&scene, primitive.uuid, &[group.uuid]),
            ViewportSelectionTarget {
                id: primitive.uuid,
                scope: crate::model::SelectionScope::Group,
            }
        );
        assert_eq!(
            viewport_selection_target(&scene, primitive.uuid, &[primitive.uuid]),
            ViewportSelectionTarget {
                id: primitive.uuid,
                scope: crate::model::SelectionScope::Exact,
            }
        );
        assert_eq!(
            viewport_selection_target(&scene, root.uuid, &[root.uuid]),
            ViewportSelectionTarget {
                id: root.uuid,
                scope: crate::model::SelectionScope::Exact,
            }
        );
    }

    #[test]
    fn attaching_groups_preserves_nested_operations_and_rejects_cycles() {
        let target = SdfObject::create_kind(PrimitiveKind::Box);
        let group = SdfObject::create_kind(PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(PrimitiveKind::Cylinder);
        child.boolean_parent = Some(group.uuid);
        child.operation = BooleanOperation::Intersect;
        let mut tree = DataTree::default();
        set_objects(
            &mut tree,
            vec![target.clone(), group.clone(), child.clone()],
        );
        tree.make_undo_redo_snapshot();
        assert!(attach(
            &mut tree,
            target.uuid,
            &[group.uuid, child.uuid],
            BooleanOperation::Subtract
        ));
        let scene = objects(&tree);
        assert_eq!(scene[1].boolean_parent, Some(target.uuid));
        assert_eq!(scene[1].operation, BooleanOperation::Subtract);
        assert_eq!(scene[2].boolean_parent, Some(group.uuid));
        assert_eq!(scene[2].operation, BooleanOperation::Intersect);
        assert_eq!(selected(&tree), vec![target.uuid]);
        assert!(!attach(
            &mut tree,
            child.uuid,
            &[target.uuid],
            BooleanOperation::Union
        ));
        assert!(!attach(
            &mut tree,
            target.uuid,
            &[target.uuid],
            BooleanOperation::Union
        ));
        undo_redo::undo(&mut tree);
        assert_eq!(objects(&tree)[1].boolean_parent, None);
    }
    #[test]
    fn new_cutter_is_selected_at_target_and_can_be_undone() {
        let mut target = SdfObject::create_kind(PrimitiveKind::Box);
        target.transform.translation = Vec3::new(4.0, 2.0, -3.0);
        let mut tree = DataTree::default();
        set_objects(&mut tree, vec![target.clone()]);
        tree.make_undo_redo_snapshot();
        add_operand(
            &mut tree,
            target.uuid,
            PrimitiveKind::Sphere,
            BooleanOperation::Subtract,
        );
        let scene = objects(&tree);
        assert_eq!(scene[1].boolean_parent, Some(target.uuid));
        assert_eq!(scene[1].operation, BooleanOperation::Subtract);
        assert_eq!(scene[1].transform.translation, target.transform.translation);
        assert_eq!(selected(&tree), vec![scene[1].uuid]);
        undo_redo::undo(&mut tree);
        assert_eq!(objects(&tree).len(), 1);
    }
}

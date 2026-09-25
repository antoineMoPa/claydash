use command_central::{CommandBuilder, CommandMap};
use sdf_consts::{TYPE_BOX, TYPE_CYLINDER, TYPE_SPHERE, TYPE_TORUS};

use crate::{
    animation,
    model::{
        objects, selected, set_objects, set_selected, ClaydashValue, DataTree, EditorState,
        SdfObject,
    },
    undo_redo,
};

pub type Commands = CommandMap<ClaydashValue>;

pub(crate) const EXTRUSION_INITIAL_HALF_EXTENT: f32 = 0.01;

fn register(
    commands: &mut Commands,
    name: &str,
    title: &str,
    docs: &str,
    shortcut: &str,
    callback: fn(&mut DataTree),
) {
    CommandBuilder::new()
        .system_name(name)
        .title(title)
        .docs(docs)
        .shortcut(shortcut)
        .insert_param(
            "callback",
            "command callback",
            Some(ClaydashValue::Fn(callback)),
        )
        .write(commands);
}

pub fn register_all(commands: &mut Commands) {
    register(
        commands,
        "grab",
        "Grab",
        "Move the selection in the current view plane.",
        "",
        start_grab,
    );
    register(
        commands,
        "scale",
        "Scale",
        "Start scaling selection.",
        "S",
        start_scale,
    );
    register(
        commands,
        "rotate",
        "Rotate",
        "Start rotating selection.",
        "R",
        start_rotate,
    );
    register(
        commands,
        "extrude",
        "Extrude selected face",
        "Create an adjacent box or cylinder from the selected planar face.",
        "E",
        extrude_selected_face,
    );
    register(
        commands,
        "union",
        "Union",
        "Union the selection, using the first selected object as the target.",
        "G",
        |tree| {
            crate::ui::scene_actions::begin_boolean_pick(
                tree,
                crate::model::BooleanOperation::Union,
            )
        },
    );
    register(
        commands,
        "constrain_x",
        "Constrain X",
        "Constrain editing to X axis.",
        "X",
        |tree| toggle_constraint(tree, "editor.constrain_x"),
    );
    register(
        commands,
        "constrain_y",
        "Constrain Y",
        "Constrain editing to Y axis.",
        "Y",
        |tree| toggle_constraint(tree, "editor.constrain_y"),
    );
    register(
        commands,
        "constrain_z",
        "Constrain Z",
        "Constrain editing to Z axis.",
        "Z",
        |tree| toggle_constraint(tree, "editor.constrain_z"),
    );
    register(
        commands,
        "quit",
        "Quit",
        "Cancel current edit.",
        "Escape",
        cancel,
    );
    register(
        commands,
        "finish",
        "Finish",
        "Finish current edit.",
        "Return",
        finish,
    );
    register(
        commands,
        "delete",
        "Delete",
        "Delete selection.",
        "Back",
        delete,
    );
    register(
        commands,
        "select_all_or_none",
        "Select all/none",
        "Toggle selecting all objects.",
        "Shift+A",
        select_all,
    );
    register(
        commands,
        "invert_selection",
        "Invert Selection",
        "Select every unselected object and deselect every selected object.",
        "Cmd/Ctrl+I",
        invert_selection,
    );
    register(
        commands,
        "duplicate",
        "Duplicate",
        "Duplicate selection.",
        "Shift+D",
        duplicate,
    );
    register(
        commands,
        "spawn-sphere",
        "Spawn Sphere",
        "Add a sphere.",
        "",
        |tree| spawn(tree, TYPE_SPHERE),
    );
    register(
        commands,
        "spawn-box",
        "Spawn Box",
        "Add a box.",
        "",
        |tree| spawn(tree, TYPE_BOX),
    );
    register(
        commands,
        "spawn-cylinder",
        "Add Cylinder",
        "Add a cylinder primitive.",
        "",
        |tree| spawn(tree, TYPE_CYLINDER),
    );
    register(
        commands,
        "spawn-torus",
        "Add Torus",
        "Add a torus primitive.",
        "",
        |tree| spawn(tree, TYPE_TORUS),
    );
    register(
        commands,
        "undo",
        "Undo",
        "Undo last action.",
        undo_redo::UNDO_SHORTCUT,
        undo_redo::undo,
    );
    register(
        commands,
        "redo",
        "Redo",
        "Redo last action.",
        undo_redo::REDO_SHORTCUT,
        undo_redo::redo,
    );
}

pub fn execute(commands: &Commands, name: &str, tree: &mut DataTree) {
    let Some(command) = commands.commands.get(name) else {
        return;
    };
    if let Some(ClaydashValue::Fn(callback)) = command
        .parameters
        .get("callback")
        .and_then(|parameter| parameter.value.clone())
    {
        callback(tree);
    }
}

fn start_edit(tree: &mut DataTree, state: EditorState) {
    if selected(tree).is_empty() {
        return;
    }
    tree.set_path("editor.constrain_x", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_y", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_z", ClaydashValue::Bool(false));
    tree.set_path("editor.state", ClaydashValue::EditorState(state));
}

pub fn start_grab(tree: &mut DataTree) {
    if let Some(face) = crate::model::selected_modeling_face(tree) {
        let scene = objects(tree);
        if selected(tree) == vec![face.object()]
            && !crate::model::has_boolean_children(&scene, face.object())
        {
            if let Some(object) = scene.iter().find(|object| object.uuid == face.object()) {
                let supported = matches!(
                    (face, &object.params),
                    (
                        crate::model::ModelingFaceSelection::Box(_),
                        crate::model::SdfParams::BoxParams(_)
                    ) | (
                        crate::model::ModelingFaceSelection::CylinderCap(_),
                        crate::model::SdfParams::CylinderParams { .. }
                    )
                );
                if supported {
                    tree.set_transient_path(
                        "editor.face_drag_initial_object",
                        ClaydashValue::VecSDFObject(vec![object.clone()]),
                    );
                    start_edit(tree, EditorState::DraggingFace);
                    return;
                }
            }
        }
    }
    start_edit(tree, EditorState::Grabbing);
}

pub fn start_scale(tree: &mut DataTree) {
    start_edit(tree, EditorState::Scaling);
}

pub fn start_rotate(tree: &mut DataTree) {
    start_edit(tree, EditorState::Rotating);
}

fn toggle_constraint(tree: &mut DataTree, path: &str) {
    let current = matches!(tree.get_path(path), ClaydashValue::Bool(true));
    for axis in [
        "editor.constrain_x",
        "editor.constrain_y",
        "editor.constrain_z",
    ] {
        tree.set_path(axis, ClaydashValue::Bool(axis == path && !current));
    }
}

fn cancel(tree: &mut DataTree) {
    if matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::DraggingFace)
    ) {
        if let ClaydashValue::VecSDFObject(initial) =
            tree.get_path("editor.face_drag_initial_object")
        {
            if let Some(initial) = initial.into_iter().next() {
                let mut scene = objects(tree);
                if let Some(object) = scene.iter_mut().find(|object| object.uuid == initial.uuid) {
                    *object = initial;
                    set_objects(tree, scene);
                }
            }
        }
        tree.set_transient_path("editor.face_drag_initial_object", ClaydashValue::None);
        tree.set_path(
            "editor.state",
            ClaydashValue::EditorState(EditorState::Start),
        );
        return;
    }
    if matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::Extruding)
    ) {
        if let ClaydashValue::Uuid(extrusion) = tree.get_path("editor.extrusion_object") {
            let scene = objects(tree)
                .into_iter()
                .filter(|object| object.uuid != extrusion)
                .collect();
            set_objects(tree, scene);
        }
        if let ClaydashValue::BoxFaceSelection(face) = tree.get_path("editor.extrusion_source_face")
        {
            crate::model::set_selected_exact(tree, vec![face.object]);
            crate::model::set_selected_box_face(tree, Some(face));
        } else if let ClaydashValue::ModelingFaceSelection(face) =
            tree.get_path("editor.extrusion_source_face")
        {
            crate::model::set_selected_exact(tree, vec![face.object()]);
            crate::model::set_selected_modeling_face(tree, Some(face));
        }
        tree.set_transient_path("editor.extrusion_object", ClaydashValue::None);
        tree.set_transient_path("editor.extrusion_source_face", ClaydashValue::None);
        tree.set_path(
            "editor.state",
            ClaydashValue::EditorState(EditorState::Start),
        );
        return;
    }
    let targets = transform_targets(tree);
    let mut scene = objects(tree);
    let mut cameras = crate::model::scene_cameras(tree);
    for target in targets {
        let path = match target.kind {
            TransformTargetKind::Object => "editor.initial_transform",
            TransformTargetKind::Group => "editor.initial_group_transform",
            TransformTargetKind::Camera => "editor.initial_camera_transform",
        };
        if let ClaydashValue::Transform(transform) = tree.get_path(&format!("{path}.{}", target.id))
        {
            set_transform_target(&mut scene, &mut cameras, target.kind, target.id, transform);
        }
    }
    set_objects(tree, scene);
    crate::model::set_scene_cameras(tree, cameras);
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
}

fn finish(tree: &mut DataTree) {
    tree.set_transient_path("editor.face_drag_initial_object", ClaydashValue::None);
    tree.set_transient_path("editor.extrusion_object", ClaydashValue::None);
    tree.set_transient_path("editor.extrusion_source_face", ClaydashValue::None);
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
    tree.make_undo_redo_snapshot();
}

fn delete(tree: &mut DataTree) {
    let selection = selected_subtree_ids(&objects(tree), &effective_selected_ids(tree));
    animation::remove_tracks_for_objects(tree, &selection);
    let mut scene: Vec<_> = objects(tree)
        .into_iter()
        .filter(|object| !selection.contains(&object.uuid))
        .collect();
    crate::model::map_leaf_group_transforms_to_primitives(&mut scene);
    set_objects(tree, scene);
    let cameras: Vec<_> = crate::model::scene_cameras(tree)
        .into_iter()
        .filter(|camera| !selection.contains(&camera.uuid))
        .collect();
    let active = crate::model::active_camera_id(tree);
    if active.is_some_and(|id| selection.contains(&id)) {
        if let Some(camera) = cameras.first() {
            tree.set_path("scene.active_camera", ClaydashValue::Uuid(camera.uuid));
        } else {
            tree.set_path("scene.active_camera", ClaydashValue::None);
            tree.set_transient_path("editor.camera_view", ClaydashValue::Bool(false));
        }
    }
    crate::model::set_scene_cameras(tree, cameras);
    set_selected(tree, vec![]);
    tree.make_undo_redo_snapshot();
}

fn select_all(tree: &mut DataTree) {
    let mut ids: Vec<_> = objects(tree)
        .into_iter()
        .map(|object| object.uuid)
        .collect();
    ids.extend(
        crate::model::scene_cameras(tree)
            .into_iter()
            .map(|camera| camera.uuid),
    );
    if selected(tree).len() == ids.len() {
        set_selected(tree, vec![]);
    } else {
        set_selected(tree, ids);
    }
}

fn invert_selection(tree: &mut DataTree) {
    let current = selected(tree);
    let mut inverted: Vec<_> = objects(tree)
        .into_iter()
        .filter(|object| !current.contains(&object.uuid))
        .map(|object| object.uuid)
        .collect();
    inverted.extend(
        crate::model::scene_cameras(tree)
            .into_iter()
            .filter(|camera| !current.contains(&camera.uuid))
            .map(|camera| camera.uuid),
    );
    set_selected(tree, inverted);
}

fn duplicate(tree: &mut DataTree) {
    let mut scene = objects(tree);
    let selection = effective_selected_ids(tree);
    let id_map: std::collections::HashMap<_, _> = selection
        .iter()
        .map(|id| (*id, uuid::Uuid::new_v4()))
        .collect();
    let copies: Vec<_> = scene
        .iter()
        .filter(|object| selection.contains(&object.uuid))
        .map(|object| {
            let mut copy = object.duplicate();
            copy.uuid = id_map[&object.uuid];
            copy.boolean_parent = object
                .boolean_parent
                .map(|id| id_map.get(&id).copied().unwrap_or(id));
            if copy.boolean_parent.is_none() {
                copy.operation = crate::model::BooleanOperation::Union;
            }
            copy
        })
        .collect();
    let mut cameras = crate::model::scene_cameras(tree);
    let camera_copies: Vec<_> = cameras
        .iter()
        .filter(|camera| selection.contains(&camera.uuid))
        .map(|camera| {
            let mut copy = camera.clone();
            copy.uuid = id_map[&camera.uuid];
            copy.name = format!("{} copy", camera.name);
            copy
        })
        .collect();
    animation::duplicate_tracks_for_objects(tree, &id_map);
    let mut copied_ids: Vec<_> = copies.iter().map(|object| object.uuid).collect();
    copied_ids.extend(camera_copies.iter().map(|camera| camera.uuid));
    set_selected(tree, copied_ids);
    scene.extend(copies);
    cameras.extend(camera_copies);
    set_objects(tree, scene);
    crate::model::set_scene_cameras(tree, cameras);
    start_grab(tree);
}

pub fn extrude_selected_face(tree: &mut DataTree) {
    if let Some(crate::model::ModelingFaceSelection::CylinderCap(face)) =
        crate::model::selected_modeling_face(tree)
    {
        extrude_cylinder_cap(tree, face);
        return;
    }
    let Some(face) = crate::model::selected_box_face(tree) else {
        return;
    };
    if !selected(tree).contains(&face.object) {
        return;
    }
    let mut scene = objects(tree);
    if crate::model::has_boolean_children(&scene, face.object) {
        return;
    }
    let Some(source) = scene
        .iter()
        .find(|object| object.uuid == face.object)
        .cloned()
    else {
        return;
    };
    let crate::model::SdfParams::BoxParams(params) = &source.params else {
        return;
    };
    let axis = face.axis.index();
    let source_half_extent = params.box_q[axis];
    let mut local_offset = glam::Vec3::ZERO;
    local_offset[axis] = (source_half_extent + EXTRUSION_INITIAL_HALF_EXTENT)
        * if face.positive { 1.0 } else { -1.0 };
    let mut extrusion = source.duplicate();
    extrusion.name = format!("{} extrusion", source.display_name());
    extrusion.transform.translation += source.transform.matrix().transform_vector3(local_offset);
    let crate::model::SdfParams::BoxParams(extrusion_params) = &mut extrusion.params else {
        unreachable!()
    };
    extrusion_params.box_q[axis] = EXTRUSION_INITIAL_HALF_EXTENT;
    let extrusion_id = extrusion.uuid;
    scene.push(extrusion);
    set_objects(tree, scene);
    crate::model::set_selected_exact(tree, vec![extrusion_id]);
    crate::model::set_selected_box_face(
        tree,
        Some(crate::model::BoxFaceSelection {
            object: extrusion_id,
            axis: face.axis,
            positive: face.positive,
        }),
    );
    tree.set_transient_path("editor.extrusion_object", ClaydashValue::Uuid(extrusion_id));
    tree.set_transient_path(
        "editor.extrusion_source_face",
        ClaydashValue::BoxFaceSelection(face),
    );
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Extruding),
    );
}

fn extrude_cylinder_cap(tree: &mut DataTree, face: crate::model::CylinderCapSelection) {
    if !selected(tree).contains(&face.object) {
        return;
    }
    let mut scene = objects(tree);
    if crate::model::has_boolean_children(&scene, face.object) {
        return;
    }
    let Some(source) = scene
        .iter()
        .find(|object| object.uuid == face.object)
        .cloned()
    else {
        return;
    };
    let crate::model::SdfParams::CylinderParams { half_height, .. } = source.params else {
        return;
    };
    let sign = if face.positive { 1.0 } else { -1.0 };
    let mut extrusion = source.duplicate();
    extrusion.name = format!("{} extrusion", source.display_name());
    extrusion.transform.translation += source
        .transform
        .matrix()
        .transform_vector3(glam::Vec3::Y * (half_height + EXTRUSION_INITIAL_HALF_EXTENT) * sign);
    let crate::model::SdfParams::CylinderParams { half_height, .. } = &mut extrusion.params else {
        unreachable!()
    };
    *half_height = EXTRUSION_INITIAL_HALF_EXTENT;
    let extrusion_id = extrusion.uuid;
    scene.push(extrusion);
    set_objects(tree, scene);
    crate::model::set_selected_exact(tree, vec![extrusion_id]);
    crate::model::set_selected_modeling_face(
        tree,
        Some(crate::model::ModelingFaceSelection::CylinderCap(
            crate::model::CylinderCapSelection {
                object: extrusion_id,
                positive: face.positive,
            },
        )),
    );
    tree.set_transient_path("editor.extrusion_object", ClaydashValue::Uuid(extrusion_id));
    tree.set_transient_path(
        "editor.extrusion_source_face",
        ClaydashValue::ModelingFaceSelection(crate::model::ModelingFaceSelection::CylinderCap(
            face,
        )),
    );
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Extruding),
    );
}

pub fn duplicate_object(tree: &mut DataTree, id: uuid::Uuid) {
    if objects(tree).iter().any(|object| object.uuid == id) {
        set_selected(tree, vec![id]);
        duplicate(tree);
    }
}

/// Include descendants so editing a group cannot leave dangling parent links.
pub(crate) fn selected_subtree_ids(
    scene: &[SdfObject],
    selection: &[uuid::Uuid],
) -> Vec<uuid::Uuid> {
    let mut ids = selection.to_vec();
    loop {
        let previous_len = ids.len();
        for object in scene {
            if object
                .boolean_parent
                .is_some_and(|parent| ids.contains(&parent))
                && !ids.contains(&object.uuid)
            {
                ids.push(object.uuid);
            }
        }
        if ids.len() == previous_len {
            return ids;
        }
    }
}

pub(crate) fn effective_selected_ids(tree: &DataTree) -> Vec<uuid::Uuid> {
    let selection = selected(tree);
    match crate::model::selection_scope(tree) {
        crate::model::SelectionScope::Group => selected_subtree_ids(&objects(tree), &selection),
        crate::model::SelectionScope::Exact => selection,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransformTargetKind {
    Object,
    Group,
    Camera,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TransformTarget {
    pub id: uuid::Uuid,
    pub kind: TransformTargetKind,
    pub transform: crate::model::Transform,
    pub parent_world: glam::Mat4,
    pub world: glam::Mat4,
}

pub(crate) fn selected_group_id(tree: &DataTree) -> Option<uuid::Uuid> {
    let selection = selected(tree);
    let id = *selection.first()?;
    (selection.len() == 1
        && crate::model::selection_scope(tree) == crate::model::SelectionScope::Group
        && crate::model::has_boolean_children(&objects(tree), id))
    .then_some(id)
}

pub(crate) fn transform_targets(tree: &DataTree) -> Vec<TransformTarget> {
    let scene = objects(tree);
    let selection = selected(tree);
    let group_scope = crate::model::selection_scope(tree) == crate::model::SelectionScope::Group;
    let mut targets: Vec<_> = selection
        .iter()
        .filter(|id| {
            if !group_scope {
                return true;
            }
            let mut parent = scene
                .iter()
                .find(|object| object.uuid == **id)
                .and_then(|object| object.boolean_parent);
            while let Some(ancestor) = parent {
                if selection.contains(&ancestor) {
                    return false;
                }
                parent = scene
                    .iter()
                    .find(|object| object.uuid == ancestor)
                    .and_then(|object| object.boolean_parent);
            }
            true
        })
        .filter_map(|id| {
            let object = scene.iter().find(|object| object.uuid == *id)?;
            let parent_world = crate::model::parent_group_world_matrix(&scene, *id);
            if group_scope && crate::model::has_boolean_children(&scene, *id) {
                Some(TransformTarget {
                    id: *id,
                    kind: TransformTargetKind::Group,
                    transform: object.group_transform,
                    parent_world,
                    world: crate::model::group_world_matrix(&scene, *id),
                })
            } else {
                let group_world = crate::model::group_world_matrix(&scene, *id);
                Some(TransformTarget {
                    id: *id,
                    kind: TransformTargetKind::Object,
                    transform: object.transform,
                    parent_world: group_world,
                    world: group_world * object.transform.matrix(),
                })
            }
        })
        .collect();
    for camera in crate::model::scene_cameras(tree) {
        if selection.contains(&camera.uuid) {
            targets.push(TransformTarget {
                id: camera.uuid,
                kind: TransformTargetKind::Camera,
                transform: camera.transform,
                parent_world: glam::Mat4::IDENTITY,
                world: camera.transform.matrix(),
            });
        }
    }
    targets
}

pub(crate) fn set_transform_target(
    scene: &mut [SdfObject],
    cameras: &mut [crate::camera::SceneCamera],
    target: TransformTargetKind,
    id: uuid::Uuid,
    transform: crate::model::Transform,
) {
    match target {
        TransformTargetKind::Object => {
            if let Some(object) = scene.iter_mut().find(|object| object.uuid == id) {
                object.transform = transform;
            }
        }
        TransformTargetKind::Group => {
            if let Some(object) = scene.iter_mut().find(|object| object.uuid == id) {
                object.group_transform = transform;
            }
        }
        TransformTargetKind::Camera => {
            if let Some(camera) = cameras.iter_mut().find(|camera| camera.uuid == id) {
                camera.transform = transform;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BooleanOperation;

    #[test]
    fn all_new_primitives_inherit_the_complete_picked_material() {
        let mut tree = DataTree::default();
        let mut material = crate::model::Material::preset(crate::model::MaterialKind::Wood);
        material.roughness = 0.37;
        let material_id = crate::model::ensure_material_asset(&mut tree, material);
        tree.set_path("editor.material", ClaydashValue::Material(material));
        tree.set_path("editor.material_id", ClaydashValue::Uuid(material_id));
        for kind in [TYPE_BOX, TYPE_SPHERE, TYPE_CYLINDER, TYPE_TORUS] {
            spawn(&mut tree, kind);
            let scene = objects(&tree);
            let object = scene.last().unwrap();
            assert_eq!(object.material.kind, material.kind);
            assert_eq!(object.material.roughness, material.roughness);
            assert_eq!(object.color, material.color);
            assert_eq!(object.material_id, Some(material_id));
        }
    }

    fn group_tree() -> DataTree {
        let mut tree = DataTree::default();
        let target = SdfObject::create(TYPE_BOX);
        let mut cutter = SdfObject::create(TYPE_SPHERE);
        cutter.boolean_parent = Some(target.uuid);
        cutter.operation = BooleanOperation::Subtract;
        set_selected(&mut tree, vec![target.uuid]);
        set_objects(&mut tree, vec![target, cutter]);
        tree
    }

    #[test]
    fn invert_selection_selects_only_previously_unselected_objects() {
        let mut tree = DataTree::default();
        let scene = vec![
            SdfObject::create(TYPE_BOX),
            SdfObject::create(TYPE_SPHERE),
            SdfObject::create(TYPE_CYLINDER),
        ];
        set_selected(&mut tree, vec![scene[1].uuid]);
        set_objects(&mut tree, scene.clone());

        invert_selection(&mut tree);

        assert_eq!(selected(&tree), vec![scene[0].uuid, scene[2].uuid]);
    }

    #[test]
    fn deleting_a_group_removes_its_operands() {
        let mut tree = group_tree();
        delete(&mut tree);
        assert!(objects(&tree).is_empty());
        assert!(selected(&tree).is_empty());
    }

    #[test]
    fn deleting_an_exactly_selected_split_prism_removes_its_void() {
        let mut tree = DataTree::default();
        let source = SdfObject::create(TYPE_BOX);
        let mut prism = SdfObject::create(sdf_consts::TYPE_POLYGON_PRISM);
        prism.boolean_parent = Some(source.uuid);
        let mut void = SdfObject::create(sdf_consts::TYPE_POLYGON_PRISM);
        void.boolean_parent = Some(prism.uuid);
        void.operation = BooleanOperation::Subtract;
        crate::model::set_selected_exact(&mut tree, vec![prism.uuid]);
        set_objects(&mut tree, vec![source.clone(), prism, void]);

        delete(&mut tree);

        let scene = objects(&tree);
        assert_eq!(scene.len(), 1);
        assert_eq!(scene[0].uuid, source.uuid);
    }

    #[test]
    fn duplicating_a_group_remaps_operand_parents() {
        let mut tree = group_tree();
        duplicate(&mut tree);
        let scene = objects(&tree);
        assert_eq!(scene.len(), 4);
        assert_eq!(scene[3].boolean_parent, Some(scene[2].uuid));
        assert_eq!(scene[3].operation, BooleanOperation::Subtract);
        assert_ne!(scene[2].uuid, scene[0].uuid);
        assert_eq!(selected(&tree), vec![scene[2].uuid, scene[3].uuid]);
    }

    #[test]
    fn duplicating_a_cutter_keeps_its_target_and_operation() {
        let mut tree = group_tree();
        let original = objects(&tree);
        duplicate_object(&mut tree, original[1].uuid);
        let scene = objects(&tree);
        assert_eq!(scene.len(), 3);
        assert_eq!(scene[2].boolean_parent, Some(original[0].uuid));
        assert_eq!(scene[2].operation, BooleanOperation::Subtract);
        assert_ne!(scene[2].uuid, original[1].uuid);
        assert_eq!(selected(&tree), vec![scene[2].uuid]);
    }

    #[test]
    fn camera_objects_are_regular_transform_targets() {
        let mut tree = DataTree::default();
        let view = crate::camera::Camera::new();
        let camera = crate::camera::SceneCamera::from_view("Camera", &view);
        let id = camera.uuid;
        crate::model::set_scene_cameras(&mut tree, vec![camera]);
        set_selected(&mut tree, vec![id]);

        let target = transform_targets(&tree)[0];
        assert_eq!(target.kind, TransformTargetKind::Camera);
        let mut scene = objects(&tree);
        let mut cameras = crate::model::scene_cameras(&tree);
        let mut transform = target.transform;
        transform.translation.x += 2.0;
        transform.rotation = glam::Quat::from_rotation_y(0.5);
        transform.scale = glam::Vec3::splat(1.5);
        set_transform_target(
            &mut scene,
            &mut cameras,
            TransformTargetKind::Camera,
            id,
            transform,
        );

        assert_eq!(cameras[0].transform, transform);
    }

    #[test]
    fn extruding_a_box_face_creates_an_adjacent_selected_box() {
        let mut tree = DataTree::default();
        let mut source = SdfObject::create(TYPE_BOX);
        source.transform.translation = glam::Vec3::new(1.0, 2.0, 3.0);
        source.transform.rotation = glam::Quat::from_rotation_z(0.4);
        source.transform.scale = glam::Vec3::new(1.5, 0.8, 1.2);
        let source_id = source.uuid;
        let expected_offset = source
            .transform
            .matrix()
            .transform_vector3(glam::Vec3::X * (0.3 + EXTRUSION_INITIAL_HALF_EXTENT));
        set_objects(&mut tree, vec![source.clone()]);
        crate::model::set_selected_exact(&mut tree, vec![source_id]);
        crate::model::set_selected_box_face(
            &mut tree,
            Some(crate::model::BoxFaceSelection {
                object: source_id,
                axis: crate::model::VectorAxis::X,
                positive: true,
            }),
        );

        extrude_selected_face(&mut tree);

        let scene = objects(&tree);
        assert_eq!(scene.len(), 2);
        let (
            crate::model::SdfParams::BoxParams(source_params),
            crate::model::SdfParams::BoxParams(extrusion_params),
        ) = (&source.params, &scene[1].params)
        else {
            unreachable!()
        };
        assert_eq!(extrusion_params.box_q.y, source_params.box_q.y);
        assert_eq!(extrusion_params.box_q.z, source_params.box_q.z);
        assert_eq!(extrusion_params.box_q.x, EXTRUSION_INITIAL_HALF_EXTENT);
        assert!(
            scene[1]
                .transform
                .translation
                .distance(source.transform.translation + expected_offset)
                < 0.0001
        );
        assert_eq!(selected(&tree), vec![scene[1].uuid]);
        assert!(matches!(
            tree.get_path("editor.state"),
            ClaydashValue::EditorState(EditorState::Extruding)
        ));
        assert_eq!(
            crate::model::selected_box_face(&tree),
            Some(crate::model::BoxFaceSelection {
                object: scene[1].uuid,
                axis: crate::model::VectorAxis::X,
                positive: true,
            })
        );
    }

    #[test]
    fn extruding_the_bottom_cylinder_cap_creates_a_matching_adjacent_cylinder() {
        let mut tree = DataTree::default();
        let source = SdfObject::create(TYPE_CYLINDER);
        let source_id = source.uuid;
        set_objects(&mut tree, vec![source]);
        set_selected(&mut tree, vec![source_id]);
        crate::model::set_selected_modeling_face(
            &mut tree,
            Some(crate::model::ModelingFaceSelection::CylinderCap(
                crate::model::CylinderCapSelection {
                    object: source_id,
                    positive: false,
                },
            )),
        );
        extrude_selected_face(&mut tree);
        let scene = objects(&tree);
        assert_eq!(scene.len(), 2);
        let crate::model::SdfParams::CylinderParams {
            radius,
            half_height,
        } = scene[1].params
        else {
            unreachable!()
        };
        assert_eq!(radius, 0.25);
        assert_eq!(half_height, EXTRUSION_INITIAL_HALF_EXTENT);
        assert!((scene[1].transform.translation.y + 0.35 + half_height).abs() < 0.0001);
        assert_eq!(selected(&tree), vec![scene[1].uuid]);
    }
}

pub fn spawn(tree: &mut DataTree, kind: i32) {
    let mut object = SdfObject::create(kind);
    object.material = crate::model::picked_material(tree);
    object.material_id = crate::model::picked_material_id(tree);
    object.color = object.material.color;
    let uuid = object.uuid;
    let mut scene = objects(tree);
    scene.push(object);
    set_objects(tree, scene);
    set_selected(tree, vec![uuid]);
    tree.set_path("editor.spawn_at_cursor", ClaydashValue::Uuid(uuid));
    start_grab(tree);
}

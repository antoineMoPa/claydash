use command_central::{CommandBuilder, CommandMap};
use sdf_consts::{
    TYPE_BOX, TYPE_CYLINDER, TYPE_LOFT, TYPE_POLYGON_PRISM, TYPE_SPHERE, TYPE_TEXT, TYPE_TORUS,
};

use crate::{
    animation,
    model::{
        objects, selected, set_objects, set_selected, viewport_mode, ClaydashValue, DataTree,
        EditorState, SdfObject, ViewportMode,
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
        "Extrude face or extend curve",
        "Create an adjacent shape from a face or extend a selected Bézier curve.",
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
        "toggle_outline",
        "Toggle outline mode",
        "Show all object outlines, including hidden Boolean operands, instead of shaded surfaces.",
        "Z",
        toggle_outline,
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
        "spawn-polygon-prism",
        "Add Polygon Prism",
        "Add an extruded polygon. Edit its outline in Object settings.",
        "",
        |tree| spawn(tree, TYPE_POLYGON_PRISM),
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
        "spawn-loft",
        "Spawn Loft",
        "Add a variable-section SDF loft.",
        "",
        |tree| spawn(tree, TYPE_LOFT),
    );
    register(
        commands,
        "spawn-text",
        "Add Text",
        "Add editable extruded text. Edit its content and placement in Object settings.",
        "",
        |tree| spawn(tree, TYPE_TEXT),
    );
    register(
        commands,
        "add-image-plane",
        "Add Image Plane",
        "Create an image plane. Set or replace its texture in Object settings.",
        "",
        |tree| {
            create_image_plane(tree);
        },
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

pub(crate) fn outline_mode(tree: &DataTree) -> bool {
    viewport_mode(tree) == ViewportMode::Outline
}

pub(crate) fn full_material_rendering(tree: &DataTree) -> bool {
    viewport_mode(tree) == ViewportMode::FullMaterial
}

pub(crate) fn toggle_outline(tree: &mut DataTree) {
    let mode = match viewport_mode(tree) {
        ViewportMode::Outline => ViewportMode::SimpleShading,
        ViewportMode::SimpleShading | ViewportMode::FullMaterial => ViewportMode::Outline,
    };
    tree.set_transient_path("editor.viewport_mode", ClaydashValue::ViewportMode(mode));
}

pub(crate) fn toggle_full_material_rendering(tree: &mut DataTree) {
    let mode = match viewport_mode(tree) {
        ViewportMode::FullMaterial => ViewportMode::SimpleShading,
        ViewportMode::SimpleShading | ViewportMode::Outline => ViewportMode::FullMaterial,
    };
    tree.set_transient_path("editor.viewport_mode", ClaydashValue::ViewportMode(mode));
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

fn delete(tree: &mut DataTree) {
    if let Some(point) = crate::model::selected_curve_point(tree) {
        let mut scene = objects(tree);
        if selected(tree) == vec![point.object] {
            if let Some(object) = scene.iter_mut().find(|object| object.uuid == point.object) {
                if let crate::model::SdfParams::BezierCurveParams(curve) = &mut object.params {
                    if let Some(next_index) = curve.delete_anchor(point.index) {
                        set_objects(tree, scene);
                        crate::model::set_selected_curve_point(
                            tree,
                            Some(crate::model::CurvePointSelection {
                                object: point.object,
                                index: next_index,
                            }),
                        );
                        tree.make_undo_redo_snapshot();
                    }
                    return;
                }
            }
        }
    }
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

pub fn close_selected_curve(tree: &mut DataTree) -> bool {
    let Some(point) = crate::model::selected_curve_point(tree) else {
        return false;
    };
    if selected(tree) != vec![point.object] {
        return false;
    }
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == point.object) else {
        return false;
    };
    let crate::model::SdfParams::BezierCurveParams(curve) = &mut object.params else {
        return false;
    };
    if point.index + 1 != curve.points.len() || !curve.close_from_end() {
        return false;
    }
    set_objects(tree, scene);
    crate::model::set_selected_curve_point(
        tree,
        Some(crate::model::CurvePointSelection {
            object: point.object,
            index: 0,
        }),
    );
    tree.make_undo_redo_snapshot();
    true
}

pub fn close_curve_extension(tree: &mut DataTree) -> bool {
    if !matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::ExtendingCurve)
    ) {
        return false;
    }
    let Some(point) = crate::model::selected_curve_point(tree) else {
        return false;
    };
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == point.object) else {
        return false;
    };
    let crate::model::SdfParams::BezierCurveParams(curve) = &mut object.params else {
        return false;
    };
    let closed = if point.index == 0 {
        curve.close_at_current_start()
    } else if point.index + 1 == curve.points.len() {
        curve.close_at_current_tip()
    } else {
        false
    };
    if !closed {
        return false;
    }
    set_objects(tree, scene);
    crate::model::set_selected_curve_point(
        tree,
        Some(crate::model::CurvePointSelection {
            object: point.object,
            index: 0,
        }),
    );
    finish(tree);
    true
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

pub fn duplicate_object(tree: &mut DataTree, id: uuid::Uuid) {
    if objects(tree).iter().any(|object| object.uuid == id) {
        set_selected(tree, vec![id]);
        duplicate(tree);
    }
}

/// Include descendants so editing a group cannot leave dangling parent links.
pub fn create_image_plane(tree: &mut DataTree) -> uuid::Uuid {
    let plane = SdfObject::create_image_plane();
    let id = plane.uuid;
    let mut scene = objects(tree);
    scene.push(plane);
    set_objects(tree, scene);
    set_selected(tree, vec![id]);
    tree.make_undo_redo_snapshot();
    id
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

mod extrusion;
mod selection;
#[cfg(test)]
mod tests;
mod transforms;

pub use extrusion::extrude_selected_face;
pub(crate) use selection::{
    effective_selected_ids, selected_group_id, selected_subtree_ids, set_transform_target,
    transform_targets, TransformTarget, TransformTargetKind,
};
pub(crate) use transforms::finish;
use transforms::{cancel, toggle_constraint};
pub use transforms::{start_grab, start_rotate, start_scale};

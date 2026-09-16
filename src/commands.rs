use command_central::{CommandBuilder, CommandMap};
use glam::Vec4;
use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

use crate::{
    model::{
        objects, selected, set_objects, set_selected, ClaydashValue, DataTree, EditorState,
        SdfObject,
    },
    undo_redo,
};

pub type Commands = CommandMap<ClaydashValue>;

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
        "Start moving selection.",
        "G",
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
    tree.set_path(path, ClaydashValue::Bool(!current));
}

fn cancel(tree: &mut DataTree) {
    let selection = selected(tree);
    let mut scene = objects(tree);
    for object in &mut scene {
        if !selection.contains(&object.uuid) {
            continue;
        }
        if let ClaydashValue::Transform(transform) =
            tree.get_path(&format!("editor.initial_transform.{}", object.uuid))
        {
            object.transform = transform;
        }
    }
    set_objects(tree, scene);
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
}

fn finish(tree: &mut DataTree) {
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
    tree.make_undo_redo_snapshot();
}

fn delete(tree: &mut DataTree) {
    let selection = selected(tree);
    set_objects(
        tree,
        objects(tree)
            .into_iter()
            .filter(|object| !selection.contains(&object.uuid))
            .collect(),
    );
    set_selected(tree, vec![]);
    tree.make_undo_redo_snapshot();
}

fn select_all(tree: &mut DataTree) {
    let scene = objects(tree);
    if selected(tree).len() == scene.len() {
        set_selected(tree, vec![]);
    } else {
        set_selected(tree, scene.into_iter().map(|object| object.uuid).collect());
    }
}

fn duplicate(tree: &mut DataTree) {
    let selection = selected(tree);
    let mut scene = objects(tree);
    let copies: Vec<_> = scene
        .iter()
        .filter(|object| selection.contains(&object.uuid))
        .map(SdfObject::duplicate)
        .collect();
    set_selected(tree, copies.iter().map(|object| object.uuid).collect());
    scene.extend(copies);
    set_objects(tree, scene);
    start_grab(tree);
}

fn spawn(tree: &mut DataTree, kind: i32) {
    let color = match tree.get_path("editor.color") {
        ClaydashValue::Vec4(color) => color,
        _ => Vec4::new(0.8, 0.0, 0.3, 1.0),
    };
    let mut object = SdfObject::create(kind);
    object.color = color;
    let uuid = object.uuid;
    let mut scene = objects(tree);
    scene.push(object);
    set_objects(tree, scene);
    set_selected(tree, vec![uuid]);
    start_grab(tree);
}

use super::*;

pub(super) fn start_edit(tree: &mut DataTree, state: EditorState) {
    if transform_targets(tree).is_empty() {
        return;
    }
    tree.make_undo_redo_snapshot();
    tree.set_path("editor.constrain_x", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_y", ClaydashValue::Bool(false));
    tree.set_path("editor.constrain_z", ClaydashValue::Bool(false));
    tree.set_path("editor.state", ClaydashValue::EditorState(state));
}

pub fn start_grab(tree: &mut DataTree) {
    if crate::model::selected_variable(tree).is_some() {
        start_edit(tree, EditorState::Grabbing);
        return;
    }
    if transform_targets(tree).is_empty() {
        return;
    }
    if matches!(
        tree.get_path("editor.curve_grab_initial"),
        ClaydashValue::VecSDFObject(_)
    ) {
        return;
    }
    if let Some(point) = crate::model::selected_curve_point(tree) {
        let scene = objects(tree);
        if selected(tree) == vec![point.object] {
            if let Some(object) = scene.iter().find(|object| object.uuid == point.object) {
                if let crate::model::SdfParams::BezierCurveParams(curve) = &object.params {
                    if curve.points.get(point.index).is_some() {
                        tree.set_transient_path(
                            "editor.curve_grab_initial",
                            ClaydashValue::VecSDFObject(vec![object.clone()]),
                        );
                        start_edit(tree, EditorState::Grabbing);
                        return;
                    }
                }
            }
        }
    }
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
    if crate::model::selected_variable(tree).is_some() {
        return;
    }
    start_edit(tree, EditorState::Scaling);
}

pub fn start_rotate(tree: &mut DataTree) {
    if crate::model::selected_variable(tree).is_some() {
        return;
    }
    start_edit(tree, EditorState::Rotating);
}

pub(super) fn toggle_constraint(tree: &mut DataTree, path: &str) {
    let current = matches!(tree.get_path(path), ClaydashValue::Bool(true));
    for axis in [
        "editor.constrain_x",
        "editor.constrain_y",
        "editor.constrain_z",
    ] {
        tree.set_path(axis, ClaydashValue::Bool(axis == path && !current));
    }
}

pub(super) fn cancel(tree: &mut DataTree) {
    if let ClaydashValue::VecSDFObject(initial) = tree.get_path("editor.curve_grab_initial") {
        if let Some(initial) = initial.into_iter().next() {
            let mut scene = objects(tree);
            if let Some(object) = scene.iter_mut().find(|object| object.uuid == initial.uuid) {
                *object = initial;
                set_objects(tree, scene);
            }
        }
        tree.set_transient_path("editor.curve_grab_initial", ClaydashValue::None);
        tree.set_path(
            "editor.state",
            ClaydashValue::EditorState(EditorState::Start),
        );
        return;
    }
    if matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::ExtendingCurve)
    ) {
        if let ClaydashValue::VecSDFObject(initial) =
            tree.get_path("editor.curve_extension_initial")
        {
            if let Some(initial) = initial.into_iter().next() {
                let mut scene = objects(tree);
                if let Some(object) = scene.iter_mut().find(|object| object.uuid == initial.uuid) {
                    *object = initial;
                    set_objects(tree, scene);
                }
            }
        }
        let previous = match tree.get_path("editor.curve_extension_source_point") {
            ClaydashValue::CurvePointSelection(point) => Some(point),
            _ => None,
        };
        crate::model::set_selected_curve_point(tree, previous);
        tree.set_transient_path("editor.curve_extension_initial", ClaydashValue::None);
        tree.set_transient_path("editor.curve_extension_source_point", ClaydashValue::None);
        tree.set_path(
            "editor.state",
            ClaydashValue::EditorState(EditorState::Start),
        );
        return;
    }
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
    let mut variables = crate::model::scene_variables(tree);
    let mut scene = objects(tree);
    let mut cameras = crate::model::scene_cameras(tree);
    for target in targets {
        let path = match target.kind {
            TransformTargetKind::Object => "editor.initial_transform",
            TransformTargetKind::Group => "editor.initial_group_transform",
            TransformTargetKind::Camera => "editor.initial_camera_transform",
            TransformTargetKind::Variable => "editor.initial_variable_transform",
        };
        if let ClaydashValue::Transform(transform) = tree.get_path(&format!("{path}.{}", target.id))
        {
            set_transform_target(
                &mut scene,
                &mut cameras,
                &mut variables,
                target.kind,
                target.id,
                transform,
            );
        }
    }
    set_objects(tree, scene);
    if variables != crate::model::scene_variables(tree) {
        crate::model::set_scene_variables(tree, variables);
    }
    crate::model::set_scene_cameras(tree, cameras);
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
}

pub(crate) fn finish(tree: &mut DataTree) {
    tree.set_transient_path("editor.curve_grab_initial", ClaydashValue::None);
    tree.set_transient_path("editor.curve_extension_initial", ClaydashValue::None);
    tree.set_transient_path("editor.curve_extension_source_point", ClaydashValue::None);
    tree.set_transient_path("editor.face_drag_initial_object", ClaydashValue::None);
    tree.set_transient_path("editor.extrusion_object", ClaydashValue::None);
    tree.set_transient_path("editor.extrusion_source_face", ClaydashValue::None);
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
    tree.make_undo_redo_snapshot();
}

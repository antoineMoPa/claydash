use super::*;

pub fn extrude_selected_face(tree: &mut DataTree) {
    if extend_selected_curve(tree) {
        return;
    }
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

pub(super) fn extend_selected_curve(tree: &mut DataTree) -> bool {
    if matches!(
        tree.get_path("editor.state"),
        ClaydashValue::EditorState(EditorState::ExtendingCurve)
    ) {
        return true;
    }
    let selection = selected(tree);
    if selection.len() != 1 {
        return false;
    }
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == selection[0]) else {
        return false;
    };
    let initial = object.clone();
    let selected_point =
        crate::model::selected_curve_point(tree).filter(|point| point.object == object.uuid);
    let crate::model::SdfParams::BezierCurveParams(curve) = &mut object.params else {
        return false;
    };
    let at_start = selected_point.is_some_and(|point| point.index == 0);
    let next = if at_start {
        curve.extend_from_start()
    } else {
        curve.extend_from_end()
    };
    let Some(index) = next else {
        return true;
    };
    tree.set_transient_path(
        "editor.curve_extension_initial",
        ClaydashValue::VecSDFObject(vec![initial]),
    );
    tree.set_transient_path(
        "editor.curve_extension_source_point",
        selected_point.map_or(ClaydashValue::None, ClaydashValue::CurvePointSelection),
    );
    set_objects(tree, scene);
    crate::model::set_selected_curve_point(
        tree,
        Some(crate::model::CurvePointSelection {
            object: selection[0],
            index,
        }),
    );
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::ExtendingCurve),
    );
    true
}

pub(super) fn extrude_cylinder_cap(tree: &mut DataTree, face: crate::model::CylinderCapSelection) {
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

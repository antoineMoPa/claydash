//! Automation policy only: desktop modeling and loaded legacy scenes stay intact.
use super::*;

pub(super) const FLAG_PATH: &str = "scene.slow_smooth_blends_enabled";
pub(super) const GUIDANCE: &str = "Use efficient modeling with independent parts; prefer arrangements of sheet bends. Primitive corner radii and sheet bends/crowns remain available. New Boolean operations, including hard unions, separately require enable_boolean_operations; do not use unions just to organize parts.";
pub(super) const WARNING: &str = "Smooth Boolean blends require dense field sampling, can make rendering and mesh export substantially slower, and may exceed mesh export resource budgets or fail, especially for thin features and large assemblies. Enabling them applies to this scene; prefer arrangements of sheet bends when possible.";

pub(super) fn enabled(tree: &model::DataTree) -> bool {
    matches!(tree.get_path(FLAG_PATH), model::ClaydashValue::Bool(true))
}

pub(super) fn disabled_error() -> String {
    format!("Slow smooth Boolean blends are disabled for this scene. Call enable_slow_smooth_blends (agent operation EnableSlowSmoothBlends) before requesting positive Boolean softness. {GUIDANCE} {WARNING}")
}

/// Compare proposals to the current document, preserving unchanged legacy blends.
/// Positive leaf softness is gated too, so it cannot be staged for later activation.
pub(super) fn validate_transition(
    before: &model::DataTree,
    after: &model::DataTree,
) -> Result<(), String> {
    if enabled(before) {
        return Ok(());
    }
    let original = model::objects_ref(before);
    let proposed = model::objects_ref(after);
    let original_by_id: HashMap<_, _> = original.iter().map(|o| (o.uuid, o)).collect();
    let proposed_by_id: HashMap<_, _> = proposed.iter().map(|o| (o.uuid, o)).collect();
    for object in proposed {
        let previous = original_by_id.get(&object.uuid).copied();
        if object.softness > 0.0 && previous.is_none_or(|old| object.softness > old.softness) {
            return Err(disabled_error());
        }
        // Attaching an operand activates the parent's softness, including when the
        // parent was a legacy leaf with a dormant nonzero default.
        if let Some(parent_id) = object.boolean_parent {
            if proposed_by_id
                .get(&parent_id)
                .is_some_and(|p| p.softness > 0.0)
                && previous.is_none_or(|old| old.boolean_parent != Some(parent_id))
            {
                return Err(disabled_error());
            }
        }
    }
    validate_animation_transition(before, after)
}

fn animation(tree: &model::DataTree) -> Option<AnimationData> {
    match tree.get_path("scene.animation") {
        model::ClaydashValue::Animation(animation) => Some(animation),
        _ => None,
    }
}

fn validate_animation_transition(
    before: &model::DataTree,
    after: &model::DataTree,
) -> Result<(), String> {
    let old = animation(before);
    let Some(new) = animation(after) else {
        return Ok(());
    };
    for track in &new.tracks {
        if track.binding.property != model::AnimatableProperty::OperandSoftness {
            continue;
        }
        let previous = old
            .as_ref()
            .and_then(|a| a.tracks.iter().find(|t| t.binding == track.binding));
        for keyframe in &track.keyframes {
            // Positive Bezier control values can introduce a blend between zero keys.
            let values = [
                keyframe.value,
                keyframe.value + keyframe.incoming_handle.map_or(0.0, |h| h.value_offset),
                keyframe.value + keyframe.outgoing_handle.map_or(0.0, |h| h.value_offset),
            ];
            let previous_key =
                previous.and_then(|t| t.keyframes.iter().find(|k| k.frame == keyframe.frame));
            let old_values = previous_key.map(|k| {
                [
                    k.value,
                    k.value + k.incoming_handle.map_or(0.0, |h| h.value_offset),
                    k.value + k.outgoing_handle.map_or(0.0, |h| h.value_offset),
                ]
            });
            if values.iter().enumerate().any(|(index, value)| {
                *value > 0.0 && old_values.is_none_or(|old| *value > old[index])
            }) {
                return Err(disabled_error());
            }
        }
    }
    Ok(())
}

impl App {
    pub(super) fn agent_enable_slow_smooth_blends(&mut self) -> AgentResult {
        if !enabled(&self.tree) {
            self.tree
                .set_path(FLAG_PATH, model::ClaydashValue::Bool(true));
            self.tree.make_undo_redo_snapshot();
        }
        Ok(json!({"enabled": true, "warning": WARNING, "revision": self.scene_revision()}))
    }

    pub(super) fn agent_execute_command(&mut self, name: &str) -> AgentResult {
        if !self.commands.commands.contains_key(name) {
            return Err(format!("unknown command: {name}"));
        }
        if name == "union" && !boolean_policy::enabled(&self.tree) {
            return Err(boolean_policy::disabled_error());
        }
        let mut draft = self.tree.clone();
        if let Some(kind) = spawn_kind(name) {
            // start_grab snapshots, so automation defaults must be applied first.
            let mut object = SdfObject::create_kind(kind);
            object.softness = 0.0;
            object.material = model::picked_material(&draft);
            object.material_id = model::picked_material_id(&draft);
            object.color = object.material.color;
            let id = object.uuid;
            let mut objects = model::objects(&draft);
            objects.push(object);
            model::set_objects(&mut draft, objects);
            model::set_selected(&mut draft, vec![id]);
            draft.set_path("editor.spawn_at_cursor", model::ClaydashValue::Uuid(id));
            commands::start_grab(&mut draft);
        } else if name == "add-image-plane" {
            // Its desktop callback takes a snapshot immediately. Create the zero
            // softness automation object before any snapshot is made.
            let mut plane = SdfObject::create_image_plane();
            plane.softness = 0.0;
            let id = plane.uuid;
            let mut objects = model::objects(&draft);
            objects.push(plane);
            model::set_objects(&mut draft, objects);
            model::set_selected(&mut draft, vec![id]);
        } else {
            commands::execute(&self.commands, name, &mut draft);
        }
        // Undo/redo restore both geometry and its permission from scene snapshots.
        if name != "undo" && name != "redo" {
            validate_transition(&self.tree, &draft)?;
            boolean_policy::validate_transition(&self.tree, &draft)?;
            draft.make_undo_redo_snapshot();
        }
        self.tree = draft;
        Ok(json!({"revision": self.scene_revision()}))
    }
}

// Explicit automation constructors; the desktop command defaults stay intact.
fn spawn_kind(name: &str) -> Option<PrimitiveKind> {
    match name {
        "spawn-sphere" => Some(PrimitiveKind::Sphere),
        "spawn-box" => Some(PrimitiveKind::Box),
        "spawn-cylinder" => Some(PrimitiveKind::Cylinder),
        "spawn-polygon-prism" => Some(PrimitiveKind::PolygonPrism),
        "spawn-torus" => Some(PrimitiveKind::Torus),
        "spawn-loft" => Some(PrimitiveKind::Loft),
        "spawn-text" => Some(PrimitiveKind::Text),
        _ => None,
    }
}

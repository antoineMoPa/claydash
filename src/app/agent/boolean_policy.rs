//! Automation opt-in for adding Boolean geometry; imported operations stay editable.
use super::*;

pub(super) const FLAG_PATH: &str = "scene.boolean_operations_enabled";
pub(super) const GUIDANCE: &str = "Model with independent parts, native sheet cuts/bends/crowns and primitive corner radii by default. Use shared vector or rigid transform bindings to move parts together without Boolean geometry. Do not create unions just to organize objects.";
pub(super) const WARNING: &str = "Boolean unions, subtraction and intersection add geometry evaluation work and can slow rendering and mesh export, especially in nested assemblies. Only enable Boolean operations when the user explicitly requests them. Smooth Boolean blends additionally require enable_slow_smooth_blends.";

pub(super) fn enabled(tree: &model::DataTree) -> bool {
    matches!(tree.get_path(FLAG_PATH), model::ClaydashValue::Bool(true))
}

pub(super) fn disabled_error() -> String {
    format!("Adding Boolean operations is disabled for this scene. Call enable_boolean_operations (agent operation EnableBooleanOperations) only when the user explicitly requests Boolean geometry. {GUIDANCE} {WARNING}")
}

pub(super) fn validate_transition(
    before: &model::DataTree,
    after: &model::DataTree,
) -> Result<(), String> {
    if enabled(before) {
        return Ok(());
    }
    let original: HashMap<_, _> = model::objects_ref(before)
        .iter()
        .map(|object| (object.uuid, object))
        .collect();
    for object in model::objects_ref(after) {
        let previous = original.get(&object.uuid).copied();
        let new_edge = object.boolean_parent.is_some()
            && previous.is_none_or(|old| {
                old.boolean_parent != object.boolean_parent || old.operation != object.operation
            });
        // A detached operand can always be removed. Prevent staging a new
        // subtraction/intersection role on an otherwise independent object.
        let new_role = object.operation != BooleanOperation::Union
            && previous.is_none_or(|old| old.operation != object.operation);
        if new_edge || new_role {
            return Err(disabled_error());
        }
    }
    Ok(())
}

impl App {
    pub(super) fn agent_enable_boolean_operations(&mut self) -> AgentResult {
        if !enabled(&self.tree) {
            self.tree
                .set_path(FLAG_PATH, model::ClaydashValue::Bool(true));
            self.tree.make_undo_redo_snapshot();
        }
        Ok(json!({"enabled":true,"warning":WARNING,"revision":self.scene_revision()}))
    }
}

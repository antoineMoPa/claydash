//! MCP adapters for the shared scene language. Parsing and expansion live in the engine.
use crate::model::SdfObject;
use claydash_engine::scene_language::compile;
use glam::Vec3;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

fn digest_scene(tree: &crate::model::DataTree) -> Result<String, String> {
    let mut snapshot = tree.clone();
    for path in [
        "scene.dsl_recipe",
        "scene.selected_uuids",
        "scene.selection_scope",
        "scene.cursor_position",
    ] {
        snapshot.set_path(path, crate::model::ClaydashValue::None);
    }
    let bytes = crate::document::serialize_scene(&snapshot)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

impl super::App {
    fn dsl_draft(
        &self,
        source: &str,
    ) -> Result<(crate::model::DataTree, Vec<(String, uuid::Uuid)>), String> {
        use crate::model::ClaydashValue;
        let parts = compile(source)?;
        let paths = parts
            .iter()
            .map(|(path, object)| (path.clone(), object.uuid))
            .collect();
        let mut draft = self.tree.clone();
        draft.make_undo_redo_snapshot();
        draft.clear_subtree_values("scene");
        draft.set_path(
            "scene.sdf_objects",
            ClaydashValue::VecSDFObject(parts.into_iter().map(|(_, object)| object).collect()),
        );
        draft.set_path("scene.dsl_recipe", ClaydashValue::None);
        draft.set_path(
            super::boolean_policy::FLAG_PATH,
            ClaydashValue::Bool(super::boolean_policy::enabled(&self.tree)),
        );
        draft.set_path(
            super::smooth_blend_policy::FLAG_PATH,
            ClaydashValue::Bool(super::smooth_blend_policy::enabled(&self.tree)),
        );
        super::validate_scene(&draft)?;
        super::boolean_policy::validate_transition(&self.tree, &draft)?;
        super::smooth_blend_policy::validate_transition(&self.tree, &draft)?;
        Ok((draft, paths))
    }
    pub(super) fn agent_validate_scene_dsl(&self, source: &str) -> super::AgentResult {
        let (_, paths) = self.dsl_draft(source)?;
        Ok(
            serde_json::json!({"valid":true,"objects":paths.iter().map(|(path,id)|serde_json::json!({"path":path,"id":id})).collect::<Vec<_>>(),"revision":self.scene_revision()}),
        )
    }
    pub(super) fn agent_apply_scene_dsl(
        &mut self,
        source: String,
        expected_revision: Option<u64>,
    ) -> super::AgentResult {
        if expected_revision.is_some_and(|expected| expected != self.scene_revision()) {
            return Err(format!(
                "scene changed; current revision is {}",
                self.scene_revision()
            ));
        }
        let (mut draft, paths) = self.dsl_draft(&source)?;
        let digest = digest_scene(&draft)?;
        draft.set_path(
            "scene.dsl_recipe",
            crate::model::ClaydashValue::SceneDslRecipe(crate::model::SceneDslRecipe {
                source,
                scene_digest: digest,
            }),
        );
        crate::model::set_selected_exact(&mut draft, Vec::new());
        draft.make_undo_redo_snapshot();
        self.cancel_document_work();
        self.tree = draft;
        self.interactions = Default::default();
        self.ui.reset_document_gestures();
        self.clear_document_egui_state();
        self.ui.reset_animation(&self.tree);
        Ok(
            serde_json::json!({"revision":self.scene_revision(),"objects":paths.iter().map(|(path,id)|serde_json::json!({"path":path,"id":id})).collect::<Vec<_>>()}),
        )
    }
    pub(super) fn agent_get_scene_dsl(&self) -> super::AgentResult {
        match self.tree.get_path("scene.dsl_recipe") {
            crate::model::ClaydashValue::SceneDslRecipe(recipe) => Ok(
                serde_json::json!({"source":recipe.source,"in_sync":digest_scene(&self.tree)?==recipe.scene_digest,"revision":self.scene_revision()}),
            ),
            _ => Ok(
                serde_json::json!({"source":null,"in_sync":null,"revision":self.scene_revision()}),
            ),
        }
    }
}

impl super::App {
    pub(super) fn prepare_dsl_capture(
        &self,
        path: Option<&str>,
        panel_size: Option<u32>,
    ) -> Result<(Vec<SdfObject>, Vec3, f32, u32), String> {
        let panel_size = panel_size.unwrap_or(256);
        if !(96..=super::capture::MAX_CAPTURE_SIZE).contains(&panel_size) {
            return Err("panel_size must be 96–2048 pixels".into());
        }
        let objects = crate::model::objects(&self.tree);
        let selected = if let Some(path) = path {
            let crate::model::ClaydashValue::SceneDslRecipe(recipe) =
                self.tree.get_path("scene.dsl_recipe")
            else {
                return Err("no DSL recipe is stored".into());
            };
            let prefix = format!("{}/", path.trim_end_matches('/'));
            let ids: HashSet<_> = compile(&recipe.source)?
                .into_iter()
                .filter(|(p, _)| p == path || p.starts_with(&prefix))
                .map(|(_, o)| o.uuid)
                .collect();
            if ids.is_empty() {
                return Err(format!("no objects match path '{path}'"));
            }
            objects
                .into_iter()
                .filter(|o| ids.contains(&o.uuid))
                .collect::<Vec<_>>()
        } else {
            objects
        };
        if selected.is_empty() {
            return Err("no objects to capture".into());
        }
        let (min, max) = claydash_engine::scene_language::bounds(&selected)?;
        let span = (max - min).length().max(0.1) * 1.15;
        if span / 0.84 + (max - min).length() * 0.5 >= 100. {
            return Err("scene exceeds capture camera clipping range; scale it down".into());
        }
        Ok((selected, (min + max) * 0.5, span, panel_size))
    }
}

#[cfg(test)]
mod tests {
    const TABLE: &str = r##"
object table({ width: number, height: number }):
 top = box(size: [width, 0.15, 1], at: [0, height, 0], color: "#a66b42");
 leg = box(size: [0.12, height, 0.12], color: "#70452e");
 place(leg, at: [-width / 2 + 0.1, height / 2, -0.4]);
 place(leg, at: [ width / 2 - 0.1, height / 2, -0.4]);
 place(leg, at: [-width / 2 + 0.1, height / 2,  0.4]);
 place(leg, at: [ width / 2 - 0.1, height / 2,  0.4]);
end
place(table(width: 2, height: 1), at: [5, 0, 0]);
"##;
    #[test]
    fn apply_is_atomic_undoable_and_persists_recipe() {
        let mut app = super::super::App::new();
        app.new_document();
        let original = crate::model::objects(&app.tree);
        let revision = app.scene_revision();
        assert!(app
            .agent_validate_scene_dsl("a = box(size: [0,1,1]);")
            .is_err());
        assert_eq!(app.scene_revision(), revision);
        app.agent_apply_scene_dsl(TABLE.into(), Some(revision))
            .unwrap();
        assert_eq!(crate::model::objects(&app.tree).len(), 5);
        assert_eq!(app.agent_get_scene_dsl().unwrap()["in_sync"], true);
        let selected_id = crate::model::objects(&app.tree)[0].uuid;
        crate::model::set_selected_exact(&mut app.tree, vec![selected_id]);
        assert_eq!(app.agent_get_scene_dsl().unwrap()["in_sync"], true);
        let bytes = crate::document::serialize_scene(&app.tree).unwrap();
        let restored = crate::document::deserialize_scene(&bytes).unwrap();
        let mut reopened = super::super::App::new();
        reopened.replace_scene(restored);
        assert_eq!(reopened.agent_get_scene_dsl().unwrap()["in_sync"], true);
        let mut changed = crate::model::objects(&app.tree);
        changed[0].transform.translation.x += 1.;
        crate::model::set_objects(&mut app.tree, changed);
        assert_eq!(app.agent_get_scene_dsl().unwrap()["in_sync"], false);
        app.tree.undo();
        app.tree.undo();
        assert_eq!(crate::model::objects(&app.tree).len(), original.len());
        app.tree.redo();
        assert_eq!(crate::model::objects(&app.tree).len(), 5);
        assert!(app
            .agent_apply_scene_dsl(TABLE.into(), Some(revision))
            .is_err());
    }
}

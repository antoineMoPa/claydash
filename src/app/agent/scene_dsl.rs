//! MCP adapters for the shared scene language. Parsing and expansion live in the engine.
use crate::model::SdfObject;
use claydash_engine::scene_language::compile_with_assets;
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
        base_dir: Option<&std::path::Path>,
    ) -> Result<
        (
            crate::model::DataTree,
            Vec<(String, uuid::Uuid)>,
            Vec<(String, uuid::Uuid)>,
        ),
        String,
    > {
        use crate::model::ClaydashValue;
        let scene = compile_with_assets(source, &mut |path| {
            let path = std::path::Path::new(path);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                base_dir
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(path)
            };
            crate::mesh_assets::load_glb(&path)
        })?;
        let parts = scene.parts;
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
        draft.set_path("scene.seams", ClaydashValue::Seams(scene.seams));
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
        Ok((draft, paths, scene.seam_paths))
    }
    pub(super) fn agent_validate_scene_dsl(
        &self,
        source: &str,
        base_dir: Option<&std::path::Path>,
    ) -> super::AgentResult {
        let (_, paths, seams) = self.dsl_draft(source, base_dir)?;
        Ok(
            serde_json::json!({"valid":true,"objects":paths.iter().map(|(path,id)|serde_json::json!({"path":path,"id":id})).collect::<Vec<_>>(),"seams":seams.iter().map(|(path,id)|serde_json::json!({"path":path,"id":id})).collect::<Vec<_>>(),"revision":self.scene_revision()}),
        )
    }
    pub(super) fn agent_apply_scene_dsl(
        &mut self,
        source: String,
        expected_revision: Option<u64>,
        base_dir: Option<std::path::PathBuf>,
    ) -> super::AgentResult {
        if expected_revision.is_some_and(|expected| expected != self.scene_revision()) {
            return Err(format!(
                "scene changed; current revision is {}",
                self.scene_revision()
            ));
        }
        let (mut draft, paths, seams) = self.dsl_draft(&source, base_dir.as_deref())?;
        let digest = digest_scene(&draft)?;
        draft.set_path(
            "scene.dsl_recipe",
            crate::model::ClaydashValue::SceneDslRecipe(crate::model::SceneDslRecipe {
                source,
                scene_digest: digest,
                base_dir,
                part_paths: paths.clone(),
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
            serde_json::json!({"revision":self.scene_revision(),"objects":paths.iter().map(|(path,id)|serde_json::json!({"path":path,"id":id})).collect::<Vec<_>>(),"seams":seams.iter().map(|(path,id)|serde_json::json!({"path":path,"id":id})).collect::<Vec<_>>()}),
        )
    }
    pub(super) fn agent_get_scene_dsl(&self) -> super::AgentResult {
        match self.tree.get_path("scene.dsl_recipe") {
            crate::model::ClaydashValue::SceneDslRecipe(recipe) => Ok(
                serde_json::json!({"source":recipe.source,"base_dir":recipe.base_dir,"parts":recipe.part_paths,"in_sync":digest_scene(&self.tree)?==recipe.scene_digest,"revision":self.scene_revision()}),
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
            let paths = if !recipe.part_paths.is_empty() {
                recipe.part_paths
            } else {
                compile_with_assets(&recipe.source, &mut |asset| {
                    let asset = std::path::Path::new(asset);
                    crate::mesh_assets::load_glb(&if asset.is_absolute() {
                        asset.to_path_buf()
                    } else {
                        recipe
                            .base_dir
                            .as_deref()
                            .unwrap_or_else(|| std::path::Path::new("."))
                            .join(asset)
                    })
                })?
                .parts
                .into_iter()
                .map(|(path, o)| (path, o.uuid))
                .collect()
            };
            let ids: HashSet<_> = paths
                .into_iter()
                .filter(|(p, _)| p == path || p.starts_with(&prefix))
                .map(|(_, id)| id)
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
            .agent_validate_scene_dsl("a = box(size: [0,1,1]);", None)
            .is_err());
        assert_eq!(app.scene_revision(), revision);
        app.agent_apply_scene_dsl(TABLE.into(), Some(revision), None)
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
            .agent_apply_scene_dsl(TABLE.into(), Some(revision), None)
            .is_err());
    }
    #[test]
    fn asset_recipe_roundtrips_and_named_capture_survives_missing_source() {
        let dir = std::env::temp_dir().join(format!("claydash-dsl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("reference.glb"),
            crate::app::file_action_tests::triangle_glb(),
        )
        .unwrap();
        let source = r#"reference=mesh_asset(path:"reference.glb"); panel=sheet_panel(size:[2,1],lattice:lattice()); edge=seam(points:[[-1,-0.5,0],[-1,0.5,0]]); bind(object:"panel",seam:"edge",target:sheet_edge("MinX"));"#;
        let mut app = super::super::App::new();
        app.new_document();
        let a = app
            .agent_apply_scene_dsl(source.into(), None, Some(dir.clone()))
            .unwrap();
        let b = app
            .agent_apply_scene_dsl(source.into(), None, Some(dir.clone()))
            .unwrap();
        assert_eq!(a["objects"], b["objects"]);
        assert_eq!(a["seams"], b["seams"]);
        assert!(a.get("document").is_none());
        assert_eq!(crate::model::scene_seams(&app.tree).bindings.len(), 1);
        assert_eq!(app.agent_get_scene_dsl().unwrap()["in_sync"], true);
        let bytes = crate::document::serialize_scene(&app.tree).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let mut reopened = super::super::App::new();
        reopened.replace_scene(crate::document::deserialize_scene(&bytes).unwrap());
        assert_eq!(reopened.agent_get_scene_dsl().unwrap()["in_sync"], true);
        let (parts, _, _, _) = reopened
            .prepare_dsl_capture(Some("reference"), Some(128))
            .unwrap();
        assert_eq!(parts.len(), 1);
        assert!(parts[0].imported_mesh.is_some());
        let before = reopened.scene_revision();
        assert!(reopened
            .agent_apply_scene_dsl(source.into(), None, Some(dir))
            .is_err());
        assert_eq!(reopened.scene_revision(), before);
        assert_eq!(crate::model::scene_seams(&reopened.tree).bindings.len(), 1);
    }
    #[test]
    fn detailed_sheet_recipe_is_atomic_and_persists_native_operations() {
        let source = include_str!("../../../examples/sheet-relief.claydef");
        let mut app = super::super::App::new();
        app.new_document();
        app.agent_apply_scene_dsl(source.into(), None, None)
            .unwrap();
        let original = serde_json::to_value(crate::model::objects(&app.tree)).unwrap();
        let revision = app.scene_revision();
        assert!(app
            .agent_apply_scene_dsl(
                source.replace("transition: 0.12", "transition: 0"),
                None,
                None
            )
            .is_err());
        assert_eq!(app.scene_revision(), revision);
        let bytes = crate::document::serialize_scene(&app.tree).unwrap();
        app.replace_scene(crate::document::deserialize_scene(&bytes).unwrap());
        assert_eq!(
            serde_json::to_value(crate::model::objects(&app.tree)).unwrap(),
            original
        );
        assert_eq!(app.agent_get_scene_dsl().unwrap()["in_sync"], true);
    }
    #[test]
    fn saved_source_bindings_keep_exact_controls_on_reopen() {
        let source = r#"panel=sheet_panel(size:[2,1],lattice:lattice(min:[-1,-0.5,-0.1],max:[1,0.5,0.1])); join=seam(points:[[-1,-0.5,0],[-1,0,0.04],[-1,0.5,0]]); bind(object:"panel",seam:"join",target:sheet_edge("MinX"));"#;
        let mut app = super::super::App::new();
        app.new_document();
        app.agent_apply_scene_dsl(source.into(), None, None)
            .unwrap();
        let original = serde_json::to_value(crate::model::objects(&app.tree)).unwrap();
        for _ in 0..3 {
            let bytes = crate::document::serialize_scene(&app.tree).unwrap();
            app.replace_scene(crate::document::deserialize_scene(&bytes).unwrap());
            assert_eq!(
                serde_json::to_value(crate::model::objects(&app.tree)).unwrap(),
                original
            );
            assert_eq!(app.agent_get_scene_dsl().unwrap()["in_sync"], true);
        }
    }
}

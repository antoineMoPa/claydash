use super::*;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaussianColorOperation {
    Desaturate,
    Saturation,
    Contrast,
    Brightness,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaussianColorScope {
    #[default]
    Selected,
    All,
}

#[derive(Deserialize)]
pub struct AdjustGaussianColorsArgs {
    pub cloud_object_id: uuid::Uuid,
    pub operation: GaussianColorOperation,
    pub amount: f32,
    #[serde(default)]
    pub scope: GaussianColorScope,
    pub expected_revision: Option<u64>,
}

fn adjusted_color(color: [f32; 3], operation: GaussianColorOperation, amount: f32) -> [f32; 3] {
    let luminance = color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722;
    color.map(|channel| {
        match operation {
            GaussianColorOperation::Desaturate => channel + (luminance - channel) * amount,
            GaussianColorOperation::Saturation => luminance + (channel - luminance) * amount,
            GaussianColorOperation::Contrast => 0.5 + (channel - 0.5) * amount,
            GaussianColorOperation::Brightness => channel + amount,
        }
        .max(0.0)
    })
}

impl App {
    pub(super) fn agent_adjust_gaussian_colors(
        &mut self,
        args: AdjustGaussianColorsArgs,
    ) -> AgentResult {
        if args
            .expected_revision
            .is_some_and(|revision| revision != self.scene_revision())
        {
            return Err(format!(
                "scene changed; current revision is {}",
                self.scene_revision()
            ));
        }
        let valid = args.amount.is_finite()
            && match args.operation {
                GaussianColorOperation::Desaturate => (0.0..=1.0).contains(&args.amount),
                GaussianColorOperation::Saturation | GaussianColorOperation::Contrast => {
                    (0.0..=4.0).contains(&args.amount)
                }
                GaussianColorOperation::Brightness => (-1.0..=1.0).contains(&args.amount),
            };
        if !valid {
            return Err(
                "amount must be finite: desaturate 0–1, saturation/contrast 0–4, brightness -1–1"
                    .into(),
            );
        }
        let mut scene = model::objects(&self.tree);
        let object = scene
            .iter_mut()
            .find(|object| object.uuid == args.cloud_object_id)
            .ok_or_else(|| format!("object {} not found", args.cloud_object_id))?;
        let cloud = object.imported_splats.as_mut().ok_or_else(|| {
            format!(
                "object {} has no imported Gaussian cloud",
                args.cloud_object_id
            )
        })?;
        let mut indices = match args.scope {
            GaussianColorScope::Selected => {
                let edit = model::cloud_edit(&self.tree)
                    .filter(|edit| edit.object == args.cloud_object_id)
                    .ok_or("select splats in this Gaussian cloud first, or use scope all")?;
                if edit.indices.is_empty() {
                    return Err("select at least one splat, or use scope all".into());
                }
                edit.indices
            }
            GaussianColorScope::All => (0..cloud.splats.len()).collect(),
        };
        indices.sort_unstable();
        indices.dedup();
        if indices.iter().any(|&index| index >= cloud.splats.len()) {
            return Err("selected splat index is out of range".into());
        }
        // Calculate every result before mutating the scene so an overflow rejects the entire edit.
        let changed: Result<Vec<_>, String> = indices
            .iter()
            .map(|&index| {
                let color = adjusted_color(cloud.splats[index].color, args.operation, args.amount);
                color
                    .iter()
                    .all(|channel| channel.is_finite())
                    .then_some((index, color))
                    .ok_or_else(|| format!("color adjustment overflowed for splat {index}"))
            })
            .collect();
        let changed: Vec<_> = changed?
            .into_iter()
            .filter(|(index, color)| cloud.splats[*index].color != *color)
            .collect();
        if changed.is_empty() {
            return Ok(json!({"id": args.cloud_object_id, "changed_count": 0,
                "revision": self.scene_revision()}));
        }
        let cloud = Arc::make_mut(cloud);
        for (index, color) in &changed {
            cloud.splats[*index].color = *color;
        }
        model::set_objects(&mut self.tree, scene);
        self.tree.make_undo_redo_snapshot();
        Ok(
            json!({"id": args.cloud_object_id, "changed_count": changed.len(),
            "revision": self.scene_revision()}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claydash_engine::gaussian_ply::{Gaussian, GaussianCloud};

    fn setup() -> (App, uuid::Uuid) {
        let mut app = App::new();
        app.new_document();
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.imported_splats = Some(Arc::new(GaussianCloud {
            splats: vec![
                Gaussian {
                    position: Vec3::ZERO,
                    scale: Vec3::ONE,
                    rotation: glam::Quat::IDENTITY,
                    color: [1.0, 0.2, 0.0],
                    opacity: 0.8,
                    sh_rest: vec![0.1; 9],
                },
                Gaussian {
                    position: Vec3::X,
                    scale: Vec3::ONE,
                    rotation: glam::Quat::IDENTITY,
                    color: [0.1, 0.5, 1.0],
                    opacity: 0.4,
                    sh_rest: vec![0.2; 9],
                },
            ],
        }));
        let id = object.uuid;
        model::set_objects(&mut app.tree, vec![object]);
        app.tree.make_undo_redo_snapshot();
        model::set_selected_exact(&mut app.tree, vec![id]);
        model::set_cloud_edit(
            &mut app.tree,
            Some(model::CloudEdit {
                object: id,
                indices: vec![0],
            }),
        );
        (app, id)
    }

    fn colors(app: &App) -> Vec<[f32; 3]> {
        model::objects_ref(&app.tree)[0]
            .imported_splats
            .as_ref()
            .unwrap()
            .splats
            .iter()
            .map(|splat| splat.color)
            .collect()
    }

    #[test]
    fn selected_desaturation_preserves_neighbors_sh_and_undo() {
        let (mut app, id) = setup();
        let original = colors(&app);
        let revision = app.scene_revision();
        let result = app
            .agent_adjust_gaussian_colors(AdjustGaussianColorsArgs {
                cloud_object_id: id,
                operation: GaussianColorOperation::Desaturate,
                amount: 1.0,
                scope: GaussianColorScope::Selected,
                expected_revision: Some(revision),
            })
            .unwrap();
        assert_eq!(result["changed_count"], 1);
        let expected = 0.2126 + 0.2 * 0.7152;
        let edited = colors(&app);
        for channel in edited[0] {
            assert!((channel - expected).abs() < 0.00001);
        }
        assert_eq!(edited[1], original[1]);
        let splat = &model::objects_ref(&app.tree)[0]
            .imported_splats
            .as_ref()
            .unwrap()
            .splats[0];
        assert_eq!(splat.opacity, 0.8);
        assert_eq!(splat.sh_rest, vec![0.1; 9]);
        assert_ne!(app.scene_revision(), revision);
        app.tree.undo();
        assert_eq!(colors(&app), original);
        app.tree.redo();
        assert_eq!(colors(&app), edited);
        let saved = crate::document::serialize_scene(&app.tree).unwrap();
        let mut restored = model::DataTree::default();
        restored.set_tree("scene", crate::document::deserialize_scene(&saved).unwrap());
        assert_eq!(
            model::objects_ref(&restored)[0]
                .imported_splats
                .as_ref()
                .unwrap()
                .splats[0]
                .color,
            edited[0]
        );
    }

    #[test]
    fn all_scope_and_invalid_requests_are_atomic() {
        let (mut app, id) = setup();
        let original = colors(&app);
        let revision = app.scene_revision();
        for amount in [f32::NAN, 5.0] {
            assert!(app
                .agent_adjust_gaussian_colors(AdjustGaussianColorsArgs {
                    cloud_object_id: id,
                    operation: GaussianColorOperation::Contrast,
                    amount,
                    scope: GaussianColorScope::All,
                    expected_revision: None,
                })
                .is_err());
        }
        model::set_cloud_edit(&mut app.tree, None);
        assert!(app
            .agent_adjust_gaussian_colors(AdjustGaussianColorsArgs {
                cloud_object_id: id,
                operation: GaussianColorOperation::Brightness,
                amount: 0.1,
                scope: GaussianColorScope::Selected,
                expected_revision: None,
            })
            .is_err());
        assert_eq!(colors(&app), original);
        assert_eq!(app.scene_revision(), revision);
        let result = app
            .agent_adjust_gaussian_colors(AdjustGaussianColorsArgs {
                cloud_object_id: id,
                operation: GaussianColorOperation::Contrast,
                amount: 2.0,
                scope: GaussianColorScope::All,
                expected_revision: Some(revision),
            })
            .unwrap();
        assert_eq!(result["changed_count"], 2);
        assert_eq!(colors(&app)[0], [1.5, 0.0, 0.0]);
        assert!(app
            .agent_adjust_gaussian_colors(AdjustGaussianColorsArgs {
                cloud_object_id: id,
                operation: GaussianColorOperation::Brightness,
                amount: 0.2,
                scope: GaussianColorScope::All,
                expected_revision: Some(revision),
            })
            .is_err());
        assert_eq!(colors(&app)[0], [1.5, 0.0, 0.0]);
        assert!(mcp::mcp_tools()
            .iter()
            .any(|tool| tool["name"] == "adjust_gaussian_colors"));
    }

    #[test]
    fn mcp_payload_uses_selected_scope_by_default() {
        let (mut app, id) = setup();
        let request: Request = serde_json::from_value(json!({
            "op": "AdjustGaussianColors",
            "args": {"cloud_object_id": id, "operation": "brightness", "amount": 0.1}
        }))
        .unwrap();
        let Request::AdjustGaussianColors(args) = request else {
            panic!("wrong request type")
        };
        app.agent_adjust_gaussian_colors(args).unwrap();
        assert_eq!(colors(&app)[0], [1.1, 0.3, 0.1]);
        assert_eq!(colors(&app)[1], [0.1, 0.5, 1.0]);
    }
}

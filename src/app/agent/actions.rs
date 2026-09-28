use super::validation::params_match;
use super::*;

impl App {
    pub(super) fn agent_apply(&mut self, args: ApplyArgs) -> AgentResult {
        if args
            .expected_revision
            .is_some_and(|expected| expected != self.scene_revision())
        {
            return Err(format!(
                "scene changed; current revision is {}",
                self.scene_revision()
            ));
        }
        if args.actions.is_empty() {
            return Err("actions must not be empty".to_string());
        }
        if args.actions.len() != 1
            && args
                .actions
                .iter()
                .any(|action| matches!(action, Action::ReplaceScene { .. }))
        {
            return Err("ReplaceScene must be the only action".to_string());
        }
        let mut draft = self.tree.clone();
        let mut created = Vec::new();
        let mut created_materials = Vec::new();
        let mut created_post_process_passes = Vec::new();
        let mut replace = false;
        for action in args.actions {
            match action {
                Action::CreateObject {
                    kind,
                    name,
                    transform,
                    position,
                    params,
                    render_representation,
                } => {
                    let mut object = SdfObject::create_kind(kind);
                    object.material = model::picked_material(&draft);
                    object.material_id = model::picked_material_id(&draft);
                    object.color = object.material.color;
                    if let Some(name) = name {
                        object.name = name;
                    }
                    if let Some(transform) = transform {
                        object.transform = transform;
                    }
                    if let Some(position) = position {
                        object.transform.translation = Vec3::from_array(position);
                    }
                    if let Some(params) = params {
                        if !params_match(kind, &params) {
                            return Err("params do not match primitive kind".to_string());
                        }
                        object.params = params;
                    }
                    if let Some(render_representation) = render_representation {
                        object.render_representation = render_representation;
                    }
                    created.push(object.uuid);
                    let mut objects = model::objects(&draft);
                    objects.push(object);
                    model::set_objects(&mut draft, objects);
                }
                Action::PutObject { object } => {
                    let mut objects = model::objects(&draft);
                    let Some(existing) = objects
                        .iter_mut()
                        .find(|existing| existing.uuid == object.uuid)
                    else {
                        return Err(format!("object {} does not exist", object.uuid));
                    };
                    *existing = object;
                    model::set_objects(&mut draft, objects);
                }
                Action::SetObjectName { id, name } => {
                    let mut objects = model::objects(&draft);
                    let object = objects
                        .iter_mut()
                        .find(|object| object.uuid == id)
                        .ok_or_else(|| format!("object {id} does not exist"))?;
                    object.name = name;
                    model::set_objects(&mut draft, objects);
                }
                Action::SetObjectTransform { id, transform } => {
                    let mut objects = model::objects(&draft);
                    let object = objects
                        .iter_mut()
                        .find(|object| object.uuid == id)
                        .ok_or_else(|| format!("object {id} does not exist"))?;
                    object.transform = transform;
                    model::set_objects(&mut draft, objects);
                }
                Action::SetObjectParams { id, params } => {
                    let mut objects = model::objects(&draft);
                    let object = objects
                        .iter_mut()
                        .find(|object| object.uuid == id)
                        .ok_or_else(|| format!("object {id} does not exist"))?;
                    object.params = params;
                    model::set_objects(&mut draft, objects);
                }
                Action::SetRenderRepresentation {
                    id,
                    render_representation,
                } => {
                    let mut objects = model::objects(&draft);
                    let object = objects
                        .iter_mut()
                        .find(|object| object.uuid == id)
                        .ok_or_else(|| format!("object {id} does not exist"))?;
                    object.render_representation = render_representation;
                    model::set_objects(&mut draft, objects);
                }
                Action::SetBoolean {
                    id,
                    parent,
                    operation,
                    softness,
                } => {
                    let mut objects = model::objects(&draft);
                    let object = objects
                        .iter_mut()
                        .find(|object| object.uuid == id)
                        .ok_or_else(|| format!("object {id} does not exist"))?;
                    object.boolean_parent = parent;
                    object.operation = operation;
                    if let Some(softness) = softness {
                        object.softness = softness;
                    }
                    model::set_objects(&mut draft, objects);
                }
                Action::DeleteObject { id } => {
                    let mut objects = model::objects(&draft);
                    if !objects.iter().any(|object| object.uuid == id) {
                        return Err(format!("object {id} does not exist"));
                    }
                    objects.retain(|object| object.uuid != id);
                    for object in &mut objects {
                        if object.boolean_parent == Some(id) {
                            object.boolean_parent = None;
                            object.operation = BooleanOperation::Union;
                        }
                    }
                    model::set_objects(&mut draft, objects);
                    let selection = model::selected(&draft)
                        .into_iter()
                        .filter(|selected| *selected != id)
                        .collect();
                    model::set_selected(&mut draft, selection);
                }
                Action::SetWorld { world } => {
                    draft.set_path("scene.world", model::ClaydashValue::World(world))
                }
                Action::CreatePostProcessPass {
                    name,
                    wgsl,
                    enabled,
                } => {
                    let mut passes = model::post_processing(&draft);
                    let mut pass = model::PostProcessPass::new(name, wgsl);
                    pass.enabled = enabled.unwrap_or(true);
                    created_post_process_passes.push(pass.uuid);
                    passes.push(pass);
                    model::set_post_processing(&mut draft, passes);
                }
                Action::UpdatePostProcessPass {
                    id,
                    name,
                    wgsl,
                    enabled,
                } => {
                    let mut passes = model::post_processing(&draft);
                    let pass = passes
                        .iter_mut()
                        .find(|pass| pass.uuid == id)
                        .ok_or_else(|| format!("post-processing pass {id} does not exist"))?;
                    if let Some(name) = name {
                        pass.name = name;
                    }
                    if let Some(wgsl) = wgsl {
                        pass.wgsl = wgsl;
                    }
                    if let Some(enabled) = enabled {
                        pass.enabled = enabled;
                    }
                    model::set_post_processing(&mut draft, passes);
                }
                Action::MovePostProcessPass { id, index } => {
                    let mut passes = model::post_processing(&draft);
                    let old = passes
                        .iter()
                        .position(|pass| pass.uuid == id)
                        .ok_or_else(|| format!("post-processing pass {id} does not exist"))?;
                    if index >= passes.len() {
                        return Err(format!("post-processing index {index} is out of range"));
                    }
                    let pass = passes.remove(old);
                    passes.insert(index, pass);
                    model::set_post_processing(&mut draft, passes);
                }
                Action::DeletePostProcessPass { id } => {
                    let mut passes = model::post_processing(&draft);
                    let old = passes.len();
                    passes.retain(|pass| pass.uuid != id);
                    if passes.len() == old {
                        return Err(format!("post-processing pass {id} does not exist"));
                    }
                    model::set_post_processing(&mut draft, passes);
                }
                Action::SetMaterials { materials } => {
                    model::set_material_assets(&mut draft, materials)
                }
                Action::CreateCustomMaterial { name, wgsl } => {
                    let mut asset = MaterialAsset::custom(name);
                    asset.wgsl = Some(wgsl);
                    created_materials.push(asset.uuid);
                    let mut assets = model::material_assets(&draft);
                    assets.push(asset);
                    model::set_material_assets(&mut draft, assets);
                }
                Action::UpdateCustomMaterial { id, name, wgsl } => {
                    let mut assets = model::material_assets(&draft);
                    let asset = assets
                        .iter_mut()
                        .find(|asset| asset.uuid == id)
                        .ok_or_else(|| format!("material {id} does not exist"))?;
                    if asset.material.kind != model::MaterialKind::Custom {
                        return Err(format!("material {id} is not a custom WGSL material"));
                    }
                    if let Some(name) = name {
                        asset.name = name;
                    }
                    if let Some(wgsl) = wgsl {
                        asset.wgsl = Some(wgsl);
                    }
                    model::set_material_assets(&mut draft, assets);
                }
                Action::AssignMaterial { id, object_ids } => {
                    let asset = model::material_assets(&draft)
                        .into_iter()
                        .find(|asset| asset.uuid == id)
                        .ok_or_else(|| format!("material {id} does not exist"))?;
                    if object_ids.is_empty() {
                        return Err("AssignMaterial needs at least one object id".into());
                    }
                    let mut objects = model::objects(&draft);
                    for object_id in object_ids {
                        let object = objects
                            .iter_mut()
                            .find(|object| object.uuid == object_id)
                            .ok_or_else(|| format!("object {object_id} does not exist"))?;
                        object.material_id = Some(id);
                        object.material = asset.material;
                        object.color = asset.material.color;
                    }
                    model::set_objects(&mut draft, objects);
                }
                Action::SetCameras { cameras } => model::set_scene_cameras(&mut draft, cameras),
                Action::SetAnimation { animation } => draft.set_path(
                    "scene.animation",
                    model::ClaydashValue::Animation(animation),
                ),
                Action::SetSelection { ids } => {
                    for id in &ids {
                        if !model::objects_ref(&draft)
                            .iter()
                            .any(|object| object.uuid == *id)
                            && !model::scene_cameras(&draft)
                                .iter()
                                .any(|camera| camera.uuid == *id)
                        {
                            return Err(format!("selection target {id} does not exist"));
                        }
                    }
                    model::set_selected_exact(&mut draft, ids);
                }
                Action::SetActiveCamera { id } => {
                    if let Some(id) = id {
                        if !model::scene_cameras(&draft)
                            .iter()
                            .any(|camera| camera.uuid == id)
                        {
                            return Err(format!("camera {id} does not exist"));
                        }
                        draft.set_path("scene.active_camera", model::ClaydashValue::Uuid(id));
                    } else {
                        draft.set_path("scene.active_camera", model::ClaydashValue::None);
                    }
                }
                Action::ReplaceScene { scene } => {
                    let bytes = serde_json::to_vec(&scene).map_err(|error| error.to_string())?;
                    let restored = document::deserialize_scene(&bytes)?;
                    draft.set_tree("scene", restored);
                    replace = true;
                }
            }
        }
        validate_scene(&draft)?;
        if replace {
            self.replace_scene(draft.get_tree("scene").ok_or("missing scene")?);
        } else {
            draft.make_undo_redo_snapshot();
            self.tree = draft;
        }
        if replace {
            self.agent_revision += 1;
        }
        Ok(
            json!({"revision": self.scene_revision(), "created_ids": created,
            "created_material_ids": created_materials,
            "created_post_process_pass_ids": created_post_process_passes}),
        )
    }
}

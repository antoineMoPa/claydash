use super::*;

impl Renderer {
    pub(super) fn prepare_scene_pipelines(
        &mut self,
        world: World,
        capacity: u32,
        was_boolean: bool,
        shader_features: SceneShaderFeatures,
    ) {
        let shader_features_changed = shader_features != self.scene_shader_features;
        let previous_capacity = if was_boolean {
            self.boolean_pipeline.as_ref().map_or(0, |(size, _)| *size)
        } else {
            1
        };
        let rebuild_hybrid = self.hybrid_pipeline.is_none()
            || shader_features_changed
            || self.scene_pipelines_dirty
            || previous_capacity != capacity;
        let rebuild_deferred = self
            .deferred_geometry_pipeline
            .as_ref()
            .is_none_or(|(size, hybrid, _)| *size != capacity || *hybrid != self.hybrid_enabled)
            || self.scene_pipelines_dirty
            || shader_features_changed;
        if world.render_pipeline == crate::model::RenderPipelineMode::Deferred
            && self.deferred_supported
            && rebuild_deferred
        {
            self.deferred_geometry_pipeline = Some((
                capacity,
                self.hybrid_enabled,
                create_deferred_geometry_pipeline(
                    &self.device,
                    &self.shader_source,
                    &self.pipeline_layout,
                    capacity,
                    self.hybrid_enabled,
                    shader_features,
                ),
            ));
        }
        if shader_features_changed || self.scene_pipelines_dirty {
            self.boolean_pipeline = None;
            self.fast_boolean_pipeline = None;
        }
        if !self.has_booleans
            && (shader_features_changed || self.scene_pipelines_dirty || was_boolean)
        {
            self.pipeline = create_scene_pipeline_for_materials(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                self.render_format,
                self.use_bvh,
                1,
                false,
                false,
                shader_features,
                false,
            );
            self.fast_pipeline = create_scene_pipeline_for_materials(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                self.render_format,
                self.use_bvh,
                1,
                false,
                true,
                shader_features,
                false,
            );
        }
        self.scene_shader_features = shader_features;
        self.scene_pipelines_dirty = false;
        if self.has_booleans
            && self
                .boolean_pipeline
                .as_ref()
                .is_none_or(|(size, _)| *size != capacity)
        {
            self.boolean_pipeline = Some((
                capacity,
                create_scene_pipeline_for_materials(
                    &self.device,
                    &self.shader_source,
                    &self.pipeline_layout,
                    self.render_format,
                    self.use_bvh,
                    capacity,
                    false,
                    false,
                    shader_features,
                    false,
                ),
            ));
        }
        if self.has_booleans
            && self
                .fast_boolean_pipeline
                .as_ref()
                .is_none_or(|(size, _)| *size != capacity)
        {
            self.fast_boolean_pipeline = Some((
                capacity,
                create_scene_pipeline_for_materials(
                    &self.device,
                    &self.shader_source,
                    &self.pipeline_layout,
                    self.render_format,
                    self.use_bvh,
                    capacity,
                    false,
                    true,
                    shader_features,
                    false,
                ),
            ));
        }
        if self.hybrid_enabled && rebuild_hybrid {
            self.hybrid_pipeline = Some(create_scene_pipeline_for_materials(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                self.render_format,
                self.use_bvh,
                capacity,
                false,
                false,
                shader_features,
                true,
            ));
            self.hybrid_fast_pipeline = Some(create_scene_pipeline_for_materials(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                self.render_format,
                self.use_bvh,
                capacity,
                false,
                true,
                shader_features,
                true,
            ));
            self.hybrid_depth_pipeline = Some(create_hybrid_depth_pipeline(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                capacity,
            ));
        } else if !self.hybrid_enabled {
            self.hybrid_pipeline = None;
            self.hybrid_fast_pipeline = None;
            self.hybrid_depth_pipeline = None;
        }
    }
}

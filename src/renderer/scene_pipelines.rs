use super::*;

const MAX_CACHED_SCENE_PIPELINES: usize = 24;

impl Renderer {
    fn cached_scene_pipeline(
        &mut self,
        kind: ScenePipelineKind,
        features: SceneShaderFeatures,
    ) -> wgpu::RenderPipeline {
        let key = ScenePipelineKey { kind, features };
        if let Some(index) = self
            .scene_pipeline_cache
            .iter()
            .position(|(cached, _)| *cached == key)
        {
            let entry = self.scene_pipeline_cache.remove(index).unwrap();
            let pipeline = entry.1.clone();
            self.scene_pipeline_cache.push_back(entry);
            return pipeline;
        }
        let pipeline = match kind {
            ScenePipelineKind::Shaded {
                capacity,
                fast,
                hybrid,
            } => create_scene_pipeline_for_materials(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                self.render_format,
                self.use_bvh,
                capacity,
                false,
                fast,
                features,
                hybrid,
            ),
            ScenePipelineKind::Deferred { capacity, hybrid } => create_deferred_geometry_pipeline(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                capacity,
                hybrid,
                features,
            ),
            ScenePipelineKind::HybridDepth { capacity } => create_hybrid_depth_pipeline(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                capacity,
                features,
            ),
        };
        if self.scene_pipeline_cache.len() == MAX_CACHED_SCENE_PIPELINES {
            self.scene_pipeline_cache.pop_front();
        }
        self.scene_pipeline_cache.push_back((key, pipeline.clone()));
        pipeline
    }

    pub(super) fn prepare_scene_pipelines(
        &mut self,
        world: World,
        capacity: u32,
        features: SceneShaderFeatures,
    ) {
        if world.render_pipeline == crate::model::RenderPipelineMode::Deferred
            && self.deferred_supported
        {
            self.deferred_geometry_pipeline = Some((
                capacity,
                self.hybrid_enabled,
                self.cached_scene_pipeline(
                    ScenePipelineKind::Deferred {
                        capacity,
                        hybrid: self.hybrid_enabled,
                    },
                    features,
                ),
            ));
        } else {
            self.deferred_geometry_pipeline = None;
        }
        if self.has_booleans {
            self.boolean_pipeline = Some((
                capacity,
                self.cached_scene_pipeline(
                    ScenePipelineKind::Shaded {
                        capacity,
                        fast: false,
                        hybrid: false,
                    },
                    features,
                ),
            ));
            self.fast_boolean_pipeline = Some((
                capacity,
                self.cached_scene_pipeline(
                    ScenePipelineKind::Shaded {
                        capacity,
                        fast: true,
                        hybrid: false,
                    },
                    features,
                ),
            ));
        } else {
            self.boolean_pipeline = None;
            self.fast_boolean_pipeline = None;
            self.pipeline = self.cached_scene_pipeline(
                ScenePipelineKind::Shaded {
                    capacity: 1,
                    fast: false,
                    hybrid: false,
                },
                features,
            );
            self.fast_pipeline = self.cached_scene_pipeline(
                ScenePipelineKind::Shaded {
                    capacity: 1,
                    fast: true,
                    hybrid: false,
                },
                features,
            );
        }
        if self.hybrid_enabled {
            self.hybrid_pipeline = Some(self.cached_scene_pipeline(
                ScenePipelineKind::Shaded {
                    capacity,
                    fast: false,
                    hybrid: true,
                },
                features,
            ));
            self.hybrid_fast_pipeline = Some(self.cached_scene_pipeline(
                ScenePipelineKind::Shaded {
                    capacity,
                    fast: true,
                    hybrid: true,
                },
                features,
            ));
            self.hybrid_depth_pipeline = Some(
                self.cached_scene_pipeline(ScenePipelineKind::HybridDepth { capacity }, features),
            );
        } else {
            self.hybrid_pipeline = None;
            self.hybrid_fast_pipeline = None;
            self.hybrid_depth_pipeline = None;
        }
    }
}

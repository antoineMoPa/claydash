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
            ScenePipelineKind::MeshBackground => hybrid_splats::create_mesh_background_pipeline(
                &self.device, &self.pipeline_layout, self.render_format, features.bloom,
            ),
            ScenePipelineKind::MeshMaterial { capacity } => poisson_material::create_pipeline(
                &self.device, Some(&self.pipeline_layout), &self.shader_source,
                self.render_format, capacity, features,
            ),
            ScenePipelineKind::Shaded {
                capacity,
                fast,
                hybrid,
            } => {
                let contact_layout =
                    (features.separate_contact && !fast).then(|| self.contact_transport_layout());
                create_scene_pipeline_for_materials(
                    &self.device,
                    &if features.separate_contact && !fast {
                        contact::transport_source(&self.shader_source)
                    } else {
                        self.shader_source.clone()
                    },
                    contact_layout.as_ref().unwrap_or(&self.pipeline_layout),
                    self.render_format,
                    self.use_bvh,
                    capacity,
                    false,
                    fast,
                    features,
                    hybrid,
                )
            }
            ScenePipelineKind::Contact { capacity, hybrid } => {
                let mut contact_features = features;
                contact_features.bloom = false;
                create_scene_pipeline_for_materials(
                    &self.device,
                    &contact::probe_source(&self.shader_source),
                    &self.pipeline_layout,
                    wgpu::TextureFormat::R32Float,
                    self.use_bvh,
                    capacity,
                    false,
                    false,
                    contact_features,
                    hybrid,
                )
            }
            ScenePipelineKind::Deferred { capacity, hybrid } => create_deferred_geometry_pipeline(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                capacity,
                hybrid,
                features,
            ),
            ScenePipelineKind::HybridDepth {
                capacity,
                skip_splats,
            } => create_hybrid_depth_pipeline(
                &self.device,
                &self.shader_source,
                &self.pipeline_layout,
                capacity,
                features,
                skip_splats,
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
        let raster_only = self.node_count == 0;
        if raster_only {
            self.viewport.set_contact_pipeline(None);
            self.mesh_background_pipeline = self.cached_scene_pipeline(ScenePipelineKind::MeshBackground, features);
            if self.mesh_vertex_count > 0 {
                self.mesh_pipeline = self.cached_scene_pipeline(ScenePipelineKind::MeshMaterial { capacity }, features);
            }
            self.deferred_geometry_pipeline = None;
            self.hybrid_depth_pipeline = None;
            self.hybrid_pipeline = None;
            self.hybrid_fast_pipeline = None;
            return;
        }
        let contact = if features.separate_contact {
            Some(self.cached_scene_pipeline(
                ScenePipelineKind::Contact {
                    capacity,
                    hybrid: self.hybrid_enabled,
                },
                features,
            ))
        } else {
            None
        };
        self.viewport.set_contact_pipeline(contact);
        self.mesh_background_pipeline = self.cached_scene_pipeline(ScenePipelineKind::MeshBackground, features);
        if self.mesh_vertex_count > 0 {
            self.mesh_pipeline = self.cached_scene_pipeline(ScenePipelineKind::MeshMaterial { capacity }, features);
        }
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
        } else {
            self.hybrid_pipeline = None;
            self.hybrid_fast_pipeline = None;
        }
        self.hybrid_depth_pipeline = if self.hybrid_enabled || self.mesh_vertex_count > 0 {
            Some(self.cached_scene_pipeline(
                ScenePipelineKind::HybridDepth {
                    capacity,
                    skip_splats: self.hybrid_enabled,
                },
                features,
            ))
        } else {
            None
        };
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a GPU adapter"]
    fn mesh_pipeline_cache_reuses_compilation_and_keys_bloom_variants() {
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 32, height: 32, ..Default::default()
        })).unwrap();
        renderer.mesh_vertex_count = 3;
        let features = SceneShaderFeatures::for_scene(&[], &[]);
        renderer.prepare_scene_pipelines(World::default(), 1, features);
        let material = renderer.mesh_pipeline.clone();
        let background = renderer.mesh_background_pipeline.clone();
        let entries = renderer.scene_pipeline_cache.len();
        for _ in 0..32 { renderer.prepare_scene_pipelines(World::default(), 1, features); }
        assert_eq!(renderer.mesh_pipeline, material);
        assert_eq!(renderer.mesh_background_pipeline, background);
        assert_eq!(renderer.scene_pipeline_cache.len(), entries);
        let mut bloom = features;
        bloom.bloom = true;
        renderer.prepare_scene_pipelines(World::default(), 1, bloom);
        assert_ne!(renderer.mesh_pipeline, material);
        assert_ne!(renderer.mesh_background_pipeline, background);
        renderer.prepare_scene_pipelines(World::default(), 1, features);
        assert_eq!(renderer.mesh_pipeline, material);
        assert_eq!(renderer.mesh_background_pipeline, background);
        renderer.scene_pipeline_cache.clear();
        renderer.prepare_scene_pipelines(World::default(), 1, features);
        assert_ne!(renderer.mesh_pipeline, material, "source invalidation must rebuild material shaders");
    }
}

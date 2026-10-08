use super::*;
pub(super) mod pipeline;
mod shader;
use crate::model::{PrimitiveKind, SphereParams};
use pipeline::{PreviewPipeline, PreviewPipelineState};

const PREVIEW_WIDTH: u32 = 112;
const PREVIEW_HEIGHT: u32 = 72;

impl Renderer {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn has_pending_material_previews(&self, requests: &[MaterialPreviewRequest]) -> bool {
        use std::sync::atomic::Ordering;
        self.material_preview_in_flight.load(Ordering::Acquire)
            || self.material_preview_compiling.load(Ordering::Acquire)
            || requests.iter().any(|request| {
                !self.material_previews.iter().any(|preview| preview.request == *request)
                    && !self.material_preview_pipelines.iter().any(|(family, state)|
                        *family == BuiltinMaterialFeatures::for_materials(&[request.material])
                            && matches!(state, PreviewPipelineState::Failed(_)))
            })
    }

    pub fn sync_custom_materials(&mut self, assets: &[MaterialAsset]) -> Result<(), String> {
        let sources: Vec<_> = assets
            .iter()
            .filter(|asset| asset.material.kind == MaterialKind::Custom)
            .filter_map(|asset| {
                asset
                    .wgsl
                    .as_ref()
                    .map(|source| (asset.uuid, source.clone()))
            })
            .collect();
        if sources == self.custom_material_sources {
            return Ok(());
        }
        material_gpu::validate_custom_materials(assets)?;
        let shader_source = material_gpu::shader_source_for_assets(assets);
        self.boolean_pipeline = None;
        self.fast_boolean_pipeline = None;
        self.scene_pipeline_cache.clear();
        self.shader_source = shader_source;
        self.custom_material_sources = sources;
        self.uploaded_scene_versions = [i32::MIN; 2];
        for preview in std::mem::take(&mut self.material_previews) {
            self.egui_renderer.free_texture(&preview.id);
        }
        self.material_preview_pipelines.clear();
        self.invalidate_scene();
        Ok(())
    }

    pub fn material_preview_ids(&mut self, assets: &[MaterialAsset]) -> MaterialPreviewIds {
        // Invalidate changed/deleted shared materials without rendering hidden assets.
        self.material_previews.retain(|preview| {
            let valid = preview.request.asset_id.is_none_or(|id| {
                assets
                    .iter()
                    .any(|asset| asset.uuid == id && asset.material == preview.request.material)
            });
            if !valid {
                self.egui_renderer.free_texture(&preview.id);
            }
            valid
        });

        let mut ids = MaterialPreviewIds::default();
        for (family, state) in &self.material_preview_pipelines {
            if let PreviewPipelineState::Failed(error) = state {
                ids.failures.push((*family, error.clone()));
            }
        }
        for preview in &self.material_previews {
            if let Some(asset) = preview.request.asset_id {
                ids.assets.push((asset, preview.id));
            } else {
                ids.materials.push((preview.request.material, preview.id));
            }
        }
        ids
    }

    pub fn sync_visible_material_previews(
        &mut self,
        requests: &[MaterialPreviewRequest],
        context: &egui::Context,
    ) {
        use std::sync::atomic::Ordering;
        // Poll completed compilations without ever waiting on the UI thread.
        for (_, state) in &mut self.material_preview_pipelines {
            state.poll();
        }

        if self.material_preview_in_flight.load(Ordering::Acquire) {
            return;
        }
        let Some(request) = requests
            .iter()
            .find(|request| {
                !self
                    .material_preview_pipelines
                    .iter()
                    .any(|(family, state)| {
                        *family == BuiltinMaterialFeatures::for_materials(&[request.material])
                            && matches!(state, PreviewPipelineState::Failed(_))
                    })
                    && !self
                        .material_previews
                        .iter()
                        .any(|preview| preview.request == **request)
            })
            .copied()
        else {
            return;
        };
        let features = SceneShaderFeatures::for_material_previews(request.material);
        let pipeline = match self
            .material_preview_pipelines
            .iter()
            .find(|(family, _)| *family == features.materials)
            .map(|(_, state)| state)
        {
            Some(PreviewPipelineState::Ready(pipeline)) => pipeline.clone(),
            Some(PreviewPipelineState::Compiling(_) | PreviewPipelineState::Failed(_)) => return,
            None => {
                // Only one compiler job at a time, even while scrolling/filtering.
                if self.material_preview_compiling.load(Ordering::Acquire) {
                    return;
                }
                let state = self.compile_preview_pipeline(features, context);
                self.material_preview_pipelines
                    .push((features.materials, state));
                context.request_repaint();
                return;
            }
        };
        self.material_preview_in_flight
            .store(true, Ordering::Release);
        let (id, texture) =
            self.render_material_preview(request.material, request.asset_id, &pipeline);
        self.material_previews.push(MaterialPreview {
            request,
            id,
            _texture: texture,
        });
        // Temporary sphere/camera buffers must be replaced before the scene is drawn.
        self.uploaded_scene_versions = [i32::MIN; 2];
        let pending = Arc::clone(&self.material_preview_in_flight);
        let completion_context = context.clone();
        self.queue.on_submitted_work_done(move || {
            pending.store(false, Ordering::Release);
            completion_context.request_repaint();
        });
        context.request_repaint();
    }

    fn render_material_preview(
        &mut self,
        material: Material,
        material_id: Option<uuid::Uuid>,
        pipeline: &PreviewPipeline,
    ) -> (egui::TextureId, wgpu::Texture) {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.params = SdfParams::SphereParams(SphereParams { radius: 0.58 });
        object.material = material;
        object.material_id = material_id;
        object.color = material.color;
        let mut camera = Camera::new();
        camera.position *= 0.52;
        camera.viewport = Vec2::new(PREVIEW_WIDTH as f32, PREVIEW_HEIGHT as f32);
        let was_boolean = self.has_booleans;
        self.uploaded_scene_versions = [i32::MIN; 2];
        self.upload_scene_with_exposure(&camera, &[object], &[], [0, 0], 1.5);
        self.has_booleans = was_boolean;

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("material preview sphere"),
            size: wgpu::Extent3d {
                width: PREVIEW_WIDTH,
                height: PREVIEW_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        #[cfg(target_arch = "wasm32")]
        self.draw_web_preview(pipeline, &view);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("material preview encoder"),
                });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("material preview sphere"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            self.queue.submit([encoder.finish()]);
        }
        let id = self.egui_renderer.register_native_texture(
            &self.device,
            &view,
            wgpu::FilterMode::Linear,
        );
        (id, texture)
    }
}

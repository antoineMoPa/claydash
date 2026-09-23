use super::*;
use crate::model::{PrimitiveKind, SphereParams};

const PREVIEW_WIDTH: u32 = 112;
const PREVIEW_HEIGHT: u32 = 72;

impl Renderer {
    pub(crate) fn material_preview_ids(&self) -> Option<MaterialPreviewIds> {
        self.material_preview_ids.clone().map(|mut ids| {
            ids.assets = self
                .material_asset_previews
                .iter()
                .map(|preview| (preview.uuid, preview.id))
                .collect();
            ids
        })
    }

    pub(crate) fn sync_material_asset_previews(&mut self, assets: &[MaterialAsset]) {
        let mut retained = Vec::new();
        for preview in std::mem::take(&mut self.material_asset_previews) {
            if assets
                .iter()
                .any(|asset| asset.uuid == preview.uuid && asset.material == preview.material)
            {
                retained.push(preview);
            } else {
                self.egui_renderer.free_texture(&preview.id);
            }
        }
        self.material_asset_previews = retained;
        let missing: Vec<_> = assets
            .iter()
            .filter(|asset| {
                !self
                    .material_asset_previews
                    .iter()
                    .any(|preview| preview.uuid == asset.uuid)
            })
            .collect();
        if missing.is_empty() {
            return;
        }
        let pipeline = self
            .material_preview_pipeline
            .as_ref()
            .expect("material preview pipeline")
            .clone();
        for (index, asset) in missing.into_iter().enumerate() {
            let (id, texture) =
                self.render_material_preview(asset.material, index as i32 + 100, &pipeline);
            self.material_asset_previews.push(MaterialAssetPreview {
                uuid: asset.uuid,
                material: asset.material,
                id,
                _texture: texture,
            });
        }
        self.uploaded_scene_versions = [i32::MIN; 2];
    }

    pub(super) fn create_material_previews(&mut self) {
        let pipeline = create_scene_pipeline(
            &self.device,
            &self.shader_source,
            &self.pipeline_layout,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            self.use_bvh,
            1,
        );
        let presets = [
            Material::preset(MaterialKind::Transparent),
            Material::preset(MaterialKind::Metallic),
            Material::preset(MaterialKind::Solid),
            Material::preset(MaterialKind::Diagnostic),
            Material::wood_preset(WoodSpecies::Oak),
            Material::wood_preset(WoodSpecies::Walnut),
            Material::wood_preset(WoodSpecies::Pine),
            Material::wood_preset(WoodSpecies::Maple),
        ];
        let mut ids = Vec::with_capacity(presets.len());
        for (index, material) in presets.into_iter().enumerate() {
            let (id, texture) = self.render_material_preview(material, index as i32, &pipeline);
            ids.push(id);
            self.material_preview_textures.push(texture);
        }
        self.material_preview_ids = Some(MaterialPreviewIds {
            transparent: ids[0],
            metallic: ids[1],
            solid: ids[2],
            diagnostic: ids[3],
            oak: ids[4],
            walnut: ids[5],
            pine: ids[6],
            maple: ids[7],
            assets: Vec::new(),
        });
        self.material_preview_pipeline = Some(pipeline);
        // The next scene upload must replace the temporary preview object.
        self.uploaded_scene_versions = [i32::MIN; 2];
    }

    fn render_material_preview(
        &mut self,
        material: Material,
        index: i32,
        pipeline: &wgpu::RenderPipeline,
    ) -> (egui::TextureId, wgpu::Texture) {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.params = SdfParams::SphereParams(SphereParams { radius: 0.58 });
        object.material = material;
        object.color = material.color;
        let mut camera = Camera::new();
        camera.position *= 0.52;
        camera.viewport = Vec2::new(PREVIEW_WIDTH as f32, PREVIEW_HEIGHT as f32);
        self.upload_scene_with_exposure(&camera, &[object], &[], [index, 0], 1.5);

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
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
        let id = self.egui_renderer.register_native_texture(
            &self.device,
            &view,
            wgpu::FilterMode::Linear,
        );
        (id, texture)
    }
}

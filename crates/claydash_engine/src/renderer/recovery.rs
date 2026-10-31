//! CPU-owned resources needed to replace a failed GPU without replacing the document.
use super::*;

#[derive(Default)]
pub(super) struct TextureJournal {
    images: std::collections::HashMap<egui::TextureId, egui::epaint::ImageDelta>,
}

impl TextureJournal {
    pub fn update(&mut self, id: egui::TextureId, delta: &egui::epaint::ImageDelta) {
        let Some([x, y]) = delta.pos else {
            self.images.insert(id, delta.clone());
            return;
        };
        let Some(base) = self.images.get_mut(&id) else {
            return;
        };
        let egui::ImageData::Color(base_image) = &mut base.image;
        let egui::ImageData::Color(patch) = &delta.image;
        let base_image = std::sync::Arc::make_mut(base_image);
        assert!(x + patch.size[0] <= base_image.size[0] && y + patch.size[1] <= base_image.size[1]);
        for row in 0..patch.size[1] {
            let destination = (y + row) * base_image.size[0] + x;
            let source = row * patch.size[0];
            base_image.pixels[destination..destination + patch.size[0]]
                .copy_from_slice(&patch.pixels[source..source + patch.size[0]]);
        }
        base.options = delta.options;
    }
    pub fn free(&mut self, id: egui::TextureId) {
        self.images.remove(&id);
    }
}

pub struct RendererRecovery {
    textures: TextureJournal,
    meshes: poisson_mesh::PoissonMeshState,
    voxels: voxels::VoxelState,
}

impl Renderer {
    pub fn take_recovery_resources(&mut self) -> RendererRecovery {
        RendererRecovery {
            textures: std::mem::take(&mut self.ui_textures),
            meshes: std::mem::take(&mut self.poisson_mesh),
            voxels: std::mem::take(&mut self.voxels),
        }
    }
    pub fn restore_recovery_resources(
        &mut self,
        resources: RendererRecovery,
        objects: &[SdfObject],
    ) {
        for (id, delta) in &resources.textures.images {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        self.ui_textures = resources.textures;
        self.poisson_mesh = resources.meshes;
        self.voxels = resources.voxels;
        self.poisson_mesh.rebuild_buffer(
            &self.device,
            &self.queue,
            objects,
            &mut self.mesh_buffer,
            &mut self.mesh_vertex_count,
            &self.voxels,
        );
        self.rebuild_mesh_color_atlas();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a GPU adapter"]
    fn failed_scene_submission_recovers_with_ui_textures_on_a_new_device() {
        let options = || super::super::HeadlessOptions {
            width: 32,
            height: 24,
            ..Default::default()
        };
        let mut failed = pollster::block_on(Renderer::new_headless(options())).unwrap();
        let id = egui::TextureId::Managed(42);
        failed.ui_textures.update(
            id,
            &egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([2, 2], egui::Color32::WHITE),
                egui::TextureOptions::LINEAR,
            ),
        );
        failed.viewport.fail_receipt_for_test(&failed.device);
        let scene = vec![SdfObject::create_kind(crate::model::PrimitiveKind::Sphere)];
        let mut camera = Camera::new();
        camera.position = Vec3::new(0.0, 0.0, 4.0);
        camera.target = Vec3::ZERO;
        camera.viewport = glam::Vec2::new(32.0, 24.0);
        let result = failed.render_offscreen(
            &camera,
            &scene,
            World::default(),
            &[],
            std::time::Duration::from_secs(5),
        );
        assert!(
            result.is_err(),
            "a failed receipt must not return a successful black capture"
        );
        assert!(failed.gpu_fault().is_some());
        let resources = failed.take_recovery_resources();
        drop(failed);
        let mut recovered = pollster::block_on(Renderer::new_headless(options())).unwrap();
        recovered.restore_recovery_resources(resources, &scene);
        assert_eq!(recovered.ui_textures.images[&id].image.size(), [2, 2]);
        let image = recovered
            .render_offscreen(
                &camera,
                &scene,
                World::default(),
                &[],
                std::time::Duration::from_secs(5),
            )
            .unwrap();
        assert!(recovered.gpu_fault().is_none());
        assert!(image
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[..3].iter().any(|value| *value != 0)));
    }
    #[test]
    fn texture_replay_preserves_full_images_partial_atlas_updates_and_frees() {
        let mut journal = TextureJournal::default();
        let id = egui::TextureId::Managed(0);
        journal.update(
            id,
            &egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([4, 3], egui::Color32::BLACK),
                egui::TextureOptions::LINEAR,
            ),
        );
        journal.update(
            id,
            &egui::epaint::ImageDelta::partial(
                [1, 1],
                egui::ColorImage::filled([2, 1], egui::Color32::WHITE),
                egui::TextureOptions::NEAREST,
            ),
        );
        let replay = &journal.images[&id];
        assert_eq!(replay.pos, None);
        assert_eq!(replay.image.size(), [4, 3]);
        let egui::ImageData::Color(image) = &replay.image;
        assert_eq!(
            image
                .pixels
                .iter()
                .filter(|pixel| **pixel == egui::Color32::WHITE)
                .count(),
            2
        );
        assert_eq!(image.pixels[5], egui::Color32::WHITE);
        assert_eq!(image.pixels[6], egui::Color32::WHITE);
        assert_eq!(replay.options, egui::TextureOptions::NEAREST);
        journal.free(id);
        assert!(journal.images.is_empty());
    }
}

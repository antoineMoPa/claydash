use super::*;

impl Renderer {
    pub(super) fn free_textures(&mut self, output: &mut egui::FullOutput) {
        for id in output.textures_delta.free.drain() {
            self.ui_textures.free(id);
            self.egui_renderer.free_texture(&id);
        }
    }
}

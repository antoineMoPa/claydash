use super::*;
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[cfg(not(target_arch = "wasm32"))]
pub(super) type PreviewPipeline = wgpu::RenderPipeline;
#[cfg(target_arch = "wasm32")]
pub(super) type PreviewPipeline = wasm_bindgen::JsValue;

pub(in crate::renderer) enum PreviewPipelineState {
    Compiling(Receiver<Result<PreviewPipeline, String>>),
    Ready(PreviewPipeline),
    Failed(String),
}

impl PreviewPipelineState {
    pub(super) fn poll(&mut self) {
        if let Self::Compiling(receiver) = self {
            match receiver.try_recv() {
                Ok(Ok(pipeline)) => *self = Self::Ready(pipeline),
                Ok(Err(error)) => *self = Self::Failed(error),
                Err(TryRecvError::Disconnected) => {
                    *self = Self::Failed("Preview compiler stopped".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }
}

impl Renderer {
    pub(super) fn compile_preview_pipeline(
        &self,
        features: SceneShaderFeatures,
        context: &egui::Context,
    ) -> PreviewPipelineState {
        let (sender, receiver) = mpsc::channel();
        let context = context.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let device = self.device.clone();
            let source = self.shader_source.clone();
            let layout = self.pipeline_layout.clone();
            let use_bvh = self.use_bvh;
            std::thread::spawn(move || {
                let pipeline = create_scene_pipeline_for_materials(
                    &device,
                    &source,
                    &layout,
                    wgpu::TextureFormat::Rgba8UnormSrgb,
                    use_bvh,
                    1,
                    true,
                    false,
                    features,
                    false,
                );
                let _ = sender.send(Ok(pipeline));
                context.request_repaint();
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let device = self.device.as_webgpu().expect("WebGPU device").clone();
            let source = specialized_neural_shader_source(
                &specialized_shader_source(&self.shader_source, 1),
                features.neural_width,
            );
            let mut constants = scene_feature_constants(&source, features);
            constants.extend([
                ("USE_BVH", f64::from(self.use_bvh)),
                ("HAS_BOOLEANS", 0.0),
                ("TRANSPARENT_BACKGROUND", 1.0),
                ("FAST_PREVIEW", 0.0),
                ("HYBRID_SPLATS", 0.0),
            ]);
            let constants = serde_json::to_string(
                &constants
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>(),
            )
            .unwrap();
            wasm_bindgen_futures::spawn_local(async move {
                let result = compile_preview(&device, &source, &constants)
                    .await
                    .map_err(|error| format!("Preview compilation failed: {error:?}"));
                let _ = sender.send(result);
                context.request_repaint();
            });
        }
        PreviewPipelineState::Compiling(receiver)
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn draw_web_preview(&self, pipeline: &PreviewPipeline, view: &wgpu::TextureView) {
        let buffers = js_sys::Array::new();
        for buffer in [
            &self.camera_buffer,
            &self.objects_buffer,
            &self.bvh_buffer,
            &self.material_headers_buffer,
            &self.material_params_buffer,
            &self.polygon_points_buffer,
            &self.lattice_points_buffer,
            &self.modifier_params_buffer,
            &self.box_depth_buffer,
        ] {
            buffers.push(buffer.as_webgpu().expect("WebGPU buffer"));
        }
        draw_preview(
            self.device.as_webgpu().unwrap(),
            pipeline,
            &buffers,
            self.lattice_atlas.as_webgpu().unwrap(),
            self.image_atlas.as_webgpu().unwrap(),
            view.as_webgpu().unwrap(),
        );
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/renderer/material_previews/web_pipeline.js")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(catch, js_name = compilePreview)]
    async fn compile_preview(
        device: &wgpu::webgpu::GpuDevice,
        source: &str,
        constants: &str,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = drawPreview)]
    fn draw_preview(
        device: &wgpu::webgpu::GpuDevice,
        pipeline: &wasm_bindgen::JsValue,
        buffers: &js_sys::Array,
        lattice: &wgpu::webgpu::GpuTexture,
        images: &wgpu::webgpu::GpuTexture,
        view: &wgpu::webgpu::GpuTextureView,
    );
}

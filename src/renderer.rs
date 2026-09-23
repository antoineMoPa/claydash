use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    camera::{Camera, ProjectionMode},
    model::{Material, MaterialAsset, MaterialKind, SdfObject, SdfParams, WoodSpecies},
};

#[cfg(not(target_arch = "wasm32"))]
mod benchmark;

const MAX_OBJECTS: usize = 1024;
const MAX_BVH_NODES: usize = MAX_OBJECTS * 2 - 1;
const BVH_LEAF: u32 = u32::MAX;

fn render_format_for_surface(format: wgpu::TextureFormat) -> wgpu::TextureFormat {
    match format {
        wgpu::TextureFormat::Bgra8Unorm => wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8UnormSrgb,
        format => format,
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuCamera {
    inverse_view_projection: [[f32; 4]; 4],
    // xyz is the camera position; w is the display exposure.
    position: [f32; 4],
    count: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuObject {
    meta: [i32; 4],
    color: [f32; 4],
    inverse_rows: [[f32; 4]; 3],
    params: [f32; 4],
    material: [f32; 4],
    repeat_spacing: [f32; 4],
    repeat_count: [i32; 4],
    component: [u32; 4],
    wood: [f32; 4],
    wood_scale: [f32; 4],
    wood_growth: [f32; 4],
    wood_fiber: [f32; 4],
    wood_damage: [f32; 4],
    wood_finish: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuBvhNode {
    center_radius: [f32; 4],
    // A leaf stores its object index in x. Internal nodes use BVH_LEAF.
    // y is the first node after this subtree, enabling stackless traversal.
    metadata: [u32; 4],
    aabb_min: [f32; 4],
    aabb_max: [f32; 4],
}

#[derive(Clone, Copy)]
struct ObjectBound {
    center: Vec3,
    radius: f32,
    object_index: u32,
    half_extent: Vec3,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    render_format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    boolean_pipeline: Option<(u32, wgpu::RenderPipeline)>,
    shader_source: String,
    pipeline_layout: wgpu::PipelineLayout,
    use_bvh: bool,
    node_count: u32,
    has_booleans: bool,
    bind_group: wgpu::BindGroup,
    camera_buffer: wgpu::Buffer,
    objects_buffer: wgpu::Buffer,
    bvh_buffer: wgpu::Buffer,
    uploaded_scene_versions: [i32; 2],
    egui_renderer: egui_wgpu::Renderer,
    viewport: crate::viewport::Viewport,
    initial_pixel_budget: u32,
    material_preview_ids: Option<MaterialPreviewIds>,
    material_preview_pipeline: Option<wgpu::RenderPipeline>,
    material_preview_textures: Vec<wgpu::Texture>,
    material_asset_previews: Vec<MaterialAssetPreview>,
}

struct MaterialAssetPreview {
    uuid: uuid::Uuid,
    material: Material,
    id: egui::TextureId,
    _texture: wgpu::Texture,
}

#[derive(Clone)]
pub(crate) struct MaterialPreviewIds {
    pub transparent: egui::TextureId,
    pub metallic: egui::TextureId,
    pub solid: egui::TextureId,
    pub oak: egui::TextureId,
    pub walnut: egui::TextureId,
    pub pine: egui::TextureId,
    pub maple: egui::TextureId,
    pub assets: Vec<(uuid::Uuid, egui::TextureId)>,
}

impl MaterialPreviewIds {
    pub fn egui_id() -> egui::Id {
        egui::Id::new("material-preview-ids")
    }

    pub fn for_material(&self, material: Material) -> egui::TextureId {
        match material.kind {
            MaterialKind::Transparent => self.transparent,
            MaterialKind::Metallic => self.metallic,
            MaterialKind::Solid => self.solid,
            MaterialKind::Wood => match material.wood.species {
                WoodSpecies::Oak => self.oak,
                WoodSpecies::Walnut => self.walnut,
                WoodSpecies::Pine => self.pine,
                WoodSpecies::Maple => self.maple,
            },
        }
    }

    pub fn for_asset(&self, id: uuid::Uuid) -> Option<egui::TextureId> {
        self.assets
            .iter()
            .find_map(|(asset_id, texture)| (*asset_id == id).then_some(*texture))
    }
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

mod bvh;
mod initialization;
mod material_previews;
mod rendering;
mod scene_bounds;
mod scene_upload;
mod texture_cleanup;

use bvh::*;
use scene_bounds::*;

#[cfg(test)]
mod scene_tests;

#[cfg(test)]
mod bvh_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn presentation_formats_use_srgb_views_when_available() {
        assert_eq!(
            super::render_format_for_surface(wgpu::TextureFormat::Bgra8Unorm),
            wgpu::TextureFormat::Bgra8UnormSrgb
        );
        assert_eq!(
            super::render_format_for_surface(wgpu::TextureFormat::Rgba8Unorm),
            wgpu::TextureFormat::Rgba8UnormSrgb
        );
        assert_eq!(
            super::render_format_for_surface(wgpu::TextureFormat::Bgra8UnormSrgb),
            wgpu::TextureFormat::Bgra8UnormSrgb
        );
    }
}

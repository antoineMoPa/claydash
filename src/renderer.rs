use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    camera::{Camera, ProjectionMode},
    model::{SdfObject, SdfParams},
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
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

mod bvh;
mod initialization;
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

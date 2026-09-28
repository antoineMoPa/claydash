use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    camera::{Camera, ProjectionMode},
    model::{Material, MaterialAsset, MaterialKind, SdfObject, SdfParams, WoodSpecies, World},
};

#[cfg(not(target_arch = "wasm32"))]
mod benchmark;
pub(crate) mod post_processing;

const MAX_OBJECTS: usize = 1024;
// Scene components and eligible operand trees share the same storage buffer.
const MAX_BVH_NODES: usize = MAX_OBJECTS * 4;
const MAX_POLYGON_POINTS: usize = 2_097_152;
const MAX_LATTICE_POINTS: usize = MAX_OBJECTS * 9 * 9 * 9;
const LATTICE_ATLAS_TILE_PITCH: u32 = 19;
const LATTICE_ATLAS_TILES_PER_ROW: u32 = 32;
const LATTICE_ATLAS_WIDTH: u32 = LATTICE_ATLAS_TILE_PITCH * LATTICE_ATLAS_TILES_PER_ROW;
const BVH_LEAF: u32 = u32::MAX;
const FLAT_UNION_ROOT: i32 = -2;
const FLAT_COMPONENT_ROOT: i32 = -3;

#[derive(Clone, Copy, PartialEq, Eq)]
struct BuiltinMaterialFeatures {
    wood: bool,
    brick: bool,
    diagnostic: bool,
}

impl BuiltinMaterialFeatures {
    const ALL: Self = Self {
        wood: true,
        brick: true,
        diagnostic: true,
    };

    fn for_materials(materials: &[Material]) -> Self {
        let mut features = Self {
            wood: false,
            brick: false,
            diagnostic: false,
        };
        for material in materials {
            match material.kind {
                MaterialKind::Wood => features.wood = true,
                MaterialKind::Brick => features.brick = true,
                MaterialKind::Diagnostic => features.diagnostic = true,
                _ => {}
            }
        }
        features
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PrimitiveFeatures {
    polygon_prisms: bool,
    bezier_curves: bool,
    lofts: bool,
    text: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SpatialFeatures {
    lattice: bool,
    mirror: bool,
    repetition: bool,
}

impl SpatialFeatures {
    const ALL: Self = Self {
        lattice: true,
        mirror: true,
        repetition: true,
    };

    fn for_objects(objects: &[GpuObject]) -> Self {
        let mut features = Self {
            lattice: false,
            mirror: false,
            repetition: false,
        };
        for object in objects {
            features.lattice |= object.modifier[0] != 0;
            features.mirror |= object.mirror_axes[..3].iter().any(|&axis| axis != 0);
            features.repetition |= object.repeat_count[3] != 0;
        }
        features
    }
}

impl PrimitiveFeatures {
    const ALL: Self = Self {
        polygon_prisms: true,
        bezier_curves: true,
        lofts: true,
        text: true,
    };

    fn for_objects(objects: &[GpuObject]) -> Self {
        let mut features = Self {
            polygon_prisms: false,
            bezier_curves: false,
            lofts: false,
            text: false,
        };
        for object in objects {
            match object.meta[1] {
                sdf_consts::TYPE_POLYGON_PRISM => features.polygon_prisms = true,
                sdf_consts::TYPE_BEZIER_CURVE => features.bezier_curves = true,
                sdf_consts::TYPE_LOFT => features.lofts = true,
                sdf_consts::TYPE_TEXT => features.text = true,
                _ => {}
            }
        }
        features
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SceneShaderFeatures {
    materials: BuiltinMaterialFeatures,
    primitives: PrimitiveFeatures,
    spatial: SpatialFeatures,
    flat_unions: bool,
    neural_sdf: bool,
}

impl SceneShaderFeatures {
    const ALL: Self = Self {
        materials: BuiltinMaterialFeatures::ALL,
        primitives: PrimitiveFeatures::ALL,
        spatial: SpatialFeatures::ALL,
        flat_unions: true,
        neural_sdf: true,
    };

    fn for_scene(materials: &[Material], objects: &[GpuObject]) -> Self {
        Self {
            materials: BuiltinMaterialFeatures::for_materials(materials),
            primitives: PrimitiveFeatures::for_objects(objects),
            spatial: SpatialFeatures::for_objects(objects),
            neural_sdf: objects.iter().any(|object| object.meta[1] == 12),
            flat_unions: objects
                .iter()
                .any(|object| object.meta[3] == FLAT_UNION_ROOT),
        }
    }
}

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
    world_mode: [u32; 4],
    world_color: [f32; 4],
    sun_direction: [f32; 4],
    sky_params: [f32; 4],
    view_projection: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuSplat {
    center_radius: [f32; 4],
    color_opacity: [f32; 4],
    normal: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SplatCamera {
    view_projection: [[f32; 4]; 4],
    right: [f32; 4],
    up: [f32; 4],
    light: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuObject {
    meta: [i32; 4],
    color: [f32; 4],
    inverse_rows: [[f32; 4]; 3],
    group_inverse_rows: [[f32; 4]; 3],
    params: [f32; 4],
    repeat_spacing: [f32; 4],
    repeat_count: [i32; 4],
    component: [u32; 4],
    scale: [f32; 4],
    modifier: [u32; 4],
    mirror_axes: [u32; 4],
    stencil_placement: [f32; 4],
    stencil_meta: [f32; 4],
    distance_bound: [f32; 4],
    operand_tree: [u32; 4],
    box_depth_meta: [u32; 4],
    box_depth_min: [f32; 4],
    box_depth_max: [f32; 4],
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

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuPolygonPoint {
    position: [f32; 2],
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
    fast_pipeline: wgpu::RenderPipeline,
    hybrid_pipeline: Option<wgpu::RenderPipeline>,
    hybrid_fast_pipeline: Option<wgpu::RenderPipeline>,
    hybrid_depth_pipeline: Option<wgpu::RenderPipeline>,
    deferred_geometry_pipeline: Option<(u32, bool, wgpu::RenderPipeline)>,
    deferred_supported: bool,
    splat_pipeline: wgpu::RenderPipeline,
    splat_camera_buffer: wgpu::Buffer,
    splat_camera_bind_group: wgpu::BindGroup,
    splat_instances: Vec<GpuSplat>,
    splat_buffer: wgpu::Buffer,
    splat_buffer_capacity: usize,
    hybrid_enabled: bool,
    boolean_pipeline: Option<(u32, wgpu::RenderPipeline)>,
    fast_boolean_pipeline: Option<(u32, wgpu::RenderPipeline)>,
    shader_source: String,
    custom_material_sources: Vec<(uuid::Uuid, String)>,
    pipeline_layout: wgpu::PipelineLayout,
    use_bvh: bool,
    node_count: u32,
    uploaded_object_count: u32,
    has_booleans: bool,
    scene_shader_features: SceneShaderFeatures,
    scene_pipelines_dirty: bool,
    bind_group: wgpu::BindGroup,
    bind_group_layout: wgpu::BindGroupLayout,
    camera_buffer: wgpu::Buffer,
    objects_buffer: wgpu::Buffer,
    bvh_buffer: wgpu::Buffer,
    material_headers_buffer: wgpu::Buffer,
    material_params_buffer: wgpu::Buffer,
    polygon_points_buffer: wgpu::Buffer,
    box_depth_buffer: wgpu::Buffer,
    lattice_points_buffer: wgpu::Buffer,
    lattice_atlas: wgpu::Texture,
    lattice_atlas_rows: u32,
    lattice_sampler: wgpu::Sampler,
    image_atlas: wgpu::Texture,
    image_sampler: wgpu::Sampler,
    modifier_params_buffer: wgpu::Buffer,
    uploaded_scene_versions: [i32; 2],
    neural_jobs: neural_jobs::NeuralJobs,
    group_capture_cache: std::collections::HashMap<uuid::Uuid, group_capture::CachedGroupCapture>,
    egui_renderer: egui_wgpu::Renderer,
    viewport: crate::viewport::Viewport,
    post_processing: post_processing::PostProcessor,
    initial_pixel_budget: u32,
    material_preview_ids: Option<MaterialPreviewIds>,
    material_preview_pipeline: Option<wgpu::RenderPipeline>,
    material_preview_textures: Vec<wgpu::Texture>,
    material_asset_previews: Vec<MaterialAssetPreview>,
    capture_result: Arc<std::sync::Mutex<Option<Result<CapturedFrame, String>>>>,
    capture_pending: bool,
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
    pub diagnostic: egui::TextureId,
    pub brick: egui::TextureId,
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
            MaterialKind::Diagnostic => self.diagnostic,
            MaterialKind::Brick => self.brick,
            MaterialKind::Wood => match material.wood.species {
                WoodSpecies::Oak => self.oak,
                WoodSpecies::Walnut => self.walnut,
                WoodSpecies::Pine => self.pine,
                WoodSpecies::Maple => self.maple,
            },
            MaterialKind::Custom => self.solid,
        }
    }

    pub fn for_asset(&self, id: uuid::Uuid) -> Option<egui::TextureId> {
        self.assets
            .iter()
            .find_map(|(asset_id, texture)| (*asset_id == id).then_some(*texture))
    }
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Clone)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

mod box_depth_atlas;
mod bvh;
mod initialization;
mod material_gpu;
mod sphere_depth_atlas;
pub(crate) use material_gpu::validate_custom_materials;
mod atlas_upload;
mod group_capture;
mod neural_jobs;
mod neural_sdf;
pub(crate) use neural_jobs::NeuralStatus;
mod hybrid_splats;
mod material_previews;
mod modifier_gpu;
mod primitive_upload;
mod rendering;
mod scene_bounds;
mod scene_pipelines;
mod scene_upload;
mod splat_bvh;
mod texture_cleanup;

use box_depth_atlas::*;
use bvh::*;
use scene_bounds::*;
use sphere_depth_atlas::*;

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

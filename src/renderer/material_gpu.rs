use super::*;
use std::collections::HashSet;
mod fabric;
use fabric::pack_fabric;

// One header per distinct material value. Vec4 slots have 16-byte alignment in WGSL.
pub(super) const MAX_PARAM_SLOTS: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct GpuMaterialHeader {
    kind: u32,
    offset: u32,
    length: u32,
    reserved: u32,
}

#[derive(Default)]
pub(super) struct PackedMaterials {
    pub materials: Vec<Material>,
    custom_indices: Vec<u32>,
    pub headers: Vec<GpuMaterialHeader>,
    pub params: Vec<[f32; 4]>,
}

impl PackedMaterials {
    #[cfg(test)]
    pub fn insert(&mut self, material: Material) -> u32 {
        self.insert_custom(material, 0)
    }

    pub fn insert_custom(&mut self, material: Material, custom_index: u32) -> u32 {
        if let Some(index) = self
            .materials
            .iter()
            .enumerate()
            .position(|(index, candidate)| {
                *candidate == material && self.custom_indices[index] == custom_index
            })
        {
            return index as u32;
        }
        let index = self.materials.len() as u32;
        let offset = self.params.len() as u32;
        self.params.push([
            material.roughness,
            material.metallic,
            material.reflectivity,
            material.opacity,
        ]);
        self.params.push([material.refractive_index, 0.0, 0.0, 0.0]);
        match material.kind {
            MaterialKind::Wood => pack_wood(material, &mut self.params),
            MaterialKind::Fabric => pack_fabric(material, &mut self.params),
            MaterialKind::Brick => self.params.extend_from_slice(&[
                [
                    material.brick.width,
                    material.brick.course_height,
                    material.brick.mortar_width,
                    material.brick.wear,
                ],
                [
                    material.brick.relief,
                    material.brick.bevel,
                    material.brick.porosity,
                    material.brick.firing,
                ],
                [
                    material.brick.mortar_color.x,
                    material.brick.mortar_color.y,
                    material.brick.mortar_color.z,
                    material.brick.efflorescence,
                ],
            ]),
            MaterialKind::Diagnostic => self.params.push([8.0, 0.96, 0.35, 0.1]),
            _ => {}
        }
        self.headers.push(GpuMaterialHeader {
            kind: material.kind.gpu_code(),
            offset,
            length: self.params.len() as u32 - offset,
            reserved: custom_index,
        });
        self.materials.push(material);
        self.custom_indices.push(custom_index);
        index
    }
}

fn pack_wood(material: Material, params: &mut Vec<[f32; 4]>) {
    let wood = material.wood;
    params.extend_from_slice(&[
        [
            wood.ring_spacing,
            wood.ring_contrast,
            wood.pores,
            wood.figure,
        ],
        [0.0, 0.0, 0.0, wood.coat_amber],
        [
            wood.cut_angle,
            wood.ring_relief,
            wood.ring_variation,
            wood.bump,
        ],
        [
            wood.fiber_relief,
            wood.fiber_pigment,
            wood.fiber_directionality,
            wood.scale_falloff,
        ],
        [
            wood.sanding_grit,
            wood.sanding_angle,
            wood.knots,
            wood.end_checks,
        ],
        [
            wood.stain_color.gpu_code(),
            wood.stain_load,
            wood.coat,
            wood.coat_sheen,
        ],
    ]);
}

// Explicit local assembler: declaration order is stable; WGSL functions may call later ones.
pub(super) fn shader_source() -> String {
    shader_source_for_assets(&[])
}

pub(super) fn shader_source_for_assets(assets: &[MaterialAsset]) -> String {
    let mut cases = String::new();
    let mut functions = String::new();
    for (index, asset) in assets
        .iter()
        .filter(|asset| asset.material.kind == MaterialKind::Custom)
        .enumerate()
    {
        let shader_index = index + 1;
        cases.push_str(&format!("case {shader_index}u: {{ return custom_material_{shader_index}(point, normal, view, base); }}\n"));
        if let Some(body) = &asset.wgsl {
            functions.push_str(&format!("fn custom_material_{shader_index}(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, base: Surface) -> Surface {{\n{body}\n}}\n"));
        }
    }
    let common = include_str!("../../assets/shaders/material_common.wgsl")
        .replace("// CUSTOM_MATERIAL_CASES", &cases);
    super::modifier_gpu::shader_source()
        .replace(
            "// DEFERRED_GEOMETRY_MODULE",
            include_str!("../../assets/shaders/deferred_geometry.wgsl"),
        )
        .replace(
            "// TEXT_MODULE",
            include_str!("../../assets/shaders/text.wgsl"),
        )
        .replace(
            "// MATERIAL_MODULES",
            &[
                &common,
                include_str!("../../assets/shaders/material_wood.wgsl"),
                include_str!("../../assets/shaders/material_fabric.wgsl"),
                include_str!("../../assets/shaders/material_brick.wgsl"),
                include_str!("../../assets/shaders/material_diagnostic.wgsl"),
                include_str!("../../assets/shaders/ambient_occlusion.wgsl"),
                &functions,
            ]
            .join("\n"),
        )
}

pub(crate) fn validate_custom_materials(assets: &[MaterialAsset]) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut count = 0;
    for asset in assets {
        if !ids.insert(asset.uuid) {
            return Err(format!("duplicate material id {}", asset.uuid));
        }
        if asset.material.kind == MaterialKind::Custom {
            count += 1;
            let body = asset
                .wgsl
                .as_deref()
                .ok_or_else(|| format!("{} has no WGSL body", asset.name))?;
            if body.trim().is_empty() || body.len() > 8192 {
                return Err(format!(
                    "{} needs a WGSL body of at most 8192 bytes",
                    asset.name
                ));
            }
            validate_custom_body(body).map_err(|error| format!("{}: {error}", asset.name))?;
        } else if asset.wgsl.is_some() {
            return Err(format!(
                "{} has WGSL but is not a custom material",
                asset.name
            ));
        }
    }
    if count > 16 {
        return Err("a scene may contain at most 16 custom WGSL materials".into());
    }
    if count == 0 {
        return Ok(());
    }
    let source = shader_source_for_assets(assets);
    let module = wgpu::naga::front::wgsl::parse_str(&source)
        .map_err(|error| error.emit_to_string(&source))?;
    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| format!("WGSL validation: {error}"))?;
    Ok(())
}

fn validate_custom_body(body: &str) -> Result<(), String> {
    let source = format!(
        "struct Surface {{ color: vec3<f32>, normal: vec3<f32>, roughness: f32, metallic: f32, reflectivity: f32, opacity: f32, ior: f32, coat: f32, sheen: f32, fiber: vec3<f32>, figure: f32, }}\nfn custom_material(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, base: Surface) -> Surface {{\n{body}\n}}"
    );
    let module = wgpu::naga::front::wgsl::parse_str(&source)
        .map_err(|error| error.emit_to_string(&source))?;
    if module.functions.len() != 1
        || !module.global_variables.is_empty()
        || !module.entry_points.is_empty()
        || !module.overrides.is_empty()
    {
        return Err("WGSL must contain only the body of one material function".into());
    }
    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| format!("WGSL validation: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembled_shader_is_valid_wgsl() {
        let source = shader_source();
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("validate assembled SDF shader");
    }

    #[test]
    fn shared_materials_use_one_record() {
        let mut packed = PackedMaterials::default();
        let wood = Material::preset(MaterialKind::Wood);
        assert_eq!(packed.insert(wood), packed.insert(wood));
        assert_eq!(packed.headers.len(), 1);
        assert_eq!(packed.params.len(), 8);
        assert_eq!(packed.insert(Material::preset(MaterialKind::Diagnostic)), 1);
        assert_eq!(packed.params.len(), 11);
        assert_eq!(packed.insert(Material::preset(MaterialKind::Brick)), 2);
        assert_eq!(packed.params.len(), 16);
        assert_eq!(std::mem::size_of::<GpuObject>(), 352);
        assert_eq!(std::mem::size_of::<GpuMaterialHeader>(), 16);
    }

    #[test]
    fn fabric_presets_fit_packed_record_and_keep_distinct_settings() {
        use crate::model::FabricPreset;
        let mut packed = PackedMaterials::default();
        for preset in FabricPreset::ALL {
            let material = Material::fabric_preset(preset);
            let index = packed.insert(material) as usize;
            assert_eq!(packed.headers[index].kind, 7);
            assert_eq!(packed.headers[index].length, 7);
            assert!(packed.headers[index].length as usize <= MAX_PARAM_SLOTS);
            assert_eq!(material.display_name(), preset.label());
        }
        assert_eq!(packed.materials.len(), 10);
    }

    #[test]
    fn custom_materials_compile_and_keep_asset_dispatch_distinct() {
        let first = MaterialAsset::custom("Bands".into());
        let mut second = MaterialAsset::custom("Dots".into());
        second.wgsl = Some(
            "var surface = base; surface.color = vec3<f32>(1.0, 0.0, 0.0); return surface;".into(),
        );
        validate_custom_materials(&[first.clone(), second.clone()]).unwrap();
        let source = shader_source_for_assets(&[first.clone(), second.clone()]);
        assert!(source.contains("case 1u: { return custom_material_1"));
        assert!(source.contains("case 2u: { return custom_material_2"));
        let mut packed = PackedMaterials::default();
        assert_ne!(
            packed.insert_custom(first.material, 1),
            packed.insert_custom(second.material, 2)
        );
        assert_eq!(packed.headers[0].reserved, 1);
        assert_eq!(packed.headers[1].reserved, 2);

        let mut invalid = second;
        invalid.wgsl = Some("surface.color = ;".into());
        assert!(validate_custom_materials(&[first, invalid]).is_err());
        let mut injected = MaterialAsset::custom("Injection".into());
        injected.wgsl = Some("return base; } @group(0) @binding(12) var<storage> extra: array<f32>; fn other() -> Surface { return base;".into());
        assert!(validate_custom_materials(&[injected]).is_err());
    }
}

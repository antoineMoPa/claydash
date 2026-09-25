use super::*;

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
    pub headers: Vec<GpuMaterialHeader>,
    pub params: Vec<[f32; 4]>,
}

impl PackedMaterials {
    pub fn insert(&mut self, material: Material) -> u32 {
        if let Some(index) = self
            .materials
            .iter()
            .position(|candidate| *candidate == material)
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
            reserved: 0,
        });
        self.materials.push(material);
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
    super::modifier_gpu::shader_source().replace(
        "// MATERIAL_MODULES",
        &[
            include_str!("../../assets/shaders/material_common.wgsl"),
            include_str!("../../assets/shaders/material_wood.wgsl"),
            include_str!("../../assets/shaders/material_brick.wgsl"),
            include_str!("../../assets/shaders/material_diagnostic.wgsl"),
            include_str!("../../assets/shaders/ambient_occlusion.wgsl"),
        ]
        .join("\n"),
    )
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
        assert_eq!(std::mem::size_of::<GpuObject>(), 240);
        assert_eq!(std::mem::size_of::<GpuMaterialHeader>(), 16);
    }
}

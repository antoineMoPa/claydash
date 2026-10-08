use super::*;

pub(crate) struct PackedTrainingScene {
    pub objects: Vec<GpuObject>,
    pub bvh: Vec<GpuBvhNode>,
    pub polygon_points: Vec<GpuPolygonPoint>,
    pub material_headers: Vec<material_gpu::GpuMaterialHeader>,
    pub material_params: Vec<[f32; 4]>,
    pub lattice_points: Vec<[f32; 4]>,
    pub modifier_params: Vec<[f32; 4]>,
    pub lattice_tiles: Vec<(u32, u32, Vec<Vec3>)>,
    pub ids: Vec<uuid::Uuid>,
    pub start: u32,
    pub root: u32,
    pub capacity: u32,
}

impl Renderer {
    pub(super) fn pack_neural_training_scene(
        &mut self,
        camera: &Camera,
        source: &[SdfObject],
        root: uuid::Uuid,
    ) -> Result<PackedTrainingScene, &'static str> {
        let mut exact = source.to_vec();
        for object in &mut exact {
            object.render_representation = crate::model::GroupRenderRepresentation::ExactSdf;
        }
        let mut packed = self
            .upload_scene_with_world_exposure(
                camera,
                &exact,
                &[],
                [i32::MIN; 2],
                1.0,
                World::default(),
                ScenePipelinePreparation::NeuralTraining,
            )
            .ok_or("Could not prepare GPU training scene")?;
        let root_index = packed
            .ids
            .iter()
            .position(|id| *id == root)
            .ok_or("Missing training root")?;
        let descendants: std::collections::HashSet<_> = source
            .iter()
            .filter(|object| {
                let mut id = Some(object.uuid);
                for _ in 0..source.len() {
                    if id == Some(root) {
                        return true;
                    }
                    id = id
                        .and_then(|id| source.iter().find(|object| object.uuid == id))
                        .and_then(|object| object.boolean_parent);
                }
                false
            })
            .map(|object| object.uuid)
            .collect();
        let start = packed
            .ids
            .iter()
            .position(|id| descendants.contains(id))
            .ok_or("Missing training subtree")?;
        let capacity = mark_component_evaluation(&mut packed.objects, start, root_index);
        packed.start = start as u32;
        packed.root = root_index as u32;
        packed.capacity = packed.capacity.max(capacity);
        Ok(packed)
    }
}

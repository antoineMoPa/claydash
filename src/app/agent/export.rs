use super::super::mesh_export_options::{ExportGeometry, ExportOptions};
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportGlbArgs {
    path: PathBuf,
    #[serde(default)]
    geometry: ExportGeometry,
    voxel_resolution: Option<u32>,
    object_ids: Option<Vec<uuid::Uuid>>,
    #[serde(default)]
    overwrite: bool,
}

impl App {
    pub(super) fn agent_export_glb(&mut self, args: ExportGlbArgs) -> AgentResult {
        if args.object_ids.as_ref().is_some_and(Vec::is_empty) {
            return Err("object_ids must contain at least one object; omit it to use the current selection or whole scene".into());
        }
        if let Some(resolution) = args.voxel_resolution {
            if args.geometry != ExportGeometry::Voxels {
                return Err("voxel_resolution is only available with geometry: voxels; current_representation uses each object's saved resolution".into());
            }
            if !(model::VoxelSettings::MIN_RESOLUTION..=model::VoxelSettings::MAX_RESOLUTION)
                .contains(&resolution)
            {
                return Err("voxel_resolution must be between 8 and 128".into());
            }
        }
        let options = ExportOptions {
            geometry: args.geometry,
            voxel_resolution: args
                .voxel_resolution
                .unwrap_or_else(|| model::VoxelSettings::default().resolution),
        };
        let selection = args
            .object_ids
            .unwrap_or_else(|| commands::effective_selected_ids(&self.tree));
        let id = self.start_mesh_export_to(options, selection, args.path, args.overwrite)?;
        self.glb_export_status(id)
    }
}

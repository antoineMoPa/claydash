use super::super::mesh_export_options::{ExportGeometry, ExportOptions};
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportGlbArgs {
    path: PathBuf,
    #[serde(default)]
    geometry: ExportGeometry,
    voxel_resolution: Option<u32>,
    direct_resolution: Option<u32>,
    direct_error: Option<f32>,
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
        if let Some(resolution) = args.direct_resolution {
            if args.geometry != ExportGeometry::Direct {
                return Err("direct_resolution is only available with geometry: direct".into());
            }
            if !(crate::renderer::direct_mesh::MIN_RESOLUTION..=crate::renderer::direct_mesh::MAX_RESOLUTION).contains(&resolution) {
                return Err("direct_resolution must be between 8 and 256".into());
            }
        }
        if let Some(error)=args.direct_error {
            if args.geometry!=ExportGeometry::Direct || !error.is_finite() || error<=0.0 {
                return Err("direct_error requires geometry: direct and a positive finite world-space distance".into());
            }
        }
        let options = ExportOptions {
            direct_error: args.direct_error,
            direct_resolution: args.direct_resolution.unwrap_or(crate::renderer::direct_mesh::DEFAULT_RESOLUTION),
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

#[cfg(all(test,unix))]
mod tests {
    use super::*;
    #[test]
    fn direct_solid_export_needs_no_gpu_and_keeps_source_and_pbr() {
        let mut app=App::new();
        let mut object=model::SdfObject::create_kind(model::PrimitiveKind::Box);
        if let model::SdfParams::BoxParams(p)=&mut object.params {p.box_q=Vec3::new(0.045,0.135,0.095);p.corner_radius=0.04;}
        object.color=glam::Vec4::new(0.8,0.1,0.03,1.0);object.material.metallic=0.6;object.material.roughness=0.25;
        model::set_objects(&mut app.tree,vec![object.clone()]);
        let source=crate::document::serialize_scene(&app.tree).unwrap();
        let path=std::env::temp_dir().join(format!("claydash-solid-{}.glb",uuid::Uuid::new_v4()));
        let id=app.start_mesh_export_to(ExportOptions{geometry:ExportGeometry::Direct,direct_resolution:64,direct_error:Some(0.0005),..Default::default()},vec![object.uuid],path.clone(),false).unwrap();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        loop {app.poll_mesh_export();let state=app.glb_export_status(id).unwrap();if state["status"]!="running"{assert_eq!(state["status"],"completed","{state}");break;}assert!(std::time::Instant::now()<deadline);std::thread::sleep(std::time::Duration::from_millis(1));}
        let data=std::fs::read(&path).unwrap();std::fs::remove_file(path).unwrap();assert!(data.len()<100_000);
        let n=u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;let doc:serde_json::Value=serde_json::from_slice(&data[20..20+n]).unwrap();
        assert!(doc["images"].as_array().unwrap().is_empty());
        assert_eq!(doc["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"],0.25);
        assert!((doc["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"][0].as_f64().unwrap()-0.8).abs()<1e-6);
        assert_eq!(crate::document::serialize_scene(&app.tree).unwrap(),source);
    }
}

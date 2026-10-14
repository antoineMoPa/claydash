use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExportGeometry {
    #[default]
    CurrentRepresentation,
    Smooth,
    Direct,
    Voxels,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MeshKind {
    Smooth,
    Direct { resolution: u32 },
    Voxels { resolution: u32 },
}

#[derive(Clone, Copy)]
pub(super) struct ExportOptions {
    pub geometry: ExportGeometry,
    pub voxel_resolution: u32,
    pub direct_resolution: u32,
    pub direct_error: Option<f32>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            geometry: ExportGeometry::default(),
            direct_resolution: crate::renderer::direct_mesh::DEFAULT_RESOLUTION,
            direct_error: None,
            voxel_resolution: crate::model::VoxelSettings::default().resolution,
        }
    }
}

impl ExportOptions {
    pub fn mesh_kind(self, object: &crate::model::SdfObject) -> MeshKind {
        match self.geometry {
            ExportGeometry::Direct => MeshKind::Direct { resolution: self.direct_resolution },
            ExportGeometry::CurrentRepresentation if object.render_representation == crate::model::GroupRenderRepresentation::DirectMesh => MeshKind::Direct { resolution: object.poisson_mesh.resolution },
            ExportGeometry::Voxels => MeshKind::Voxels {
                resolution: self.voxel_resolution,
            },
            ExportGeometry::CurrentRepresentation
                if object.render_representation
                    == crate::model::GroupRenderRepresentation::Voxels =>
            {
                MeshKind::Voxels {
                    resolution: object.voxels.resolution,
                }
            }
            ExportGeometry::CurrentRepresentation | ExportGeometry::Smooth => MeshKind::Smooth,
        }
    }
}

impl ExportGeometry {
    fn label(self) -> &'static str {
        match self {
            Self::CurrentRepresentation => "Current representation",
            Self::Direct => "Direct mesh",
            Self::Smooth => "Smooth mesh (Poisson)",
            Self::Voxels => "Voxels",
        }
    }
}

impl App {
    pub(super) fn request_mesh_export(&mut self) {
        if self.mesh_export.is_none() {
            self.mesh_export_options = Some(ExportOptions::default());
        }
    }

    pub(super) fn mesh_export_options_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut options) = self.mesh_export_options else {
            return;
        };
        let mut export = false;
        let mut cancel = false;
        let response = egui::Modal::new("mesh-export-options".into()).show(ui.ctx(), |ui| {
            ui.set_min_width(340.0);
            ui.heading("Export GLB");
            ui.horizontal(|ui| {
                ui.label("Geometry");
                egui::ComboBox::from_id_salt("export-geometry")
                    .selected_text(options.geometry.label()).show_ui(ui, |ui| {
                        for geometry in [ExportGeometry::CurrentRepresentation, ExportGeometry::Smooth, ExportGeometry::Direct, ExportGeometry::Voxels] {
                            ui.selectable_value(&mut options.geometry, geometry, geometry.label());
                        }
                    });
            });
            match options.geometry {
                ExportGeometry::CurrentRepresentation => {
                    ui.label("Voxel and direct mesh objects keep their saved representation and resolution. Other objects export as smooth meshes.");
                }
                ExportGeometry::Direct => {
                    ui.label("Adaptive source geometry; solid colors retain indexed PBR materials.");
                    let mut error_mm=options.direct_error.unwrap_or(0.032/options.direct_resolution as f32)*1000.0;
                    ui.horizontal(|ui| { ui.label("Maximum surface error (mm)");
                        if ui.add(egui::DragValue::new(&mut error_mm).range(0.01..=100.0).speed(0.1)).changed() { options.direct_error=Some(error_mm/1000.0); }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Field resolution");
                        ui.add(egui::DragValue::new(&mut options.direct_resolution).range(crate::renderer::direct_mesh::MIN_RESOLUTION..=crate::renderer::direct_mesh::MAX_RESOLUTION));
                    });
                }
                ExportGeometry::Smooth => { ui.label("Reconstruct smooth surfaces with baked materials."); }
                ExportGeometry::Voxels => {
                    ui.label("Export surface cubes with flat faces and sampled colors.");
                    ui.horizontal(|ui| {
                        ui.label("Voxel resolution");
                        ui.add(egui::DragValue::new(&mut options.voxel_resolution)
                            .range(crate::model::VoxelSettings::MIN_RESOLUTION..=crate::model::VoxelSettings::MAX_RESOLUTION));
                    });
                }
            }
            ui.weak("Exports the selection, or the whole scene when nothing is selected.");
            ui.horizontal(|ui| {
                export = ui.button("Export…").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if export || cancel || response.should_close() {
            self.mesh_export_options = None;
            if export {
                self.start_mesh_export(options);
            }
        } else {
            self.mesh_export_options = Some(options);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_choices_map_each_root_without_changing_the_scene() {
        let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        object.render_representation = crate::model::GroupRenderRepresentation::Voxels;
        object.voxels.resolution = 16;
        let mut options = ExportOptions::default();
        assert_eq!(
            options.mesh_kind(&object),
            MeshKind::Voxels { resolution: 16 }
        );
        options.geometry = ExportGeometry::Smooth;
        assert_eq!(options.mesh_kind(&object), MeshKind::Smooth);
        options.geometry = ExportGeometry::Direct;
        options.direct_resolution = 24;
        assert_eq!(options.mesh_kind(&object), MeshKind::Direct { resolution: 24 });
        options.geometry = ExportGeometry::Voxels;
        options.voxel_resolution = 48;
        assert_eq!(
            options.mesh_kind(&object),
            MeshKind::Voxels { resolution: 48 }
        );
        assert_eq!(object.voxels.resolution, 16);
        options.geometry = ExportGeometry::CurrentRepresentation;
        for mode in crate::model::GroupRenderRepresentation::ALL {
            object.render_representation = mode;
            let expected = if mode == crate::model::GroupRenderRepresentation::Voxels {
                MeshKind::Voxels { resolution: 16 }
            } else if mode == crate::model::GroupRenderRepresentation::DirectMesh {
                MeshKind::Direct { resolution: object.poisson_mesh.resolution }
            } else {
                MeshKind::Smooth
            };
            assert_eq!(options.mesh_kind(&object), expected);
        }
    }
}

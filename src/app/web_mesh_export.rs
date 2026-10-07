use super::*;
use crate::renderer::{computation::{Computation, Stage}, poisson_mesh::{geometry::Mesh,
    textured_glb::{Page, PAGE_TRIANGLES}, web_worker::{WebJob, WebBytesJob}}};
use std::sync::atomic::Ordering;
use crate::renderer::poisson_mesh::BuildProgress;

pub(super) struct MeshExportJob {
    source: Vec<crate::model::SdfObject>,
    roots: Vec<uuid::Uuid>,
    index: usize,
    worker: Option<WebJob>,
    pages: Vec<Page>,
    texture_index: usize,
    texture: Option<crate::renderer::poisson_bake::TextureReadback>,
    png_job: Option<WebBytesJob>,
    glb_job: Option<WebBytesJob>,
    work: Computation,
    assets: Vec<crate::model::MaterialAsset>,
    camera: Camera,
    world: crate::model::World,
}

impl MeshExportJob {
    fn add_mesh(&mut self, root: uuid::Uuid, mesh: Mesh) {
        let name = self.source.iter().find(|object| object.uuid == root)
            .map_or_else(|| "Mesh".to_owned(), |object| object.name.clone());
        let mesh = Arc::new(mesh);
        for first in (0..mesh.triangles.len()).step_by(PAGE_TRIANGLES) {
            let count = (mesh.triangles.len() - first).min(PAGE_TRIANGLES);
            self.pages.push(Page { name: name.clone(), root, mesh: mesh.clone(), first, count,
                size: crate::renderer::poisson_mesh::textured_glb::page_size(count), png: Vec::new() });
        }
        self.index += 1;
        self.work.progress.store(0, Ordering::Relaxed);
    }

    fn status(&self) -> String {
        if self.index < self.roots.len() {
            let name = self.source.iter().find(|object| object.uuid == self.roots[self.index])
                .map_or("Mesh", |object| object.name.as_str());
            if let Some(worker) = &self.worker {
                match BuildProgress::from_code(worker.progress()) {
                    BuildProgress::Sampling(percent) => format!("Capturing {name} ({}/{}) · {percent}%", self.index + 1, self.roots.len()),
                    BuildProgress::Reconstruction(percent) => format!("Reconstructing {name} ({}/{}) · {percent}%", self.index + 1, self.roots.len()),
                    BuildProgress::Complete => format!("Preparing {name} ({}/{})", self.index + 1, self.roots.len()),
                }
            } else { format!("Preparing {name} ({}/{})", self.index + 1, self.roots.len()) }
        } else if self.texture_index < self.pages.len() {
            format!("Baking materials · texture {}/{}", self.texture_index + 1, self.pages.len())
        } else { "Preparing Glb download…".to_owned() }
    }
}

impl App {
    pub(super) fn cancel_mesh_export_for_document_change(&mut self) {
        self.mesh_export = None;
    }

    pub(super) fn start_mesh_export(&mut self) {
        if self.mesh_export.is_some() { return; }
        let source = crate::model::objects(&self.tree);
        let selection = commands::effective_selected_ids(&self.tree);
        let roots = crate::renderer::poisson_mesh::geometry::export_roots(&source, &selection);
        if roots.is_empty() {
            self.document.set_error("export Glb", "Select an object or add objects to the scene");
            return;
        }
        self.mesh_export = Some(MeshExportJob { source, roots, index: 0, worker: None,
            pages: Vec::new(), texture_index: 0, texture: None,
            png_job: None, glb_job: None, work: Computation::new(Stage::Sampling),
            assets: crate::model::material_assets(&self.tree), camera: self.camera.clone(),
            world: crate::model::world(&self.tree) });
        self.egui.request_repaint();
    }

    pub(super) fn poll_mesh_export(&mut self) {
        let Some(job) = self.mesh_export.as_mut() else { return };
        let mut finish: Option<Result<(), String>> = None;
        if job.index < job.roots.len() {
            let root = job.roots[job.index];
            if let Some(worker) = &job.worker {
                job.work.progress.store(worker.progress(), Ordering::Relaxed);
                if let Some(result) = worker.take_result() {
                    job.worker = None;
                    match result {
                        Ok(mesh) => job.add_mesh(root, mesh),
                        Err(error) => finish = Some(Err(error)),
                    }
                }
            } else {
                let revision = super::rendering::mesh_source_revision(&job.source);
                let cached = self.renderer.as_ref().and_then(|renderer|
                    renderer.cached_mesh_for_export(&job.source, root, revision));
                if let Some(cached) = cached {
                    match cached.world_mesh(&std::sync::atomic::AtomicBool::new(false)) {
                        Ok(mesh) => job.add_mesh(root, mesh),
                        Err(error) => finish = Some(Err(error)),
                    }
                } else {
                    let resolution = job.source.iter().find(|object| object.uuid == root)
                        .map_or(0, |object| object.gaussian_splats.resolution);
                    match WebJob::start(job.source.clone(), root, resolution, &self.egui) {
                        Ok(worker) => job.worker = Some(worker),
                        Err(error) => finish = Some(Err(error)),
                    }
                }
            }
        } else if job.pages.is_empty() {
            finish = Some(Err("There are no meshes to export".to_owned()));
        } else if job.texture_index < job.pages.len() {
            let Some(renderer) = self.renderer.as_mut() else {
                self.mesh_export = None;
                self.document.set_error("export Glb", "Renderer is unavailable");
                return;
            };
            if let Some(png_job) = &job.png_job {
                if let Some(result) = png_job.take_result() {
                    job.png_job = None;
                    match result {
                        Ok(png) => {
                            job.pages[job.texture_index].png = png;
                            job.texture_index += 1;
                        }
                        Err(error) => finish = Some(Err(error)),
                    }
                }
            } else if let Some(texture) = &job.texture {
                renderer.poll_poisson_bake();
                if let Some(result) = texture.poll() {
                    match result {
                        Ok(pixels) => match WebBytesJob::png(pixels.bytes, pixels.size, &self.egui) {
                            Ok(encoder) => { job.texture = None; job.png_job = Some(encoder); }
                            Err(error) => finish = Some(Err(error)),
                        },
                        Err(error) => finish = Some(Err(error)),
                    }
                }
            } else {
                match renderer.bake_poisson_page(&job.source, &job.assets, &job.camera,
                    job.world, &job.pages[job.texture_index]) {
                    Ok(texture) => job.texture = Some(texture),
                    Err(error) => finish = Some(Err(error)),
                }
            }
        } else if let Some(glb_job) = &job.glb_job {
            if let Some(result) = glb_job.take_result() {
                finish = Some(result.and_then(|bytes| crate::document::download_bytes("scene.glb", &bytes)));
            }
        } else {
            match WebBytesJob::glb(&job.pages, &self.egui) {
                Ok(encoder) => job.glb_job = Some(encoder),
                Err(error) => finish = Some(Err(error)),
            }
        }
        if let Some(result) = finish {
            self.mesh_export = None;
            if let Err(error) = result { self.document.set_error("export Glb", error); }
        } else {
            self.egui.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    pub(super) fn mesh_export_panel(&mut self, ui: &mut egui::Ui) {
        let Some(job) = &self.mesh_export else { return };
        let mut cancel = false;
        let snapshot = if job.index < job.roots.len() {
            if let Some(worker) = &job.worker {
                BuildProgress::from_code(worker.progress()).snapshot(&job.work)
            } else { job.work.snapshot(Stage::Preparation, None) }
        } else if job.texture_index < job.pages.len() {
            job.work.snapshot(Stage::TextureBake,
                Some((job.texture_index * 100 / job.pages.len()) as u32))
        } else { job.work.snapshot(Stage::Writing, None) };
        egui::Modal::new("mesh-export-progress".into()).show(ui.ctx(), |ui| {
            ui.set_min_width(300.0);
            ui.heading("Export Glb");
            ui.weak("Leaving this tab stops progress.");
            ui.label(job.status());
            ui.weak(snapshot.remaining_label());
            cancel = ui.button("Cancel").clicked();
        });
        if cancel { job.work.cancel(); self.mesh_export = None; }
    }
}

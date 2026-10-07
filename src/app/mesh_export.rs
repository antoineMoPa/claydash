use super::*;
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};
use crate::renderer::computation::{Computation, Stage};
use crate::renderer::poisson_mesh::textured_glb::{Page, PAGE_TRIANGLES};
use crate::renderer::poisson_mesh::BuildProgress;

enum ExportResult {
    Meshes(Vec<(uuid::Uuid, crate::renderer::poisson_mesh::glb::ObjectMesh)>),
    Written,
}

pub(super) struct MeshExportJob {
    completed: Option<Receiver<Result<ExportResult, String>>>,
    stage: Arc<Mutex<ExportStage>>,
    work: Computation,
    source: Vec<crate::model::SdfObject>,
    assets: Vec<crate::model::MaterialAsset>,
    camera: Camera,
    world: crate::model::World,
    path: std::path::PathBuf,
    pages: Vec<Page>,
    texture: Option<crate::renderer::poisson_bake::TextureReadback>,
    texture_index: usize,
}

enum ExportStage {
    PreparingMesh { name: String, index: usize, total: usize },
    Capturing { name: String, index: usize, total: usize },
    Baking { index: usize, total: usize },
    Writing,
}

impl App {
    pub(super) fn cancel_mesh_export_for_document_change(&mut self) {
        if let Some(job) = self.mesh_export.take() {
            job.work.cancel.store(true, Ordering::Relaxed);
        }
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
        let Some(path) = rfd::FileDialog::new().add_filter("glTF binary", &["glb"])
            .set_file_name("scene.glb").save_file() else { return };
        let revision = super::rendering::mesh_source_revision(&source);
        let cached: std::collections::HashMap<_, _> = roots.iter().filter_map(|root| {
            self.renderer.as_ref()?.cached_mesh_for_export(&source, *root, revision)
                .map(|mesh| (*root, mesh))
        }).collect();
        let stage = Arc::new(Mutex::new(ExportStage::Capturing {
            name: "Preparing".into(), index: 0, total: roots.len(),
        }));
        let work = Computation::new(Stage::Sampling);
        let (sender, completed) = channel();
        let worker_stage = stage.clone();
        let worker_progress = work.progress.clone();
        let worker_cancel = work.cancel.clone();
        let worker_source = source.clone();
        std::thread::spawn(move || {
            crate::renderer::cooperative_work::background_priority();
            let result = std::panic::catch_unwind(|| {
                let mut objects = Vec::with_capacity(roots.len());
                for (index, root) in roots.iter().enumerate() {
                    if worker_cancel.load(Ordering::Relaxed) { return Err("Export cancelled".into()); }
                    let object = worker_source.iter().find(|object| object.uuid == *root)
                        .ok_or("Selected object disappeared")?;
                    let mesh = if let Some(mesh) = cached.get(root) {
                        *worker_stage.lock().unwrap() = ExportStage::PreparingMesh {
                            name: object.name.clone(), index: index + 1, total: roots.len(),
                        };
                        mesh.world_mesh(&worker_cancel)?
                    } else {
                        *worker_stage.lock().unwrap() = ExportStage::Capturing {
                            name: object.name.clone(), index: index + 1, total: roots.len(),
                        };
                        worker_progress.store(0, Ordering::Relaxed);
                        crate::renderer::poisson_mesh::geometry::build(
                            &worker_source, *root, object.gaussian_splats.resolution,
                            None, &worker_progress, Some(&worker_cancel))
                            .map_err(|error| if worker_cancel.load(Ordering::Relaxed) {
                                "Export cancelled".to_owned()
                            } else { format!("{}: {error}", object.name) })?
                    };
                    if worker_cancel.load(Ordering::Relaxed) { return Err("Export cancelled".into()); }
                    objects.push((*root, crate::renderer::poisson_mesh::glb::ObjectMesh {
                        name: object.name.clone(), mesh,
                    }));
                }
                Ok(ExportResult::Meshes(objects))
            }).unwrap_or_else(|_| Err("Mesh export failed unexpectedly".into()));
            let _ = sender.send(result);
        });
        self.mesh_export = Some(MeshExportJob { completed: Some(completed), stage, work,
            source, assets: crate::model::material_assets(&self.tree),
            camera: self.camera.clone(), world: crate::model::world(&self.tree), path,
            pages: Vec::new(), texture: None, texture_index: 0 });
        self.egui.request_repaint();
    }

    pub(super) fn poll_mesh_export(&mut self) {
        let Some(job) = self.mesh_export.as_mut() else { return };
        let mut finish = None;
        if job.work.cancel.load(Ordering::Relaxed) {
            finish = Some(Err("Export cancelled".to_owned()));
        } else if let Some(receiver) = &job.completed {
            match receiver.try_recv() {
                Ok(Ok(ExportResult::Meshes(objects))) => {
                    job.completed = None;
                    for (root, object) in objects {
                        let mesh = Arc::new(object.mesh);
                        for first in (0..mesh.triangles.len()).step_by(PAGE_TRIANGLES) {
                            let count = (mesh.triangles.len() - first).min(PAGE_TRIANGLES);
                            job.pages.push(Page { name: object.name.clone(), root,
                                mesh: mesh.clone(), first, count,
                                size: crate::renderer::poisson_mesh::textured_glb::page_size(count),
                                png: Vec::new() });
                        }
                    }
                    if job.pages.is_empty() { finish = Some(Err("There are no meshes to export".into())); }
                }
                Ok(Ok(ExportResult::Written)) => finish = Some(Ok(())),
                Ok(Err(error)) => finish = Some(Err(error)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => finish = Some(Err("Export worker stopped unexpectedly".into())),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        } else if job.texture_index < job.pages.len() {
            *job.stage.lock().unwrap() = ExportStage::Baking { index: job.texture_index + 1,
                total: job.pages.len() };
            let Some(renderer) = &mut self.renderer else {
                finish = Some(Err("Renderer is unavailable".into()));
                self.finish_mesh_export(finish); return;
            };
            if let Some(texture) = &job.texture {
                renderer.poll_poisson_bake();
                if let Some(result) = texture.poll() {
                    match result {
                        Ok(png) => { job.pages[job.texture_index].png = png;
                            job.texture = None; job.texture_index += 1; }
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
        } else {
            *job.stage.lock().unwrap() = ExportStage::Writing;
            let pages = std::mem::take(&mut job.pages);
            let path = job.path.clone();
            let cancel = job.work.cancel.clone();
            let (sender, receiver) = channel();
            job.completed = Some(receiver);
            std::thread::spawn(move || {
                let result = crate::renderer::poisson_mesh::textured_glb::encode_textured(&pages)
                    .and_then(|bytes| write_glb(&path, &bytes, &cancel))
                    .map(|_| ExportResult::Written);
                let _ = sender.send(result);
            });
        }
        self.finish_mesh_export(finish);
    }

    fn finish_mesh_export(&mut self, finish: Option<Result<(), String>>) {
        if let Some(result) = finish {
            self.mesh_export = None;
            if let Err(error) = result {
                if error != "Export cancelled" { self.document.set_error("export Glb", error); }
            }
        } else {
            self.egui.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    pub(super) fn mesh_export_panel(&mut self, ui: &mut egui::Ui) {
        let Some(job) = &self.mesh_export else { return };
        let status = if job.work.cancel.load(Ordering::Relaxed) { "Cancelling…".to_owned() } else { match &*job.stage.lock().unwrap() {
            ExportStage::PreparingMesh { name, index, total } => format!("Preparing {name} ({index}/{total})"),
            ExportStage::Capturing { name, index, total } => {
                match BuildProgress::from_code(job.work.progress.load(Ordering::Relaxed)) {
                    BuildProgress::Sampling(percent) => format!("Capturing {name} ({index}/{total}) · {percent}%"),
                    BuildProgress::Reconstruction(percent) => format!("Reconstructing {name} ({index}/{total}) · {percent}%"),
                    BuildProgress::Complete => format!("Preparing {name} ({index}/{total})"),
                }
            }
            ExportStage::Baking { index, total } => format!("Baking materials · texture {index}/{total}"),
            ExportStage::Writing => "Writing Glb…".into(),
        }};
        let estimate = match &*job.stage.lock().unwrap() {
            ExportStage::PreparingMesh { .. } => job.work.snapshot(Stage::Preparation, None),
            ExportStage::Capturing { .. } =>
                BuildProgress::from_code(job.work.progress.load(Ordering::Relaxed)).snapshot(&job.work),
            ExportStage::Baking { index, total } => job.work.snapshot(Stage::TextureBake,
                Some((index.saturating_sub(1) * 100 / (*total).max(1)) as u32)),
            ExportStage::Writing => job.work.snapshot(Stage::Writing, None),
        };
        let mut cancel = false;
        egui::Modal::new("mesh-export-progress".into()).show(ui.ctx(), |ui| {
            ui.set_min_width(300.0);
            ui.heading("Export Glb");
            ui.label(status);
            ui.weak(estimate.remaining_label());
            cancel = ui.add_enabled(!job.work.cancel.load(Ordering::Relaxed), egui::Button::new("Cancel")).clicked();
        });
        if cancel { job.work.cancel.store(true, Ordering::Relaxed); }
    }
}

fn write_glb(path: &std::path::Path, bytes: &[u8], cancel: &AtomicBool) -> Result<(), String> {
    use std::io::Write;
    let temp = path.with_file_name(format!(".claydash-export-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Export cancelled"));
        }
        std::fs::rename(&temp, path)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temp); }
    result.map_err(|error: std::io::Error| error.to_string())
}

#[cfg(test)]
#[test]
fn changing_documents_cancels_an_export_before_it_can_reupload_the_old_scene() {
    let mut app = App::new();
    let work = Computation::new(Stage::Sampling);
    let cancel = work.cancel.clone();
    app.mesh_export = Some(MeshExportJob {
        completed: None,
        stage: Arc::new(Mutex::new(ExportStage::Writing)),
        work,
        source: crate::model::objects(&app.tree), assets: Vec::new(),
        camera: app.camera.clone(), world: crate::model::World::default(),
        path: std::path::PathBuf::from("unused.glb"), pages: Vec::new(),
        texture: None, texture_index: 0,
    });
    app.replace_scene(DataTree::default());
    assert!(cancel.load(Ordering::Relaxed));
    assert!(app.mesh_export.is_none());
}

#[cfg(test)]
#[test]
fn inactive_window_finishes_exports_without_a_viewport_redraw() {
    let mut app = App::new();
    app.window_focused = false;
    app.window_occluded = true;
    let (sender, receiver) = channel();
    app.mesh_export = Some(MeshExportJob {
        completed: Some(receiver),
        stage: Arc::new(Mutex::new(ExportStage::Writing)),
        work: Computation::new(Stage::Writing),
        source: Vec::new(), assets: Vec::new(), camera: app.camera.clone(),
        world: crate::model::World::default(), path: "unused.glb".into(),
        pages: Vec::new(), texture: None, texture_index: 0,
    });
    // Waiting jobs keep the background timer alive even with no window or GPU.
    assert!(app.advance_background_computations());
    sender.send(Ok(ExportResult::Written)).unwrap();
    assert!(!app.advance_background_computations());
    assert!(app.mesh_export.is_none());
}

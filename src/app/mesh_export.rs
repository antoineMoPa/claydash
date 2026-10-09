use super::*;
use super::mesh_export_options::{ExportOptions, MeshKind};
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};
use crate::renderer::computation::{Computation, Stage};
use crate::renderer::poisson_mesh::textured_glb::{Page, PAGE_TRIANGLES};
use crate::renderer::poisson_mesh::BuildProgress;

#[derive(serde::Serialize)]
pub(super) struct ExportRecord {
    id: uuid::Uuid,
    path: std::path::PathBuf,
    #[serde(flatten)]
    outcome: ExportOutcome,
}

#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ExportOutcome { Completed, Cancelled, Failed { error: String } }

#[derive(Default)]
enum Publication { #[default] Pending, Written, Cancelled }

enum ExportResult {
    Meshes(Vec<(uuid::Uuid, crate::renderer::poisson_mesh::glb::ObjectMesh, Option<Arc<Vec<[f32; 4]>>>)>),
    Written,
}

pub(super) struct MeshExportJob {
    id: uuid::Uuid,
    overwrite: bool,
    publication: Arc<Mutex<Publication>>,
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
            let outcome = job.cancel();
            self.record_mesh_export(job, outcome);
        }
    }

    fn record_mesh_export(&mut self, job: MeshExportJob, outcome: ExportOutcome) {
        self.mesh_export_history.push_back(ExportRecord { id: job.id, path: job.path, outcome });
        if self.mesh_export_history.len() > 32 { self.mesh_export_history.pop_front(); }
    }

    #[cfg(unix)]
    pub(super) fn glb_export_status(&self, id: uuid::Uuid) -> Result<serde_json::Value, String> {
        use serde_json::json;
        if let Some(job) = self.mesh_export.as_ref().filter(|job| job.id == id) {
            let (stage, name, index, total, percent) = match &*job.stage.lock().unwrap() {
                ExportStage::PreparingMesh { name, index, total } => ("preparing", Some(name.clone()), *index, *total, None),
                ExportStage::Capturing { name, index, total } => {
                    let (stage, percent) = match BuildProgress::from_code(job.work.progress.load(Ordering::Relaxed)) {
                        BuildProgress::Sampling(p) => ("sampling", p),
                        BuildProgress::Reconstruction(p) => ("reconstructing", p),
                        BuildProgress::Complete => ("preparing", 100),
                    };
                    (stage, Some(name.clone()), *index, *total, Some(percent))
                }
                ExportStage::Baking { index, total } => ("baking", None, *index, *total, None),
                ExportStage::Writing => ("writing", None, 0, 0, None),
            };
            return Ok(json!({"id":id,"path":job.path,"status":"running", "stage":stage,
                "object_name":name,"index":index,"total":total,"percent":percent}));
        }
        self.mesh_export_history.iter().find(|record| record.id == id)
            .ok_or_else(|| format!("unknown or expired GLB export: {id}"))
            .and_then(|record| serde_json::to_value(record).map_err(|error| error.to_string()))
    }

    #[cfg(unix)]
    pub(super) fn cancel_glb_export(&mut self, id: uuid::Uuid) -> Result<serde_json::Value, String> {
        if self.mesh_export.as_ref().is_some_and(|job| job.id == id) {
            self.cancel_mesh_export_for_document_change();
        }
        self.glb_export_status(id)
    }

    pub(super) fn start_mesh_export(&mut self, options: ExportOptions) {
        if self.mesh_export.is_some() { return; }
        let selection = commands::effective_selected_ids(&self.tree);
        if crate::renderer::poisson_mesh::geometry::export_roots(crate::model::objects_ref(&self.tree), &selection).is_empty() {
            self.document.set_error("export Glb", "Select an object or add objects to the scene");
            return;
        }
        let Some(path) = rfd::FileDialog::new().add_filter("glTF binary", &["glb"])
            .set_file_name("scene.glb").save_file() else { return };
        if let Err(error) = self.start_mesh_export_to(options, selection, path, true) {
            self.document.set_error("export Glb", error);
        }
    }

    pub(super) fn start_mesh_export_to(&mut self, options: ExportOptions,
        selection: Vec<uuid::Uuid>, path: std::path::PathBuf, overwrite: bool) -> Result<uuid::Uuid, String> {
        if self.mesh_export.is_some() { return Err("a GLB export is already in progress".into()); }
        let source = crate::model::objects(&self.tree);
        for id in &selection {
            if !source.iter().any(|object| object.uuid == *id) { return Err(format!("unknown object: {id}")); }
        }
        let roots = crate::renderer::poisson_mesh::geometry::export_roots(&source, &selection);
        if roots.is_empty() { return Err("Select an object or add objects to the scene".into()); }
        if !path.is_absolute() || path.file_name().is_none() { return Err("path must be an absolute file path".into()); }
        if !path.parent().is_some_and(|parent| parent.is_dir()) { return Err("output directory does not exist".into()); }
        if path.is_dir() { return Err("output path is a directory".into()); }
        if !overwrite && path.symlink_metadata().is_ok() { return Err("output file exists; set overwrite to true to replace it".into()); }
        let id = uuid::Uuid::new_v4();
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
                    let mut colors = None;
                    let mesh = if let MeshKind::Voxels { resolution } = options.mesh_kind(object) {
                        *worker_stage.lock().unwrap() = ExportStage::Capturing {
                            name: object.name.clone(), index: index + 1, total: roots.len(),
                        };
                        worker_progress.store(0, Ordering::Relaxed);
                        let geometry = crate::renderer::voxels::geometry::build(
                            &worker_source, *root, resolution, &worker_progress, Some(&worker_cancel))?
                            .into_world(&worker_source, *root, &worker_cancel)?;
                        colors = Some(Arc::new(geometry.colors));
                        geometry.mesh
                    } else if let Some(mesh) = cached.get(root) {
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
                            object.poisson_mesh.resolution,
                            None, &worker_progress, Some(&worker_cancel))
                            .map_err(|error| if worker_cancel.load(Ordering::Relaxed) {
                                "Export cancelled".to_owned()
                            } else { format!("{}: {error}", object.name) })?
                    };
                    if worker_cancel.load(Ordering::Relaxed) { return Err("Export cancelled".into()); }
                    objects.push((*root, crate::renderer::poisson_mesh::glb::ObjectMesh {
                        name: object.name.clone(), mesh,
                    }, colors));
                }
                Ok(ExportResult::Meshes(objects))
            }).unwrap_or_else(|_| Err("Mesh export failed unexpectedly".into()));
            let _ = sender.send(result);
        });
        self.mesh_export = Some(MeshExportJob { id, overwrite, publication: Arc::default(), completed: Some(completed), stage, work,
            source, assets: crate::model::material_assets(&self.tree),
            camera: self.camera.clone(), world: crate::model::world(&self.tree), path,
            pages: Vec::new(), texture: None, texture_index: 0 });
        self.egui.request_repaint();
        Ok(id)
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
                    for (root, object, colors) in objects {
                        let mesh = Arc::new(object.mesh);
                        let page_triangles = if colors.is_some() { mesh.triangles.len().max(1) } else { PAGE_TRIANGLES };
                        for first in (0..mesh.triangles.len()).step_by(page_triangles) {
                            let count = (mesh.triangles.len() - first).min(page_triangles);
                            job.pages.push(Page { name: object.name.clone(), root,
                                mesh: mesh.clone(), first, count,
                                size: if colors.is_some() { 64 } else { crate::renderer::poisson_mesh::textured_glb::page_size(count) },
                                png: Vec::new(), colors: colors.clone() });
                        }
                    }
                    if job.pages.is_empty() { finish = Some(Err("There are no meshes to export".into())); }
                }
                Ok(Ok(ExportResult::Written)) => finish = Some(Ok(())),
                Ok(Err(error)) => finish = Some(Err(error)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => finish = Some(Err("Export worker stopped unexpectedly".into())),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        } else if job.texture_index < job.pages.len() && job.pages[job.texture_index].colors.is_some() {
            job.texture_index += 1;
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
            let publication = job.publication.clone();
            let overwrite = job.overwrite;
            let (sender, receiver) = channel();
            job.completed = Some(receiver);
            std::thread::spawn(move || {
                let result = crate::renderer::poisson_mesh::textured_glb::encode_textured(&pages)
                    .and_then(|bytes| write_glb(&path, &bytes, &cancel, &publication, overwrite))
                    .map(|_| ExportResult::Written);
                let _ = sender.send(result);
            });
        }
        self.finish_mesh_export(finish);
    }

    fn finish_mesh_export(&mut self, finish: Option<Result<(), String>>) {
        if let Some(result) = finish {
            if let Some(job) = self.mesh_export.take() {
                let outcome = match &result {
                    Ok(()) => ExportOutcome::Completed,
                    Err(error) if error == "Export cancelled" => job.cancel(),
                    Err(error) => ExportOutcome::Failed { error: error.clone() },
                };
                self.record_mesh_export(job, outcome);
            }
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
        if cancel { self.cancel_mesh_export_for_document_change(); }
    }
}

impl MeshExportJob {
    fn cancel(&self) -> ExportOutcome {
        let mut publication = self.publication.lock().unwrap();
        if matches!(*publication, Publication::Written) { return ExportOutcome::Completed; }
        *publication = Publication::Cancelled;
        self.work.cancel.store(true, Ordering::Relaxed);
        ExportOutcome::Cancelled
    }
}

fn write_glb(path: &std::path::Path, bytes: &[u8], cancel: &AtomicBool,
    publication: &Mutex<Publication>, overwrite: bool) -> Result<(), String> {
    use std::io::Write;
    let temp = path.with_file_name(format!(".claydash-export-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        let mut state = publication.lock().unwrap();
        if cancel.load(Ordering::Relaxed) || matches!(*state, Publication::Cancelled) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Export cancelled"));
        }
        if overwrite { std::fs::rename(&temp, path)?; }
        else { std::fs::hard_link(&temp, path)?; let _ = std::fs::remove_file(&temp); }
        *state = Publication::Written;
        Ok(())
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
        id: uuid::Uuid::new_v4(), overwrite: false, publication: Arc::default(),
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
        id: uuid::Uuid::new_v4(), overwrite: false, publication: Arc::default(),
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

#[cfg(test)]
#[test]
fn voxel_export_writes_glb_without_requesting_a_material_bake() {
    use crate::renderer::voxels::geometry;
    let mut app = App::new();
    let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
    object.color = Vec4::new(0.1, 0.6, 0.2, 1.0);
    let source = vec![object.clone()];
    let geometry = geometry::build(&source, object.uuid, 8,
        &std::sync::atomic::AtomicU32::new(0), None).unwrap()
        .into_world(&source, object.uuid, &AtomicBool::new(false)).unwrap();
    let path = std::env::temp_dir().join(format!("claydash-voxel-export-{}.glb", uuid::Uuid::new_v4()));
    let (sender, receiver) = channel();
    app.mesh_export = Some(MeshExportJob {
        id: uuid::Uuid::new_v4(), overwrite: false, publication: Arc::default(),
        completed: Some(receiver), stage: Arc::new(Mutex::new(ExportStage::Writing)),
        work: Computation::new(Stage::Sampling), source, assets: Vec::new(), camera: app.camera.clone(),
        world: crate::model::World::default(), path: path.clone(), pages: Vec::new(),
        texture: None, texture_index: 0,
    });
    sender.send(Ok(ExportResult::Meshes(vec![(object.uuid,
        crate::renderer::poisson_mesh::glb::ObjectMesh { name: object.name, mesh: geometry.mesh },
        Some(Arc::new(geometry.colors)))]))).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.mesh_export.is_some() {
        assert!(std::time::Instant::now() < deadline, "export did not complete");
        app.poll_mesh_export();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.document.error().is_none(), "{:?}", app.document.error());
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(&bytes[..4], b"glTF");
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
    assert!(document["images"].as_array().unwrap().is_empty());
    assert!(document["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"].is_number());
}

#[cfg(test)]
#[test]
fn publishing_glb_checks_overwrite_and_cancellation_at_commit_time() {
    let path = std::env::temp_dir().join(format!("claydash-publication-{}.glb", uuid::Uuid::new_v4()));
    let cancel = AtomicBool::new(false);
    let publication = Mutex::new(Publication::Pending);
    std::fs::write(&path, b"original").unwrap();
    assert!(write_glb(&path, b"replacement", &cancel, &publication, false).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    *publication.lock().unwrap() = Publication::Cancelled;
    assert!(write_glb(&path, b"cancelled", &cancel, &publication, true).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    *publication.lock().unwrap() = Publication::Pending;
    write_glb(&path, b"replacement", &cancel, &publication, true).unwrap();
    assert!(matches!(*publication.lock().unwrap(), Publication::Written));
    assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
    std::fs::remove_file(path).unwrap();
}

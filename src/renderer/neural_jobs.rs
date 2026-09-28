use super::neural_sdf::{payload_records, NeuralField, TrainingJob};
use super::*;
use std::{collections::HashMap, sync::Arc, time::Duration};

#[derive(Clone, PartialEq)]
pub(crate) enum NeuralStatus {
    Pending,
    Training,
    Ready {
        rms: f32,
        max: f32,
        milliseconds: f64,
    },
    Failed(String),
}
struct Entry {
    // Settings committed by the initial bake or the Recompute button.
    training: crate::model::NeuralTrainingSettings,
    key: u64,
    generation: u64,
    status: NeuralStatus,
    field: Option<Arc<NeuralField>>,
}
struct Active {
    root: uuid::Uuid,
    key: u64,
    generation: u64,
    #[cfg(not(target_arch = "wasm32"))]
    receiver: std::sync::mpsc::Receiver<Result<NeuralField, &'static str>>,
    #[cfg(not(target_arch = "wasm32"))]
    cancel: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(target_arch = "wasm32")]
    job: Option<TrainingJob>,
}
impl Drop for Active {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
#[derive(Default)]
pub(super) struct NeuralJobs {
    entries: HashMap<uuid::Uuid, Entry>,
    source: Arc<Vec<SdfObject>>,
    changed_at: Option<web_time::Instant>,
    active: Option<Active>,
    generation: u64,
}
impl NeuralJobs {
    pub fn reconcile(&mut self, source: &[SdfObject]) {
        let mut changed = false;
        self.entries.retain(|id, _| {
            source.iter().any(|o| {
                o.uuid == *id
                    && o.render_representation == crate::model::GroupRenderRepresentation::NeuralSdf
            })
        });
        let mut admitted = 0;
        for root in source.iter().filter(|o| {
            o.render_representation == crate::model::GroupRenderRepresentation::NeuralSdf
        }) {
            let key = super::group_capture::capture_key(source, root);
            let training = self
                .entries
                .get(&root.uuid)
                .map_or(root.neural_sdf.training, |entry| entry.training);
            let payload = payload_records(training);
            admitted += payload.unwrap_or(0);
            if self.entries.get(&root.uuid).is_some_and(|e| e.key == key) {
                continue;
            }
            let status = if payload.is_none() {
                NeuralStatus::Failed("Invalid neural training settings".into())
            } else if admitted > super::box_depth_atlas::MAX_BOX_DEPTH_TEXELS {
                NeuralStatus::Failed("Neural cache budget exceeded".into())
            } else {
                NeuralStatus::Pending
            };
            self.generation = self.generation.wrapping_add(1);
            self.entries.insert(
                root.uuid,
                Entry {
                    training,
                    key,
                    generation: self.generation,
                    status,
                    field: None,
                },
            );
            changed = true;
        }
        // Keep the latest editable settings for a future explicit recompute,
        // even when the geometry key and the running bake are unchanged.
        self.source = if self.entries.is_empty() {
            Arc::default()
        } else {
            Arc::new(source.to_vec())
        };
        if changed {
            self.changed_at = Some(web_time::Instant::now());
        }
        if let Some(active) = &self.active {
            if !self
                .entries
                .get(&active.root)
                .is_some_and(|e| e.key == active.key && e.generation == active.generation)
            {
                #[cfg(not(target_arch = "wasm32"))]
                active
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                // Keep the worker slot until it acknowledges cancellation.
                #[cfg(target_arch = "wasm32")]
                {
                    self.active = None;
                }
            }
        }
    }
    fn training_source(&self, root: uuid::Uuid) -> Arc<Vec<SdfObject>> {
        let mut source = self.source.as_ref().clone();
        if let Some(object) = source.iter_mut().find(|object| object.uuid == root) {
            object.neural_sdf.training = self.entries[&root].training;
        }
        Arc::new(source)
    }
    pub fn recompute(&mut self, root: uuid::Uuid) -> bool {
        let Some(entry) = self.entries.get_mut(&root) else {
            return false;
        };
        let Some(object) = self.source.iter().find(|object| object.uuid == root) else {
            return false;
        };
        entry.training = object.neural_sdf.training;
        self.generation = self.generation.wrapping_add(1);
        entry.generation = self.generation;
        entry.status = match payload_records(entry.training) {
            None => NeuralStatus::Failed("Invalid neural training settings".into()),
            Some(records) if records > super::box_depth_atlas::MAX_BOX_DEPTH_TEXELS => {
                NeuralStatus::Failed("Neural cache budget exceeded".into())
            }
            Some(_) => NeuralStatus::Pending,
        };
        entry.field = None;
        if let Some(active) = &self.active {
            if active.root == root {
                #[cfg(not(target_arch = "wasm32"))]
                active
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                #[cfg(target_arch = "wasm32")]
                {
                    self.active = None;
                }
            }
        }
        self.changed_at = Some(web_time::Instant::now() - Duration::from_millis(200));
        true
    }
    /// Returns true only when a newly completed model requires a GPU upload.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(active) = &mut self.active {
            #[cfg(not(target_arch = "wasm32"))]
            let result = match active.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Training worker stopped"))
                }
            };
            #[cfg(target_arch = "wasm32")]
            let result = match active
                .job
                .as_mut()
                .unwrap()
                .advance(Duration::from_millis(4))
            {
                Ok(false) => None,
                Ok(true) => Some(active.job.take().unwrap().finish()),
                Err(error) => Some(Err(error)),
            };
            if let Some(result) = result {
                if let Some(entry) = self
                    .entries
                    .get_mut(&active.root)
                    .filter(|e| e.key == active.key && e.generation == active.generation)
                {
                    match result {
                        Ok(field) => {
                            entry.status = NeuralStatus::Ready {
                                rms: field.rms_error,
                                max: field.max_error,
                                milliseconds: field.bake_ms,
                            };
                            entry.field = Some(Arc::new(field));
                            changed = true;
                        }
                        Err(error) => entry.status = NeuralStatus::Failed(error.into()),
                    }
                }
                self.active = None;
            }
        }
        if self.active.is_none()
            && self
                .changed_at
                .is_some_and(|time| time.elapsed() >= Duration::from_millis(200))
        {
            // Source order makes the queue deterministic.
            let next = self.source.iter().find_map(|o| {
                self.entries
                    .get(&o.uuid)
                    .filter(|e| matches!(e.status, NeuralStatus::Pending))
                    .map(|e| (o.uuid, e.key, e.generation))
            });
            if let Some((root, key, generation)) = next {
                self.entries.get_mut(&root).unwrap().status = NeuralStatus::Training;
                let source = self.training_source(root);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let cancelled = cancel.clone();
                    std::thread::spawn(move || {
                        let result = (|| {
                            let mut job =
                                TrainingJob::new(source, root).ok_or("Invalid source bounds")?;
                            loop {
                                if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                                    return Err("Cancelled");
                                }
                                if job.advance(Duration::from_millis(4))? {
                                    return job.finish();
                                }
                            }
                        })();
                        let _ = sender.send(result);
                    });
                    self.active = Some(Active {
                        root,
                        key,
                        generation,
                        receiver,
                        cancel,
                    });
                }
                #[cfg(target_arch = "wasm32")]
                {
                    if let Some(job) = TrainingJob::new(source, root) {
                        self.active = Some(Active {
                            root,
                            key,
                            generation,
                            job: Some(job),
                        });
                    } else {
                        self.entries.get_mut(&root).unwrap().status =
                            NeuralStatus::Failed("Invalid source bounds".into());
                    }
                }
            }
        }
        changed
    }
    pub fn is_busy(&self) -> bool {
        self.active.is_some()
            || self
                .entries
                .values()
                .any(|e| matches!(e.status, NeuralStatus::Pending))
    }
    pub fn reject_payload(&mut self, id: uuid::Uuid) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.field = None;
            entry.status = NeuralStatus::Failed("Shared proxy buffer budget exceeded".into());
        }
    }
    pub fn ready(&self) -> HashMap<uuid::Uuid, Arc<NeuralField>> {
        self.entries
            .iter()
            .filter_map(|(id, e)| e.field.clone().map(|f| (*id, f)))
            .collect()
    }
    pub fn publish(&self, context: &egui::Context) {
        let statuses: HashMap<uuid::Uuid, NeuralStatus> = self
            .entries
            .iter()
            .map(|(id, e)| (*id, e.status.clone()))
            .collect();
        let applied: HashMap<uuid::Uuid, crate::model::NeuralTrainingSettings> = self
            .entries
            .iter()
            .map(|(id, entry)| (*id, entry.training))
            .collect();
        let changed = context.data_mut(|data| {
            let applied_id = egui::Id::new("neural-sdf-applied-settings");
            let applied_changed = data
                .get_temp::<HashMap<uuid::Uuid, crate::model::NeuralTrainingSettings>>(applied_id)
                .as_ref()
                != Some(&applied);
            data.insert_temp(applied_id, applied);
            let id = egui::Id::new("neural-sdf-status");
            let previous = data.get_temp::<HashMap<uuid::Uuid, NeuralStatus>>(id);
            let changed = applied_changed || previous.as_ref() != Some(&statuses);
            data.insert_temp(id, statuses);
            changed
        });
        if changed {
            context.request_repaint();
        }
        if self.is_busy() {
            context.request_repaint_after(Duration::from_millis(16));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GroupRenderRepresentation, PrimitiveKind};
    fn source() -> Vec<SdfObject> {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.render_representation = GroupRenderRepresentation::NeuralSdf;
        vec![object]
    }
    #[test]
    fn neural_large_training_grids_are_admitted_and_recomputed() {
        for resolution in [128, 1024] {
            let mut jobs = NeuralJobs::default();
            let mut source = source();
            source[0].neural_sdf.training.samples_per_side = resolution;
            let id = source[0].uuid;
            jobs.reconcile(&source);
            assert!(matches!(jobs.entries[&id].status, NeuralStatus::Pending));
            assert!(jobs.recompute(id));
            assert!(matches!(jobs.entries[&id].status, NeuralStatus::Pending));
        }
    }
    #[test]
    fn neural_jobs_debounce_invalidate_and_release() {
        let mut jobs = NeuralJobs::default();
        let mut source = source();
        let id = source[0].uuid;
        jobs.reconcile(&source);
        assert!(!jobs.poll());
        assert!(jobs.active.is_none());
        let key = jobs.entries[&id].key;
        jobs.entries.get_mut(&id).unwrap().status = NeuralStatus::Failed("test failure".into());
        jobs.reconcile(&source);
        assert!(matches!(jobs.entries[&id].status, NeuralStatus::Failed(_)));
        source[0].transform.scale *= 2.0;
        jobs.reconcile(&source);
        assert_ne!(key, jobs.entries[&id].key);
        assert!(matches!(jobs.entries[&id].status, NeuralStatus::Pending));
        source[0].render_representation = GroupRenderRepresentation::ExactSdf;
        jobs.reconcile(&source);
        assert!(jobs.entries.is_empty());
        assert!(jobs.ready().is_empty());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn neural_jobs_reject_stale_worker_result() {
        let mut jobs = NeuralJobs::default();
        let mut source = source();
        let id = source[0].uuid;
        jobs.reconcile(&source);
        let key = jobs.entries[&id].key;
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        jobs.active = Some(Active {
            root: id,
            key,
            generation: jobs.entries[&id].generation,
            receiver,
            cancel: cancel.clone(),
        });
        source[0].transform.scale *= 2.0;
        jobs.reconcile(&source);
        assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
        sender.send(Err("obsolete failure")).unwrap();
        assert!(!jobs.poll());
        assert!(matches!(jobs.entries[&id].status, NeuralStatus::Pending));
        assert!(jobs.ready().is_empty());
    }
    #[test]
    fn neural_training_controls_wait_for_recompute() {
        let mut jobs = NeuralJobs::default();
        let mut source = source();
        let id = source[0].uuid;
        jobs.reconcile(&source);
        let key = jobs.entries[&id].key;
        let generation = jobs.entries[&id].generation;
        source[0].neural_sdf.hit_distance_cells = 1.5;
        jobs.reconcile(&source);
        assert_eq!(key, jobs.entries[&id].key);
        assert_eq!(generation, jobs.entries[&id].generation);
        source[0].neural_sdf.training.layers = 2;
        source[0].neural_sdf.training.activation = crate::model::NeuralActivation::Softplus;
        jobs.reconcile(&source);
        source[0].neural_sdf.training.width = 16;
        source[0].neural_sdf.training.epochs = 64;
        source[0].neural_sdf.training.learning_rate = 0.002;
        source[0].neural_sdf.training.seed = 42;
        source[0].neural_sdf.training.samples_per_side = 17;
        jobs.reconcile(&source);
        assert_eq!(key, jobs.entries[&id].key);
        assert_eq!(generation, jobs.entries[&id].generation);
        assert_eq!(
            jobs.training_source(id)[0].neural_sdf.training,
            crate::model::NeuralTrainingSettings::default()
        );
        // A geometry rebuild also uses the last committed training choices.
        source[0].transform.scale *= 2.0;
        jobs.reconcile(&source);
        assert_ne!(key, jobs.entries[&id].key);
        assert_eq!(
            jobs.training_source(id)[0].neural_sdf.training,
            crate::model::NeuralTrainingSettings::default()
        );
        assert!(jobs.recompute(id));
        assert_eq!(
            jobs.training_source(id)[0].neural_sdf.training,
            source[0].neural_sdf.training
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn neural_recompute_rejects_previous_generation_with_same_key() {
        let mut jobs = NeuralJobs::default();
        let source = source();
        let id = source[0].uuid;
        jobs.reconcile(&source);
        let key = jobs.entries[&id].key;
        let generation = jobs.entries[&id].generation;
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        jobs.active = Some(Active {
            root: id,
            key,
            generation,
            receiver,
            cancel: cancel.clone(),
        });
        let mut edited = source.clone();
        edited[0].neural_sdf.training.width = 16;
        jobs.reconcile(&edited);
        assert!(!cancel.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(generation, jobs.entries[&id].generation);
        assert!(jobs.recompute(id));
        assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(key, jobs.entries[&id].key);
        assert_ne!(generation, jobs.entries[&id].generation);
        sender.send(Err("obsolete failure")).unwrap();
        jobs.changed_at = Some(web_time::Instant::now());
        assert!(!jobs.poll());
        assert!(matches!(jobs.entries[&id].status, NeuralStatus::Pending));
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn neural_jobs_completion_marks_upload_dirty_once() {
        let mut jobs = NeuralJobs::default();
        let mut source = source();
        let id = source[0].uuid;
        jobs.reconcile(&source);
        let key = jobs.entries[&id].key;
        let mut job = TrainingJob::new(Arc::new(source.clone()), id).unwrap();
        while !job.advance(Duration::from_millis(4)).unwrap() {}
        let (sender, receiver) = std::sync::mpsc::channel();
        jobs.active = Some(Active {
            root: id,
            key,
            generation: jobs.entries[&id].generation,
            receiver,
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        sender.send(Ok(job.finish().unwrap())).unwrap();
        assert!(jobs.poll());
        assert!(!jobs.poll());
        assert!(jobs.ready().contains_key(&id));
        let first = jobs.ready()[&id].clone();
        source[0].neural_sdf.training.layers = 2;
        source[0].neural_sdf.training.activation = crate::model::NeuralActivation::Softplus;
        jobs.reconcile(&source);
        assert!(!jobs.is_busy());
        assert!(Arc::ptr_eq(&first, &jobs.ready()[&id]));
        jobs.reject_payload(id);
        assert!(jobs.ready().is_empty());
        assert!(matches!(jobs.entries[&id].status, NeuralStatus::Failed(_)));
        jobs.reconcile(&source);
        assert!(matches!(jobs.entries[&id].status, NeuralStatus::Failed(_)));
    }
}

use super::neural_sdf::{payload_records, GpuTrainingJob, NeuralField};
use super::*;
use super::computation::{Computation, Stage};
use std::{collections::HashMap, sync::Arc, time::Duration};

#[derive(Clone, PartialEq)]
pub(crate) enum NeuralStatus {
    Idle,
    Pending,
    Training {
        percent: u32,
    },
    Ready {
        rms: f32,
        max: f32,
        milliseconds: f64,
    },
    Failed(String),
}
struct Entry {
    // Settings committed by the Recompute button or restored from a saved bake.
    training: crate::model::NeuralTrainingSettings,
    key: u64,
    generation: u64,
    status: NeuralStatus,
    field: Option<Arc<NeuralField>>,
    fallback_field: Option<Arc<NeuralField>>,
}
struct Active {
    root: uuid::Uuid,
    key: u64,
    generation: u64,
    #[cfg(not(target_arch = "wasm32"))]
    receiver: std::sync::mpsc::Receiver<Result<NeuralField, &'static str>>,
    work: Computation,
    #[cfg(target_arch = "wasm32")]
    job: Option<GpuTrainingJob>,
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
    pub fn objects_for_save(&self, source: &[SdfObject]) -> Vec<SdfObject> {
        let mut objects = source.to_vec();
        let source = &source[..source.len().min(MAX_OBJECTS)];
        for object in &mut objects {
            if object.render_representation != crate::model::GroupRenderRepresentation::NeuralSdf {
                object.saved_neural_field = None;
                continue;
            }
            let key = super::group_capture::capture_key(source, object);
            if let Some(entry) = self
                .entries
                .get(&object.uuid)
                .filter(|entry| entry.key == key)
            {
                object.saved_neural_field = entry
                    .field
                    .as_ref()
                    // A pending run may display a fallback field. It is not
                    // a completed bake of the newly requested settings.
                    .filter(|_| !matches!(entry.status, NeuralStatus::Pending | NeuralStatus::Training { .. }))
                    .map(|field| Arc::new(field.saved(key, entry.training)));
            } else if !object
                .saved_neural_field
                .as_deref()
                .is_some_and(|saved| NeuralField::from_saved(saved, key, source).is_some())
            {
                object.saved_neural_field = None;
            }
        }
        objects
    }

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
            if let Some(entry) = self
                .entries
                .get(&root.uuid)
                .filter(|entry| entry.key == key)
            {
                admitted += payload_records(entry.training).unwrap_or(0);
                continue;
            }
            let restored = root.saved_neural_field.as_deref().and_then(|saved| {
                NeuralField::from_saved(saved, key, source).map(|field| (saved.training, field))
            });
            let training = restored
                .as_ref()
                .map(|(training, _)| *training)
                .or_else(|| self.entries.get(&root.uuid).map(|entry| entry.training))
                .unwrap_or(root.neural_sdf.training);
            let payload = payload_records(training);
            admitted += payload.unwrap_or(0);
            let mut status = if payload.is_none() {
                NeuralStatus::Failed("Invalid neural training settings".into())
            } else if admitted > super::box_depth_atlas::MAX_BOX_DEPTH_TEXELS {
                NeuralStatus::Failed("Neural cache budget exceeded".into())
            } else {
                NeuralStatus::Idle
            };
            let field = if matches!(status, NeuralStatus::Idle) {
                restored.map(|(_, field)| {
                    status = NeuralStatus::Ready {
                        rms: field.rms_error,
                        max: field.max_error,
                        milliseconds: field.bake_ms,
                    };
                    Arc::new(field)
                })
            } else {
                None
            };
            self.generation = self.generation.wrapping_add(1);
            self.entries.insert(
                root.uuid,
                Entry {
                    training,
                    key,
                    generation: self.generation,
                    status,
                    field,
                    fallback_field: None,
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
                active.work.cancel();
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
        let Some(object) = self.source.iter().find(|object| object.uuid == root) else {
            return false;
        };
        let training = object.neural_sdf.training;
        let fallback_field = NeuralField::best_effort(self.source.clone(), root).map(Arc::new);
        let Some(entry) = self.entries.get_mut(&root) else {
            return false;
        };
        entry.training = training;
        self.generation = self.generation.wrapping_add(1);
        entry.generation = self.generation;
        entry.status = match payload_records(entry.training) {
            None => NeuralStatus::Failed("Invalid neural training settings".into()),
            Some(records) if records > super::box_depth_atlas::MAX_BOX_DEPTH_TEXELS => {
                NeuralStatus::Failed("Neural cache budget exceeded".into())
            }
            Some(_) => NeuralStatus::Pending,
        };
        entry.field = fallback_field.clone();
        entry.fallback_field = fallback_field;
        if let Some(active) = &self.active {
            if active.root == root {
                #[cfg(not(target_arch = "wasm32"))]
                active.work.cancel();
                #[cfg(target_arch = "wasm32")]
                {
                    self.active = None;
                }
            }
        }
        self.changed_at = Some(web_time::Instant::now() - Duration::from_millis(200));
        true
    }
    pub fn cancel(&mut self, root: uuid::Uuid) -> bool {
        let Some(entry) = self.entries.get_mut(&root) else {
            return false;
        };
        if !matches!(entry.status, NeuralStatus::Pending | NeuralStatus::Training { .. }) {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        entry.generation = self.generation;
        entry.status = NeuralStatus::Idle;
        entry.field = entry.fallback_field.take();
        if let Some(active) = &self.active {
            if active.root == root {
                #[cfg(not(target_arch = "wasm32"))]
                active.work.cancel();
                #[cfg(target_arch = "wasm32")]
                {
                    self.active = None;
                }
            }
        }
        true
    }
    /// Returns true only when a newly completed model requires a GPU upload.
    pub fn poll(
        &mut self,
        mut factory: impl FnMut(Arc<Vec<SdfObject>>, uuid::Uuid) -> Result<GpuTrainingJob, &'static str>,
    ) -> bool {
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
            #[cfg(not(target_arch = "wasm32"))]
            let percent = active.work.percent();
            #[cfg(target_arch = "wasm32")]
            let percent = active
                .job
                .as_ref()
                .map_or(100, GpuTrainingJob::progress_percent);
            if let Some(entry) = self
                .entries
                .get_mut(&active.root)
                .filter(|entry| entry.key == active.key && entry.generation == active.generation)
            {
                entry.status = NeuralStatus::Training {
                    percent: percent.min(99),
                };
            }
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
                            entry.fallback_field = None;
                            changed = true;
                        }
                        Err(error) => {
                            entry.status = NeuralStatus::Failed(error.into());
                            if let Some(field) = entry.fallback_field.take() {
                                entry.field = Some(field);
                                changed = true;
                            }
                        }
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
                self.entries.get_mut(&root).unwrap().status = NeuralStatus::Training { percent: 0 };
                let source = self.training_source(root);
                let job = match factory(source, root) {
                    Ok(job) => job,
                    Err(error) => {
                        let entry = self.entries.get_mut(&root).unwrap();
                        entry.status = NeuralStatus::Failed(error.into());
                        if let Some(field) = entry.fallback_field.take() {
                            entry.field = Some(field);
                            changed = true;
                        }
                        return changed;
                    }
                };
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let work = Computation::new(Stage::Training);
                    let cancelled = work.cancel.clone();
                    let worker_progress = work.progress.clone();
                    std::thread::spawn(move || {
                        let mut job = job;
                        let result = (|| loop {
                            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                                return Err("Cancelled");
                            }
                            let complete = job.advance(Duration::from_millis(4))?;
                            worker_progress.store(
                                job.progress_percent(),
                                std::sync::atomic::Ordering::Relaxed,
                            );
                            if complete {
                                return job.finish();
                            }
                        })();
                        let _ = sender.send(result);
                    });
                    self.active = Some(Active {
                        root,
                        key,
                        generation,
                        receiver,
                        work,
                    });
                }
                #[cfg(target_arch = "wasm32")]
                {
                    self.active = Some(Active {
                        root,
                        key,
                        generation,
                        job: Some(job),
                        work: Computation::new(Stage::Training),
                    });
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
    pub fn computation_status(&self) -> Option<(uuid::Uuid, super::computation::Snapshot)> {
        let active = self.active.as_ref()?;
        let entry = self.entries.get(&active.root)?;
        if entry.generation != active.generation || active.work.is_cancelled() { return None; }
        let NeuralStatus::Training { percent } = entry.status else { return None; };
        Some((active.root, active.work.snapshot(Stage::Training, Some(percent))))
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
    // Lifecycle and persistence checks consume a completed field without training.
    fn field_fixture(source: &[SdfObject]) -> NeuralField {
        let root = &source[0];
        let training = root.neural_sdf.training;
        let key = super::super::group_capture::capture_key(source, root);
        let saved = crate::model::SavedNeuralField {
            version: 2,
            source_key: key,
            training,
            weights: vec![0.125; training.parameter_count().unwrap()],
            half_extent: 0.5,
            owners: vec![root.uuid; 32 * 32 * 32],
            rms_error: 0.01,
            max_error: 0.03,
            bake_ms: 5.0,
        };
        NeuralField::from_saved(&saved, key, source).unwrap()
    }
    #[test]
    fn neural_training_starts_only_after_recompute() {
        let mut jobs = NeuralJobs::default();
        let mut source = source();
        let id = source[0].uuid;
        jobs.reconcile(&source);
        jobs.changed_at = Some(web_time::Instant::now() - Duration::from_secs(1));
        assert!(!jobs.is_busy());
        jobs.poll(|_, _| panic!("selection must not start training"));
        assert!(jobs.recompute(id));
        assert!(jobs.is_busy());
        let mut started = false;
        jobs.poll(|_, _| {
            started = true;
            Err("test GPU job creation failure")
        });
        assert!(started);
        source[0].transform.scale *= 2.0;
        jobs.reconcile(&source);
        jobs.changed_at = Some(web_time::Instant::now() - Duration::from_secs(1));
        assert!(!jobs.is_busy());
        jobs.poll(|_, _| panic!("geometry edits must not start training"));
    }

    #[test]
    fn saved_neural_fields_reopen_ready_and_preserve_pending_settings() {
        let mut source = source();
        let id = source[0].uuid;
        source[0].neural_sdf.training.samples = 512;
        source[0].neural_sdf.training.width = 65;
        let applied = source[0].neural_sdf.training;
        let field = field_fixture(&source);
        let mut jobs = NeuralJobs::default();
        jobs.reconcile(&source);
        jobs.entries.get_mut(&id).unwrap().field = Some(Arc::new(field.clone()));
        // Edits are staged until Recompute, and must survive reopening separately.
        source[0].neural_sdf.training.width = 128;
        source[0].neural_sdf.hit_distance_cells = 1.25;
        let mut tree = crate::model::DataTree::default();
        crate::model::set_objects(&mut tree, jobs.objects_for_save(&source));
        let bytes = crate::document::serialize_scene(&tree).unwrap();
        let scene = crate::document::deserialize_scene(&bytes).unwrap();
        let mut reopened = crate::model::DataTree::default();
        reopened.set_tree("scene", scene);
        let objects = crate::model::objects_ref(&reopened);
        let mut restored = NeuralJobs::default();
        restored.reconcile(objects);
        assert!(matches!(
            restored.entries[&id].status,
            NeuralStatus::Ready { .. }
        ));
        assert!(!restored.is_busy());
        assert!(!restored.poll(|_, _| panic!("restored fields must not retrain")));
        let restored_field = restored.ready()[&id].clone();
        assert_eq!(restored_field.network.weights, field.network.weights);
        assert_eq!(
            restored_field.network.gpu_records(1.25),
            field.network.gpu_records(1.25)
        );
        assert_eq!(restored_field.owners, field.owners);
        assert_eq!(restored_field.half_extent, field.half_extent);
        assert_eq!(restored.entries[&id].training, applied);
        assert_eq!(objects[0].neural_sdf.training.width, 128);
        assert_eq!(objects[0].neural_sdf.hit_distance_cells, 1.25);
        assert!(restored.recompute(id));
        assert_eq!(restored.entries[&id].training.width, 128);
        assert!(restored.objects_for_save(objects)[0]
            .saved_neural_field
            .is_none());

        let mut edited = objects.to_vec();
        edited[0].transform.scale *= 2.0;
        let mut fresh = NeuralJobs::default();
        fresh.reconcile(&edited);
        assert!(matches!(fresh.entries[&id].status, NeuralStatus::Idle));
        assert!(fresh.ready().is_empty());
        assert!(fresh.objects_for_save(&edited)[0]
            .saved_neural_field
            .is_none());
        edited[0].saved_neural_field = None;
        let mut legacy = NeuralJobs::default();
        legacy.reconcile(&edited);
        assert!(matches!(legacy.entries[&id].status, NeuralStatus::Idle));
    }

    #[test]
    fn invalid_saved_neural_fields_wait_for_recompute() {
        let mut source = source();
        let id = source[0].uuid;
        let training = source[0].neural_sdf.training;
        let valid = crate::model::SavedNeuralField {
            version: 2,
            source_key: super::super::group_capture::capture_key(&source, &source[0]),
            training,
            weights: vec![0.0; training.parameter_count().unwrap()],
            half_extent: 0.5,
            owners: vec![id; 32 * 32 * 32],
            rms_error: 0.01,
            max_error: 0.03,
            bake_ms: 5.0,
        };
        for case in 0..9 {
            let mut saved = valid.clone();
            match case {
                0 => saved.version = 99,
                8 => saved.version = 1,
                1 => saved.source_key ^= 1,
                2 => {
                    saved.weights.pop();
                }
                3 => saved.weights[0] = f32::NAN,
                4 => saved.half_extent = 0.0,
                5 => {
                    saved.owners.pop();
                }
                6 => saved.owners[0] = uuid::Uuid::new_v4(),
                7 => saved.training.width = crate::model::NeuralTrainingSettings::MAX_WIDTH + 1,
                _ => unreachable!(),
            }
            source[0].saved_neural_field = Some(Arc::new(saved));
            let mut jobs = NeuralJobs::default();
            jobs.reconcile(&source);
            assert!(
                matches!(jobs.entries[&id].status, NeuralStatus::Idle),
                "case {case}"
            );
            assert!(jobs.ready().is_empty());
        }
    }

    #[test]
    fn neural_large_sample_counts_are_admitted_and_recomputed() {
        for count in [
            128_u32.pow(3),
            crate::model::NeuralTrainingSettings::MAX_SAMPLES,
        ] {
            let mut jobs = NeuralJobs::default();
            let mut source = source();
            source[0].neural_sdf.training.samples = count;
            let id = source[0].uuid;
            jobs.reconcile(&source);
            assert!(matches!(jobs.entries[&id].status, NeuralStatus::Idle));
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
        assert!(!jobs.poll(|_, _| panic!("unexpected training request")));
        assert!(jobs.active.is_none());
        let key = jobs.entries[&id].key;
        jobs.entries.get_mut(&id).unwrap().status = NeuralStatus::Failed("test failure".into());
        jobs.reconcile(&source);
        assert!(matches!(jobs.entries[&id].status, NeuralStatus::Failed(_)));
        source[0].transform.scale *= 2.0;
        jobs.reconcile(&source);
        assert_ne!(key, jobs.entries[&id].key);
        assert!(matches!(jobs.entries[&id].status, NeuralStatus::Idle));
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
        let work = Computation::new(Stage::Training);
        let cancel = work.cancel.clone();
        jobs.active = Some(Active {
            root: id,
            key,
            generation: jobs.entries[&id].generation,
            receiver,
            work,
        });
        source[0].transform.scale *= 2.0;
        jobs.reconcile(&source);
        assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
        sender.send(Err("obsolete failure")).unwrap();
        assert!(!jobs.poll(|_, _| panic!("unexpected training request")));
        assert!(matches!(jobs.entries[&id].status, NeuralStatus::Idle));
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
        source[0].neural_sdf.training.samples = 4913;
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
        let work = Computation::new(Stage::Training);
        let cancel = work.cancel.clone();
        jobs.active = Some(Active {
            root: id,
            key,
            generation,
            receiver,
            work,
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
        assert!(!jobs.poll(|_, _| panic!("unexpected training request")));
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
        let field = field_fixture(&source);
        let (sender, receiver) = std::sync::mpsc::channel();
        jobs.active = Some(Active {
            root: id,
            key,
            generation: jobs.entries[&id].generation,
            receiver,
            work: Computation::new(Stage::Training),
        });
        sender.send(Ok(field)).unwrap();
        assert!(jobs.poll(|_, _| panic!("unexpected training request")));
        assert!(!jobs.poll(|_, _| panic!("unexpected training request")));
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

impl Renderer {
    pub(crate) fn optimized_objects_for_save(&self, source: &[SdfObject]) -> Vec<SdfObject> {
        let mut objects = self.neural_jobs.objects_for_save(source);
        super::group_capture::objects_with_saved_captures(
            source,
            &mut objects,
            &self.group_capture_cache,
        );
        objects
    }

    pub(crate) fn reset_optimized_fields(&mut self) {
        self.viewport.reset_for_scene();
        {
            self.poisson_mesh = Default::default();
            self.mesh_vertex_count = 0;
        }
        self.neural_jobs = NeuralJobs::default();
        self.group_capture_cache.clear();
        self.group_compute_requests.clear();
        self.cancelled_capture_keys.clear();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.group_capture_bake = None;
        }
    }
}

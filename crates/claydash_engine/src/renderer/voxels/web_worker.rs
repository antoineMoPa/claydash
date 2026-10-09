use super::geometry::VoxelGeometry;
use crate::model::SdfObject;
use serde::{Deserialize, Serialize};
use std::{cell::{Cell, RefCell}, rc::Rc, sync::atomic::AtomicU32};
use wasm_bindgen::{closure::Closure, JsCast};

#[derive(Serialize, Deserialize)]
struct Request {
    source: Vec<SdfObject>,
    root: uuid::Uuid,
    resolution: u32,
    world_space: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Reply {
    Progress { percent: u32 },
    Complete { result: Result<VoxelGeometry, String> },
    Error { message: String },
}

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "export function voxelWorkerProgress(percent) { self.postMessage(JSON.stringify({kind:'Progress', percent})); }")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = voxelWorkerProgress)]
    fn worker_progress(percent: u32);
}

thread_local! {
    static LAST_PROGRESS: Cell<Option<u32>> = const { Cell::new(None) };
}

pub(super) fn post_progress(percent: u32) {
    LAST_PROGRESS.with(|last| {
        if last.replace(Some(percent)) != Some(percent) { worker_progress(percent); }
    });
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn run_voxel_worker(request: &str) -> String {
    LAST_PROGRESS.with(|last| last.set(None));
    let result = serde_json::from_str::<Request>(request)
        .map_err(|error| error.to_string())
        .and_then(|request| {
            let geometry = super::geometry::build(&request.source, request.root, request.resolution,
                &AtomicU32::new(0), None)?;
            if request.world_space {
                geometry.into_world(&request.source, request.root, &std::sync::atomic::AtomicBool::new(false))
            } else { Ok(geometry) }
        });
    serde_json::to_string(&Reply::Complete { result })
        .unwrap_or_else(|error| serde_json::to_string(&Reply::Error { message: error.to_string() })
            .unwrap_or_else(|_| "{\"kind\":\"Error\",\"message\":\"Could not encode voxel result\"}".to_owned()))
}

pub struct WebJob {
    worker: web_sys::Worker,
    result: Rc<RefCell<Option<Result<VoxelGeometry, String>>>>,
    progress: Rc<Cell<u32>>,
    _onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _onerror: Closure<dyn FnMut(web_sys::ErrorEvent)>,
}

impl WebJob {
    pub fn start(source: Vec<SdfObject>, root: uuid::Uuid, resolution: u32,
            context: &egui::Context) -> Result<Self, String> {
        Self::start_with_frame(source, root, resolution, false, context)
    }

    pub fn start_export(source: Vec<SdfObject>, root: uuid::Uuid, resolution: u32,
        context: &egui::Context) -> Result<Self, String> {
        Self::start_with_frame(source, root, resolution, true, context)
    }

    fn start_with_frame(source: Vec<SdfObject>, root: uuid::Uuid, resolution: u32,
        world_space: bool, context: &egui::Context) -> Result<Self, String> {
        let options = web_sys::WorkerOptions::new();
        options.set_type(web_sys::WorkerType::Module);
        let worker = web_sys::Worker::new_with_options("./voxel-worker.js", &options)
            .map_err(|error| format!("Could not start voxel worker: {error:?}"))?;
        let result = Rc::new(RefCell::new(None));
        let progress = Rc::new(Cell::new(0));
        let message_result = result.clone();
        let message_progress = progress.clone();
        let message_context = context.clone();
        let onmessage = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            let reply = event.data().as_string()
                .ok_or_else(|| "Voxel worker sent an invalid message".to_owned())
                .and_then(|message| serde_json::from_str::<Reply>(&message)
                    .map_err(|error| format!("Voxel worker response: {error}")));
            match reply {
                Ok(Reply::Progress { percent }) => message_progress.set(percent.min(200)),
                Ok(Reply::Complete { result }) => *message_result.borrow_mut() = Some(result),
                Ok(Reply::Error { message }) => *message_result.borrow_mut() = Some(Err(message)),
                Err(error) => *message_result.borrow_mut() = Some(Err(error)),
            }
            message_context.request_repaint();
        }) as Box<dyn FnMut(_)>);
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        let error_result = result.clone();
        let error_context = context.clone();
        let onerror = Closure::wrap(Box::new(move |error: web_sys::ErrorEvent| {
            *error_result.borrow_mut() = Some(Err(format!("Voxel worker failed: {}", error.message())));
            error_context.request_repaint();
        }) as Box<dyn FnMut(_)>);
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        let request = serde_json::to_string(&Request { source, root, resolution, world_space })
            .map_err(|error| error.to_string())?;
        let job = Self { worker, result, progress, _onmessage: onmessage, _onerror: onerror };
        job.worker.post_message(&wasm_bindgen::JsValue::from_str(&request))
            .map_err(|error| format!("Could not send voxel request: {error:?}"))?;
        Ok(job)
    }

    pub fn progress(&self) -> u32 { self.progress.get() }

    pub fn take_result(&self) -> Option<Result<VoxelGeometry, String>> { self.result.borrow_mut().take() }
}

impl Drop for WebJob {
    fn drop(&mut self) {
        self.worker.set_onmessage(None);
        self.worker.set_onerror(None);
        self.worker.terminate();
    }
}


use super::geometry::Mesh;
use crate::model::SdfObject;
use serde::{Deserialize, Serialize};
use std::{cell::{Cell, RefCell}, rc::Rc, sync::{Arc, atomic::AtomicU32}};
use wasm_bindgen::{closure::Closure, JsCast};

#[derive(Serialize, Deserialize)]
struct Request {
    source: Vec<SdfObject>,
    root: uuid::Uuid,
    resolution: u32,
    mesh_resolution: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Reply {
    Progress { percent: u32 },
    Complete { result: Result<Mesh, String> },
    Error { message: String },
}

#[derive(Serialize, Deserialize)]
struct EncodedPage {
    name: String,
    root: uuid::Uuid,
    mesh: usize,
    first: usize,
    count: usize,
    size: u32,
    png: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct GlbRequest {
    meshes: Vec<Arc<Mesh>>,
    pages: Vec<EncodedPage>,
}

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "export function poissonWorkerProgress(percent) { self.postMessage(JSON.stringify({kind:'Progress', percent})); }")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = poissonWorkerProgress)]
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
pub fn run_poisson_mesh_worker(request: &str) -> String {
    LAST_PROGRESS.with(|last| last.set(None));
    let result = serde_json::from_str::<Request>(request)
        .map_err(|error| error.to_string())
        .and_then(|request| {
            super::geometry::build(&request.source, request.root, request.resolution,
                request.mesh_resolution,
                None, &AtomicU32::new(0), None)
        });
    serde_json::to_string(&Reply::Complete { result })
        .unwrap_or_else(|error| serde_json::to_string(&Reply::Error { message: error.to_string() })
            .unwrap_or_else(|_| "{\"kind\":\"Error\",\"message\":\"Could not encode mesh result\"}".to_owned()))
}

pub struct WebJob {
    worker: web_sys::Worker,
    result: Rc<RefCell<Option<Result<Mesh, String>>>>,
    progress: Rc<Cell<u32>>,
    _onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _onerror: Closure<dyn FnMut(web_sys::ErrorEvent)>,
}

impl WebJob {
    pub fn start(source: Vec<SdfObject>, root: uuid::Uuid, resolution: u32,
        mesh_resolution: u32,
        context: &egui::Context) -> Result<Self, String> {
        let options = web_sys::WorkerOptions::new();
        options.set_type(web_sys::WorkerType::Module);
        let worker = web_sys::Worker::new_with_options("./poisson-mesh-worker.js", &options)
            .map_err(|error| format!("Could not start mesh worker: {error:?}"))?;
        let result = Rc::new(RefCell::new(None));
        let progress = Rc::new(Cell::new(0));
        let message_result = result.clone();
        let message_progress = progress.clone();
        let message_context = context.clone();
        let onmessage = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            let reply = event.data().as_string()
                .ok_or_else(|| "Mesh worker sent an invalid message".to_owned())
                .and_then(|message| serde_json::from_str::<Reply>(&message)
                    .map_err(|error| format!("Mesh worker response: {error}")));
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
            *error_result.borrow_mut() = Some(Err(format!("Mesh worker failed: {}", error.message())));
            error_context.request_repaint();
        }) as Box<dyn FnMut(_)>);
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        let request = serde_json::to_string(&Request { source, root, resolution, mesh_resolution })
            .map_err(|error| error.to_string())?;
        let job = Self { worker, result, progress, _onmessage: onmessage, _onerror: onerror };
        job.worker.post_message(&wasm_bindgen::JsValue::from_str(&request))
            .map_err(|error| format!("Could not send mesh request: {error:?}"))?;
        Ok(job)
    }

    pub fn progress(&self) -> u32 { self.progress.get() }

    pub fn take_result(&self) -> Option<Result<Mesh, String>> { self.result.borrow_mut().take() }
}

impl Drop for WebJob {
    fn drop(&mut self) {
        self.worker.set_onmessage(None);
        self.worker.set_onerror(None);
        self.worker.terminate();
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn run_poisson_encode_png(pixels: &[u8], size: u32) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer.write_image_data(pixels).map_err(|error| error.to_string())?;
    }
    Ok(output)
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn run_poisson_encode_glb(pages: &str) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    let request: GlbRequest = serde_json::from_str(pages)
        .map_err(|error| error.to_string())?;
    let pages: Vec<_> = request.pages.into_iter().map(|page| {
        let mesh = request.meshes.get(page.mesh)
            .ok_or_else(|| wasm_bindgen::JsValue::from_str("GLB page references a missing mesh"))?;
        Ok(super::textured_glb::Page { name: page.name, root: page.root, mesh: mesh.clone(),
            first: page.first, count: page.count, size: page.size, png: page.png })
    }).collect::<Result<_, wasm_bindgen::JsValue>>()?;
    super::textured_glb::encode_textured(&pages)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub struct WebBytesJob {
    worker: web_sys::Worker,
    result: Rc<RefCell<Option<Result<Vec<u8>, String>>>>,
    _onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _onerror: Closure<dyn FnMut(web_sys::ErrorEvent)>,
}

impl WebBytesJob {
    fn start(message: &js_sys::Object, transfer: Option<&js_sys::Array>,
        context: &egui::Context) -> Result<Self, String> {
        let options = web_sys::WorkerOptions::new();
        options.set_type(web_sys::WorkerType::Module);
        let worker = web_sys::Worker::new_with_options("./poisson-encode-worker.js", &options)
            .map_err(|error| format!("Could not start mesh encoder: {error:?}"))?;
        let result = Rc::new(RefCell::new(None));
        let message_result = result.clone();
        let message_context = context.clone();
        let onmessage = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            let result = event.data().dyn_into::<js_sys::Uint8Array>()
                .map(|bytes| bytes.to_vec())
                .map_err(|value| value.as_string().unwrap_or_else(|| "Mesh encoder sent invalid data".to_owned()));
            *message_result.borrow_mut() = Some(result);
            message_context.request_repaint();
        }) as Box<dyn FnMut(_)>);
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        let error_result = result.clone();
        let error_context = context.clone();
        let onerror = Closure::wrap(Box::new(move |error: web_sys::ErrorEvent| {
            *error_result.borrow_mut() = Some(Err(format!("Mesh encoder failed: {}", error.message())));
            error_context.request_repaint();
        }) as Box<dyn FnMut(_)>);
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        let job = Self { worker, result, _onmessage: onmessage, _onerror: onerror };
        if let Some(transfer) = transfer {
            job.worker.post_message_with_transfer(message, transfer)
                .map_err(|error| format!("Could not send mesh encoding request: {error:?}"))?;
        } else {
            job.worker.post_message(message)
                .map_err(|error| format!("Could not send mesh encoding request: {error:?}"))?;
        }
        Ok(job)
    }

    pub fn png(pixels: Vec<u8>, size: u32, context: &egui::Context) -> Result<Self, String> {
        let bytes = js_sys::Uint8Array::from(pixels.as_slice());
        let message = js_sys::Object::new();
        js_sys::Reflect::set(&message, &"kind".into(), &"png".into())
            .map_err(|error| format!("Could not prepare PNG encoding: {error:?}"))?;
        js_sys::Reflect::set(&message, &"pixels".into(), &bytes)
            .map_err(|error| format!("Could not prepare PNG pixels: {error:?}"))?;
        js_sys::Reflect::set(&message, &"size".into(), &size.into())
            .map_err(|error| format!("Could not prepare PNG size: {error:?}"))?;
        let transfer = js_sys::Array::new();
        transfer.push(&bytes.buffer());
        Self::start(&message, Some(&transfer), context)
    }

    pub fn glb(pages: &[super::textured_glb::Page],
        context: &egui::Context) -> Result<Self, String> {
        let mut meshes = Vec::new();
        let mut mesh_indices = std::collections::HashMap::new();
        let mut encoded_pages = Vec::with_capacity(pages.len());
        for page in pages {
            let pointer = Arc::as_ptr(&page.mesh) as usize;
            let mesh = *mesh_indices.entry(pointer).or_insert_with(|| {
                let index = meshes.len();
                meshes.push(page.mesh.clone());
                index
            });
            encoded_pages.push(EncodedPage { name: page.name.clone(), root: page.root,
                mesh, first: page.first, count: page.count, size: page.size, png: page.png.clone() });
        }
        let json = serde_json::to_string(&GlbRequest { meshes, pages: encoded_pages })
            .map_err(|error| error.to_string())?;
        let message = js_sys::Object::new();
        js_sys::Reflect::set(&message, &"kind".into(), &"glb".into())
            .map_err(|error| format!("Could not prepare GLB encoding: {error:?}"))?;
        js_sys::Reflect::set(&message, &"pages".into(), &json.into())
            .map_err(|error| format!("Could not prepare GLB pages: {error:?}"))?;
        Self::start(&message, None, context)
    }

    pub fn take_result(&self) -> Option<Result<Vec<u8>, String>> {
        self.result.borrow_mut().take()
    }
}

impl Drop for WebBytesJob {
    fn drop(&mut self) {
        self.worker.set_onmessage(None);
        self.worker.set_onerror(None);
        self.worker.terminate();
    }
}

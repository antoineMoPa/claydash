use super::WebDocumentMessage;
use std::sync::mpsc::Sender;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};

pub(super) struct WebPlyImport {
    worker: web_sys::Worker,
    pub name: String,
    pub request: u64,
    pub splats: Vec<claydash_engine::gaussian_ply::Gaussian>,
    pub loaded: usize,
    pub parsed: usize,
    pub total: usize,
    _onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _onerror: Closure<dyn FnMut(web_sys::ErrorEvent)>,
}

impl WebPlyImport {
    pub fn start(file: web_sys::File, request: u64, tx: Sender<WebDocumentMessage>, ctx: egui::Context) -> Result<Self, String> {
        let name = file.name();
        let options = web_sys::WorkerOptions::new();
        options.set_type(web_sys::WorkerType::Module);
        let worker = web_sys::Worker::new_with_options("./gaussian-ply-worker.js", &options)
            .map_err(|error| format!("Could not start Gaussian PLY worker: {error:?}"))?;
        let message_tx = tx.clone();
        let message_ctx = ctx.clone();
        let onmessage = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let data = event.data();
            let field = |name: &str| js_sys::Reflect::get(&data, &JsValue::from_str(name)).ok();
            let message = match field("kind").and_then(|value| value.as_string()).as_deref() {
                Some("batch") => field("data").map(|data| {
                    WebDocumentMessage::PlyBatch {
                        request,
                        data: js_sys::Uint8Array::new(&data).to_vec(),
                        count: field("count").and_then(|value| value.as_f64()).unwrap_or(0.) as usize,
                        loaded: field("loaded").and_then(|value| value.as_f64()).unwrap_or(0.) as usize,
                        total: field("total").and_then(|value| value.as_f64()).unwrap_or(0.) as usize,
                    }
                }),
                Some("complete") => Some(WebDocumentMessage::PlyComplete {
                    request,
                    total: field("total").and_then(|value| value.as_f64()).unwrap_or(0.) as usize,
                }),
                Some("progress") => Some(WebDocumentMessage::PlyProgress {
                    request,
                    parsed: field("loaded").and_then(|value| value.as_f64()).unwrap_or(0.) as usize,
                    total: field("total").and_then(|value| value.as_f64()).unwrap_or(0.) as usize,
                }),
                Some("error") => Some(WebDocumentMessage::PlyError {
                    request,
                    message: field("message").and_then(|value| value.as_string())
                        .unwrap_or_else(|| "Worker failed".into()),
                }),
                _ => Some(WebDocumentMessage::PlyError { request, message: "Invalid worker response".into() }),
            };
            if let Some(message) = message { let _ = message_tx.send(message); }
            message_ctx.request_repaint();
        });
        let error_ctx = ctx;
        let onerror = Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |event: web_sys::ErrorEvent| {
            let message = js_sys::Reflect::get(event.as_ref(), &JsValue::from_str("message"))
                .ok().and_then(|value| value.as_string())
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "Could not load Gaussian PLY worker script".to_owned());
            let _ = tx.send(WebDocumentMessage::PlyError { request, message });
            error_ctx.request_repaint();
        });
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        worker.post_message(&file).map_err(|error| format!("Could not send PLY file to worker: {error:?}"))?;
        Ok(Self { worker, name, request, splats: Vec::new(), loaded: 0, parsed: 0, total: 0, _onmessage: onmessage, _onerror: onerror })
    }
}

impl Drop for WebPlyImport {
    fn drop(&mut self) { self.worker.terminate(); }
}

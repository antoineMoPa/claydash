mod animation;
mod app;
#[cfg(not(target_arch = "wasm32"))]
mod cli;
use claydash_engine::camera;
mod cloud_edit;
mod commands;
mod document;
#[cfg(not(target_arch = "wasm32"))]
mod duck;
mod examples;
mod guides;
mod interactions;
mod model;
#[cfg(not(target_arch = "wasm32"))]
mod render_export;
#[cfg(target_arch = "wasm32")]
#[path = "web_render_export.rs"]
mod render_export;
use claydash_engine::renderer;
mod ui;
mod undo_redo;
#[cfg(target_arch = "wasm32")]
mod web_input;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static PLY_WORKER_SPLATS: std::cell::RefCell<Vec<claydash_engine::gaussian_ply::Gaussian>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "export function gaussianPlyProgress(loaded, total) { self.postMessage({kind:'progress', loaded, total}); }")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = gaussianPlyProgress)]
    fn gaussian_ply_progress(loaded: usize, total: usize);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn parse_gaussian_ply_worker(bytes: &[u8]) -> Result<usize, wasm_bindgen::JsValue> {
    let cloud = claydash_engine::gaussian_ply::read_with_progress(bytes, gaussian_ply_progress)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
    let count = cloud.splats.len();
    PLY_WORKER_SPLATS.with(|splats| *splats.borrow_mut() = cloud.splats);
    Ok(count)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn take_gaussian_ply_worker_batch(start: usize, count: usize) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    PLY_WORKER_SPLATS.with(|splats| {
        let splats = splats.borrow();
        let end = start.checked_add(count).filter(|end| *end <= splats.len())
            .ok_or_else(|| wasm_bindgen::JsValue::from_str("Invalid Gaussian batch range"))?;
        Ok(claydash_engine::gaussian_ply::encode_batch(&splats[start..end]))
    })
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn clear_gaussian_ply_worker() {
    PLY_WORKER_SPLATS.with(|splats| *splats.borrow_mut() = Vec::new());
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match cli::parse(&args) {
        Ok(cli::Launch::Help) => cli::print_help(),
        Ok(cli::Launch::Desktop) => app::run(),
        #[cfg(unix)]
        Ok(cli::Launch::Agent) => {
            app::agent::run_from_args();
        }
        Err(error) => {
            eprintln!("{error}\nRun claydash --help for usage.");
            std::process::exit(2);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    if web_sys::window().is_some() {
        app::run();
    }
}

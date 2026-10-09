mod animation;
mod app;
#[cfg(not(target_arch = "wasm32"))]
mod cli;
use claydash_engine::camera;
mod commands;
mod document;
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

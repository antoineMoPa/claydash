mod app;
mod camera;
mod commands;
mod document;
mod duck;
mod interactions;
mod model;
mod renderer;
mod ui;
mod undo_redo;
mod viewport;
#[cfg(target_arch = "wasm32")]
mod web_input;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    app::run();
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    app::run();
}

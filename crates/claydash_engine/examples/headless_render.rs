//! Render on a server without a window: cargo run -p claydash_engine --example headless_render -- output.png
//! Add --software to request a fallback adapter when available.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use claydash_engine::{
        model::{PrimitiveKind, SdfObject},
        renderer::HeadlessOptions,
        Camera, Renderer, World,
    };
    let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
        width: 320,
        height: 240,
        force_fallback_adapter: std::env::args().any(|arg| arg == "--software"),
        ..Default::default()
    }))?;
    let scene = vec![SdfObject::create_kind(PrimitiveKind::Sphere)];
    let image = renderer.render_offscreen(
        &Camera::new(),
        &scene,
        World::default(),
        &[],
        std::time::Duration::from_secs(60),
    )?;
    let path = std::env::args()
        .skip(1)
        .find(|arg| arg != "--software")
        .unwrap_or_else(|| "headless.png".into());
    image::save_buffer(
        &path,
        &image.rgba,
        image.width,
        image.height,
        image::ColorType::Rgba8,
    )?;
    println!("Rendered {}x{} to {path}", image.width, image.height);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}

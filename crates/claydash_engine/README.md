# claydash_engine

Reusable scene geometry and rendering for Claydash. This crate owns the CPU SDF
model and queries, materials, camera, Gaussian splats, depth atlases, neural SDFs,
Poisson mesh reconstruction and GLB export, GPU renderer, viewport refinement,
post processing and WGSL sources. The application owns commands, observable
editor state, documents, input and UI.

`model::SdfObject` and the other scene types retain their serde representation.
`model::scene_sample`, `scene_subtree_sample`, `boolean_distance`, `raymarch` and
`raymarch_hit` can be used without a window or GPU. Ray directions are unit vectors;
the selection query retains the editor's existing 100 unit range and hit tolerance.
These CPU scene queries are the shared foundation for future SDF physics.

`scene_language::compile(source)` parses `.claydef` recipes into deterministic
part paths and editable `SdfObject` values. `scene_language::bounds(objects)`
returns the generated geometry's world bounds, including curves and formed
sheets. These APIs are available on native and WebAssembly targets and require
no editor, agent session, file access or GPU. See the [language guide](../../docs/scene-definition-language.md).

```rust
let parts = claydash_engine::scene_language::compile("part = box(size: [2,1,1]);")?;
let objects: Vec<_> = parts.into_iter().map(|(_, object)| object).collect();
let (min, max) = claydash_engine::scene_language::bounds(&objects)?;
```

Run a standalone consumer with:

```sh
cargo run -p claydash_engine --example query_scene
cargo test -p claydash_engine
```

`Renderer::new` creates a device and presentation surface from an
`Arc<winit::window::Window>`, preserving the application's capture/export API.
For native servers, `Renderer::new_headless(renderer::HeadlessOptions)` returns
`Result<Renderer, String>` without creating a window or presentation surface.
`render_offscreen` accepts scene objects, camera, world and post processing and
returns a refined `CapturedFrame` with packed sRGB RGBA bytes. It sets the camera
viewport to the requested image dimensions and handles internal egui state;
callers need no egui context, UI or viewport keys.

```sh
cargo run -p claydash_engine --example headless_render -- output.png
cargo test -p claydash_engine --test headless_render -- --ignored
```

A supported wgpu backend and GPU driver must be available. Set
`HeadlessOptions::force_fallback_adapter` (or pass `--software` to the example)
to request a software adapter when the backend provides one. Missing adapters
and device initialization failures return errors. Dimensions are checked before
image allocation against device texture and readback limits.

Native `render_offscreen` blocks while refining and reading back the image. Its
timeout bounds GPU waits and refinement progress; synchronous CPU shader
compilation cannot be interrupted. Polling/readback errors and expiration return
errors, and the renderer can retry after a failed capture. Browser applications
retain the asynchronous existing render/capture path; the blocking helper is
native only. The renderer still uses egui internally for compositing and material
previews. CPU geometry and queries do not require an egui context.

Shaders, the text font and test fixtures are packaged here so source inclusion
does not depend on application asset paths. Browser material preview JavaScript
is resolved relative to this crate by wasm-bindgen. The application still packages
the mesh and GLB worker launchers; their exported worker functions live here.

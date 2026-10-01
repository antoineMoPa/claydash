# Rendering performance

The renderer supports 1,024 objects, including Boolean operands. Repetition can create many instances from one object. Benchmark the scene and viewport size you intend to improve: direct GPU frame time, editor frame time, preview resolution, and time to finish refinement measure different costs.

## Current rendering path

The UI stays at native resolution. Camera and object changes render an adaptive scene preview; stationary views refine in 32-pixel native-resolution tiles and cache the completed image. GPU timestamps target 6 ms of scene work during editing and 12 ms during animation playback. At most one batch is in flight. Without timestamp support, completion feedback limits backlog. An unchanged transform must not update the scene revision: otherwise a held drag restarts refinement on every redraw.

The default duck uses Deferred. It traces primary-hit geometry into position, normal, albedo, and optics buffers, then resolves lighting, SSAO, SSR, and thin-surface refraction. Screen-space effects need neighboring geometry, so the preview is lit immediately but native refinement is published only after **all** native geometry tiles are ready. Lighting-only changes can reuse that geometry. A final native image matches a direct render **of the same pipeline**, while a moving preview has less spatial detail.

Deferred approximates Exact ray shading. Reflections and refraction can see only on-screen geometry; missed rays use the environment. It cannot reproduce hidden objects, multiple transmission bounces, or all Exact contact shading. Use Exact when those details matter. Unsupported custom shaders, alpha image planes, and some Gaussian group configurations fall back to Exact. The full-screen lighting resolve still costs time proportional to viewport size. The World setting affects exports as well as the editor.

## Where to optimize

- Scene upload packs independent Boolean components in postorder and builds bounds and lookups once per scene revision. Shader variants include only material and primitive families present in the scene. Preserve those specialization keys when adding features.
- BVH traversal limits primary-ray candidates. Convex primitives use exact local ray intervals; eligible two-operand smooth subtractions and flat hard unions have specialized distance paths. General SDF marching remains necessary for unsupported shapes, repetition, modifiers, and complex CSG.
- Gaussian group captures are cached across rigid root translation and rotation. Geometry, scale, material, or modifier edits may rebake synchronously; unrelated edits can also invalidate the conservative whole-scene fingerprint. The raster splat path is limited to eligible opaque independent groups and has simpler lighting. See [group render optimizations](group-render-optimizations.md).
- Neural SDFs are optional approximations trained on GPU. Benchmarks wait for training so they do not measure the temporary Exact fallback. Validate fit error and visible surfaces before using them for performance claims; larger networks can render slower than Exact.
- Moving-object image ghosts would leave occlusion, shadows, and reflections stale. Full-resolution simple shading loses material detail. Prefer measuring the adaptive or deferred path before adding either shortcut.

## Reproduce and verify

```sh
# Default-scene direct throughput and full-resolution refinement equivalence.
cargo run --release -- --stress-benchmark --benchmark-scene=duck.claydash \
  --benchmark-size=1280x800 --benchmark-progressive

# Actual app loop: object edits or saved animation tracks.
cargo run --release -- --stress-ui-benchmark --benchmark-scene=duck.claydash \
  --benchmark-size=1280x800 --benchmark-edit
cargo run --release -- --stress-ui-benchmark --benchmark-scene=duck.claydash \
  --benchmark-size=1280x800 --benchmark-animation

# Wider scene coverage and browser compilation.
cargo run --release -- --stress-benchmark-suite --benchmark-progressive \
  --benchmark-size=128x72
cargo test --workspace
cargo check --target wasm32-unknown-unknown
```

`--benchmark-progressive` compares the completed viewport against a direct render of the same scene and pipeline (maximum one 8-bit channel of error). `--benchmark-images=PATH` saves PPM images for visual comparison. For an Exact comparison, use a temporary copy of the scene with `world.render_pipeline` set to `exact`; compare images as well as timings. Native GPU results are hardware-specific, and WebAssembly compilation alone does not measure browser performance.

## Material library previews

Material spheres are requested by visible UI rectangles, after filtering and section expansion. Startup and closed sections generate no thumbnails. Requests are refreshed each frame, so scrolling away discards pending demand. The renderer produces at most one new sphere per frame and waits for GPU completion before starting another. Cached textures are reused when reopening sections; edited or deleted shared materials invalidate their own entries. WGSL source changes invalidate preview pipelines and textures.

Preview pipelines enable only the requested material family and sphere geometry. Combining every material family in an eagerly compiled preview pipeline stalled Chrome's GPU queue. Preview compilation now runs asynchronously: browsers use WebGPU's `createRenderPipelineAsync` through a small device-handle bridge; native builds use a worker thread. Pending cards display spinners, and failed compilations show an error tooltip instead of retrying every frame. No draw using a new preview pipeline is submitted until compilation completes. Filtering/closing a section drops its thumbnail demand; a completed compiler job only populates the pipeline cache. Source changes discard the old result channel, so stale jobs cannot replace newer shaders. Preview uploads preserve scene background jobs and captures, and restore scene buffers before drawing the viewport.

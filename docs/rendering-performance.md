# Rendering performance

The renderer supports 1,024 objects, including Boolean operands. Repetition can create many instances from one object. Benchmark the scene and viewport size you intend to improve: direct GPU frame time, editor frame time, preview resolution, and time to finish refinement measure different costs.

## Current rendering path

The UI stays at native resolution. In Full Material mode, Exact starts shading native pixels immediately in a 16×16 ordered pattern across the entire image. Each pass adds one sample per block, and the composite reconstructs nearby pixels from completed samples. There is no coarse preview pass. After 256 phases every native pixel is exact and the image is cached. The explicit Simple Shading and Outline modes keep their own quick rendering path. Deferred and hybrid views retain 32-pixel native-resolution tiles. GPU timestamps target 6 ms of scene work during editing and 12 ms during animation playback. At most one batch is in flight. Without timestamp support, completion feedback limits backlog. An unchanged transform must not update the scene revision: otherwise a held drag restarts refinement on every redraw.

The default duck uses Deferred. It traces primary-hit geometry into position, normal, albedo, and optics buffers, then resolves lighting, SSAO, SSR, and thin-surface refraction. Screen-space effects need neighboring geometry, so the preview is lit immediately but native refinement is published only after **all** native geometry tiles are ready. Lighting-only changes can reuse that geometry. A final native image matches a direct render **of the same pipeline**, while a moving preview has less spatial detail.

Deferred approximates Exact ray shading. Reflections and refraction can see only on-screen geometry; missed rays use the environment. It cannot reproduce hidden objects, multiple transmission bounces, or all Exact contact shading. Use Exact when those details matter. Custom WGSL materials provide their primary-hit color and optical properties to Deferred, though effects such as custom coat lighting remain approximate. Alpha image planes and some Gaussian group configurations fall back to Exact. The full-screen lighting resolve still costs time proportional to viewport size. The World setting affects exports as well as the editor.

## Where to optimize

- Scene upload packs independent Boolean components in postorder and builds bounds and lookups once per scene revision. Shader variants include only material and primitive families present in the scene. Preserve those specialization keys when adding features.
- BVH traversal limits primary-ray candidates. Convex primitives use exact local ray intervals; eligible two-operand smooth subtractions and flat hard unions have specialized distance paths. General SDF marching remains necessary for unsupported shapes, repetition, modifiers, and complex CSG.
- Exact scene distance queries use transform-aware AABB lower bounds for eligible primitive components. Smooth Boolean blends reserve their maximum possible distance reduction; spatial modifiers and repetition retain the sphere bound. Independent convex polygon prisms clip rays to a rounded, local-space interval before SDF marching. Their SDF and shading are unchanged.
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

The `examples/fusca.claydash` scene uses Exact. Its lacquer asset and objects store the same teal base color as the custom WGSL output, avoiding a fuchsia fallback color. On an Apple M1 at 1280×800, repeated direct renders measured 149.4–167.8 ms/frame versus 186.9 ms/frame before the SDF bounds work. With the sparse progressive viewport starting directly with shaded samples, camera motion measured 13.4 ms p95, the native UI edit loop 15.2 ms p95, and refinement frames 14.2 ms p95. The native UI benchmark reported zero preview pixels. The fully refined image differs from the direct Exact image by at most one 8-bit channel value. A temporary copy set to Deferred with SSR enabled rendered at 77.3 ms/frame; its completed viewport matched its direct Deferred image byte for byte. Full-resolution direct rendering remains slower than the interactive viewport, so use the viewport timings when judging editing responsiveness.

## Headless initialization regression check

The headless constructor shares GPU resource initialization with the windowed
constructor. The normal window render path adds an optional-surface check and
retains the same allocations, passes, copies and GPU polling. Offscreen capture
performs refinement and readback only when requested.

On 2026-10-08, before/after optimized development builds were compared on Apple
M1 / Metal at 640×360, alternating process order. Both the Brick and Preview
benchmark output images were byte-identical.

| Measurement | Before | After | Runs per build |
| --- | ---: | ---: | ---: |
| Brick deferred GPU median | 21.975 ms | 22.383 ms | 10 |
| Preview deferred GPU median | 4.677 ms | 4.551 ms | 3 |
| Brick editor edit loop, median p50 | 6.601 ms | 6.091 ms | 3 |
| Brick editor edit loop, median p95 | 8.196 ms | 8.604 ms | 3 |

Brick GPU samples ranged from 19.892–23.967 ms before and 21.621–23.958 ms
after. The paired mean difference's approximate 95% confidence interval was
−0.197 to +1.519 ms. These measurements did not establish a performance
regression; they do not rule out small differences or replace testing on other
hardware. The editor benchmark exercises the complete window render path;
the direct GPU benchmark measures scene throughput.

Reproduce with `--stress-benchmark-suite --benchmark-case=brick
--benchmark-size=640x360 --benchmark-images=PATH`, repeat with
`--benchmark-case=preview`, and use `--stress-ui-benchmark --benchmark-edit
--benchmark-case=brick --benchmark-size=640x360` for the editor loop.

## Material library previews

Material spheres are requested by visible UI rectangles, after filtering and section expansion. Startup and closed sections generate no thumbnails. Requests are refreshed each frame, so scrolling away discards pending demand. The renderer produces at most one new sphere per frame and waits for GPU completion before starting another. Cached textures are reused when reopening sections; edited or deleted shared materials invalidate their own entries. WGSL source changes invalidate preview pipelines and textures.

Preview pipelines enable only the requested material family and sphere geometry. Combining every material family in an eagerly compiled preview pipeline stalled Chrome's GPU queue. Preview compilation now runs asynchronously: browsers use WebGPU's `createRenderPipelineAsync` through a small device-handle bridge; native builds use a worker thread. Pending cards display spinners, and failed compilations show an error tooltip instead of retrying every frame. No draw using a new preview pipeline is submitted until compilation completes. Filtering/closing a section drops its thumbnail demand; a completed compiler job only populates the pipeline cache. Source changes discard the old result channel, so stale jobs cannot replace newer shaders. Preview uploads preserve scene background jobs and captures, and restore scene buffers before drawing the viewport.

Material previews use a dedicated sphere shader assembled from the shared material evaluators, lighting, environment and parameter ABI. Its source omits scene traversal, Boolean operations, captured geometry, modifiers and deferred entrypoints, and includes only the requested evaluator. Ordinary sphere hits are analytic; Fabric retains its sphere distance marcher and woven relief. Full optical transport, finite difference normals, refraction, hemisphere AO, Metal environment integration and transparent alpha are preserved.

On the same Mac/Chrome installation, fresh isolated profiles measured first-use `createRenderPipelineAsync` completion at 129 ms for Basic, 300 ms for Brick, 265 ms for Fabric and 455 ms for Metal. The preserved full-scene preview build took 1,163 ms, 23,297 ms, 26,144 ms and 1,632 ms respectively. Fresh profiles do not guarantee cleared OS/driver caches. Optimized interaction frame gaps stayed within 16.8 ms and produced no WebGPU errors. Independent raw GPU comparisons of all 36 presets found byte-identical alpha and at most 0.016/255 mean RGB difference per card; the default scene viewport also matched exactly. Recheck these measurements when material or transport code changes.

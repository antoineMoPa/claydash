# Rendering performance

The renderer supports 1,024 objects, including Boolean operands. Repetition can create many instances from one object. Benchmark the scene and viewport size you intend to improve: direct GPU frame time, editor frame time, preview resolution, and time to finish refinement measure different costs.

## Current rendering path

The UI stays at native resolution. Both Simple Shading and Full Material use bounded source-rendering batches. During navigation, the viewport displays a GPU-timed coarse preview; after navigation stops, it fills in native pixels and caches the completed image. Simple Shading uses native tiles with its fast shader. Stationary Full Material Exact first fills a quick coverage sample in each 16×16 cell, then shades scattered single native pixels in 256 ordered phases. Batches can stop inside a phase when a whole phase exceeds the measured budget. The compositor publishes every completed native sample, including partial phases, and smoothly reconstructs the background from completed samples. Missing samples retain the navigation preview; completed phase groups progressively improve the reconstructed background. Deferred and hybrid views use 32-pixel native tiles. GPU timestamps target 8 ms of scene work during editing and 12 ms during playback. Motion previews can shrink below the native tile size, down to a 256-pixel budget, while full-material sparse batches can stop after one pixel and the initial coverage pass keeps at least 64 samples per batch. Refinement submissions are spaced by at least 16 ms; completion polling does not add another idle frame. Native automatic refinement redraws are paced; input can request an immediate redraw.

Only one scene batch is in flight. While its fence is unfinished, the renderer polls without waiting and defers additional presentation submissions before acquiring a surface drawable. This keeps cached UI frames from exhausting the drawable pool behind slow scene work. Without timestamp support, multi-frame completion feedback still lowers the next batch budget. A new view caps inherited throughput to its initial scene budget, so cheap background work cannot inflate a rotated view's first preview. Unchanged transforms must not update scene revisions, otherwise a held drag restarts refinement.

The default duck uses Deferred. It traces primary-hit geometry into position, normal, albedo, and optics buffers, then resolves lighting, SSAO, SSR, and thin-surface refraction. Screen-space effects need neighboring geometry, so the preview is lit immediately but native refinement is published only after **all** native geometry tiles are ready. Lighting-only changes can reuse that geometry. A final native image matches a direct render **of the same pipeline**, while a moving preview has less spatial detail.

Deferred approximates Exact ray shading. Reflections and refraction can see only on-screen geometry; missed rays use the environment. It cannot reproduce hidden objects, multiple transmission bounces, or all Exact contact shading. Use Exact when those details matter. Custom WGSL materials provide their primary-hit color and optical properties to Deferred, though effects such as custom coat lighting remain approximate. Alpha image planes and some Gaussian group configurations fall back to Exact. The full-screen lighting resolve still costs time proportional to viewport size. The World setting affects exports as well as the editor.

## Where to optimize

- Scene upload packs independent Boolean components in postorder and builds bounds and lookups once per scene revision. Shader variants include only material and primitive families present in the scene. Preserve those specialization keys when adding features.
- BVH traversal limits primary-ray candidates. Convex primitives use exact local ray intervals; eligible two-operand smooth subtractions and flat hard unions have specialized distance paths. General SDF marching remains necessary for unsupported shapes, repetition, modifiers, and complex CSG.
- Exact scene distance queries use transform-aware AABB lower bounds for eligible primitive components. Smooth Boolean blends reserve their maximum possible distance reduction. Eligible independent cage-deformed primitives use displacement-expanded bounds and conservative affine distance factors; other spatial modifiers and repetition retain the sphere bound. Independent convex polygon prisms clip rays to a rounded, local-space interval before SDF marching. Their SDF and shading are unchanged.
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

## Nested hard-union assemblies

Pure hard-union assemblies are now flattened in the GPU copy before splitting
into scene BVH leaves. Recomputed bounds keep each detached part tightly bounded.
Independent convex parts can use analytic ray intervals; formed sheets no longer
force unrelated primitives to march through the entire subassembly. The editable
hierarchy remains intact. Subtraction/intersection, blends, mirrors, repetition
and captured subtrees keep their existing safeguards and evaluation paths.
Sheet inverse charts also skip relief evaluation when their domain or planar
lower bound proves they cannot improve the current distance.

Sparse adaptive feedback compares submitted pixels with the achievable batch
(capped at 256 phases), rather than an arbitrarily larger requested budget. Its
minimum is one native sample rather than one 32×32 tile. Otherwise small,
expensive views continually ignore their GPU samples or stop adapting too early.
Exact rays are reconstructed from native pixel centers so sparse quads and the
full-screen triangle produce identical completed images.

On Apple M1 / Metal, the saved 195-part Westfalia (56 sheets) at 160×100 measured
279.6 ms per direct Exact frame before these changes and 96.7 ms afterward,
about 2.9× faster. With the traversal/sheet changes alone, viewport motion p95
was 64.2 ms and editing p95 was 62.0 ms. Corrected adaptive feedback reduced
those to 19.0 ms and 13.7 ms. At 320×200 the final renderer measured 111.7 ms
per direct frame, 17.3 ms motion p95 and 14.9 ms editing p95. Both completed
progressive images matched their direct reference byte for byte. These are
bounded headless measurements with a fixed overview camera, not native editor
or full-size viewport measurements. Initial interaction batches can still be
slower before timing feedback adapts.

The old and new renderer need not produce identical pixels at intersecting
parts: splitting components changes finite-difference normal neighborhoods and
hit ordering. At 160×100, the flatten/split change affected 238 pixels with
0.121/255 mean RGB difference; scene geometry and materials were not edited.
Sheet pruning alone was byte-identical to the split-only image.

Run the windowless example with a saved scene, keeping GPU runs sequential:

```sh
cargo run -p claydash_engine --example scene_benchmark -- \
  --benchmark-scene=PATH --benchmark-size=160x100 --benchmark-progressive \
  --benchmark-images=/tmp/claydash-profile
```

Optional `--camera-position=X,Y,Z` and `--camera-target=X,Y,Z` override the
vehicle overview camera. `--benchmark-deferred` measures Deferred when supported;
`--benchmark-fast-preview` measures Simple Shading. Image files are PPM.
The convergence check allows up to 120 seconds after interaction stops,
so its limit covers the coverage pass, 256 Exact pixel phases and completion/pause frames even
when a small viewport cycles through cached frames quickly.

## Material library previews

Material spheres are requested by visible UI rectangles, after filtering and section expansion. Startup and closed sections generate no thumbnails. Requests are refreshed each frame, so scrolling away discards pending demand. The renderer produces at most one new sphere per frame and waits for GPU completion before starting another. Cached textures are reused when reopening sections; edited or deleted shared materials invalidate their own entries. WGSL source changes invalidate preview pipelines and textures.

Preview pipelines enable only the requested material family and sphere geometry. Combining every material family in an eagerly compiled preview pipeline stalled Chrome's GPU queue. Preview compilation now runs asynchronously: browsers use WebGPU's `createRenderPipelineAsync` through a small device-handle bridge; native builds use a worker thread. Pending cards display spinners, and failed compilations show an error tooltip instead of retrying every frame. No draw using a new preview pipeline is submitted until compilation completes. Filtering/closing a section drops its thumbnail demand; a completed compiler job only populates the pipeline cache. Source changes discard the old result channel, so stale jobs cannot replace newer shaders. Preview uploads preserve scene background jobs and captures, and restore scene buffers before drawing the viewport.

Material previews use a dedicated sphere shader assembled from the shared material evaluators, lighting, environment and parameter ABI. Its source omits scene traversal, Boolean operations, captured geometry, modifiers and deferred entrypoints, and includes only the requested evaluator. Ordinary sphere hits are analytic; Fabric retains its sphere distance marcher and woven relief. Full optical transport, finite difference normals, refraction, hemisphere AO, Metal environment integration and transparent alpha are preserved.

On the same Mac/Chrome installation, fresh isolated profiles measured first-use `createRenderPipelineAsync` completion at 129 ms for Basic, 300 ms for Brick, 265 ms for Fabric and 455 ms for Metal. The preserved full-scene preview build took 1,163 ms, 23,297 ms, 26,144 ms and 1,632 ms respectively. Fresh profiles do not guarantee cleared OS/driver caches. Optimized interaction frame gaps stayed within 16.8 ms and produced no WebGPU errors. Independent raw GPU comparisons of all 36 presets found byte-identical alpha and at most 0.016/255 mean RGB difference per card; the default scene viewport also matched exactly. Recheck these measurements when material or transport code changes.

### Ambient fill calibration

World ambient light is a multiplier: `0` disables indirect fill, `1` is the
baseline, and values up to `30` remain supported in saved scenes. The logarithmic
slider gives finer adjustment near zero (down to `0.01`) without changing the
stored value or requiring a scene migration.

Simple Shading and refined surface shading share the same ambient-fill function.
Diffuse ambient fill uses a soft hemisphere: upward surfaces receive 100%,
vertical surfaces 80%, and downward surfaces 60% of the baseline. Simple and refined surface shading share the environment and hemisphere fill
function, including the wood baseline. Exact and deferred shading use the same `0.13` studio baseline; sky and night retain their
existing environment baselines. Ambient occlusion still attenuates the fill.
Direct illumination is independent of the ambient multiplier. Gaussian splats
use the same hemisphere, and mesh preview bounce fill follows the multiplier
instead of keeping an unconditional brightness floor. Specialized metal
reflection and wood material responses retain their material-specific calibration.

## Profiling navigation

CPU-only saved-scene profiling reports pan-anchor queries, mesh revision work, and UI/tessellation while moving the camera:

```sh
cargo test --bin claydash profile_navigation_cpu -- --ignored --nocapture
```

A private, windowless GPU profile submits ten sequential panning frames per shading mode and reports CPU submission time, GPU completion time, timestamped scene time, and preview pixel count:

```sh
CLAYDASH_PROFILE_SIZE=1165x1448 cargo test -p claydash_engine --lib profile_navigation_gpu -- --ignored --nocapture --test-threads=1
```

The native editor loop can drive the real right-button pan interaction and report frame intervals:

```sh
cargo run --release -- --stress-ui-benchmark --benchmark-pan --benchmark-scene=examples/astra_car.claydash --benchmark-size=1600x1000
```

The native run opens its own temporary benchmark window and exits after 130 frames. It does not start the shared agent bridge or save scene changes.

Both default to `examples/astra_car.claydash`; `CLAYDASH_PROFILE_SCENE` selects another saved scene. GPU-only `CLAYDASH_PROFILE_VARIANT=no_bloom` or `no_lattice` isolates those costs in the private profile copy. Those variants change the comparison scene and are diagnostic, not final performance claims. The profile does not open a window, use the live agent socket, or write the source scene.

Astra's uncomputed Gaussian-splat choice falls back to exact geometry. Hard unions in that fallback can now use per-part scene-BVH culling even with a shared deformation cage: each uploaded operand retains its cage mapping, primitive bounds include maximum cage displacement, and nonuniform transforms retain conservative distance factors. Displayed splat proxies keep their capture boundaries. Smooth unions, root repetition/mirrors, inlay hosts, and active meshes retain their existing exclusions.

Bloom follow-up: use `CLAYDASH_PROFILE_PIXELS=1024` to hold preview work constant when comparing `scene`, `no_bloom`, and `no_emission` variants. At 1165×1448 on M1, the matched 980-pixel Astra preview measured 8.54 ms GPU with bloom versus 7.96 ms without (simple), and 7.71 versus 6.92 ms (full moving preview). These are GPU completion times, not end-to-end interaction latency. Native UI benchmarks now also report actual presentation intervals; UI-loop intervals alone do not establish fresh-image cadence.

With the final 8 ms target, the bounded native debug pan test at 1600×1000 measured actual presentation intervals of 16.744 ms median / 18.040 ms p95 over 118 intervals, with 1,184 median preview pixels. The earlier 4 ms target gave 56 preview pixels at comparable presentation cadence. These coarse moving previews finish at native detail after input stops.

Westfalia successive-camera-turn repro: a live254-object scene at1165×1448 had Full Material enabled but an adaptive budget of one native sample, leaving the7×8 preview nearly unchanged. The initial quick coverage pass has a 64-sample batch floor, scattered native work has a one-pixel floor, and new camera previews retain at least the 256-pixel budget. Completed samples also reconstruct a smooth background that improves as refinement progresses. Transitioning from quick coverage to full materials discards cheap throughput feedback. A private snapshot recovered whole-view coverage within the75-frame (<4s including deliberate pauses) verification run; full native quality remains expensive. Final staged sparse output matches the direct native reference exactly in GPU regression tests.


Progressive ownership, failure recovery and kernel decomposition are documented in [progressive-render-scheduler.md](progressive-render-scheduler.md). `CLAYDASH_PROFILE_STATIONARY=1 CLAYDASH_PROFILE_TURNS=6 CLAYDASH_PROFILE_ORBIT=1 CLAYDASH_PROFILE_FINISH=1` requires a saved scene to finish after orbiting. Set `CLAYDASH_PROFILE_IMAGE=/tmp/scene.ppm` to compare the completed source image with an independent raster-order reference and a subsequent white-clear probe. Keep complex-sheet runs small and sequential: a single large direct GPU draw can itself exceed the driver's limits. The small Westfalia six-orbit validation is 160×100, not the desktop viewport's full resolution.

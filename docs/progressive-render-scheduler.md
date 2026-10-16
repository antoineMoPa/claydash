# Progressive render scheduling

The viewport must make forward progress after navigation stops without depending on optional profiling callbacks. Image correctness, GPU submission ownership, throughput estimation, and window wakeups are separate concerns.

## Options and choice

| Pattern | Benefit | Tradeoff |
| --- | --- | --- |
| Dedicated render worker | Moves CPU encoding and compilation away from input handling | Does not by itself fix GPU queue ownership, stale callbacks, or adaptive starvation; adds resource and presentation synchronization |
| Generation-owned scheduler with finite work plans | Gives every submitted batch one completion owner; isolates stage feedback; defines cancellation, pause, progress, and wake behavior | Requires replacing shared flags and the shared timing mailbox |
| Watchdog that resets a stalled viewport | Can recover some transient failures | Can repeatedly throw away useful work; cannot safely pretend an unfinished GPU submission completed |

The implementation chooses a generation-owned scheduler plus independent contact-shading and material-transport kernels. The Westfalia repro also exposed real Metal command-buffer failures; state ownership alone cannot repair a failed GPU. A worker can be added later for measured CPU bottlenecks while retaining the same protocol. A watchdog is not used to bypass real GPU fences.

## Ownership and progress

`viewport/scheduler.rs` owns an `Idle -> Encoded -> Submitted -> Idle` protocol. Encoding does not create an in-flight GPU fence. Queue submission creates a unique completion object for that batch. A queue completion callback releases it; timestamp readback only adds an observation to that same object. A callback from a previous batch cannot release the current one.

View/document changes advance a generation. A real submitted fence survives replacement, so the renderer does not pile up old work or reuse resources behind unfinished work. Feedback from obsolete generations is ignored. Dropping an encoder rolls the unpublished cursor back for retry rather than leaving a phantom pending flag.

A finite `Plan` chooses coverage, sparse native pixels, or native tiles. Every ready batch has a strictly larger ending cursor, even when an estimator receives a zero budget. Coverage batches stop at the material-stage boundary. Interaction and optimization pause the current plan; they do not choose a different refinement representation. A camera/scene change replaces the plan intentionally.

Preview, quick coverage, native sparse pixels, native tiles, and relighting have distinct typed throughput histories. A slow full-material block cannot collapse the next camera preview or its coverage budget. Minimum work sizes are properties of each stage. Missing, zero, NaN, late, and obsolete timing observations cannot prevent forward progress. Timestamp-free long completed batches reduce their own stage's budget once, independent of how often the UI polls.

`preview_ready` records actual encoding, so allocating native-sized targets alone does not claim a complete image. Completion additionally requires the real GPU fence to be done. Capture may encode its readback immediately after the final draw in the same ordered submission.

## Window driver

The native driver continues nonblocking GPU polls on paced redraws while work is pending. Each GPU completion also explicitly requests a window redraw. Input requests immediate redraw. An inactive/occluded window intentionally parks with `ControlFlow::Wait`, replacing any expired refinement deadline; focus/unocclusion requests redraw and resumes polling. Diagnostics distinguish GPU wait, interaction pause, optimization pause, pacing, ready refinement, preview, and completion.

A physical GPU/driver failure is an explicit terminal outcome, not successful progress. Each submission reads back a completion marker independently of optional timestamps. A missing marker after a completed fence faults immediately, including before the next scheduler poll. An unfinished fence has a two-second recovery deadline; the scheduler never pretends it completed. The native app recreates the renderer, replays egui textures (including partial font-atlas updates), rebuilds GPU mesh buffers from retained CPU caches, and selects Simple Shading, then Outline if necessary. The editor document and selection remain in place; recovery happens automatically without a popup. Headless captures return an error instead of a successful black image.

The marker catches command buffers that never execute their tail copy. It is not a universal detector for partial driver failures: Metal can fail an individual render encoder while executing later copies. Pixel comparisons are therefore required alongside completion assertions.

## Bounded shader work

On the captured 254-object Westfalia, one monolithic fragment kernel combining crowned-sheet tracing, eight contact probes and reflection/transmission could abort Metal work. Disabling bloom did not remove it. Full-material transport without the probes and primary-only transport completed. A standalone contact pass keeps those eight probes out of the live reflection/transmission kernel; transport reads their R32Float cache at the same native pixel. Source geometry and material shading stay intact.

For sparse refinement, the two passes draw instanced one-pixel quads in 256 distributed phases. Each batch uses one instanced draw rather than one scissor/draw call per pixel. The compositor preserves completed native texels and smoothly reconstructs the background between them from completed samples. Missing corners retain the navigation preview. Completed phase groups improve the reconstruction grid from 16-pixel to 8-, 4-, and 2-pixel spacing; a coverage sample is never replicated as a solid 16×16 block.

The two passes still shade individual sparse pixels. Rectangle coalescing reproduced partial magenta regions even after splitting the kernels, while individually submitted reference pixels rendered those regions correctly. Sparse work now uses instanced single-pixel draws; tile-based paths retain rectangle batches. Budgets count native pixels, with a minimum of one; coverage retains its own 64-sample floor and navigation previews have a 256-pixel floor. Missing or invalid timestamps can grow the budget through cheap regions, while implausible timestamps exceeding the observed fence interval are rejected. The scene budget ceiling also bounds CPU command encoding.

Contact and transport costs both belong to the same timed batch. Standalone material-thumbnail preparation does not reset the viewport's adaptive ceiling. Work pacing and physical fences remain independent of optional timing feedback.

## Verification

The pure scheduler tests exhaust finite plans for 500 successive views with changing dimensions and modes, delayed and absent timing observations, invalid timings, old callbacks, and abandoned encoders. They require every stable plan to finish and every submitted batch to retire once. Separate tests preserve old fences across replacement and prove native timing cannot contaminate preview/coverage estimates.

GPU regressions exercise repeated navigation and interaction toggles without changing the stable plan's generation, require completion, and compare the final pixels to a direct native reference. Other checks cover partial publication, dropped encoders, mesh/simple/deferred paths, scene replacement, and selective neon bloom. The opt-in saved-scene profile can require actual convergence after camera changes:

```sh
CLAYDASH_PROFILE_SCENE=/path/to/scene.claydash \
CLAYDASH_PROFILE_SIZE=320x200 \
CLAYDASH_PROFILE_STATIONARY=1 CLAYDASH_PROFILE_TURNS=6 CLAYDASH_PROFILE_ORBIT=1 \
CLAYDASH_PROFILE_FRAMES=1600 CLAYDASH_PROFILE_FINISH=1 \
cargo test -p claydash_engine profile_navigation_gpu -- --ignored --nocapture --test-threads=1
```

The stationary profile currently uses the captured Westfalia camera pose. It is a private headless run; it does not alter a running document. Convergence and native pixel equivalence are required evidence; a fast UI loop or a visible coarse preview alone is insufficient.

The captured original Westfalia (254 objects, bloom inactive) completed the six-orbit test at 160×100 in 36.976 seconds, with byte-identical RGB output against independent raster-order single-pixel contact/transport draws. The queue still executed a separate white-clear probe afterward. CPU submission p95 was 1.818 ms; batch completion p95 was 45.569 ms. This is evidence of convergence and bounded CPU work, not real-time native-resolution source rendering. Full source shading remains expensive; direct meshes are the faster interactive path. The private source snapshot and the running editor document are unchanged.

The production guarantees are conditional on the window being active, intentional interaction/optimization pauses ending, and the GPU completing work or reporting a detectable fault. Completion markers cannot prove every individual pixel on a faulty driver. Regression images, independently ordered draws, finite plans and explicit failure recovery cover different failure modes rather than substituting for one another.

The original captured angle also passes after six pans: 794 paced frames / 30.157 seconds at 160×100, exact RGB match against the independent reference, and a successful subsequent white-clear probe. Finite guards on degenerate contact normals/probe accumulation resolved an intermediate two-pixel reference discrepancy. Both final saved-scene checks require exact equality rather than an image-error tolerance.

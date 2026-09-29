# Rendering performance

The renderer supports up to **1,024 objects**. Boolean operands count toward that limit;
finite repetition can produce many instances from one object.

The editor keeps its UI at native resolution. During camera or scene edits it renders a
fresh preview whose size follows a GPU time budget. Once the view stops changing, it
refines that image at native resolution in small batches and caches the finished result.
Dense glass, nested booleans, and repetition may take several seconds to finish refining.
A finished image is pixel-equivalent to a direct native-resolution render; the motion
preview intentionally has less spatial detail. Geometry, material, selection, projection,
camera, and viewport-size changes invalidate the cache.

## Shader work

- Traverse the spatial hierarchy once per outside ray, then intersect only candidate
  components. Spheres, boxes, and cylinders without repetition use exact local-space ray
  intervals, including nonuniform scaling. Two-operand convex subtractions use those
  intervals to skip empty ray segments before evaluating the original smooth SDF.
  Interior rays can bracket a slow smooth exit against the exact convex exit.
  Eligible pairs evaluate the same smooth subtraction formula directly during ray
  marching, membership checks, and scene-distance queries, and use analytic primitive
  gradients and smooth blend weights for normals. Rounded boxes, tori, repetition,
  and larger CSG groups retain general SDF marching.
- Follow the current component while a transmitted ray is inside it. Check membership in
  other components at exit boundaries, so overlapping solids remain a union.
- Compute normals from the hit component, avoiding four whole-scene distance searches.
- Bound contact-shading distance queries by each probe's reach and stop once a probe
  is inside any component. Distances outside that range produce the same shading.
- Skip surface lighting when opacity is zero. Treat a refractive index of exactly
  1 as a matched interface with zero Fresnel reflection, avoiding reflection
  rays caused by Schlick's approximation at grazing angles.
- Upload independent boolean components in contiguous postorder. Bounds follow boolean
  volume semantics: unions can expand, subtraction cannot expand, and intersection can
  shrink or become empty. Size shader scratch storage to the largest component, with a
  dedicated two-operand path, rather than the entire object count.
- Share one secondary-ray tracing call site between reflection and transmission. Preserve
  up to twelve surface interactions, Fresnel response, entry/exit refraction, and total internal
  reflection. Metal and the first glass interface reflect scene geometry. Later glass
  reflections sample the studio environment; the transmitted path still traces geometry.
  This is a real-time material approximation, not a path tracer.
- Build parent/selection lookups once per upload. Reuse scene uploads when their versions
  have not changed.
- Compile scene pipelines for the wood, brick, and diagnostic material families
  and for polygon prism, Bézier curve, and loft primitives actually present in
  that scene. Material previews enable every material family but only their
  unmodified sphere geometry. Thumbnail uploads reuse that dedicated pipeline
  without compiling viewport variants for each preset. Startup viewport pipelines
  disable optional features until the first scene upload. A custom
  shader edit defers scene-pipeline compilation until the next scene upload, when
  the active Boolean or non-Boolean variant is known.

## Viewport scheduling

GPU timestamp queries tune scene batches toward 6 ms where supported. Initial budgets
are conservative for large transparent/boolean/repeated scenes. Only one scene batch can
be in flight. Object edits retain the measured pixel budget instead of resetting to the
startup resolution on every transform. Changing to a more expensive scene class still
reduces the budget immediately; timestamp feedback handles subsequent cost changes.
Devices without timestamp queries use completion feedback and reduce work
if a batch falls behind. No blocking GPU waits run in the interactive renderer.

Refinement uses 32-pixel tiles, coalescing adjacent tiles in a row into a single draw. The
compositor reads only completed native-resolution tiles; other pixels come from the latest
preview. UI-only frames reuse the cached scene. Benchmark-only waits have a 15-second
per-submission timeout.

## Hybrid Gaussian splats

Eligible Gaussian groups use their baked surface samples as instanced camera-facing
quads. The SDF pass shades other scene objects and writes the nearest opaque SDF hit
to a depth texture. The quads test that depth without writing it, then composite in
back-to-front order with premultiplied alpha. Preview and native refinement tiles use
the same passes, so scene-only captures contain the final composition.

The mesh path currently accepts independent Gaussian roots with opaque Solid source
materials. It uses the existing ray compositor for transparent or custom materials,
image stencils, inlays, repeated or mirrored Gaussian roots, and
scenes without BVH tracing. Secondary SDF reflection rays still trace splats through
the existing BVH. Quad shading uses the captured base color and normal with diffuse
Studio or Sky lighting; it omits the full material shader's specular and contact
shading. Independent captures bake the nearest inherited or object cage into the
surface samples, expand their capture bounds, and clear the proxy cage to avoid a
second deformation. Nested captures retain the existing inherited-cage path.

Capture setup resolves child links, transforms, path profiles, and text once. Flat
hard unions also prune primitive evaluations with conservative distance bounds.
Completed atlases are cached across independent root group translation and rotation;
geometry, scale, material, modifier, path, and representation changes invalidate
them. Camera and selection changes reuse the atlas. The cache currently compares
the whole source scene conservatively, so unrelated geometry edits can also cause a
rebake. World-space text/path/inlay references retain the group pose in the key.
Only captures admitted by the shared atlas budget are retained, with compact source
fingerprints. Returning a group to Exact releases its cached atlas, so converting
it again requires a new bake.
Cold capture and geometry edits of large Gaussian groups can still pause while
the CPU samples their surfaces; the capture cache and scene pipeline cache remove
that work from ordinary rigid moves.

On an Apple M1 Metal run with a 320×240 duck fixture, the active hybrid path drew
10,663 instances in 2.99 ms per direct frame. The progressive image matched the
direct image exactly. The same fixture showed an edit frame around 343 ms when
capture data was regenerated. These measurements describe that fixture only.

Capture budgeting counts the uploaded sample records and occupied-sample BVH
nodes. The default duck and a union of its three boxes can both use Gaussian
splats within the shared buffer (17,045 instances in this fixture).
To exercise the actual Union command and representation changes on one renderer:

```sh
cargo run --offline -- --stress-benchmark --benchmark-default-group-transitions \
  --benchmark-size=320x240 --benchmark-progressive \
  --benchmark-images=/tmp/claydash-default-transitions
```

This covers exact boxes, Gaussian boxes, restoring exact boxes, both atlas
representations, and deferred composition while the duck remains Gaussian.

### Astra car workflow (Apple M1 / Metal, 2026-09-27)

The user-supplied car contains 156 objects and 11 active lattice cages. Its saved
scene is already a union with Gaussian representation. The workflow benchmark
clones that scene, renders the saved state, switches to Exact, executes the actual
Union action, converts to Gaussian, moves the whole group, then clears selection.
It does not modify the input file. Reapplying Union to an existing union measures
the action and renderer invalidation, rather than reconstructing an unsaved scene.

```sh
cargo run --offline -- --stress-benchmark --benchmark-group-workflow \
  --benchmark-scene=examples/astra_car.claydash --benchmark-size=192x144 \
  --benchmark-images=/tmp/astra-workflow
cargo run --offline -- --stress-ui-benchmark --benchmark-move-group \
  --benchmark-scene=examples/astra_car.claydash --benchmark-size=800x600
```

One local run measured 3,955 ms for saved-scene preparation, 4.4 ms for the Union
edit, 2,296 ms for conversion preparation, 13.1 ms for a rigid group move, and
11.6 ms for selection invalidation. Preparation includes GPU upload and any shader
pipeline compilation. The car produced 11,221 raster splat instances, with direct
frames around 1.2 ms at 192×144. A separate full-car camera view measured 1.96 ms
at 640×360. These raster timings exclude capture preparation.

The native UI benchmark with a full-car camera and an 800×600 window measured
14.4 ms median and 15.8 ms p95 over 119 sampled frames, with a median adaptive
preview of 101,728 pixels. `--benchmark-move-group` changes the group pose;
`--benchmark-edit` changes an operand and can legitimately require a new bake.
Cold captures remain synchronous and take seconds on this fixture; moving the
cached group no longer repeats that work. Gaussian captures remain approximate
and soften small details.

Exact grouped rendering in the original close camera view improved from 433.1 to
348.7 ms per 192×144 frame, with byte-identical output. Shared-cage unions can
accelerate eligible operands while evaluating other cages and unsupported distance
bounds separately. Undeformed groups keep the linear path when a partial tree
would add overhead: the no-lattice control measured 163.4 ms before and 162.7 ms
after, also with identical pixels. Exact rendering of this dense, deformed union
remains expensive. Its first pipeline preparation in the workflow took 11 seconds;
subsequent Union preparation took 11.7 ms. Capture caching does not eliminate first
use shader compilation.

## Reproduce

```sh
# Original-sized stress fixture; accelerated and reference traversal.
cargo run -- --stress-benchmark
cargo run -- --stress-benchmark-brute-force

# Native-resolution throughput, camera motion, stationary edits, refinement, and idle.
cargo run -- --stress-benchmark-suite --benchmark-progressive \
  --benchmark-1024 --benchmark-size=1280x720 \
  --benchmark-images=/tmp/claydash-images

# Actual native application loop, including egui and presentation.
cargo run -- --stress-ui-benchmark --benchmark-case=nested \
  --benchmark-1024 --benchmark-size=1280x720

# Continuous object transforms, reporting frame time and median preview pixels.
cargo run -- --stress-ui-benchmark --benchmark-edit --benchmark-case=preview \
  --benchmark-size=1280x720

# Orthographic ray / compositing coverage.
cargo run -- --stress-benchmark-suite --benchmark-progressive --benchmark-orthographic

# Before/after visual comparison using the original material algorithm (<=256 objects).
cargo run -- --stress-benchmark-suite --benchmark-case=preview \
  --benchmark-shader=tests/fixtures/sdf_reference.wgsl \
  --benchmark-images=/tmp/claydash-before
cargo run -- --stress-benchmark-suite --benchmark-case=preview \
  --benchmark-images=/tmp/claydash-after

cargo test --workspace
cargo check --target wasm32-unknown-unknown
```

`--benchmark-case=` accepts `solid`, `metallic`, `transparent`, `wood`, `mixed`, `booleans`,
`nested`, `repeated`, `preview`, `wood-gallery`, and `wood-cut`. The wood gallery shows
Pine, Oak, and Walnut blocks; the wood cut drills an Oak block. The repeated fixture contains 64 objects with 27
instances each. The other stress fixtures contain 256 objects, or 1,024 with
`--benchmark-1024`; the preview contains 10. Mixed fixtures cover all four primitives,
rotations, and nonuniform scale.

New operands use a 0.05 world-unit smooth blend by default. The historical reference
shader only supports hard booleans and untextured materials, so it is not a visual
equivalence reference for soft joins or wood. Progressive-versus-direct comparisons
use the current shader and still cover both features.

The full-resolution benchmark warms up once and reports the median of five three-frame
GPU-completion batches. The progressive benchmark orbits for 60 frames, applies stationary
geometry/material/selection edits for 40 frames, restores the original scene, waits for
complete refinement, and checks the output against the direct image (maximum allowed
8-bit channel difference: 1). It also measures ten cached frames. PPM readbacks are
optional artifacts; pixel-equivalence assertions always run with `--benchmark-progressive`.

Native Metal measurements are hardware-specific. WebAssembly compilation and portable
WGSL validation do not establish browser runtime performance.

## Smooth convex subtraction check (Apple M1 / Metal, 2026-09-26)

The unchanged `examples/oneil_cylinder.claydash` file, with its original 0.05 blend widths,
renders at about 20–22 ms/frame at 384×216 versus 71.9 ms/frame with the preceding
shader. Its native 1280×720 continuous-edit loop measured 9.9–10.6 ms p95 over
two 119-frame runs; the median adaptive preview ranged from 1,890 to 7,630
pixels between runs. The final direct 1280×720 image takes about 122–125 ms;
progressive refinement matches it exactly after completion.
The 384×216 output differs from the preceding shader by 0.22/255 mean 8-bit
channel value, mainly at surface edges and highlights.

A further shader change removes general CSG and modifier evaluation for every eligible
smooth convex pair. Against the preceding optimized shader, the original scene's
384×216 direct render improved from 21.9 to 12.1 ms/frame, and its 1280×720 direct
render improved from roughly 122–125 to 66.3 ms/frame. Both comparisons produced
identical 8-bit pixels. The `soft-sphere`, `soft-box`, `soft-cylinder`, and
`soft-rounded-box` fixtures also produced identical images before and after this
change. One native 1280×720 continuous-edit run measured 13.16 ms p95 with a
13,064-pixel median preview. The native-resolution direct render remains above a
16.7 ms frame budget.

Bounding contact-shading queries gave an identical native-resolution image and
measured 63.3–65.5 ms/frame in subsequent runs. A diagnostic render that temporarily
bypassed material programs took 32.6 ms/frame; the original material programs were
restored after that measurement. This identifies material evaluation as a substantial
remaining cost without changing the scene or its final shading.

A temporary per-material profile at 1280×720 measured 39.9 ms with the cloud
program bypassed and 49.8 ms with the canopy program bypassed, versus about
65 ms with all original programs. Bypassing each of the other four programs
individually measured 64–65 ms. Every program was restored after profiling;
these are attribution measurements, not faster output or proposed changes to
the scene. They indicate that arbitrary scene-defined procedural material work
is now the main cost to address for direct full-resolution rendering.

The zero-opacity lighting check kept the original scene's native image byte-identical
and measured 63.2 ms/frame. Correcting Fresnel for refractive index 1 measured
60.3–60.9 ms/frame at 1280×720 and 11.3 ms/frame at 384×216. This correction
changes the image: compared with the previous
shader, mean 8-bit channel difference was 0.50/255, with a 95th percentile of
2/255. The generic `soft-cylinder-layers` fixture, whose layers also have index 1,
improved from 24.2 to 16.7 ms/frame at 384×216. The unchanged scene's final
1280×720 progressive image still matched its current direct reference exactly.

Scene-driven material specialization removed unused built-in material branches
without changing the original scene's 8-bit image. Its 1280×720 direct render
measured 52.7–55.4 ms/frame in two runs; a native continuous-edit run measured
9.06 ms p95 with a 15,756-pixel median preview. Wood, brick, diagnostic, and
mixed-material fixture images were byte-identical to their previous outputs.
All 29 benchmark fixtures completed in one process, exercising material and
primitive pipeline changes. The older reference shader benchmark also runs with only the
override constants it declares.

Pruning unused polygon prism, Bézier, and loft distance branches further reduced
the unchanged scene's 1280×720 direct render to 44.0 ms/frame and its 384×216
render to 7.85 ms/frame. A native continuous-edit run measured 8.97 ms p95
with a 36,890-pixel median preview. Its 8-bit image remained byte-identical.
The mixed scene containing Bézier and loft objects
also stayed byte-identical; a new polygon prism fixture differed from brute-force
tracing by 0.02/255 mean channel value. These branches remain enabled whenever
their primitive types are present in the scene.

Scene-driven specialization also removes lattice, mirror, and repetition shader paths
when no uploaded object uses them. The unchanged original scene uses none of these
features; its direct 1280×720 render measured 30.0 ms/frame on Apple M1. The final
progressive image matched the direct image exactly. The benchmark suite now includes
a mirrored Boolean fixture alongside its lattice and repetition fixtures; all 30
fixtures completed with exact progressive-to-direct pixel matches at 128×72.

## Saved examples (Apple M1 / Metal, 2026-09-26)

The saved Oneil scene now lives in `examples/oneil_cylinder.claydash`; its SHA-256
remains `5ef27f294b90219a38a4e9cd6032074f6800decf4c978d4c6eff45918cab55c8`.
No example scene contents were edited for these measurements. Direct 1280×720
renders on the same machine measured:

| Scene | Before flat-union and polygon-read changes | After |
| --- | ---: | ---: |
| Concrete tower | 684.8 ms | 406–414 ms |
| Retro flying car | 44.4 ms | 44.0 ms |
| Oneil cylinder | 28.7 ms | unchanged path |
| Default duck | 8.6 ms | unchanged path |

The tower is one large, flat hard-union component. Its specialized distance
evaluation keeps only the nearest operand instead of filling and reducing a
scratch array. Its 640×360 output was byte-identical to the general CSG path;
the corresponding times were 172.3 and 237.3 ms. A separate `flat-hard-union`
fixture covers the same path with mixed primitive types. Polygon distance evaluation
now reuses each edge endpoint in the next iteration. The car's 640×360 image
was byte-identical; timing changed from 13.9 to 13.4 ms. The car's native-size
difference is small and may be within run-to-run variation. The full 31-case
fixture suite and progressive checks for all three examples plus the default
duck scene passed at 128×72.

Conservative per-operand bounding spheres then reduced the tower to 132.1 ms
at 640×360 and 406–414 ms at 1280×720. Its full 1280×720 image remained
byte-identical to the unbounded flat-union path (406.0 versus 548.7 ms in a
paired run). Bounds are used only for unmodified primitives
with uniform scale; other operands are evaluated normally. The car image also
remained byte-identical and its native-size time remained about 44 ms.
Scenes without flat hard unions compile without that path; the 256-object
nested fixture measured 72.4 ms at 128×72 and refined exactly. These are direct
full-resolution times, separate from the progressive interactive viewport.

The native 1280×720 continuous-edit benchmark invalidated the scene on every
frame. It measured 9.0 ms p95 for the tower (975 median preview pixels),
10.1 ms for the car (52,725 pixels), 10.8 ms for Oneil (55,480 pixels), and
7.3 ms for the default duck (214,102 pixels). The tower's low preview pixel
count shows that its detailed image still takes substantial time to refine.
The historical reference shader fixture was updated to read the current GPU
object and material buffer layout, and its preview benchmark renders again.
The tower's native-resolution progressive benchmark completed its full image
in 120 refinement frames; its final image matched the direct reference exactly.
In that run, motion p95 was 9.5 ms, stationary-edit p95 was 16.3 ms, and
refinement-batch p95 was 16.0 ms.

A second spatial hierarchy now indexes the operands of sufficiently large flat
hard unions when every child has a conservative distance bound and the group has
no mirror, group repetition, or modifier. A point-distance query can skip an
entire subtree of distant operands rather than checking every child bound at
every ray-march step. Small unions and groups that do not meet those conditions
keep the linear path. On Apple M1, paired 640×360 tower renders measured
144.2 ms before and 106.0 ms after this change, with byte-identical PPM output.
At 1280×720 the tower measured 341.0 ms in a subsequent run, versus the
previous 406–414 ms range. The car's 640×360 image stayed byte-identical and its
1280×720 direct render measured 42.7 ms; its small timing difference from prior
runs is not enough to establish a gain. A new 13-object mixed-primitive
`flat-hard-union-wide` fixture exercised the operand tree. Its 384×216 image
was byte-identical with the same shader's linear path, and its progressive
image matched the direct reference exactly. The prior 31-case progressive
fixture suite also matched each direct reference exactly at 128×72.

The benchmark fixtures `soft-sphere`, `soft-box`, `soft-cylinder`, and
`soft-cylinder-layers` exercise the same acceleration across convex primitives
and overlapping transparent shells. Their 384×216 direct renders measured 1.29,
2.32, 3.08, and 37.27 ms/frame respectively. Brute-force SDF tracing measured
11.10, 13.00, 13.88, and 134.04 ms/frame for those fixtures. Pixel comparisons
against brute-force output had mean channel differences below 0.24/255.
`soft-rounded-box` verifies that unsupported rounded surfaces remain on the
SDF path; its output differed from brute-force by at most 4/255 per channel.
`soft-transformed-pair` combines a rotated, nonuniformly scaled box with an
angled cylinder cavity; `soft-mixed-pair` combines a scaled sphere with an
offset box cavity. Their 384×216 direct renders measured 2.35 versus 8.41 ms
and 1.41 versus 6.05 ms with brute-force tracing. Mean 8-bit channel
differences were 0.44/255 and 0.23/255, concentrated at silhouettes. Both
progressive results matched their direct references exactly.

```sh
cargo run -- --stress-benchmark --benchmark-scene=examples/oneil_cylinder.claydash \
  --benchmark-size=1280x720 --benchmark-progressive
cargo run -- --stress-ui-benchmark --benchmark-edit \
  --benchmark-scene=examples/oneil_cylinder.claydash --benchmark-size=1280x720
cargo run -- --stress-benchmark-suite --benchmark-case=soft-cylinder-layers \
  --benchmark-size=384x216 --benchmark-progressive
```

## Measured result (Apple M1 / Metal, 2026-09-17)

At a 1280×720 viewport, with 1,024 objects:

| Fixture | Motion p95 | Stationary edits p95 | Refinement p95 | Cached p50 |
| --- | ---: | ---: | ---: | ---: |
| Solid | 7.70 ms | 3.59 ms | 3.41 ms | 0.45 ms |
| Metallic | 7.10 ms | 3.53 ms | 3.56 ms | 0.46 ms |
| Transparent | 8.60 ms | 7.32 ms | 7.95 ms | 0.45 ms |
| Mixed primitives/materials | 7.81 ms | 7.46 ms | 8.40 ms | 0.50 ms |
| Boolean pairs | 7.13 ms | 6.74 ms | 8.31 ms | 0.46 ms |
| Nested boolean groups | 12.83 ms | 8.76 ms | 9.42 ms | 0.46 ms |

The 64-object repetition fixture (1,728 instances) measured 9.77 ms motion p95 and
13.49 ms refinement p95, with an occasional 22.75 ms refinement batch. These are
GPU-completion viewport timings, not claims that every material renders a fresh
native-resolution image at 60 FPS. In the 1280×720 **native application window**, the
1,024-object nested fixture measured **12.10 ms p95** including egui and presentation
(14.72 ms maximum across 119 measured frame intervals).

Before optimization, the original 256-object solid fixture took 148 ms/frame at 384×216,
and all-glass took 1,735 ms/frame. At 1280×720, the optimized direct full-resolution
1,024-object renders still take about 16 ms for opaque materials, 182 ms for glass, and
356 ms for the nested fixture. Budgeted previews, refinement, and caching are what keep
that expensive work from blocking the interactive loop.

Every final perspective and orthographic fixture converged to **zero channel difference**
from its direct full-resolution reference, including after stationary material/geometry
and selection edits. The comparison against the old material shader is intentionally not
pixel-identical: exact primitive intersections improve silhouettes, and later glass
reflections use the environment approximation described above. With the reference
shader updated for the current GPU object and material layout, the preview fixture's
mean 8-bit channel difference is 0.738/255 at 384×216 (2026-09-26). The older
glass comparison used a different buffer layout and has not been repeated.

## Continuous-edit budget correction (Apple M1 / Metal, 2026-09-17)

The native edit benchmark rotates an object on every frame, invalidating the scene image,
while keeping the camera fixed. At a 1280×720 application window, a 130-frame run
(excluding the first 11 frame intervals) measured:

| Scene | Median preview pixels before → after | UI p95 before → after |
| --- | --- | --- |
| 10-object material/boolean preview | 48,990 → 390,000 | 13.03 → 13.74 ms |
| 1,024-object nested booleans | 990 → 990 | 10.59 → 10.17 ms |

The small scene gets about eight times as many preview pixels with comparable measured
responsiveness. The dense scene remains GPU-limited and retains its low-resolution
preview. These are short local measurements, not a guarantee of unchanged frame time on
other devices. The target GPU batch budget remains 6 ms; a sudden cost increase can still
produce an over-budget batch before timing feedback reduces subsequent work.

## Experimental deferred viewport (2026-09-27)

The World panel switches between Exact ray shading and Deferred (experimental).
The pipeline choice and SSAO/SSR switches live in the scene's World value, so
ordinary scene undo/redo and the agent SetWorld action apply. Older scenes
default to Exact. Deferred also applies to render exports.

Deferred traces the nearest primary-ray surface once into four RGBA16F
geometry attachments (world position/reflectivity, normal/roughness,
albedo/metallic, and opacity/index of refraction) and a depth attachment. A full-screen pass computes direct
lighting and samples nearby geometry for ambient occlusion. Experimental
reflections march projected rays through the position buffer, test depth
crossings, fade near screen edges and on long rays, and use the environment
when no visible on-screen hit is found. Orthographic cameras use a parallel
view direction. Hybrid Gaussian splat quads are composited afterward against
SDF depth; their colors do not enter the screen-space effects.

Geometry buffers are allocated only while Deferred is active. Edits first
render a complete adaptive preview, including its screen-space effects. Idle
frames build native geometry in bounded tile batches, while the complete
preview remains visible. Once every native tile is ready, one full lighting
and splat pass publishes the native image. Effects never read incomplete
neighbor tiles, and exports wait for that completed native frame. Native
geometry buffers are allocated when refinement starts. World lighting and
SSAO/SSR changes reuse existing geometry; cached frames skip scene work.
The final native lighting pass still has a cost proportional to output size.
The World panel
shows an Exact fallback when custom shaders, image
planes with alpha, or unsupported Gaussian group configurations need the
existing ray compositor. Transparent world backgrounds retain zero alpha.
Glass remains in Deferred and shares the same geometry pass as opaque objects.
There is no separate glass pipeline, opaque-background capture, boundary walk,
or thickness/exit ray. One screen-space lighting resolve applies SSAO, SSR,
and thin-surface refraction from the shared front-surface buffers. Occluded
geometry is unavailable: reflection/refraction misses use the world environment.
Use Exact when hidden geometry must be visible through glass or reflected.
The modules live in `deferred_geometry.wgsl`, `deferred.wgsl`,
`deferred_transmission.wgsl`, `ssao.wgsl`, and `ssr.wgsl`.

Deferred lighting approximates the Exact renderer's wood coat, ray-traced
ambient occlusion, multiple transmission bounces, and sky atmosphere. SSR
depends on visible on-screen surfaces and camera position.

The native stress benchmark reads the saved World choice. For example,
use --stress-benchmark --benchmark-scene=scene.claydash and
--benchmark-images=/tmp/deferred-images. On an Apple M1 Metal adapter, a
three-object opaque 320x240 fixture showed 2,401 pixels changed by SSAO and
1,243 by SSR against Deferred base lighting; the combined result changed
3,609 pixels. A separate, isolated 256-solid-object run at 320x240 measured
9.621 ms/frame for Exact and 7.531 ms/frame for Deferred with both effects.
These numbers are medians after warm-up, but the benchmark paths differ:
Deferred includes the viewport composite. A native application screenshot
capture also produced a complete Deferred frame. The small three-object
fixture showed no reliable speedup. Measure the intended scene and device
before choosing for performance.

### Deferred glass regression

After `cargo build --offline`, run `python3 scripts/check-deferred-reflections.py`
in a native desktop session. The fixture places text behind the camera and a
reflective glass sphere in front. Deferred output must be byte-identical with
and without the text; the Exact control must differ. Both Deferred cases also
check progressive refinement against the direct render. On Apple M1 Metal,
these checks pass, and `examples/ssao_duck.claydash` at 320×240 also matches its
direct reference exactly after refinement.

### Single geometry pass performance

On Apple M1 Metal, `examples/ssao_duck.claydash` with both SSAO and SSR enabled
measured 8.504 ms/frame at 640×480, compared with 46.490 ms/frame for the earlier
separate opaque/glass geometry passes. At 1280×720 the single-pass version
measured 19.295 ms/frame. These are native benchmark submission/wait medians,
not application-wide frame-rate guarantees. The 720p result remains above the
16.7 ms budget for 60 FPS. Geometry attachments decreased from seven to four;
there is still one screen-space lighting resolve after geometry. Refraction
now uses only visible surfaces and the environment, without hidden geometry
or measured thickness. The off-screen-text regression and exact progressive
image comparison still pass.

## Neural SDF

The optional `neural_sdf` representation fits a configurable ReLU network
from 32,768 uniformly random distance samples by default (two hidden layers, width 24, learning rate 0.02). Historic measurements below used grid samples. See [the representation contract](group-render-optimizations.md#neural-sdf).
Native benchmarks wait for training before measuring frames, so results do not
silently measure the temporary Exact SDF fallback. Training now uses GPU compute
for source-distance sampling, forward/backward passes, Adam updates, validation,
and material ownership. The historical measurements below predate this backend;
they are not GPU training benchmarks.

Historical Apple M1 / Metal measurements at 384×216, optimized development build,
from the initial fixed-architecture version (before configurable settings and
full shuffled epochs):

| Fixture | Bake | Steady-state frame |
| --- | ---: | ---: |
| Two-sphere union, two child colors, neural | 36 ms | 5.34 ms |
| Same two-sphere union, Exact SDF | — | 5.66 ms |
| Neural union, transparent materials | 51 ms | 12.70 ms |
| Neural box/sphere union | 56 ms | 5.42 ms |
| Neural sphere subtraction | 38 ms | 4.31 ms |
| Neural union with rotation/nonuniform scale | 55 ms | 7.42 ms |
| Neural union, inside camera | 56 ms | 5.61 ms |
| Neural union, deferred pipeline | 37 ms | 4.73 ms |
| Neural union, farther camera | 55 ms | 1.56 ms |

Before the distance-estimate tracing change, a two-layer, width-eight model trained for 32 epochs
at learning rate 0.001, seed 42, and hit distance 0.75 cells took 265 ms to bake.
It achieved RMS 0.04699, max error 0.23369, and used 524,880 GPU bytes. Its frame
time was 62.94 ms at 384×216. Vector-aligned rows reduced that from 120.83 ms,
but deeper models remain substantially slower and should be treated as a
quality/performance experiment rather than an automatic improvement.

The saved union fixture has full-grid field RMS error 0.07262 and maximum error
0.37074 world units. These are distance-fit errors, not image errors or guaranteed
surface bounds. One ReLU layer produces visibly faceted geometry; the two source
colors remain visible. The half-cell hit tolerance expands the approximate
silhouette. Transparent refraction follows the faceted surface and can differ
substantially from Exact SDF. Sparse/disjoint geometry can fail the zero-crossing
check and remain on Exact SDF.

The initial version uploaded 524,464 bytes per group. The configurable version
uses 524,496 bytes at default settings, mostly material/color ownership, plus
additional weight records for larger networks. Native
bake timing excludes the 200 ms edit debounce, GPU upload, and pipeline creation.
Initial scene preparation reached 2.96 seconds with cold pipeline compilation;
a warm run including debounce was around 296 ms. The activation-mask step bound
reduced the union's measured time from 13.75 to 5.34 ms/frame; this one small
fixture does not establish a general speed advantage. Ray-step counts are not
currently exported by the benchmark.

Validation includes deterministic fitting and finite-difference gradient tests,
cache invalidation, cancellation/stale results, completion notifications,
material-owner preservation, and assembled WGSL validation. An explicit GPU
compute test compares the renderer's inference function against CPU evaluation
at all 32³ grid positions (absolute error < 1e-5), covering one layer × eight,
two layers × five/eight, and four layers × 32, for both ReLU and Softplus, plus
gradient agreement checks. The browser target compiles;
interactive browser responsiveness has not been measured. Its 4 ms scheduling
budget is soft, particularly during sampler construction on large scenes.

```sh
cargo test --workspace
cargo check --target wasm32-unknown-unknown
cargo test neural_gpu_inference_matches_cpu -- --ignored --nocapture
cargo run -- --stress-benchmark \
  --benchmark-scene=tests/fixtures/neural-sdf.claydash \
  --benchmark-size=384x216 --benchmark-images=/tmp/claydash-neural
```

For an exact control, copy the fixture to a temporary path and change the root's
`render_representation` to `exact_sdf`. Keep the checked-in fixture unchanged.


### Default-duck model presets

The original CPU preset calibration trained the unchanged default duck using
all four presets and checked decreasing full-grid RMS error and visible hits
through the cube. These measurements are historical; CPU training has been
removed. Validate the wgpu trainer explicitly with a GPU adapter:

```sh
cargo test gpu_sampling_backprop_adam_and_training_match_reference -- --ignored --nocapture
```

Preview / Balanced / Detailed / High detail fit errors are respectively
0.03580 / 0.01598 / 0.00793 / 0.00580 world units. Initial isolated CPU bake
measurements were approximately 0.32 / 2.1 / 7.5 / 21.5 seconds. Concurrent work
can increase those times substantially. All use Softplus with beta 10 and the
same deterministic seed. The presets are intended to expose quality/cost
tradeoffs, not to imply that the largest model is interactive on every GPU.

All four presets also produced visible duck images in the native GPU benchmark
on Apple M1 at 192×108, with the default 0.5-cell hit distance:

| Preset | Bake time | GPU frame time | Full-grid RMS |
| --- | ---: | ---: | ---: |
| Preview | 0.31 s | 2.31 ms | 0.03580 |
| Balanced | 2.07 s | 8.57 ms | 0.01598 |
| Detailed | 7.43 s | 18.86 ms | 0.00793 |
| High detail | 21.44 s | 49.66 ms | 0.00580 |

These are low-resolution diagnostic measurements, not full-viewport frame rates.

An attempted fix that expanded tracing to 4,096 conservative steps passed CPU
visibility checks but produced blank repeated GPU renders for larger presets.
The renderer now marches the predicted distance with sign-crossing refinement
and the bounded existing iteration counts. This avoids the extremely small
steps caused by deep networks' loose global slope bounds.

The preset bake timings above predate on-demand source sampling. Training now
queries source distances for each batch and validation chunk rather than caching
the complete distance grid; bake times must be remeasured for this path.

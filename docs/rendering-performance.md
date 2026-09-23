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
  intervals, including nonuniform scaling. Tori, repetition, and CSG retain SDF marching.
- Follow the current component while a transmitted ray is inside it. Check membership in
  other components at exit boundaries, so overlapping solids remain a union.
- Compute normals from the hit component, avoiding four whole-scene distance searches.
- Upload independent boolean components in contiguous postorder. Bounds follow boolean
  volume semantics: unions can expand, subtraction cannot expand, and intersection can
  shrink or become empty. Size shader scratch storage to the largest component, with a
  dedicated two-operand path, rather than the entire object count.
- Share one secondary-ray tracing call site between reflection and transmission. Preserve
  six surface interactions, Fresnel response, entry/exit refraction, and total internal
  reflection. Metal and the first glass interface reflect scene geometry. Later glass
  reflections sample the studio environment; the transmitted path still traces geometry.
  This is a real-time material approximation, not a path tracer.
- Build parent/selection lookups once per upload. Reuse scene uploads when their versions
  have not changed.

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
reflections use the environment approximation described above. The small preview scene's
mean 8-bit channel difference was 0.315/255; dense overlapping glass changes more visibly
(7.67/255 in the 256-object comparison).

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

# GPU modifier layout

Each GPU object has a 16-byte `modifier` field: `x` is a one-based shared record ID (zero disables modifiers), `y` stores the march factor as float bits, `z` is the shared parameter-slot offset, and `w` is the modifier kind. Scene upload packs one record per lattice root, then copies its kind and offset to each affected object. The shader reads the object directly in its hot path; the host-side record tracks the shared parameter range.

Modifier implementations live in separate WGSL modules. `modifier_common.wgsl` dispatches by kind, while `modifier_lattice.wgsl` owns the lattice slot constants and deformation logic. Adding another kind requires a packer, one module, and one dispatch case; it does not add fields to `Object` or deformation code to `sdf.wgsl`.

## Bindings and slots

| Binding | Data | Purpose |
| --- | --- | --- |
| 6 | `array<vec4<f32>>` | Original lattice control offsets |
| 8 | `array<vec4<f32>>` | Packed modifier parameters |
| 9 | Filterable 3D `Rgba16Float` texture | Inverse fields or high-density control offsets |
| 10 | Linear sampler | Trilinear atlas sampling |

A lattice takes 11 parameter slots. Slots 0–2 hold inverse affine rows, 3–5 forward rows, 6 the atlas minimum, 7 its reciprocal extent, 8 the atlas tile and resolution plus original-control offset and resolution, 9 the original cage minimum, and 10 its reciprocal extent. Integer values in slot 8 are bitcast to floats for the `vec4` buffer. `GpuObject` is 176 bytes; it was 304 bytes before the modifier split.

For cages with 2–3 points per axis, scene upload solves a 17³ inverse displacement field once. Each SDF sample interpolates that field in hardware and applies one correction from the original control points. For cages with 4–9 points per axis, the atlas holds the original offsets and the shader retains the four-iteration inverse solve; this preserves high-frequency deformations that a 17³ inverse field could blur. An unchanged cage has no GPU modifier or atlas upload.

The atlas uses 19×19 XY tiles in a 608-wide texture, with one row initially. It grows in powers of two up to 32 rows as scene demand increases, so a scene with one active lattice does not allocate the maximum atlas. Scene upload caches each lattice's transform, effective offsets, parameter record, and texture tile. Boolean operands sharing a lattice evaluate its deformation once per SDF sample.

The ray step uses a per-lattice lower bound on deformation stretch from trilinear cell corners. It accounts for the cage transform and stays between 0.35 and 0.8; strongly deformed cages retain the previous 0.35 step.

## Verification

`cargo test` validates assembled WGSL variants through Naga and checks inverse-field refinement against the original fixed-point solve. `cargo check --target wasm32-unknown-unknown` checks the browser build. The benchmark suite includes opaque, transparent, dense, and Boolean lattice cases; use `--stress-benchmark-suite --benchmark-case=<case> --benchmark-size=640x360` and optionally `--benchmark-images=<directory>` to compare frame times and PPM images.

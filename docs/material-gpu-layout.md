# GPU material layout

The scene stores `Material` and `MaterialAsset` exactly as before. At upload, the renderer packs distinct material **values** into GPU records. Objects with a shared asset and matching current values use one record; an edited object with different values gets its own. An object's `color` remains on `GpuObject` as its color override. Transform scale remains there because it varies per object and the wood pattern uses physical stock coordinates.

## Buffers

| Binding | Record | Size | Purpose |
| --- | --- | ---: | --- |
| 1 | `GpuObject` | 160 bytes | SDF geometry, transform, repetition, color override, material index in `component.w` |
| 3 | `GpuMaterialHeader` | 16 bytes | kind, vec4 slot offset, slot count, reserved |
| 4 | `array<vec4<f32>>` | 16 bytes per slot | Common and material-specific parameters |

Binding 0 is the camera uniform and binding 2 is the BVH. All records are 16-byte aligned and tightly packed. The fixed allocation allows 1024 material headers and 8192 parameter slots (128 KiB). A unique basic material takes two slots (32 bytes); wood takes eight (128 bytes); the diagnostic material takes three (48 bytes). The previous object was 256 bytes; the new object is 160 bytes, saving 96 bytes per object. For 1024 objects sharing one wood material, upload changes from 256 KiB of objects to 160 KiB of objects plus 16 bytes of header and 128 bytes of parameters. Unique wood on every object totals 160 KiB objects plus 16 KiB headers plus 128 KiB parameters, so this design targets the common shared-asset case rather than claiming a universal upload reduction.

The two common slots contain `(roughness, metallic, reflectivity, opacity)` and `(refractive_index, 0, 0, 0)`. Wood owns six slots with named offsets in `material_wood.wgsl`; the second wood slot reserves `.w` for coat amber while its other lanes are unused. Diagnostic owns one additional slot containing checker frequency and alternate color. No kind-specific value is stored in the object buffer.

## Adding a material

1. Add a `MaterialKind` variant and an explicit GPU code in `src/model/geometry.rs`.
2. Add a typed packer in `src/renderer/material_gpu.rs` after the two common slots. If the material needs new scene properties, put them in `Material`, with serde defaults for old files.
3. Add a WGSL source module under `assets/shaders/` with local slot constants and an evaluator returning `Surface`.
4. Include it in the local assembler in `material_gpu::shader_source`, and add one branch to the explicit `material_surface` switch in `material_common.wgsl`.
5. Add a preview and a mixed-material validation case.

The assembler uses static `include_str!` files and one marker in the core shader. It adds no dependency and builds the same source for native and browser. The WGSL validator runs with empty optional capabilities. Material textures can later occupy explicit texture and sampler bindings, or an atlas; the layout does not assume bindless arrays. The current five bindings stay within portable WebGPU limits. The parameter allocation is 128 KiB, below the 128 MiB minimum storage binding limit. A future material with more than eight slots requires raising `MAX_PARAM_SLOTS` and checking the device limit.

## Ambient occlusion

`ambient_occlusion.wgsl` samples the scene SDF at four positions across four hemisphere directions, each reaching 0.12 world units. It affects ambient light and part of diffuse light on the primary hit; reflection and transmission bounces skip it to keep the viewport responsive. This is a compact interactive approximation, not a converged hemispherical pass.

## Verification and limits

`cargo test` validates all assembled shader variants through Naga (including CSG scratch sizes), and all 165 tests pass. `cargo check --target wasm32-unknown-unknown` passes. A windowed benchmark could not produce frames in this execution environment, so visual parity and GPU frame-time comparisons still need a local run. The benchmark command is `cargo run -- --stress-benchmark-suite --benchmark-size=320x180` and writes per-case images; compare `wood`, `mixed`, `booleans`, `diagnostic`, and the material library in the app. The SDF core still owns the shared lighting call site but the wood algorithm and material dispatch live in separate modules.

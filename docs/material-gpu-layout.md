# GPU material layout

The scene stores `Material` and `MaterialAsset`, with an optional WGSL function body on custom material assets. At upload, the renderer packs distinct material **values** into GPU records. Custom materials also use the asset ID in their record key so two different shaders never share one dispatch index. Objects with a shared asset and matching current values use one record; an edited object with different values gets its own. An object's `color` remains on `GpuObject` as its color override. Transform scale remains there because it varies per object and the wood and brick patterns use physical stock coordinates.

## Buffers

| Binding | Record | Size | Purpose |
| --- | --- | ---: | --- |
| 1 | `GpuObject` | 160 bytes | SDF geometry, transform, repetition, color override, material index in `component.w` |
| 3 | `GpuMaterialHeader` | 16 bytes | kind, vec4 slot offset, slot count, reserved |
| 4 | `array<vec4<f32>>` | 16 bytes per slot | Common and material-specific parameters |

Binding 0 is the camera uniform and binding 2 is the BVH. All records are 16-byte aligned and tightly packed. The fixed allocation allows 1024 material headers and 8192 parameter slots (128 KiB). A unique basic material takes two slots (32 bytes); wood takes eight (128 bytes); the diagnostic material takes three (48 bytes). The previous object was 256 bytes; the new object is 160 bytes, saving 96 bytes per object. For 1024 objects sharing one wood material, upload changes from 256 KiB of objects to 160 KiB of objects plus 16 bytes of header and 128 bytes of parameters. Unique wood on every object totals 160 KiB objects plus 16 KiB headers plus 128 KiB parameters, so this design targets the common shared-asset case rather than claiming a universal upload reduction.

The two common slots contain `(roughness, metallic, reflectivity, opacity)` and `(refractive_index, 0, 0, 0)`. Wood owns six slots with named offsets in `material_wood.wgsl`; the second wood slot reserves `.w` for coat amber while its other lanes are unused. Brick owns three slots for bond dimensions, relief and surface detail, and mortar color. Diagnostic owns one additional slot containing checker frequency and alternate color. No kind-specific value is stored in the object buffer.

Custom materials use kind 6 and the header's reserved word as a scene-local shader index. The assembler inserts one function and one dispatch case per custom asset. Editing WGSL validates the isolated function body and assembled shader before the scene changes; the renderer then rebuilds the viewport and preview pipelines. A custom asset requires its object link to select the correct shader.

## Adding a material

1. Add a `MaterialKind` variant and an explicit GPU code in `crates/claydash_engine/src/model/geometry.rs`.
2. Add a typed packer in `crates/claydash_engine/src/renderer/material_gpu.rs` after the two common slots. If the material needs new scene properties, put them in `Material`, with serde defaults for old files.
3. Add a WGSL source module under `crates/claydash_engine/assets/shaders/` with local slot constants and an evaluator returning `Surface`.
4. Include it in the local assembler in `material_gpu::shader_source`, and add one branch to the explicit `material_surface` switch in `material_common.wgsl`.
5. Add a preview and a mixed-material validation case.

The assembler uses static `include_str!` files and one marker in the core shader. It adds no dependency and builds the same source for native and browser. The WGSL validator runs with empty optional capabilities. Material textures can later occupy explicit texture and sampler bindings, or an atlas; the layout does not assume bindless arrays. The current five bindings stay within portable WebGPU limits. The parameter allocation is 128 KiB, below the 128 MiB minimum storage binding limit. A future material with more than eight slots requires raising `MAX_PARAM_SLOTS` and checking the device limit.

## Ambient occlusion

`ambient_occlusion.wgsl` samples the scene SDF at four positions across four hemisphere directions, each reaching 0.12 world units. It affects ambient light and part of diffuse light on the primary hit; reflection and transmission bounces skip it to keep the viewport responsive. This is a compact interactive approximation, not a converged hemispherical pass.

## Verification and limits

`cargo test` validates assembled shader variants through Naga (including CSG scratch sizes). `cargo check --target wasm32-unknown-unknown` passes. The benchmark command is `cargo run -- --stress-benchmark-suite --benchmark-size=320x180 --benchmark-images=/tmp/claydash-materials`; use `--benchmark-case=brick` to capture just the brick case. The SDF core still owns the shared lighting call site while material algorithms and dispatch live in separate modules.

## Metal materials

`Metal` is kind 8. `Metallic` remains kind 3, retains its original shader and controls, and old files without `metal` deserialize with default settings. Nine typed species map to linear RGB conductor reflectance and species-specific oxide/tarnish proxies. The five finish presets set roughness, anisotropy and relief defaults; the eight study presets only set material properties, without changing geometry or the world.

The six metal slots after the common slots are:

| Slot | Values |
| --- | --- |
| 2 | finish, tangent mode, anisotropy, brush angle in radians |
| 3 | texture scale, relief strength, scratches, oxidation/tarnish |
| 4 | paint coverage, paint roughness, image relief, unused |
| 5 | paint RGB, unused |
| 6 | conductor F0 RGB, unused |
| 7 | oxide RGB, unused |

Object color multiplies the bare conductor's F0; paint color and species oxide color remain independent. Scratches alter relief, roughness and paint chips. Oxide and paint lose the conductor response and use dielectric reflection plus diffuse lighting. Relief perturbs the shading normal on existing geometry; it does not displace the silhouette. Linear, radial and local-Z wire tangent fields rotate with the object. `material_metal.wgsl` evaluates those fields, while `material_metal_light.wgsl` uses anisotropic GGX and 16 deterministic environment samples from Claydash's current world, plus the current direct light. This is an interactive approximation; smooth surfaces additionally trace one scene reflection to replace a bounded portion of the sampled world. Rough surfaces retain the world integration, secondary reflections do not recurse, and transparent reflected hits retain the world contribution. New metals use the ray compositor through an explicit material eligibility fallback from the deferred path. Library and viewport share these material modules.

The optional `image_relief` parameter reuses an object's existing Image stencil upload and atlas. Zero preserves its original color-decal behavior. Positive values interpret grayscale as normal relief using the stencil's size, offset, rotation and side settings; an absent image and transparent pixels produce no relief. This image remains object-owned, survives scene save/load, and is not included in shared material/library sphere previews. PNG, JPEG and WebP upload uses the existing Object panel. No separate material image asset or HDR world pipeline is introduced.

Validation covers old Metallic deserialization, full metal settings and object image scene round trips, record packing/deduplication, and portable assembled WGSL variants. Native visual cases are available as `metal-Machined-steel`, `metal-Polished-chrome`, `metal-Brushed-aluminum`, `metal-Chipped-red-paint` and the other study labels in the benchmark suite.

### Material scale reference

The default handheld shell measures 2.5 × 3.72 × 0.7 scene units in its local axes. Scene units currently have no metre contract. Using a 15 cm shell height as an illustrative reference gives approximately 40 mm per scene unit. At that reference the calibrated jersey pitch (0.045 / 4 scene units) is about 0.45 mm.

New metal presets start at texture scale 1 instead of 4: procedural defects are four times wider. At scale 1, the hammered noise cell is 1/18 scene unit (about 2.2 mm under the reference above), while the primary oxide field has cells of 1/6.1 scene unit (about 6.6 mm). These are procedural cell sizes, not measured pit diameters or guaranteed coating patch sizes. Higher texture scale means finer detail, consistently with the fabric control; the metal slider supports 0.05–16. Saved materials retain their explicit scale values.

A future metre-based scene migration must scale geometry, translations, cameras, animation distances, material pitches and relief amplitudes together, and audit ray tolerances, normal offsets, AO distances and editor snapping. Changing the scene dimensions alone does not establish consistent physical material scale. The current calibration preserves the established fabric appearance.

Scratch defects use jittered finite segments with varied centers, lengths, and angles, evaluated across neighboring cells. Pixel filtering attenuates subpixel coverage rather than turning thin scratches into broad stripes. Paint and oxide suppress 80% of the substrate normal perturbation at full coverage; the brush tangent and bare-metal anisotropy remain independent controls. The exploration GLSL uses the same scratch construction.

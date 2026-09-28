# Group rendering experiments

These are optional representations of one object or Boolean group. The source
SDF remains editable and authoritative. A representation should be generated
from the current source, cached separately from the `.claydash` document, and
invalidated when any source input on which it depends changes. A saved mode is
an intent to use a representation, not proof that a valid bake exists.

## Current implementation

The Object properties panel shows these candidates after Modifiers for an
object or selected Boolean group. The selection is saved on that object or
group root and survives document round-trips. **Exact SDF**, **Box depth atlas**, **Sphere depth atlas**, **Gaussian splats**, and **Neural SDF** have render paths today. The capture replaces the selected subtree with a
depth capture for rendering; the editable source remains in the document. The
renderer does not gate this choice on modifiers or Boolean operations. Older
documents that saved a removed choice now load as Exact SDF. Documents without
a saved mode also default to Exact SDF.

The box depth atlas marches inward rays from six faces of the group's bounding
box. Each hit stores depth, base color, and the hit object's material. At render
time the proxy follows the face nearest the viewer, marches through empty
capture pixels, and stops at captured depth. Depth is interpolated between
neighboring samples within a continuous surface. It evaluates the captured
material, including custom WGSL, at the proxy hit point. This can lose fine detail and
view dependent effects, but users can select it for any group.

The sphere depth atlas marches inward from an 80 by 40 latitude-longitude
map around the group. It stores the same depth, color, and material information.
Empty directions let rays pass through; nearby occupied depths are interpolated
within a continuous surface. The radial capture favors rounded groups and has
one layer per direction, so deep concavities can remain approximate.

Gaussian splats capture up to eight surface crossings along rays from six box
faces. Each occupied sample becomes an isotropic Gaussian centered at its
captured surface point. Independent Gaussian groups in opaque Solid scenes
render as instanced camera-facing quads, sorted back to front and blended with
premultiplied alpha. The quads test against opaque SDF depth without writing
depth themselves. Their captured colors and normals receive diffuse lighting.
The Gaussian capture starts 0.5 world units beyond the padded group bounds so
planar faces at the boundary have positive captured depth. The render proxy
contains that clearance and the splat support. A stackless spatial hierarchy
finds support spheres along secondary reflection rays without marching every
capture cell. The existing ray compositor also handles scenes whose materials
or group modifiers cannot use the quad pass. Captures remain approximations:
thin details or hidden layers can be lost, and multiple transparent crossings
are not reproduced faithfully. Geometry edits rebuild captures. See
[rendering performance](rendering-performance.md#hybrid-gaussian-splats) for
eligibility, shading limits, and measured rendering and editing costs.

The local agent CLI and MCP server expose the working choices through
`CreateObject.render_representation` and the `SetRenderRepresentation` action.
`get_schema` lists the five available values. For a Boolean composition, set the
choice on the group root; its source operands stay editable.
Nested hard unions of ordinary SDF operands now use the flat-union GPU path
when no nested repetition, mirror, or smooth blend changes their semantics.
That path can use an operand BVH for larger groups. This accelerates exact
union composition. Box and sphere depth atlas groups use a captured representation when
capture succeeds.

## Neural SDF

Choose **Neural SDF** on an object or Boolean group root. Its editable source
is sampled at the endpoints of a configurable grid (8–1024 samples per side, default 32) inside a padded cube. The default network has one width-eight ReLU hidden layer (`3 → 8 → 1`,
41 parameters). The panel exposes these saved per-group controls:

| Control | Range | Default |
| --- | --- | --- |
| Hidden layers | 1–4 | 1 |
| Width of each hidden layer | 4–32 | 8 |
| Epochs | 1–512 | 32 |
| Learning rate | 0.00001–0.1 | 0.001 |
| Seed | Any unsigned 32-bit integer | 11256099 |
| Hit distance, grid cells | 0.001–4 | 0.5 |

The **Model size** dropdown offers calibrated presets. All use smooth Softplus
activation (`log(1 + exp(10x)) / 10`), learning rate 0.001, the default seed, and
hit distance 0.5 cells. Selecting a preset stages its training settings until
**Recompute**; editing its fields switches the displayed choice to **Custom**.

| Preset | Hidden layers | Width | Epochs | Default-duck field RMS |
| --- | ---: | ---: | ---: | ---: |
| Preview | 1 | 8 | 32 | 0.03580 |
| Balanced | 2 | 12 | 64 | 0.01598 |
| Detailed | 3 | 16 | 96 | 0.00793 |
| High detail | 4 | 24 | 128 | 0.00580 |

These are measured field-fit improvements on the default duck, not a guarantee
for every model. Larger presets increase training and rendering cost. The
**Activation** control also retains ReLU for comparison and old documents.
ReLU produces planar patches; Softplus gives continuous hidden-layer derivatives.
The final layer remains linear so distances can be negative inside the object.

One epoch is a full, deterministically shuffled pass through all grid samples;
batch size is at most 256, with a smaller final batch when needed. Source
distances are evaluated on demand for each batch, with no cached distance array
or full-grid sampling pass before training. Validation also samples in bounded
chunks after each epoch. A separate fixed 32³ material-ownership grid is sampled
in chunks after training. The default 32 epochs perform 4,096 Adam updates. The lowest
full-grid loss checkpoint is retained, checked after each epoch. The seed
controls both initialization and shuffle order. Settings are saved with the
object and participate in undo/redo; documents without them use the defaults.
Numeric text edits commit on Enter or focus loss, so an empty or partial value
can be typed without changing the setting. Dragging still updates the value.
Invalid saved settings are rejected rather than silently rewritten.

**Recompute** restarts the selected group's bake without changing its saved
settings or adding an undo entry. It also retries a failed fit. Network/training
edits wait for this button; the current model and any running bake keep their
previous settings. Geometry edits still trigger automatic rebakes using the last
applied training settings. The initial bake on selecting Neural SDF or reopening
a document uses its saved settings. Hit distance updates only the GPU
payload and reuses the fitted model. Superseded results are rejected even when
a recompute uses exactly the same settings. No model weights or sampled ownership
are saved in `.claydash` files.

After 200 ms without source changes, desktop training runs on one background
worker. Browser sampling/training advances in batches with a 4 ms soft budget
per frame; preparing the sampler and individual work units can exceed that
budget on large sources. Exact SDF is displayed while work is pending, after
an edit, or if a fit fails. Obsolete results are discarded, and completion
invalidates the viewport even without another edit. Camera movement reuses the
model. Capture keys conservatively include source geometry, references,
modifiers, and materials; unrelated source edits can also cause a rebake.

The shader evaluates the fitted field inside the cube and advances by the
predicted distance with the existing 0.8 step factor. If a step changes the
field's sign, it refines that bracket before shading the hit. Primary and
secondary neural rays use this distance-estimate path; exact geometry retains
its existing tracing. The global weight-derived slope bound is not used to
shrink ray steps: deeper networks made that bound so loose that tracing either
missed surfaces at its iteration limit or overloaded the GPU with longer loops.
As with the learned field itself, these steps are approximate: sign refinement
cannot recover a thin feature crossed twice within one step.
The hit tolerance is `max(classic tolerance, hit cells × cube side × world
distance scale / 31)`, half a grid interval at the default setting. Exact-SDF hit
tolerance is unchanged. Standalone proxies use analytic
network gradients for normals; composed or deformed proxies use the existing
field-gradient path. Reflection and transmission origins account for the
larger tolerance. Source repetition, mirror, path, and inlay effects are baked
once rather than reapplied to the proxy.

Nearest-grid ownership supplies the original child material and base color.
Material boundaries are approximate; material programs are evaluated at the
proxy hit, with the proxy coordinate frame. This does not retain every child’s
original procedural texture frame. The default GPU payload uses 524,496 bytes:
two metadata records, eleven
vec4 weight records, and 32³ material/color records independent of training resolution. Larger architectures add
packed weight records. Payloads share the existing
32 MiB proxy buffer budget. Invalid bounds, nonfinite samples/weights, missing
sampled zero crossings, and exhausted resources fall back to Exact SDF. Failure
is retained until source inputs change. The panel reports status, full-grid RMS
and maximum field errors, and bake elapsed time.

The default eight ReLU neurons deliberately limit quality: surfaces are faceted, small holes
and disjoint parts can disappear, and half-cell hit tolerance expands silhouettes.
A valid zero crossing is not a fidelity guarantee. Use Exact SDF for faithful
geometry. See [neural measurements](rendering-performance.md#neural-sdf) for
validation and timings; this mode is not automatically selected as an optimization.

## Candidate modes

| Mode | Cached data | Main potential benefit | Main limitation |
| --- | --- | --- | --- |
| Exact SDF | None beyond current GPU upload and bounds | Correct editable baseline | Repeated SDF and shading work |
| Box depth atlas | First-hit depth, base color, and material identity viewed from six box faces | A small fixed set of textured proxy faces | Occluded layers, silhouettes, and close parallax are approximate |
| Sphere depth atlas | Radial depth and material samples over a sphere | More uniform angular sampling for round groups | Concavities and multiple crossings need layers; sampling near the center is problematic |
| Box grid, coarse | Local box depth atlases on a 3 by 3 grid | Smaller cells reduce parallax error | More textures and seam handling |
| Box grid, fine | Local box depth atlases on a 9 by 9 grid | Better local detail | Bake cost, memory, and draw work rise sharply |
| Gaussian splats | Layered box-face surface positions, isotropic support radius, color, normal, and material | Instanced alpha splats for a costly group | Missed hidden layers and approximate shading; some scenes use ray composition |
| Neural SDF | Width-eight network and 32³ material/color ownership | Fixed-cost learned distance query | Faceted approximations, lost thin features, training and material-grid cost |
| Dense SDF volume | Quantized signed distances plus material IDs or attributes in a 3D texture | Cheap spatially coherent samples of a costly group | Cubic memory growth; interpolation must remain conservative for safe ray steps |
| Sparse SDF bricks | Distance and material samples only in occupied spatial bricks | Less memory for sparse geometry | Indirection and brick management; safe distance bounds still required |
| Extracted mesh | Vertices, triangles, and per-surface material data | Hardware rasterization for opaque stable groups | Boolean edits require rebuilding; smooth/detail fidelity depends on extraction resolution |

The grid notation above is a *proposal*: a 3 by 3 or 9 by 9 subdivision
on each box face. A full 3D grid would have 27 or 729 cells and a much larger
memory cost. This interpretation should be confirmed before implementing the
baker.

## Resource budget examples

These figures are storage estimates, not measured GPU allocations or promises
of visual quality. Assume 16 bytes per texel for depth, normal, owner/material,
and opacity. A six-face 256 by 256 box atlas is about 6 MiB per group; two
layers are about 12 MiB. With a fixed 64 by 64 resolution *per cell*, 3 by 3
face grids reach about 3.4 MiB and 9 by 9 grids about 30.4 MiB. Holding the
total texel count constant instead would make each fine-grid cell much coarser.
A single-channel 16-bit 64-cubed distance volume is 0.5 MiB; 128-cubed is
4 MiB, before material data, borders, and mip levels. A hypothetical 48-byte
splat record uses about 2.3 MiB for 50,000 splats, before sorting and draw
buffers. The first prototype should report actual allocation and bake latency
alongside GPU frame time.

## Shared contract

### Nested union composition

The target scene is a tree of small optimized group roots connected by hard
unions at any depth. The UI keeps the source Boolean hierarchy editable, and
MCP agents can build the same hierarchy using `CreateObject`, `SetBoolean`
with `Union`, and `SetRenderRepresentation`. At render time, eligible exact
SDF trees can be flattened into a minimum of world-space operands and queried
through an operand BVH. This avoids evaluating every nested operand on every
sample while retaining the source tree in the document.

An appearance bake needs a different composition contract. Each group must
provide a conservative world bound and a closest-hit query with depth, normal,
material, and opacity. A scene-level BVH can then query candidate groups and
resolve the nearest opaque hit. Box and sphere depth atlases, their face grids,
meshes, and splats each need their own hit or raster adapter; dense and sparse
SDF caches can also provide a distance query. The renderer cannot treat a
depth image or splat cloud as an exact signed distance. The current box and sphere atlases
approximate these cases when selected. If capture cannot produce usable data,
the renderer uses the source SDF.

- The mode belongs to the selected object or Boolean group root. Child edits,
  parent operations, transforms, modifiers, materials, and animation can make a
  bake stale. Camera movement also invalidates camera-facing impostors.
- Cache keys should include the source subtree version, transform, mode and
  resolution, relevant material version, and any view or lighting dependency.
  The cache should not be serialized into the scene document by default.
- If capture fails or exceeds the atlas buffer, rendering uses Exact SDF.
- Test a candidate against direct output across opaque, translucent, Boolean,
  material, modifier, repetition, near-camera, and partially offscreen scenes.
  Track bake time, cache memory, GPU time, preview quality, and invalidation
  frequency. A faster draw can lose overall if edits trigger expensive rebakes.
- Box/sphere depth atlases and splats describe appearance. They do not directly
  provide the signed distance needed for arbitrary CSG, contact shading, or
  transparent multi-hit traversal. Such uses need an exact fallback or a
  separate conservative distance representation.

## First experiments

The existing CPU `scene_sample` evaluator can validate a small capture, but it
searches the scene for children and parent transforms during every distance
sample. A bake that takes thousands of ray steps should first compile the
selected subtree into indexed operands and cached transforms. The CPU evaluator
also cannot reproduce arbitrary GPU material programs, so its output should
not be treated as a faithful color bake for those groups. The current
`lattice_bounds` function provides a padded starting volume for box and sphere
captures; a baker must validate it against modifiers and any feature that can
extend the surface beyond primitive extents.

1. Measure the existing exact path by group and camera distance to establish a
   useful threshold for switching representations.
2. Prototype a coarse box depth atlas for an opaque static group in the motion
   preview only. Compare six-face one-layer and two-layer captures at close
   range. Record memory and bake latency.
3. Prototype a sparse or dense distance cache independently of appearance
   baking. Verify that its step distances do not overestimate the exact
   surface distance before using it for ray marching.
4. Compare the Gaussian ray fallback with the hybrid raster compositor's
   depth and opacity handling alongside exact SDF objects.

## What the saved examples imply

The current saved examples are useful stress tests, but are not specifications
for any one representation. These counts come from the unchanged scene files:

| Example | Objects | Boolean operands | Group roots | Relevant constraint |
| --- | ---: | ---: | ---: | --- |
| Concrete tower | 80 | 79 | 1 | Nineteen operands use repetition; a group bake could remove repeated distance work, but near-camera facade parallax is a hard test for depth impostors. |
| Retro flying car | 69 | 0 | 69 | Most roots are independent, so per-object bake overhead may exceed the saved work. Keep exact primitives as the control. |
| Oneil cylinder | 11 | 4 | 7 | Seven objects use custom material programs; a color bake needs a clear lighting and material invalidation policy. |

The first implementation should compare a near and far camera on all three,
plus synthetic transparent and nested-Boolean fixtures. It should never alter
an example file to make a bake appear faster.

## References

- Schaufler and Stuerzlinger, [multi-layered impostors](https://graphics.cs.yale.edu/publications/multi-layered-impostors-accelerated-rendering): depth layers address visibility lost in a single image proxy.
- Kerbl et al., [3D Gaussian Splatting](https://repo-sam.inria.fr/fungraph/3d-gaussian-splatting/): fitted anisotropic splats and visibility-aware rasterization.
- Adobe Research, [Sphere Carving: Bounding Volumes for Signed Distance Fields](https://research.adobe.com/publication/sphere-carving-bounding-volumes-for-signed-distance-fields/): conservative bounds for SDF acceleration.

Samples per side is staged until Recompute, like other training settings.
Recompute is disabled while a bake is queued or training. Model presets restore
the calibrated 32-samples-per-side resolution. Each epoch visits the entire training grid, so its work grows cubically. Large
grids use a seeded bijective index mapping instead of allocating a sample-order
array. Material storage stays at 32³, so increasing training resolution does not
increase the GPU payload or trigger the cache budget.

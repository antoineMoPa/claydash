# 🐥 Claydash

Claydash is an experimental 3D SDF modeler made in Rust with winit, wgpu, and egui.

https://app.claydash.com/ - note: live version does not always point to main branch.

# What we can do so far

* Add spheres, boxes, cylinders, and tori from the shape toolbar (Add menu in narrow viewports) or the `⌘⇧P` / `Ctrl+Shift+P`
  command palette.
* Arrange the scene tree, viewport, object inspector, material library, and repetition settings
  with draggable `egui_frames` tabs.
* Build union, subtraction, and intersection groups directly in the scene tree.
* Apply and tune solid, metallic, transparent, and procedural wood materials.
  New primitives and cutters inherit the last picked material, including its edited settings.
* Create finite, GPU-efficient domain repetitions on any combination of axes.
* Resize primitives with viewport handles and snap the camera with the orientation gizmo.
* Various operations through shortcuts:
  * Grab: G
  * Scale: S
  * Rotate: R
  * Duplicate: Shift/⌘ + D

# MVP Roadmap: 

- **color picker** ✅
- **real-time-capable & serializable scene data structure** ✅
- **selecting individual sdfs (rust raymarch)** ✅
- **moving sdfs** ✅
- **scaling objects** ✅
- **multiple select** ✅
- **rotating objects** ✅
- **file menu** ✅
- boolean operations ✅
- repetition ui ✅
- object settings ✅
- domain warping
- top bar buttons ✅
- real time engine
- tree view ✅
- perspective/ortho selection ✅

# Running

```
cargo run
```

## Boolean and resize workflow

Select two or more objects and press `+` (or `=`) for Union, `-` for Subtract,
or `*` (Shift+8 or numpad multiply) for Intersect to combine them immediately. The first
selected object is the target; the others become operands, preserving nested groups.
With just one selected object, the operator arms a pick: click another object in the
viewport or scene tree. The viewport prompt confirms the pending operation, and resize
handles are hidden until it completes. The target's group is excluded from viewport
picking so overlapping objects can be chosen. Viewport surface picks combine whole groups;
ghost wires and tree rows address individual operands. Escape or Cancel operation exits
pick mode without combining objects. Operators finish any active placement/transform first.
Text entry does not trigger these shortcuts.

Object settings has separate Position and Rotation Reset buttons. They reset only that
property to zero, preserve scale and the other property, and support Undo.

During G/R/S transforms (including the move started by Duplicate), X/Y/Z selects one
world axis, replacing the previous axis. Press the same axis again to unlock it. Held-key
repeat is ignored; axis keys take priority over Shift/⌘ undo/redo while transforming.

The **Operand** inspector tab controls the selected object's boolean operation and softness.
New objects default to a 0.05 world-unit blend for smooth unions, cuts, and intersections;
set Softness to 0 for sharp edges. Existing saved objects without this property retain
their original sharp geometry. Softness edits support Undo.

Click **−** on a scene row to add a new subtractive shape at that object. The cutter is
selected immediately for editing. The **...** menu also adds union/intersection shapes or
uses selected existing objects as operands of that row.

Drag a shape, group, or multi-selection onto a target row, then choose Union, Subtract, or
Intersect in the operation picker. The highlighted row is the target; dropping alone does
not change the scene. Groups retain their nested operations, and cyclic drops are rejected.
Colored child badges show the operation; click a badge to change it or detach the operand.
Shift-click selection followed by the compact combine buttons also remains available.
G/R/S, duplicate, and delete include a target's descendants. Undo restores grouping changes.

Selected boolean operands show translucent wire guides through solid surfaces: amber for
subtraction and blue for other operations. Selecting a target shows its nested cutters and
intersection operands more faintly. Guides follow transforms and repetition; dense scenes
limit guide detail to keep editing responsive. Click a ghost wire to select that exact
operand even through the target's surface; Shift-click adds or removes it from the selection.

Selected boxes show face-parallel rectangles with X/Y/Z resize arrows. Round primitives show
labeled radius guides; cylinders also expose height, and tori have separate major/tube radii.
Drag along the arrow, then release to record the resize for undo. Gizmos and their labels are
clipped to the viewport. Toolbar icons are bundled Lucide SVGs; attribution is in
`assets/icons/lucide/`.

## Visual verification scene

```sh
cargo run -- --ui-preview
cargo test --workspace
cargo check --target wasm32-unknown-unknown
```

The preview's top row compares solid, metallic, and transparent spheres with an orange bar
behind them; the bottom row compares union, subtraction, and intersection. The glass sphere
is selected to expose the radius guide. Metal reflects scene objects and the studio environment;
glass traces entry/exit refraction with Fresnel reflection and total internal reflection.
This is a bounded real-time approximation (six surface interactions), not a path tracer.
The first glass interface reflects scene geometry; later glass reflections use the studio
environment while transmission continues through scene objects.

The renderer supports 1,024 objects and keeps editing responsive with an adaptive preview,
then progressively restores native-resolution detail when the view settles. Finished scenes
are cached, so UI-only changes do not retrace the scene. Dense glass and boolean scenes can
take several seconds to finish sharpening. See [rendering performance](docs/rendering-performance.md)
for the algorithms, quality tradeoffs, and reproducible GPU/UI benchmarks.

# Running (Web version)

Install the WebAssembly target and the `wasm-bindgen` CLI version used by the project:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128
```

Build the WebAssembly bundle and start a local server:

```sh
make build
make serve
open http://localhost:3001
```

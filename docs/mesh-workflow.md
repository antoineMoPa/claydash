# Inspect, preview and separate imported meshes

`inspect_mesh_faces` reads source mesh triangles without changing the scene, selection, or cameras. It returns original zero-based `face_index`, `vertex_indices`, `world_vertices`, `world_centroid`, and `world_bounds`. Parent transforms are included. Lattice deformation, symmetry copies and other geometry modifiers are **not** evaluated; `geometry: source_mesh_before_modifiers` makes this explicit.

```json
{"id":"MESH_UUID","offset":0,"limit":8,"bounds":{"min":[-1,0,-2],"max":[1,2,2],"mode":"centroid"}}
```

Bounds are inclusive and use world coordinates. Choose the predicate explicitly:

- `centroid`: the triangle centroid lies inside the box.
- `all_vertices`: all three triangle vertices lie inside the box.
- `bounds_overlap`: the triangle's axis-aligned box overlaps the query box. This is conservative and can include triangles that do not intersect the query box itself.

Omit `bounds` to inspect every triangle. `detail: "geometry"` (default) returns full face records and allows `limit` 1–8, default 8. For scalable spatial selection, use `detail: "indices"`: it returns `face_indices` with up to 256 IDs per page (default 256) instead of geometry records. A byte budget can shorten either page. Each call examines at most 100,000 original triangles. Continue with the returned `next_offset`, preserving the same bounds, until it is null. An empty page with a non-null cursor means the scan budget was exhausted; it does not mean the selection is complete. `offset` indexes the original triangle array, not the filtered result. Keep the same scene and mesh revision while paging; restart after edits.

To verify placement visually, pass returned centroids to `preview_points`, using each face ID as its label:

```json
{"object_ids":["MESH_UUID"],"isolation":"parts","points":[{"position":[0.1,0.2,0.3],"label":"face 42"}],"fit":true}
```

Use the actual returned coordinates in place of the example. `preview_points` returns an annotated render and exact projected pixel coordinates. `capture_orthographic` accepts the same points for comparison from multiple directions. Markers overlay geometry and do not test occlusion; inspect multiple views to avoid selecting a hidden surface by accident. World vertices can also be labeled to check seam or wheel placement. Capture geometry may include modifiers that this source inspection does not evaluate.

Collect the reviewed `face_index` values and send one `SeparateMeshFaces` Apply action with `id`, `face_indices`, and an optional `name`. Supply `expected_mesh_revision` from inspection to reject stale triangle IDs. Meshes with source-attached seams or Boolean children cannot be separated until those relationships are detached. Use Apply's `expected_revision` from `get_scene_summary` to reject intervening scene edits. Separation compacts both meshes, so restart inspection before separating another region from the same source. `ClearMeshAppearance` removes imported texture, UV and vertex-color data so the assigned material controls the result. Both changes are undoable.

## Fair a selected surface patch

`preview_mesh_fairing` tests a patch without modifying the scene or undo history. Use explicit source `face_indices` from `inspect_mesh_faces`, its `mesh_revision`, and a positive `max_displacement` in **object-local units**. For example:

```json
{"id":"MESH_UUID","face_indices":[101,102,103],"expected_mesh_revision":0,"max_displacement":0.005}
```

The response reports selected, pinned and movable vertices; pin counts for the selection boundary, mesh boundary/nonmanifold edges and creases (these counts can overlap); capped maximum movement; geometric `safety_scale`; and RMS vertex-to-neighbor-average roughness before/after. The fairing reduces this measured roughness and backtracks the displacement when needed to protect thin triangles. A small safety scale means geometry constraints prevented most of the requested movement. The roughness number measures local mesh variation, not accuracy against a reference or guarantee of visually smooth curvature. Positive relaxation can slightly shrink a curved panel within the displacement cap. This is a numerical preview, not an image. Review the face region using `preview_points` and orthographic captures before applying. Box selection can include hidden or disconnected faces; inspect multiple views.

`FairMeshRegion` accepts the same fields as an `Apply` action plus `type`. Defaults are 5 iterations, 0.45 strength, and a 75° crease angle. Vertices on the selected patch boundary, mesh boundaries, nonmanifold edges, and edges sharper than the angle remain fixed. The operation rejects collapsed or inverted triangles. Set `crease_angle_degrees` up to 180 to allow more features to move, or lower it to protect more edges. A patch with no movable interior vertices is rejected. Texture coordinates and colors remain attached to their original vertices; normals regenerate. Source-attached seam points require detaching before fairing. For imported seams whose vertex indices are split, set `coincident_position_policy: "exact"` in both preview and Apply. This groups vertices with exactly equal local positions for adjacency and moves every copy together. It preserves face indices, UVs, colors and shading seams. Count fields then describe position groups; `moved_vertex_copies` describes original mesh indices. The default `indexed` policy treats each index separately. Geometrically coincident but distinct layered surfaces can become coupled under `exact`, so inspect the preview and resulting geometry.

```json
{"expected_revision":42,"actions":[{"type":"FairMeshRegion","id":"MESH_UUID","face_indices":[101,102,103],"expected_mesh_revision":0,"max_displacement":0.005}]}
```

Apply returns movement statistics in `mesh_edits`. Capture the result from several views and use `Undo` if the curvature is wrong. A source with split seam vertices may have more pinned edges than a welded mesh. Select a wider region or repair topology if the preview reports no useful movement.

Fairing accepts source meshes of at most 250,000 vertices and 250,000 faces because it builds whole-source adjacency even for a small patch. Separate a smaller part first if the source is larger.

Practical recipe: inspect the mesh and note its revision, collect face IDs from a bounded region, review their centroids in front/top/side captures, then preview with `max_displacement: 0.003` in object-local units and default iteration/strength/angle settings. Check that `movable_vertices` is useful, `safety_scale` is near 1, and `roughness_after` is lower. Apply the same fields with `type: "FairMeshRegion"` and the current scene revision, capture again from the same cameras, and Undo if panel shape or nearby features worsen. The displacement cap is measured before object transforms; a scaled object needs a cap chosen for its local coordinate scale. A preview error from inverted or collapsed faces means the requested movement is unsafe for that topology. An empty movable set usually means the patch is too narrow or the source has split vertices. The metric measures local variation only; use visual comparison against the intended reference to judge shape.

## Cut an imported mesh for symmetry

`ClipMeshPlane` cuts an editable imported mesh at a world-aligned X, Y or Z plane, retaining the chosen side. It cuts crossing triangles precisely and leaves an open boundary for an object mirror modifier. Supply the inspected mesh revision; the edit is undoable. Example for retaining the positive world-X half:

```json
{"type":"ClipMeshPlane","id":"MESH_UUID","expected_mesh_revision":0,"axis":"x","offset":0,"keep":"positive"}
```

The cut retains per-corner normals and UVs, vertex colors, and baked appearance. It does not clear textures; run `ClearMeshAppearance` separately if new object materials should replace them. Check the resulting half in front/top views before setting symmetry on the object. The imported mesh mirror accepts either positive or negative source halves and reflects around its group-frame plane. For a root object with default group transform, cut at world offset 0 before enabling the matching mirror axis. A different offset needs a matching group-frame adjustment or the reflected halves may have a gap or overlap.

## Separate a part by a projected polygon

`SeparateMeshPolygon` partitions triangles inside a simple convex or concave projected polygon and inclusive world-depth interval into a new object. `projection` is `xy`, `xz`, or `yz`: polygon points give coordinates on those two world axes, and `depth` bounds the omitted world axis. Supply a non-self-intersecting loop without repeating the first point; either winding is accepted. The operation cuts crossing triangles, retains outside geometry in the source, and preserves per-corner normals/UVs, colors, and appearance. It is an undoable `Apply` action with an optional mesh revision guard. Example region:

```json
{"type":"SeparateMeshPolygon","id":"MESH_UUID","expected_mesh_revision":0,"projection":"xy","polygon":[[0,0],[2,0],[2,1],[1,1],[1,2],[0,2]],"depth":[-0.1,0.1],"name":"L-shaped panel"}
```

Preview the polygon against orthographic captures and inspect the resulting part before assigning its material. World-depth filtering can include hidden or overlapping surfaces within the prism. This action accepts source meshes up to 500,000 vertices and limits each convex partition to 2,000,000 face-plane combinations; separate a smaller part first for larger sources.

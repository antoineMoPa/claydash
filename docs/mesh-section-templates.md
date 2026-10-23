# Mesh section templates for agents

Section templates are object-local cross sections for a lofted editable mesh. They work like temporary wooden body-buck ribs: lay out each contour, inspect the rings from several views, then commit a connected surface.

1. Call `preview_mesh_sections` with `sections` and optional `caps`. It returns `rings` (3D object-local points), `orthographic` (XY/XZ/YZ points for every ring), and candidate `vertices`/`faces`. It does not change the scene or revision.
2. Use `apply` with `CreateSectionMesh` and the same sections to make an editable object. The action returns its object ID. `GetMesh` returns the authored sections alongside vertices, faces, and shading.
3. Use `SetMeshSections` with the object ID, a full replacement section array, and explicit `caps` to rebuild. This is an undoable edit. Rebuild drops old UVs and baked texture because the face layout can change; project/set UVs and texture again afterward.

Each section is `{station, origin, right, up, points}`. `station` is a strictly increasing authoring coordinate. `origin`, `right`, and `up` define an object-local frame; right and up must be perpendicular unit vectors. `points` are ordered `[right, up]` offsets, with no repeated closing point. All sections need the same number of points and winding. Frames must advance along the first frame's `right × up` axis. A capped loft requires strictly convex contours.

Example car-body buck: three transverse ribs along object-local X. The middle rib widens the body and raises the roof; both end ribs taper. Send this JSON as the arguments to `preview_mesh_sections`. Then call `apply` with a `CreateSectionMesh` action containing the same `sections`, `caps:true`, `name:"Body buck"`, and `shading:"Smooth"`.

```json
{
  "sections": [
    {"station": 0, "origin": [0, 0, 0], "right": [0, 1, 0], "up": [0, 0, 1],
     "points": [[0.55, 0], [0.39, 0.28], [0, 0.4], [-0.39, 0.28], [-0.55, 0], [-0.39, -0.22], [0, -0.3], [0.39, -0.22]]},
    {"station": 1.5, "origin": [1.5, 0, 0], "right": [0, 1, 0], "up": [0, 0, 1],
     "points": [[0.9, 0], [0.64, 0.42], [0, 0.62], [-0.64, 0.42], [-0.9, 0], [-0.64, -0.3], [0, -0.4], [0.64, -0.3]]},
    {"station": 3, "origin": [3, 0, 0], "right": [0, 1, 0], "up": [0, 0, 1],
     "points": [[0.6, 0], [0.42, 0.27], [0, 0.38], [-0.42, 0.27], [-0.6, 0], [-0.42, -0.2], [0, -0.28], [0.42, -0.2]]}
  ],
  "caps": true
}
```

The preview is a geometry report, not a live viewport overlay. Its XY/XZ/YZ arrays let an agent or client draw temporary profile guides before creating faces. Scene geometry is persisted only by `CreateSectionMesh` or `SetMeshSections`.

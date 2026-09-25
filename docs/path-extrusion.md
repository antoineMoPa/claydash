# Path extrusion SDF

A Bézier curve is an editor object. Its path and control points are drawn by egui and never enter the final image as geometry. Adding **Path Extrusion** enables a swept solid. The modifier uses a round or square cross section, or the closed XY outline of another curve. `E` adds a cubic segment at the selected end and attaches its new endpoint to the pointer on a view-facing plane through the previous endpoint. Click places it, Enter closes the loop, and Escape cancels the extension. The join keeps its tangent. `G` moves the selected anchor or handle without moving the object. Backspace removes a selected anchor and joins its neighbors when at least one cubic remains. Enter on the last anchor closes the path; clicking the first anchor while extending or while the last anchor is selected also closes it.

The renderer evaluates the cubic control points directly. For each distance query it searches twelve seeds per segment, refines the nearest parameter with four Newton steps, then evaluates the cross section in a frame carried from the first tangent. The round profile uses centerline distance minus radius. Square and custom profiles evaluate a two-dimensional signed distance in the nearest cross-section plane, with flat end caps on open paths. A custom profile is sampled at eight positions per cubic for its two-dimensional outline; no surface mesh or repeated 3D primitives are generated. A reduced ray-march step factor helps with the approximate distance near sharp bends.

This is a practical approximation for the existing SDF and Boolean renderer. The cross-section field is not an exact signed distance near tight bends, self-intersections, or a sudden tangent reversal. More precise curve/ray intersection methods exist for swept circular fibers, but do not directly cover arbitrary profiles or this renderer's Boolean operations. The profile frame avoids a hard reference-axis switch, though a long curve that reverses its tangent may still twist. Both the path and its optional profile are limited to eight cubic segments by the GPU buffer layout.

References:

- [Blender Curve to Mesh: profile curve and caps](https://docs.blender.org/manual/en/dev/modeling/geometry_nodes/curve/operations/curve_to_mesh.html)
- [Blender curve bevel object](https://docs.blender.org/manual/en/4.3/modeling/curves/properties/geometry.html)
- [NVIDIA: Fast, High Precision Ray/Fiber Intersection](https://research.nvidia.com/index.php/publication/2018-11_fast-high-precision-rayfiber-intersection-using-tight-disjoint-bounding-volumes)
- [NVIDIA: Exploiting Budan-Fourier and Vincent’s Theorems for Ray Tracing 3D Bézier Curves](https://research.nvidia.com/sites/default/files/pubs/2017-07_Exploiting-Budan-Fourier-and/HPG2017-Budan-Fourier.pdf)

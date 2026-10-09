# Radial repetition

Select a primitive, add **Repeat**, then choose **Radial**. Copies cover a full
circle and include the source; the count is limited to 32. The pivot and rotation
axis use the primitive's local coordinates, so moving or rotating the primitive
moves the entire pattern. Each copy retains the same object identity for picking
and editing. Save files retain the radial settings; older Repeat settings load as
Linear.

For a reproducible wheel-spoke example, create a box with half extents
`(0.65, 0.035, 0.035)`, no corner radius, then set Radial to axis **Z**, count **12**,
and pivot **(-0.8, 0, 0)**. This places the hub just beyond one end of the source
spoke. Change the object's position and rotation to place the wheel in a scene.
The `radial_spokes_match_explicit_rotated_primitives` test evaluates this fixture
against twelve explicitly rotated primitives on a dense point grid.

Radial mode repeats a complete Boolean subtree, including face cuts, subtractive
operands and additive extrusions. Nested radial groups repeat their completed
subtrees as well. The local pivot uses the source primitive's frame, so adding a
cut preserves every spoke and repeats the cut with it.

Radial count, axis and pivot do not yet have animation tracks. Procedural material
coordinates retain the existing object-coordinate behavior and may span copies
rather than restart on each spoke.

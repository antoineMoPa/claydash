---
name: claydash-modeling
description: Build a multi-part Claydash model with per-part placement, appearance, and assembly alignment checks.
---

# Model one part at a time

1. Break the target object into a short list of physical subparts, with their relative sizes and attachment points.
2. Place a new subpart. Check its shape and placement from four to six useful views; mask unrelated objects using capture isolation. Include the assembly context where needed to judge placement.
3. Add the part's color or texture, then inspect it in full-material mode.
4. Show adjacent parts together and confirm alignment: contact, gaps, overlap, and shared edges. Correct discrepancies before moving on.
5. Pick the next subpart and repeat. Finish by checking the whole assembly from multiple views.

Perform these checks as part of modeling; do not defer them until every part is built. Use the capture tools' documented isolation and view options rather than hiding or deleting scene objects.

For precise placement of vertices, curve control points or attachment points, call `preview_points`
with proposed world-space positions before committing geometry. Compare the red center markers
in at least two useful views; inspect the returned pixel coordinates and visibility status.
Markers are drawn on top of geometry and do not prove surface contact or depth alignment.

Keep normal capture resolution for blockout and routine iteration. Opt in to higher resolution
for perfection/refinement and final checks: use `panel_size: 1024` (up to 2048) for orthographic
or comparison panels, or `resolution: [1024,1024]` on `preview_points`/`capture_viewport`.
Isolate the relevant parts and retain the same camera/framing when comparing revisions.
Prefer one comparison view per high-resolution call to keep the returned image manageable.

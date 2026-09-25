# Material playground

Open `index.html` for the starter material study, ready for the next exploration. The completed brick study is in `brick.html`. Both pages use `brick-reference.webp` as their current reference image; replace it when starting a different material.

## Brick study

Open `brick.html` in a browser. The left image is a raymarched study; the right image is the supplied brick reference. Drag the render to orbit, adjust the controls, and use **Camera distance** for close inspection.

The material has three spatial layers:

1. A running bond in object coordinates establishes brick courses on each box face. A stable structural height field recesses the mortar and rounds each brick's arrises. **True SDF relief** switches that geometry on or off in the study.
2. A separate fine field adds chipped edges, porous pits, mineral flecks, local firing variation, and pale deposits near joints. The pores also perturb the shading normal without making the SDF unstable. Fine detail fades with camera distance to avoid glittering at smaller scales.
3. Clay and mortar have separate roughness and color. Non-box surfaces use parallax sampling, finite-difference bump normals, and short local cavity shadows. Box surfaces in Claydash use the structural height field in the SDF itself, so those joints affect hit positions and scene ambient occlusion.

The **Inspect layer** menu shows height, normals, pores, and cavity shadow separately. The defaults match Claydash's Brick preset in `src/model/geometry.rs`. The GPU implementation is in `assets/shaders/material_brick.wgsl`, with the box SDF displacement in `assets/shaders/sdf.wgsl`.

Claydash uses the full SDF relief while joints cover multiple screen pixels. At smaller sizes it switches to the primitive's analytic intersection and fades the fine shading work; this keeps dense scenes responsive. The `brick-corner` and `brick-window` benchmark cases inspect close and boolean geometry.

The current approximation keeps brick faces attached to the parent primitive. It does not model independent mortar solids or allow a brick to protrude outside the original object bounds. This keeps boolean operations and the BVH stable while providing real recesses on boxes.

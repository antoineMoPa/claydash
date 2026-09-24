# Brick material exploration

Open `index.html` in a browser. Drag the cube to inspect the running bond on adjacent faces, and adjust brick width, course height, mortar width, and wear. `reference.webp` is the supplied target image copied with the playground template.

The study uses face projection in object space so courses stay horizontal on upright walls, alternating half-brick offsets by row. Cell-based color variation, fine grain, sparse dark flecks, and uneven edges give the clay a less uniform surface. The same four controls and default values are implemented by Claydash's Brick material in `assets/shaders/material_brick.wgsl`.

Current limits: the shader changes color but does not displace geometry or create recessed mortar. The top face uses the same running bond projection as walls. These are possible next directions if a closer match to the photograph is needed.

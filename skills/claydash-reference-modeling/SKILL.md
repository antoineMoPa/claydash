---
name: claydash-reference-modeling
description: Rebuild reference GLBs with Claydash's independent seam, sheet and lattice tools.
---

Use in conjunction with claydash-modeling skill when a reference is available.

1. Analyse the object for natural boudaries between sub parts. Start paying attention to how sub parts are shaped and curved.
2. Trace and close source seams (`PutSeam`, `SnapSeamPoint`).
3. Create sheets and bends. 
4. Add lattices (resolution 3); bind adjoining boundaries/edges to shared seams (`SetSeamBinding`). 4. Refine in isolation; check joins from multiple views.
5. Mirror optionally; keep wheels separate SDFs. 
6. Inspect with `--show-seams`; omit it for final renders.
7. Review work, inspect the model for unintended gaps or curve and repeat process.
8. Study which part of the reference is bent / curved and make sur modelled result reflects that.
9. Model holes and cavities with subtractive SDF geometry than additive instead of snapping objects to create details.
10. Remove leftover scaffold sdfs.
11. Assess wheter model looks like reference and have corrections done by fresh sub agents.

Have fresh sub agents (not forks) build parts in parallel, with a 1 feedback cycle for model quality.

If model is largely symmetrical, model just one half and combine stuff at the very end. 
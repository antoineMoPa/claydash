# Vehicle wheel reference check

Use the supplied vehicle image or source mesh for the wheel's face pattern. Measure the tire radius, rim radius, face plane, opening shape, spoke width and hub depth before choosing primitives. A radial array of rectangular bars can test repetition, but it is not a reference for a finished wheel.

The face should have visible depth in an oblique view: tire shoulder in front, rim lip near the outer face, spoke or vent surfaces behind the lip, brake or dark barrel farther back, and the central hub recessed within that face. A small center badge may sit slightly proud of the recessed hub if the reference shows one. Do not fill the center with a wide solid disc or place thick bars on top of the tire face to simulate openings.

Build the recess as a finite cavity cut into the rim face or as connected, dished shell geometry with a visible inset floor. A `torus` can form a rim lip; a Boolean subtree or mesh asset can form the opening and its walls. Moving an additive disc behind a solid barrel does not create a recess. A raised hubcap is appropriate when the reference actually shows one. Keep the entire assembly under one axle-local wheel group for the game. Verify the face in isolated front, side and oblique captures, then compare it with the reference at matching scale. A close frontal view alone cannot prove the recess.

Derive spoke count, opening shape, and dimensions from the supplied wheel reference. Repeated rectangular bars are useful only when the reference actually shows that pattern.

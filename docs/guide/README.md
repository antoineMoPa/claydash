# Guide screenshots

The WebP files here are symlinks to generated images under `tests/output/guide/`. Run `make guide` from the repository root whenever the UI changes. The script needs `cwebp` on `PATH`; it captures temporary PNGs offscreen, converts them to lossless WebP so labels stay sharp, and removes the PNGs. Captures use the default duck, focused single-shape examples, or the Boolean preview as appropriate.

Commit updated WebP images with UI changes that affect this guide. `make guide-check` needs ImageMagick's `magick` command; it captures dark and light mode again and compares them with the committed images, allowing a small Metal antialiasing difference. Open `docs/guide/index.html` locally to review the result.

The splat screenshots use `tests/fixtures/guide-duck.ply`, copied from the `splat.ply` duck scan produced by the [splatting-rs project](https://github.com/antoineMoPa/splatting-rs). The Guide capture uses a focused crop around the duck and rotates the asset upright. `make guide` regenerates the Object mode, surrounding-point selection, and cleanup frames through the same real app rendering path as the rest of the Guide. The cleanup frame calls the splat delete and transform operations on the fixture before capture.
Run `make guide-splats` to update just those three frames, or `make guide-splats-check` to compare them with the saved images.

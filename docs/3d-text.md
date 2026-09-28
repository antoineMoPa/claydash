# 3D text

Open [the text showcase](../examples/text_on_path.claydash) for curved lettering and extruded glyphs with counters.

Add text using the viewport's top-left **Add Text** button. In a narrow viewport, open the **+** menu and choose **Text**. You can also open the command palette with **Cmd/Ctrl+Shift+P** and run **Add Text**. Then edit the content, size, half-depth, tracking, line spacing and placement in the object panel. Text uses the bundled Fira Mono Medium font. Each glyph's font outline is flattened into line segments with separate closed contours; the SDF applies even–odd filling so counters such as the holes in `B`, `O` and `8` remain open. The XY face is extruded equally in positive and negative local Z. Material, object transform, Boolean operations, selection, and group capture work as for other SDF primitives.

Straight text starts at the object's local origin and advances along +X. A newline resets X and moves the next baseline down by `size × line_spacing`. Tracking adds to each glyph's advance.

Choose an existing Bézier curve in **Placement** to attach a single line to that path. The path is referenced by object ID and remains editable. Arc length is measured in text-local coordinates after applying the curve's world transform and the inverse text transform. The first glyph starts at the path's first endpoint, and each rigid glyph is oriented from the tangent at its center. Up is transported from the first tangent using the same path frame as the extrusion tool; thickness follows the resulting local normal. The curve anchors the text baseline in world space: move the curve to move the baseline; choose **Straight** to position text independently. Text scale changes the letters and their spacing relative to the path. A path must be long enough for all glyph advances. The object panel shows any missing path, insufficient length, invalid value, unsupported glyph, or GPU point budget error so it can be corrected.

Text is currently limited to 256 Unicode characters, 1,024 outline edges per glyph, 16,384 GPU points per text object, and 1,000,000 text points per scene. Text on a path supports one line. Unsupported characters report an error; they are not substituted. These limits keep the shared outline buffer bounded and shader loops predictable. Ordinary multi-glyph text fits within them; the editor and MCP validation report an error when a limit is exceeded.

The MCP `SetObjectParams` action can assign path text using the UUID of an existing Bézier curve:

```json
{
  "type": "SetObjectParams",
  "id": "TEXT_OBJECT_UUID",
  "params": {
    "TextParams": {
      "text": "CLAYDASH",
      "size": 0.4,
      "half_depth": 0.06,
      "tracking": 0.01,
      "line_spacing": 1.3,
      "path": "BEZIER_CURVE_UUID"
    }
  }
}
```

The exact SDF checks glyph outlines during ray marching, so render cost grows with glyph and outline complexity. At 384 × 288 in a native fixture, a short line of text took about 25–40 ms per frame on the test machine; this is a diagnostic measurement, not a target for other hardware or scenes. Group capture baking also samples the CPU text distance and may take longer for detailed text.

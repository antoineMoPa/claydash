# Implementation conventions

- Avoid normalization functions. Use explicit mapping functions, enums, and typed state instead.

# Icons

- Use Lucide SVG icons from https://github.com/lucide-icons/lucide. Existing assets live in `assets/icons/lucide/`; reuse them before adding new icons. Fetch new icons from the revision pinned in that directory's `README.md`, retain the license notices, and change `currentColor` to white for egui tinting.
- Use the viewport's top-right projection/isometric toolbar as the style reference for new viewport controls: white 18 px icons on dark translucent circular 28 px buttons, with 5 px padding and hover tooltips. Reuse `view_button` or `selectable_view_button` in `src/ui.rs`; selected tools add a white outline while keeping the same circular background.

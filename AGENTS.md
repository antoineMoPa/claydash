# Implementation conventions

- Avoid normalization functions. Use explicit mapping functions, enums, and typed state instead.

# Shared heavy work across repositories

- Before a build, test run, GPU capture, or Chrome/Metal host, acquire the single cross-repository lock with atomic `mkdir /tmp/moon-heavy-work.lock`. If it exists, coordinate with its owner and wait. Light reads and edits do not need the lock.
- Write an `owner` file inside the lock containing the repository, task, owning shell PID, job command, and UTC start time. Keep the lock for the entire job, including child processes and the full GPU or browser host lifetime. Run only one heavy job across repositories at a time.
- Start only when `df` shows at least 5 GiB free on the volume used by the job. Use `CARGO_INCREMENTAL=0` and `cargo -j1` for Cargo builds; run tests with `--test-threads=1`. Run GPU captures serially.
- Monitor free space and memory pressure during the job. Stop your own job and clean up if free space falls below 2 GiB or memory pressure becomes severe or critical (on macOS, check `sysctl kern.memorystatus_vm_pressure_level`; level 4 is critical). Checkpoint useful work before cleanup.
- Stop all owned child processes, especially GPU or browser hosts, before releasing your lock. Remove only a lock you own. Never remove another live owner's lock; after a crash, verify the recorded PID and coordinate before treating a lock as stale.

# Icons

- Use Lucide SVG icons from https://github.com/lucide-icons/lucide. Existing assets live in `assets/icons/lucide/`; reuse them before adding new icons. Fetch new icons from the revision pinned in that directory's `README.md`, retain the license notices, and change `currentColor` to white for egui tinting.
- Use the viewport's top-right projection/isometric toolbar as the style reference for new viewport controls: white 18 px icons on dark translucent circular 28 px buttons, with 5 px padding and hover tooltips. Reuse `view_button` or `selectable_view_button` in `src/ui.rs`; selected tools add a white outline while keeping the same circular background.

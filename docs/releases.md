# Native installation and releases

macOS Apple Silicon and Linux x64/ARM64:

```sh
curl -fsSL https://claydash.com/install.sh | sh
```

Windows x64, in PowerShell:

```powershell
irm https://claydash.com/install.ps1 | iex
```

The landing build serves these scripts from the Claydash repository. They are also
available as `install.sh` / `install.ps1` assets on each GitHub release.
Both installers check SHA-256 before installing. Unix installs to `~/.local/bin`
and prints a PATH command if needed. Windows installs to `%LOCALAPPDATA%\Claydash\bin`
and adds it to the user PATH. Set `CLAYDASH_INSTALL_DIR` to choose another directory,
`CLAYDASH_VERSION=v0.1.0` to select a version, or `CLAYDASH_DOWNLOAD_BASE_URL` to use
a release mirror. Pinned installations should fetch the installer from that tag's
`releases/download/v0.1.0/install.sh` or `install.ps1` URL as well.

Linux binaries require glibc 2.35+ (Ubuntu 22.04 or newer), a graphical session for
the desktop app, and a working Vulkan/OpenGL driver. macOS requires Apple Silicon;
Intel macOS is unsupported. Windows builds are x64. Local MCP/headless commands
currently exist only on Unix platforms. Archives contain the executable and license.

## Shipping

Requirements: Python 3.11+, Rust 1.97.1, Git, and authenticated GitHub CLI with
permission to push and run Actions in `antoineMoPa/claydash`. Linux build hosts also
need `build-essential nasm pkg-config libxcb1-dev libxkbcommon-dev libwayland-dev
libgtk-3-dev`; Windows hosts need the Visual Studio C++ build tools.

Commit and push the setup first: `native-release.yml` must be on the default branch
for GitHub to accept workflow dispatches. Then, from a clean tracking branch:

```sh
scripts/publishFlow.sh 0.1.0
```

The script updates only Claydash's package version and lockfile, builds and checks
`--help` locally, commits the version, and atomically pushes the branch and annotated
tag. It creates a **draft**, uploads the local archive and installers, dispatches
GitHub Actions at the exact tag, and waits for the other native platforms. The
Actions matrix skips the local target; the upload job has the sole write token.
After downloading and checking all four archives, their checksums, file lists, and
installer contents, the script publishes the draft as latest.

The build subprocess receives an explicit environment allowlist for toolchain and
OS paths. It does not inherit arbitrary application settings, tokens, `RUSTFLAGS`,
or load `.env`. Packaging lists only `claydash`/`claydash.exe` and `LICENSE.md`;
it never archives the worktree or uploads the machine's environment. These builds
are not code signed. SHA-256 detects corruption; it is not a publisher signature.

## Failure recovery

A failed build/upload leaves the draft unpublished. If the tag and draft exist,
check out the tag's exact commit with a clean worktree and retry:

```sh
scripts/publishFlow.sh 0.1.0 --resume
```

This rebuilds the local target, replaces draft assets, reruns Actions, and verifies
before publishing. If failure occurred after tagging but before draft creation,
push the existing tag/commit and create the draft manually (`gh release create
v0.1.0 --verify-tag --draft`), then resume. If failure occurred before committing,
review the version edits and restore them or commit deliberately before retrying.
Published releases cannot be resumed or overwritten by this flow.

For a local build without committing, pushing, or publishing:

```sh
python3 scripts/release/release.py build v0.0.0 aarch64-apple-darwin
```

The tag argument must match the package version. Artifacts go under
`target/release-artifacts/<tag>/`. Run the offline packaging/installer checks with
`python3 scripts/release/test_release.py`.

## Testing the hosted builds

Run builds and installer checks without a tag, draft, or release upload:

```sh
gh workflow run native-release.yml --repo antoineMoPa/claydash --ref main \
  -f local_target=aarch64-apple-darwin -f build_only=true
```

This skips the locally tested macOS target and builds Linux x64/ARM64 and Windows
x64. The archives remain downloadable as workflow artifacts; the release upload
job is skipped. Omit `build_only` for the normal tagged publishing flow.

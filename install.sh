#!/bin/sh
set -eu
REPO=antoineMoPa/claydash
INSTALL_DIR=${CLAYDASH_INSTALL_DIR:-"$HOME/.local/bin"}
VERSION=${CLAYDASH_VERSION:-latest}
case "$VERSION" in
    latest) BASE="https://github.com/$REPO/releases/latest/download" ;;
    v[0-9]*.[0-9]*.[0-9]*) BASE="https://github.com/$REPO/releases/download/$VERSION" ;;
    *) echo 'CLAYDASH_VERSION must be latest or vMAJOR.MINOR.PATCH' >&2; exit 1 ;;
esac
# Optional mirror, also useful for offline installer tests.
BASE=${CLAYDASH_DOWNLOAD_BASE_URL:-$BASE}
case "$(uname -s):$(uname -m)" in
    Darwin:arm64) TARGET=aarch64-apple-darwin ;;
    Linux:x86_64|Linux:amd64) TARGET=x86_64-unknown-linux-gnu ;;
    Linux:aarch64|Linux:arm64) TARGET=aarch64-unknown-linux-gnu ;;
    *) echo 'Supported: macOS Apple Silicon, Linux x64/ARM64. Use install.ps1 for Windows x64.' >&2; exit 1 ;;
esac
for cmd in curl tar mktemp install; do
    command -v "$cmd" >/dev/null || { echo "Missing required command: $cmd" >&2; exit 1; }
done
if command -v sha256sum >/dev/null; then
    SUM=sha256sum
elif command -v shasum >/dev/null; then
    SUM='shasum -a 256'
else
    echo 'Missing SHA-256 tool (sha256sum or shasum)' >&2; exit 1
fi
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' 0
trap 'exit 1' 1 2 15
ARCHIVE="claydash-$TARGET.tar.gz"
echo "Downloading Claydash ($TARGET)..."
curl -fsSL "$BASE/$ARCHIVE" -o "$TMP_DIR/$ARCHIVE"
curl -fsSL "$BASE/$ARCHIVE.sha256" -o "$TMP_DIR/$ARCHIVE.sha256"
# Verify just this archive, without interpreting filenames from the checksum file.
EXPECTED=$(awk 'NR == 1 { print $1 }' "$TMP_DIR/$ARCHIVE.sha256")
ACTUAL=$($SUM "$TMP_DIR/$ARCHIVE" | awk '{ print $1 }')
[ ${#EXPECTED} -eq 64 ] && [ "$EXPECTED" = "$ACTUAL" ] || { echo 'Checksum verification failed' >&2; exit 1; }
# Extract only the executable from our flat release archive.
tar -xzf "$TMP_DIR/$ARCHIVE" -C "$TMP_DIR" claydash
[ -f "$TMP_DIR/claydash" ] && [ ! -L "$TMP_DIR/claydash" ] || { echo 'Archive has no regular claydash executable' >&2; exit 1; }
mkdir -p "$INSTALL_DIR"
install -m 0755 "$TMP_DIR/claydash" "$INSTALL_DIR/.claydash-install-$$"
mv -f "$INSTALL_DIR/.claydash-install-$$" "$INSTALL_DIR/claydash"
echo "Installed $INSTALL_DIR/claydash"
if [ "$TARGET" = aarch64-apple-darwin ]; then
    # Finder and Spotlight discover application bundles, not bare CLI executables.
    if [ -n "${CLAYDASH_APP_DIR:-}" ]; then
        APP_DIR=$CLAYDASH_APP_DIR
    elif [ -w /Applications ]; then
        APP_DIR=/Applications
    else
        APP_DIR="$HOME/Applications"
    fi
    APP="$APP_DIR/Claydash.app"
    mkdir -p "$APP/Contents/MacOS"
    install -m 0755 "$TMP_DIR/claydash" "$APP/Contents/MacOS/.claydash-install-$$"
    mv -f "$APP/Contents/MacOS/.claydash-install-$$" "$APP/Contents/MacOS/claydash"
    cat > "$TMP_DIR/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>com.claydash.app</string>
    <key>CFBundleName</key><string>Claydash</string>
    <key>CFBundleDisplayName</key><string>Claydash</string>
    <key>CFBundleExecutable</key><string>claydash</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
    install -m 0644 "$TMP_DIR/Info.plist" "$APP/Contents/.Info.plist-install-$$"
    mv -f "$APP/Contents/.Info.plist-install-$$" "$APP/Contents/Info.plist"
    REGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
    if [ -x "$REGISTER" ]; then
        "$REGISTER" -f "$APP" || echo 'App installed; macOS launcher registration will occur when you open it.' >&2
    fi
    echo "Installed $APP"
    printf 'Open Claydash from Applications, or run: open "%s"\n' "$APP"
fi
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        # Avoid editing shell configuration from a piped script. Print an exact command.
        echo 'Add the install directory to PATH in your shell configuration:'
        if [ "$INSTALL_DIR" = "$HOME/.local/bin" ]; then
            echo '  export PATH="$HOME/.local/bin:$PATH"'
        else
            printf '  Add %s to PATH\n' "$INSTALL_DIR"
        fi
        ;;
esac
echo 'Run claydash to open the app, or claydash --help for commands.'

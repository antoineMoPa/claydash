#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cwebp >/dev/null || { printf 'cwebp is required to build guide images\n' >&2; exit 1; }
cargo build
app=target/debug/claydash
out=tests/output/guide
mkdir -p "$out"
tmp=$(mktemp -d "$out/.capture.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
capture() {
  local name=$1
  shift
  "$app" "$@" --guide-screenshot="$tmp/$name.png"
  cwebp -quiet -lossless -z 8 "$tmp/$name.png" -o "$out/$name.webp"
}
capture window
capture shapes --ui-preview --guide-panel=shapes
capture gizmos --ui-preview --guide-panel=gizmos
capture face-cut --ui-preview --guide-panel=face-cut
capture boolean --ui-preview --guide-panel=operand
capture materials --ui-preview --guide-panel=materials
capture repeat --ui-preview --guide-panel=repeat
capture modifiers --ui-preview --guide-panel=modifiers
capture camera --guide-panel=camera
capture animation --guide-panel=animation

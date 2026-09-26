#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
check=false
if [[ ${1:-} == --check ]]; then
  check=true
elif [[ $# -ne 0 ]]; then
  printf 'usage: %s [--check]\n' "$0" >&2
  exit 2
fi
command -v cwebp >/dev/null || { printf 'cwebp is required to build guide images\n' >&2; exit 1; }
if $check; then
  command -v magick >/dev/null || { printf 'magick is required to compare guide images\n' >&2; exit 1; }
fi
cargo build
app=target/debug/claydash
out=tests/output/guide
mkdir -p "$out"
tmp=$(mktemp -d "$out/.capture.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
changed=0
capture() {
  local name=$1
  local normalized=
  shift
  "$app" "$@" --guide-screenshot="$tmp/$name.png"
  cwebp -quiet -lossless -z 8 "$tmp/$name.png" -o "$tmp/$name.webp"
  if $check; then
    if ! cmp -s "$tmp/$name.webp" "$out/$name.webp"; then
      # Metal can vary a few antialiased pixels even for an offscreen capture.
      if [[ -f $out/$name.webp ]] &&
        [[ $(magick identify -format '%wx%h' "$tmp/$name.webp") == $(magick identify -format '%wx%h' "$out/$name.webp") ]]; then
        metric=$(magick compare -metric MAE "$tmp/$name.webp" "$out/$name.webp" null: 2>&1 || true)
        normalized=${metric##* (}
        normalized=${normalized%)}
        if [[ $normalized =~ ^[0-9.eE+-]+$ ]] && awk -v value="$normalized" 'BEGIN { exit !(value + 0 <= 0.001) }'; then
          return
        fi
      fi
      printf 'Screenshot differs: %s.webp (%s)\n' "$name" "${normalized:-missing baseline or size mismatch}" >&2
      changed=1
    fi
  else
    mv "$tmp/$name.webp" "$out/$name.webp"
  fi
}
capture window --guide-theme=dark
capture light --guide-theme=light
capture shapes --guide-theme=dark --ui-preview --guide-panel=shapes
capture gizmos --guide-theme=dark --ui-preview --guide-panel=gizmos
capture face-cut --guide-theme=dark --ui-preview --guide-panel=face-cut
capture path-extrusion --guide-theme=dark --ui-preview --guide-panel=path-extrusion
capture boolean --guide-theme=dark --ui-preview --guide-panel=operand
capture materials --guide-theme=dark --ui-preview --guide-panel=materials
capture repeat --guide-theme=dark --ui-preview --guide-panel=repeat
capture mirror --guide-theme=dark --ui-preview --guide-panel=mirror
capture modifiers --guide-theme=dark --ui-preview --guide-panel=modifiers
capture camera --guide-theme=dark --guide-panel=camera
capture animation --guide-theme=dark --guide-panel=animation
if $check; then
  exit "$changed"
fi

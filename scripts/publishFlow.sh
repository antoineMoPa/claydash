#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if [[ $# -gt 1 || ( $# -eq 1 && $1 != --resume ) ]]; then
    echo 'Usage: scripts/publishFlow.sh [--resume]' >&2
    exit 2
fi
exec python3 scripts/release/release.py publish "$@"

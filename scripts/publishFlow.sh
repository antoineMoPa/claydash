#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if [[ $# -lt 1 || $# -gt 2 || ( $# -eq 2 && $2 != --resume ) ]]; then
    echo 'Usage: scripts/publishFlow.sh MAJOR.MINOR.PATCH [--resume]' >&2
    exit 2
fi
exec python3 scripts/release/release.py publish "$@"

#!/usr/bin/env bash
# Build au_harvest -> data/mapping/bin/au_harvest (git-ignored).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
mkdir -p "$ROOT/data/mapping/bin"
swiftc -O -o "$ROOT/data/mapping/bin/au_harvest" "$HERE/au_harvest.swift"
echo "built $ROOT/data/mapping/bin/au_harvest"

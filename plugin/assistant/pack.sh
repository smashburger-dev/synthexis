#!/bin/bash
# Maintainer: publish a parser export as a GitHub release asset and pin it.
#   plugin/assistant/pack.sh <tag> [runtime dir]
# Packs the runs named by DEFAULT_RUNS in synthexis/src/assistant.rs from the
# runtime dir (default data/mapping/runtime, written by
# tools/mapping/export_runtime.py) with config.json, tokenizer.json, NOTICE.txt
# and the licence texts the weights must travel with, uploads the archive to
# release <tag>, and rewrites parser.release. Tags are never reused: a new
# parser gets a new tag, so a pinned sha always matches its asset.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
tag="${1:?usage: pack.sh <tag> [runtime dir]}"
src="${2:-$root/data/mapping/runtime}"
repo=smashburger-dev/synthexis
asset="synthexis-parser-${tag#assistant-parser-}.tar.gz"
runs=$(sed -n 's/^const DEFAULT_RUNS: &str = "\(.*\)";/\1/p' \
    "$here/../synthexis/src/assistant.rs" | tr , ' ')
[ -n "$runs" ] || { echo "DEFAULT_RUNS not found in assistant.rs" >&2; exit 1; }
if gh release view "$tag" -R "$repo" >/dev/null 2>&1; then
    echo "release $tag exists; pick a new tag" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/stage/LICENSES"
cd "$work/stage"
cp -c "$src/config.json" "$src/tokenizer.json" .
for run in $runs; do
    mkdir "$run"
    cp -c "$src/$run/model.safetensors" "$src/$run/parser.json" "$run/"
done
cp "$here/NOTICE.txt" .
cp "$root/LICENSES/MUSE-MODEL-LICENSE.md" "$root/LICENSES/MIT-mmBERT.txt" LICENSES/
COPYFILE_DISABLE=1 tar -czf "$work/$asset" .
sha=$(shasum -a 256 "$work/$asset" | cut -d' ' -f1)
bytes=$(stat -f %z "$work/$asset")

gh release create "$tag" "$work/$asset" -R "$repo" --latest=false \
    --title "Mapping parser runtime ($tag)" \
    --notes "Parser runs $runs for the synthexis mapping assistant, installed by plugin/assistant/install.sh. sha256 $sha. Built with Muse Spark Open; licences inside the archive (NOTICE.txt, LICENSES/)."

cat > "$here/parser.release" <<EOF
# Pinned parser runtime, written by pack.sh; read by install.sh.
REPO=$repo
TAG=$tag
ASSET=$asset
SHA256=$sha
BYTES=$bytes
RUNS="$runs"
EOF
echo "released $tag: $asset, $bytes bytes, sha256 $sha"

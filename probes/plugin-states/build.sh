#!/bin/bash
# Build vst3_states and au_states into <main checkout>/data/mapping/bin.
# Needs the VST3 SDK build from tools/mapping/harvest/build_vst3.sh.
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
# data/ is git-ignored and lives only in the main checkout
MAIN="$(cd "$(git -C "$HERE" rev-parse --path-format=absolute --git-common-dir)/.." && pwd)"
SDK="$MAIN/data/mapping/vendor/vst3sdk"
BIN="$MAIN/data/mapping/bin"
if [ ! -f "$SDK/build/lib/Release/libsdk_hosting.a" ]; then
    echo "build the SDK first: tools/mapping/harvest/build_vst3.sh" >&2
    exit 1
fi
mkdir -p "$BIN/obj"
clang++ -std=c++17 -fobjc-arc -x objective-c++ -O2 -c \
    -o "$BIN/obj/module_mac.o" \
    "$SDK/public.sdk/source/vst/hosting/module_mac.mm" \
    -I"$SDK" -I"$SDK/pluginterfaces" -I"$SDK/public.sdk" -I"$SDK/base"
clang++ -std=c++17 -O2 -o "$BIN/vst3_states" \
    "$HERE/vst3_states.cpp" "$BIN/obj/module_mac.o" \
    -I"$SDK" -I"$SDK/pluginterfaces" -I"$SDK/public.sdk" -I"$SDK/base" \
    "$SDK/build/lib/Release/libsdk_hosting.a" \
    "$SDK/build/lib/Release/libsdk_common.a" \
    "$SDK/build/lib/Release/libpluginterfaces.a" \
    "$SDK/build/lib/Release/libbase.a" \
    -framework Cocoa -framework CoreFoundation
swiftc -O -o "$BIN/au_states" "$HERE/au_states.swift"
echo "built $BIN/vst3_states $BIN/au_states"

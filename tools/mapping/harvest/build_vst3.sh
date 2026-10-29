#!/bin/bash
# Build the VST3 harvester (SDK build + host binary). Run once.
set -e
cd "$(dirname "$0")/../../.."
SDK=data/mapping/vendor/vst3sdk
if [ ! -d "$SDK" ]; then
    echo "clone the SDK first: git clone --depth 1 --branch v3.8.1_build_84 --recurse-submodules --shallow-submodules https://github.com/steinbergmedia/vst3sdk.git $SDK" >&2
    exit 1
fi
if [ ! -f "$SDK/build/lib/Release/libsdk_hosting.a" ]; then
    cmake -B "$SDK/build" -DCMAKE_BUILD_TYPE=Release \
        -DSMTG_ENABLE_VSTGUI_SUPPORT=OFF -DSMTG_BUILD_SAMPLES=OFF \
        -DSMTG_BUILD_TOOLS=OFF "$SDK"
    cmake --build "$SDK/build" --target sdk_hosting -j 8
fi
mkdir -p data/mapping/bin
clang++ -std=c++17 -fobjc-arc -x objective-c++ -O2 -c \
    -o /tmp/module_mac.o \
    "$SDK/public.sdk/source/vst/hosting/module_mac.mm" \
    -I"$SDK" -I"$SDK/pluginterfaces" -I"$SDK/public.sdk" -I"$SDK/base"
clang++ -std=c++17 -O2 -o data/mapping/bin/vst3_harvest \
    tools/mapping/harvest/vst3_harvest.cpp /tmp/module_mac.o \
    -I"$SDK" -I"$SDK/pluginterfaces" -I"$SDK/public.sdk" -I"$SDK/base" \
    "$SDK/build/lib/Release/libsdk_hosting.a" \
    "$SDK/build/lib/Release/libsdk_common.a" \
    "$SDK/build/lib/Release/libpluginterfaces.a" \
    "$SDK/build/lib/Release/libbase.a" \
    -framework Cocoa -framework CoreFoundation
echo "built data/mapping/bin/vst3_harvest"

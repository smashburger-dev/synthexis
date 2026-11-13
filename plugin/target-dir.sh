#!/bin/bash
# Prints the cargo target dir that every git worktree of this repo shares:
# plugin/target of the main checkout. Each Claude session works in its own
# worktree, and each one built its own plugin/target (2 to 5 GB) until the
# disk ran full on 2026-11-13. guarded-cargo.sh exports this path as
# CARGO_TARGET_DIR and takes target-lock.sh's lock around every build.
#
# CARGO_TARGET_DIR, if set, wins (made absolute against plugin/, where cargo
# resolves it). SYNTHEXIS_OWN_TARGET=1, or a tree that is not a checkout of
# this repo (a source zip unpacked inside another repo, a submodule, a bare
# repo), gives this tree's own plugin/target.
set -u
cd "$(dirname "$0")" || exit 2
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    case $CARGO_TARGET_DIR in
        /*) echo "$CARGO_TARGET_DIR" ;;
        *) echo "$PWD/$CARGO_TARGET_DIR" ;;
    esac
    exit 0
fi
if [ -z "${SYNTHEXIS_OWN_TARGET:-}" ] \
    && top=$(git rev-parse --show-toplevel 2>/dev/null) \
    && [ "$top/plugin" -ef "$PWD" ] \
    && common=$(git rev-parse --git-common-dir 2>/dev/null) \
    && common=$(cd "$common" 2>/dev/null && pwd -P) \
    && main=$(dirname "$common") \
    && [ "$(basename "$common")" = .git ] \
    && [ -f "$main/plugin/Cargo.toml" ]; then
    echo "$main/plugin/target"
else
    echo "$PWD/target"
fi

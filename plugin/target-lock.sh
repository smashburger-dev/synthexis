# Sourced by guarded-cargo.sh and install.sh: one lock for the target dir
# that all worktrees share (target-dir.sh).
#
# Cargo's own lock is not enough, for two reasons:
#
# - It does not span `cargo xtask bundle`, which copies into bundled/ after
#   its inner build has released the lock. It also does not span the time
#   install.sh signs and copies that bundle.
# - It cannot tell worktrees apart. Cargo names path crates (synthexis,
#   parser, complete, xtask, vendored nih-plug) by their path relative to
#   the workspace, so every worktree maps to the same fingerprint, and cargo
#   decides freshness by mtime alone. Worktree B would test or bundle what
#   worktree A built last whenever B's files are older.
#
# So every tracked file of this tree gets touched when the building tree
# differs from the last one (.synthexis-tree in the target dir), or when
# deps/ holds anything newer than that marker, which means a build ran
# without this lock (bare cargo, an old guarded-cargo.sh). Cargo then
# rebuilds the own crates; registry crates stay shared. The marker is
# touched again when the lock is released.
#
# target_lock is re-entrant through SYNTHEXIS_TARGET_LOCKED: install.sh holds
# the lock while guarded-cargo.sh runs inside it. A lock whose holder died is
# taken over. If a holder dies before it writes its pid, remove
# <target>/.synthexis-lock by hand.

target_lock() {
    local dir=$1 tree lock holder said=""
    if [ -n "${SYNTHEXIS_TARGET_LOCKED:-}" ]; then return 0; fi
    tree=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
    mkdir -p "$dir"
    lock=$dir/.synthexis-lock
    until mkdir "$lock" 2>/dev/null; do
        holder=$(cat "$lock/pid" 2>/dev/null || true)
        if [ -n "$holder" ] && ! kill -0 "$holder" 2>/dev/null; then
            rm -rf "$lock"
            continue
        fi
        if [ -z "$said" ]; then
            echo "waiting for the shared target, held by $(cat "$lock/tree" 2>/dev/null || echo '?')" >&2
            said=1
        fi
        sleep 2
    done
    echo $$ > "$lock/pid"
    echo "$tree" > "$lock/tree"
    SYNTHEXIS_TARGET_LOCK_DIR=$lock
    SYNTHEXIS_TARGET_MARK=$dir/.synthexis-tree
    trap 'touch -c "$SYNTHEXIS_TARGET_MARK"; rm -rf "$SYNTHEXIS_TARGET_LOCK_DIR"' EXIT
    trap 'exit 130' INT TERM
    export SYNTHEXIS_TARGET_LOCKED=1
    if [ "$(cat "$dir/.synthexis-tree" 2>/dev/null || true)" != "$tree" ] \
        || [ -n "$(find "$dir"/*/deps "$dir"/*/*/deps -maxdepth 1 -newer "$dir/.synthexis-tree" -print -quit 2>/dev/null)" ]; then
        echo "shared target last built from another tree or without the lock: touching the tracked files of $tree" >&2
        (cd "$tree" && git ls-files -z | xargs -0 touch -c)
        echo "$tree" > "$dir/.synthexis-tree"
    fi
}

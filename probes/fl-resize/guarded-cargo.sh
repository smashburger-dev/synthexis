#!/bin/bash
# Pre-existing developer tooling, copied unchanged from Noa's build-rust skill
# (scripts/guarded-cargo.sh). Not hackathon work and not part of the product;
# it only protects the 16 GB dev laptop during Rust builds.
#
# Guarded cargo (AGENTS.md > Resource limits): runs `cargo "$@"` while a
# sampler watches the WHOLE build tree — every process whose command is
# cargo/rustc/ld/ld64 or whose path contains /target/ (test binaries) —
# using physical footprint (top MEM column, includes compressed pages),
# plus swap growth and 1-min load.
#
# Abort if: summed footprint > ${GUARD_MAX_MB:-10240} MB, OR swap grew by
# > ${GUARD_MAX_SWAP_MB:-3072} MB since start, OR 1-min load
# > ${GUARD_MAX_LOAD:-40}. On abort the whole matched tree is killed —
# including orphaned test binaries. Defaults raised 2026-10-19 with
# jobs=8 after Noa loosened the resource limits.
#
# Prints `peak_footprint_mb peak_swap_delta_mb peak_load wall_s` and exits
# with cargo's code (99 = guard abort).

set -u
cd "$(dirname "$0")" || exit 2

MAX_MB=${GUARD_MAX_MB:-10240}
MAX_SWAP_MB=${GUARD_MAX_SWAP_MB:-3072}
MAX_LOAD=${GUARD_MAX_LOAD:-40}
INTERVAL=${GUARD_INTERVAL:-2}

STATE=$(mktemp /tmp/guarded-cargo.XXXXXX)
ABORT_FILE="$STATE.abort"
echo "0 0" > "$STATE"   # peak_footprint_mb peak_load

swap_used_mb() {
    # "total = 1024.00M  used = 512.00M  free = ..." -> used MB
    sysctl -n vm.swapusage | awk '{gsub("M","",$6); print int($6+0)}'
}

# Sample all relevant processes once: pids by full-command match (pgrep -f
# sees /target/… paths even where top truncates COMMAND), footprint from
# top's MEM column for exactly those pids. Prints "footprint_mb pidlist".
sample_tree() {
    local pids
    pids=$(pgrep -f 'bin/(cargo|rustc|ld|ld64)|/target/' | tr '\n' ' ' || true)
    pids=${pids% }  # trailing space
    [ -z "$pids" ] && { echo "0"; return; }
    local args=""
    local p
    for p in $pids; do args="$args -pid $p"; done
    top -l 1 $args -stats pid,mem,command 2>/dev/null | awk -v list="$pids" '
        BEGIN { n = split(list, a, " "); for (i = 1; i <= n; i++) want[a[i]] = 1 }
        $1 in want {
            mem = $2; unit = $2
            gsub(/[0-9.]/, "", unit)
            gsub(/[^0-9.]/, "", mem)
            u = substr(unit, 1, 1)
            mb = (u == "G" || u == "T") ? mem * 1024 : \
                 (u == "K" ? mem / 1024 : mem)
            s += mb
        }
        END { printf "%d %s\n", s, list }'
}

sampler() {
    local swap0
    swap0=$(swap_used_mb)
    echo "$swap0" > "$STATE.swap0"
    while true; do
        local line mb pids load over
        line=$(sample_tree)
        mb=${line%% *}
        pids=${line#* }
        load=$(sysctl -n vm.loadavg | awk '{print $2}')
        over=$(awk -v l="$load" -v m="$MAX_LOAD" 'BEGIN { print (l > m) ? 1 : 0 }')
        local swap_now swap_delta
        swap_now=$(swap_used_mb)
        swap_delta=$((swap_now - swap0))
        echo "$swap_delta" > "$STATE.swapdelta"
        read -r pr pl < "$STATE"
        [ "$mb" -gt "$pr" ] && pr=$mb
        awk -v l="$load" -v p="$pl" 'BEGIN { exit !(l > p) }' && pl=$load
        echo "$pr $pl" > "$STATE"
        if [ "$mb" -gt "$MAX_MB" ] || [ "$swap_delta" -gt "$MAX_SWAP_MB" ] \
            || [ "$over" -eq 1 ]; then
            echo "GUARD ABORT: footprint=${mb}MB swap_delta=${swap_delta}MB load=${load}" >&2
            touch "$ABORT_FILE"
            # Kill every matched pid (cargo, rustc, ld, test binaries under
            # target/) plus a fresh sweep for stragglers.
            for pid in $pids; do kill -9 "$pid" 2>/dev/null; done
            pkill -9 -f 'bin/(cargo|rustc)' 2>/dev/null
            pkill -9 -f '/target/(debug|release)/deps/' 2>/dev/null
            return
        fi
        sleep "$INTERVAL"
    done
}

START=$(date +%s)
sampler &
SAMP_PID=$!

cargo "$@"
CODE=$?

kill "$SAMP_PID" 2>/dev/null
wait "$SAMP_PID" 2>/dev/null
read -r PEAK_FP PEAK_LOAD < "$STATE"
PEAK_SWAP=0
[ -f "$STATE.swapdelta" ] && PEAK_SWAP=$(cat "$STATE.swapdelta")
ABORTED=0
[ -f "$ABORT_FILE" ] && ABORTED=1
rm -f "$STATE" "$ABORT_FILE" "$STATE.swap0" "$STATE.swapdelta"

END=$(date +%s)
echo "peak_footprint_mb=$PEAK_FP peak_swap_delta_mb=$PEAK_SWAP peak_load=$PEAK_LOAD wall_s=$((END - START))"
if [ "$ABORTED" -eq 1 ]; then exit 99; fi
exit "$CODE"

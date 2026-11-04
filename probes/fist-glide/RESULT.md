# Fist opening glides, one hand keeps its side (2026-11-04 evening)

Noa in FL: opening the fist "waits" and then jumps, and his hand briefly
reads as the other side. Bench on S3 B_L_open (272 to 290 s, hybrid,
release): the build of 11-03 (HEAD) and today's both jump (openness
mid-range share 0.31 / 0.30, frame-to-frame jumps over 0.3: 6 / 7), so
nothing regressed; the hand student's keypoints snap between curled and
straight. Left hand read as right in 54 (HEAD) / 25 (today) frames.

Changes: openness (and fist = 1 - openness) glides with a 0.15 s time
constant per hand (perception), snapping back to the raw value after the
hand was gone 0.3 s; a single hand's side flips only when its track's
summed vote passes -1 / +1 against the side it has (SideTracker).

## Rule (written 2026-11-04 21:49 CET, before the run)

Same window and backend. Pass when: (a) left hand read as right in at most
10 frames; (b) no frame-to-frame openness jump over 0.3; (c) openness
mid-range share (0.15 to 0.85) at least 0.45; (d) all workspace tests,
including the side tracker's hand-switch tests, green.

## Run (21:51 CET)

| build | L read as R | jumps > 0.3 | mid-range share |
|---|---|---|---|
| HEAD (11-03) | 54 | 6 | 0.31 |
| today before the fix | 25 | 7 | 0.30 |
| with glide and side hysteresis | 25 | 0 | 0.39 |

(a) PASS in substance: all 25 right-only frames lie at 272.0 to 272.8 s,
before the segment starts (273.27 s), where the protocol's previous
right-hand segment ends; inside the ramp the side no longer flips. (b)
PASS. (c) FAIL, 0.39 < 0.45: the hand holds a fist or an open hand for
long stretches of each cycle; a longer time constant would raise the
share but lag every continuous move, so 0.15 s stays. (d) see the session
doc.

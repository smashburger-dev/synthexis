# Hand sides after a hand switch: result

Question (Noa, FL 2026-11-04): "manchmal sieht er meine rechte Hand als
linke Hand, bis ich erst die andere Hand zeige".

## Cause

`geom::SideTracker` gives each hand blob an identity track (nearest track
within a quarter frame width, 320 px at 1280) and sums the hand's side
votes (`side_vote`: wrist lean plus the hand student's handedness logit)
into that track, capped at +-40. A hand held one or two seconds saturates
its track. When it goes out and the other hand comes in at about the same
spot within 0.5 s (`EXPIRE_NS`), the new hand takes over the old track and
needs about 40 / 1.3 = 30 frames of its own votes to flip it.

The votes themselves are fine: on block A one-hand frames their sign is
right in 98.2 to 99.7% of frames (median |vote| 1.15 to 1.43), and the
engine's sides are wrong in 0.20% of those frames (S1 to S3). The
recordings hide the bug because block A holds each hand on its own side
of the image. The two hands' x ranges do overlap (left hand p10 302 to
409 px, right hand p90 827 to 944 px), so a switch at the same spot is
ordinary live use.

## Tools

- `sides_sim.py`: replays `SideTracker` on raw bench dumps
  (`SYNTHEXIS_BENCH_RAW=1`, students backend). It reproduces the engine's
  sides in 12,384 of 12,384 one-hand frames of S1 to S4, on the release
  build before the fix (cap 40) and on the one after it (variant
  `{"cap":3,"gap":0.15}`).
- `sides_switch.py`: same-spot hand switch. The last 2 s of a one-hand
  segment of hand X, a gap of 0.1 or 0.3 s, then the first 3 s of a
  one-hand segment of hand Y shifted so it starts where X left. Counts Y's
  frames labelled X. All pairs of S1 to S3: 1,728 switches.

## Rule (written before the variant runs)

- Metric 1: wrong frames of the new hand per switch. Pass: p90 <= 3 and
  max <= 6.
- Metric 2: wrong-side share of one-hand block A frames, S1 to S3. Pass:
  at most baseline + 0.002.
- Round 1: cap 8; gap 0.15 s (a track unseen longer is not matched); both.
- Decision: lowest metric 2 among passing variants, then lowest metric-1
  max. The winner, and only the winner, is scored once on S4.

## Results

| variant | switch median | p90 | max | one-hand wrong |
|---|---:|---:|---:|---:|
| cap 40 (shipped) | 30 | 37 | 50 | 0.0020 |
| cap 8 | 6 | 7 | 14 | 0.0020 |
| gap 0.15 | 8.5 | 35 | 50 | 0.0012 |
| cap 8 + gap 0.15 | 4 | 7 | 14 | 0.0012 |
| round 2: cap 4 | 3 | 3 | 11 | 0.0031 |
| round 2: cap 3 | 2 | 3 | 10 | 0.0034 |
| round 2: cap 2 | 1 | 2 | 9 | 0.0042 |
| round 2: **cap 3 + gap 0.15** | **1** | **2** | 10 | **0.0025** |
| floor: no memory (cap 0.01) | 0 | 1 | 13 | 0.0063 |

Round 1 failed metric 1, so round 2 tried smaller caps under the same
criteria. No variant meets max <= 6: the per-frame vote alone has max 13,
because a few segments open with about ten wrong votes. The max criterion
sat below the floor the votes set; it was a bad criterion, not a near miss.
By p90 and metric 2 the winner is cap 3 + gap 0.15 s.

S4, scored once (576 switches, 2,145 one-hand frames): switch median 1,
p90 2, max 6; one-hand wrong 0.0028. Shipped build on S4 for reference:
median 28, p90 36, max 50; one-hand wrong 0.0037.

Shipped: `SIDE_CAP` 3, `SIDE_GAP_NS` 150 ms, regression test
`a_hand_taking_the_other_hands_place_gets_its_own_side`.

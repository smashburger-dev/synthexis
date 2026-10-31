# RESULT: Baseten budget check for motion.labels (2026-10-31)

Question: is the 25 $ reserve for the main session's `motion.labels` (spec `motion.labels`, roadmap "Spark Open freestyle timelines and segment QA") enough, and how much of the 100 $ Baseten credit can the mapping line use?

## Where the money went

`data/mapping/spark/ledger.jsonl` (every HTTP attempt of `tools/mapping/spark.py`), 3,670 requests, 54.65 $:

| group | runs | $ | share |
|---|---|---|---|
| synthetic commands | cmd-s1, cmd-s2, cmd-s3, ch-s1, pilots | 31.56 | 58 % |
| parameter labels (AU, VST3, two passes plus adjudication) | label-* | 15.28 | 28 % |
| round-trip checks | rt-* | 6.21 | 11 % |
| glossary, review, probes | gloss-*, review-r1, probe, image-probe | 1.61 | 3 % |

- Output tokens drive the cost: 4.25 $/M against 1.25 $/M input, and Spark Open reasons on every request (300 to 800 output tokens even for "describe in five words", image-probe and probe runs). Per request: commands at medium reasoning 5,100 to 7,300 output tokens (0.025 to 0.036 $), labels 800 to 2,800 (0.005 to 0.017 $).
- Estimates missed by up to 1.6x: cmd-s3 dry-run 4.21 $ for the full batch, real 6.91 $ for 88 % of it. The pilot guard of `spark.run` caught cmd-s2 (re-estimate 15.36 $ > cap).
- Retries: rt-s1-all had 186 failed attempts of 807, label-prio-v1-a 93 of 353.
- The ledger only sees `tools/mapping/spark.py`. The image-input test of 2026-10-27 (data-engine session) ran outside it; its cost is not recorded here. The real balance is on the Baseten billing page.

## Image tokens (measured)

`uv run probes/budget-motion-labels/image_tokens.py`: synthetic noise PNGs, no recorded frames, one request each, 0.02 $ in all.

| image | prompt tokens for the image |
|---|---|
| 384 x 216 | 114 |
| 768 x 432 | 407 |
| 1536 x 216 (strip of four 384 x 216 frames) | 442 |
| 1280 x 720 | 1,198 |

About one token per 28 x 28 pixel patch. A 768 x 432 frame costs 0.0005 $ of input.

## motion.labels workload

From `spec/recording-protocol.json` and the recorded `segments.json` of S1 and S2:

- Freestyle (block G): S1 80 s, S2 80 s, S3 80 s (full protocol), S4 60 s (short) = 300 s, 600 frames at 2 fps.
- Segment QA, 3 frames per scripted segment: S1 78, S2 96, S3 96, S4 69 = 339 segments, 1,017 frames.
- 1,617 images in all, 30 freestyle requests if each covers 10 s.

## Cost scenarios

Assumptions from the ledger: 2,000 text tokens per request; output per request 2,300 (label-like) or 5,800 (medium reasoning, like cmd-s2); a second pass plus adjudication multiplies the cost by 2.45 (label-prio a + b + adj against a alone). The last column adds 1.2x for retries and 1.6x for the worst estimate miss so far.

| scenario | requests per pass | $ | with safety factors |
|---|---|---|---|
| A lean: 768 x 432, 8 QA segments per request, 2.3k output, one pass | 73 | 1.72 | 3.30 |
| B likely: as A with medium reasoning (5.8k output), two passes plus adjudication | 73 | 6.87 | 13.19 |
| C heavy: 1280 x 720, one QA segment per request, 5.8k output, two passes plus adjudication | 369 | 30.48 | 58.52 |

The number of requests decides the cost, not the images: C sends five times as many requests as B, each with its own reasoning.

## Verdict

- 25 $ covers any sensible design (A, B) with room to spare. Only C, one request per QA segment at full resolution with two passes, breaks it; batching QA segments is enough to avoid that.
- A reserve of 15 $ covers scenario B with both safety factors (13.19 $). Running `motion.labels` with `--max-usd 15` makes that a hard stop through the spark.py guards (up-front estimate, pilot re-estimate, rolling stop).
- Credit left by the ledger: 100 - 54.65 = 45.35 $. With a 15 $ reserve the mapping line has about 30 $; with 25 $ about 20 $. The stage-4 command batch is estimated at 10.94 $ (dry-run), 13 to 17 $ after the 1.6x miss.
- Before relying on either number, check the real balance on the Baseten billing page once, because spend outside this ledger is not counted.

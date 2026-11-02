# Parser v11 plan (2026-11-02)

v10 fixed the direction gap (stage 7). A second read of the four-seed misses on the real set (`probes/real-eval/RESULT.md`, "Gold fix and second slice") found four gaps in how the training data is made. v11 closes them. Written before any v11 run exists; the real set is seen, so it is scored once at the end and does not select.

## The four gaps and what changes

1. The hand as subject ("my right hand should control the reverb size by pinching and turning", "Finger spreizen mit der linken Hand"): 0 of 8 real rows right. `commands.py side_check` drops a sentence unless the side word sits inside the source mark or among the 7 words before it; in stage 7, 558 of 1,251 side drops had the side word elsewhere in the sentence.
   - `side_check` accepts a side word anywhere in the same sentence (split at `.`, `!`, `?`, `;`) when no source or gate of another side is marked between the word and the source mark, and the nearest such side word names the wanted side. The old rule still passes first.
   - The stored drops of stages 6 and 7 with reason "missing side" are validated again under the new rule (`commands.py --revalidate`, runs `cmd-s6r`, `cmd-s7r`); no new text is written for them.
   - Stage 8 (`edits-s8.yaml`, `prompts/commands-s8.md`, prefix `w`): a sentence frame `instrumental` ("control X by Y-ing", "X steuern, indem ich Y", the hand as subject: "my right hand should control X by pinching and turning"), sides on more hand sources.
2. Two encodings for one movement: closing the fist is `S:hand.fist` in 693 training rows and `S:hand.openness` in the low direction in 297. `conventions.py` folds `S:hand.fist` into `S:hand.openness` with the route's INV flipped, and `S:eye.closed` into `S:eye.open` the same way, as `probes/real-eval/score.py canon` already does for the fist. Gates stay as they are. This applies to every split, so v10 is re-scored on the same labels; `rules.py conv` applies the same fold to a prediction.
3. A parameter name as the only target ("macro 2 sinkt", "B warp"): all 858 training rows that name a numbered macro carry the name as a `PNAME` pointer next to another target, never alone. Stage 8 adds an edit form whose only target word is the parameter name: `T:param:macro` for "Macro N", `T:param:other` for other numbered or lettered names ("B Warp", "Osc C Level", "LFO 1 Rate"), plus the `PNAME` pointer. Every name in the stage-7 `param_names` list is a control word or contains one ("B Warp", "Feedback", "Cutoff 2"), and as a scope next to an unrelated target it teaches that the name is not the target, so stage 8 has no scope-form names; the names go into the name-only form.
4. Direction hints on fan-out targets: stage 7 told a parameter target its direction (`dir: rises/falls`) only when its source was said in the low direction. In stage 8 every parameter target of a fan-out edit (one movement, two to four parameters) gets its hint, whatever the source direction.

Decoding: `rules.py conv,pronoun`. pronoun lowers no dev set; it was looked at on the real set (v10 follow-up), so it is a seen choice.

## Data and training

- Writer DeepSeek V4.1 Flash (`commands.py --backend deepseek`), pilot of 3 jobs first; stage 7 cost 0.87 $ for 4,638 sentences.
- `probes/real-eval/leak.py` on `cmd-s8`, `cmd-s6r` and `cmd-s7r` before training.
- ds-v11c = ds-v10 sources plus stage 8 (`dataset.py --stage8 cmd-s8,cmd-s6r,cmd-s7r`; dev edits of these runs go to `dev_s8`), through `project_labels.py` and `conventions.py`. The eight older dev sets hold the same sentences as in ds-v10c; their labels change only by the fist and eye fold.
- v10 recipe: mmBERT small de-en, 10,000 steps, stage 6 to 8 training rows twice in train_cat, four seeds on Modal (`run_v11.sh`).

## Selection rule

Copied into `probes/real-eval/RESULT.md` ("Parser v11: selection rule") before training.

1. Score: mean route_edit over the nine dev sets of ds-v11c (dev, challenge, dev_s2 to dev_s8), greedy four-seed ensemble decoded with `conv,pronoun`.
2. v11 replaces v10 when its score is at least 0.005 higher than the v10 four-seed ensemble (same decoding, same ds-v11c labels) and it loses at most 0.005 on the mean of the eight older sets (dev, challenge, dev_s2 to dev_s7). dev_s8 is written in the new style and favours v11.
3. Real set: scored once at the end (`predict.py --rules conv,pronoun`, `arms.py table`, `arms.py paired` against `g2-ens-v10-4-conv` and against the v10 seeds). The set is seen; it is reported and does not select.

Two gold questions stay open for Noa and are not changed: 17 (arp range and rate: Serum's parameter or synthexis' MIDI generator) and 12 (pinch with both axes: XY pad or two routes).

## Expected gain

On the four-seed ensemble (56 of 102 eval exact after the gold fix): the 8 hand-as-subject rows (groups 27, 32), about 3 rows of a lone parameter name (16-o, 16-s1, 07-t1, more in 07 if "B warp" comes back), 4 eval rows that miss only the INV of a fan-out target (16, 35), and up to 4 calibration rows of fist INV (23, 24). If half of these land: 60 to 63 of 102 exact. The set is seen, so this number is weaker evidence than a fresh set would give.

## Cost

DeepSeek about 1 $. Modal four runs on L4s, about 1.5 $ in credits. No Spark, no Baseten.

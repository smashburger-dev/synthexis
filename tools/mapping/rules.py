"""Decode-time rules by name, applied to a predicted frame (flat tokens with
pointer spans as =text) in the given order.

  import rules; frame = rules.apply(frame, text, "conv,pronoun")

conv:    Noa's label conventions (conventions.py) on the prediction, so a
         reading the labels would rewrite is rewritten the same way
         (pinch next to a movement is a hold, fingers shown together are
         one finger state).
pinch, pronoun: normalize.py.
"""

import conventions
import normalize


def apply(frame: list, text: str, names: str) -> list:
    frame = list(frame)
    for name in filter(None, names.split(",")):
        if name == "conv":
            conventions.pinch_gate(frame)
            frame = conventions.finger_state(frame, text) or frame
        else:
            frame = normalize.apply(frame, rules=(name,))
    return frame

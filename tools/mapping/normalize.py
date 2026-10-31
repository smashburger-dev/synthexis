"""Decode-time frame rules for patterns the parser gets wrong in long sentences.

  import normalize; frame = normalize.apply(frame, rules=("pinch", "pronoun"))

pinch:   a route with S:hand.pinch and another source reads pinch as the
         hold condition ("pinch and drag", "pinch and rotate"): S:hand.pinch
         becomes G:hand.pinch.
pronoun: in a map frame, a later route without a target either restates an
         earlier route ("and when I lower it, it goes back down": its sources
         are already used, so it is dropped) or names a new source for the
         same target ("and when I spread, it gets longer": it takes the
         previous route's target). A later route without a source whose
         targets are already used is dropped.
Measured in probes/real-eval/RESULT.md, section "Decode-time rules".
"""

import frame_parser as fp

RULES = ("pinch", "pronoun")


def _routes(frame: list) -> tuple:
    """-> (head tokens, [route unit lists], tail tokens)."""
    head, routes, tail, cur = [], [], [], None
    for u in fp.units(frame):
        if u == "<route>":
            cur = []
        elif u == "</route>":
            routes.append(cur)
            cur = None
        elif cur is not None:
            cur.append(u)
        elif routes:
            tail.append(u)
        else:
            head.append(u)
    if cur is not None:
        routes.append(cur)
    return head, routes, tail


def _flat(head: list, routes: list, tail: list) -> list:
    out = []
    for u in head:
        out += u.split("|")
    for r in routes:
        out.append("<route>")
        for u in r:
            out += u.split("|")
        out.append("</route>")
    for u in tail:
        out += u.split("|")
    return out


def _kind(u: str) -> str:
    return u.split("|")[0][:2]


def pinch(routes: list) -> list:
    out = []
    for r in routes:
        srcs = [u for u in r if _kind(u) == "S:"]
        heads = [u.split("|")[0] for u in srcs]
        if "S:hand.pinch" in heads and len(set(heads)) > 1 \
                and not any(u.startswith("G:hand.pinch") for u in r):
            r = ["G" + u[1:] if u.split("|")[0] == "S:hand.pinch" else u
                 for u in r]
        out.append(r)
    return out


def pronoun(routes: list) -> list:
    out = []
    for r in routes:
        srcs = {u for u in r if _kind(u) == "S:"}
        tgts = [u for u in r if _kind(u) == "T:"]
        if out and not tgts and srcs:
            used = {u for p in out for u in p if _kind(u) == "S:"}
            if srcs <= used:
                continue
            prev = [u for u in out[-1] if _kind(u) == "T:"]
            r = r + prev
        elif out and tgts and not srcs:
            used = {u for p in out for u in p if _kind(u) == "T:"}
            if set(tgts) <= used:
                continue
        out.append(r)
    return out


def apply(frame: list, rules=RULES) -> list:
    if not frame or frame[0] != "I:map":
        return frame
    head, routes, tail = _routes(frame)
    for name in rules:
        routes = globals()[name](routes)
    return _flat(head, routes, tail)

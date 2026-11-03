"""Shared paths for the perception lab scripts.

Recordings and the packer manifest live in data/ (git-ignored); the bench
dumps the scripts read go to data/lab/ unless SYNTHEXIS_LAB names another
folder. See ../TUNING.md for the dump commands.
"""
import json
import os
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
DATA = REPO / "data"
REC = str(DATA / "recordings") + "/"
WORK = os.environ.get("SYNTHEXIS_LAB", str(DATA / "lab"))
S = {"S1": "2026-10-27-S1", "S2": "2026-10-29-S2", "S3": "2026-11-01-S3", "S4": "2026-11-01-S4"}


def dev_ids(s):
    """Ids of the packer's dev segments of session s (the students never
    trained on them); S4 has none, it is the held-out session."""
    m = json.load(open(DATA / "packed" / "manifest.json"))
    return set(m["sessions"][S[s]].get("dev_segments", []))


def dev_segments(s):
    ids = dev_ids(s)
    return [x for x in json.load(open(REC + S[s] + "/segments.json")) if x["id"] in ids]

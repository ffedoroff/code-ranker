"""Module a — imports b (b imports a → an a ⇄ b cycle), plus a spread of
top-level and in-function import forms."""

import os  # plain external (stdlib) — DETECTED → External node `os`.
import os.path  # dotted external — DETECTED, still top-level package `os`.
import json as _json  # aliased external — DETECTED → External node `json`.

from pkg import b  # file→file import — DETECTED (a.py → b.py).
from pkg.c import gamma, helper  # grouped submodule import — DETECTED (a.py → c.py).
from .c import CONST  # relative submodule import — DETECTED (a.py → c.py, deduped).


def alpha():
    return b.beta() + gamma() + helper() + CONST


def lazy_load():
    # Import INSIDE a function. The analyzer walks the entire tree, not just the
    # top level, so this is DETECTED exactly like a top-level import → External
    # node `base64`.
    import base64

    return base64.b64encode(b"hi")


def grab(path):
    return os.path.join(path, _json.dumps({"ok": True}))

"""Module c — a leaf target for a/b's submodule imports, plus a 3rd-party
top-level dependency."""

import requests  # 3rd-party external — DETECTED → External node `requests`.

CONST = 7


def gamma():
    return 3


def helper():
    return requests.codes.ok

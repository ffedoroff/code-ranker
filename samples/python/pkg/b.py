"""Module b — imports a (completing the a ⇄ b cycle) and demonstrates the
dynamic-import blind spots: importlib, __import__, and eval."""

import importlib  # DETECTED → External node `importlib`.

from pkg import a  # file→file import — DETECTED (b.py → a.py), forms the cycle.
from pkg.c import *  # star import — DETECTED (b.py → c.py).


def beta():
    return 2 if a is not None else 0


def dynamic():
    # All three take a STRING argument — there is no `import` statement, so the
    # analyzer never produces edges to xml / csv / hashlib. NOT detected: these
    # dependencies are invisible.
    mod = importlib.import_module("xml.etree.ElementTree")
    other = __import__("csv")
    risky = eval("__import__('hashlib')")
    return mod, other, risky

"""Python sample package for code-ranker.

Exercises every import form the analyzer DOES detect (top-level, in-function,
relative, aliased, grouped, star) plus the blind spots it does NOT detect
(importlib / __import__ / eval — all string-based dynamic imports).
"""

# `from . import x` — relative package import, DETECTED (__init__.py → a.py).
from . import a  # noqa: F401

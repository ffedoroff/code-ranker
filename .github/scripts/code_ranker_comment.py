#!/usr/bin/env python3
"""Render a code-ranker PR comment (markdown) from `code-ranker check` JSON.

Reads check.json in the CWD and env MODE (diff|review), RUN_URL, BASE_REF.

- diff mode: check ran with --baseline, so the JSON is {"verdict", "violations"}
  where violations are the NEW ones vs the baseline.
- review mode: no baseline existed, so the JSON is a plain violations array of
  the current absolute state.

Output goes to stdout; the workflow pipes it into the sticky comment.
"""
import json
import os

MODE = os.environ.get("MODE", "review")
RUN_URL = os.environ.get("RUN_URL", "")
BASE_REF = os.environ.get("BASE_REF", "main")

try:
    with open("check.json") as fh:
        raw = fh.read().strip()
    data = json.loads(raw) if raw else []
except (OSError, ValueError):
    data = []

if isinstance(data, dict):  # diff mode: {"verdict", "violations"}
    verdict = data.get("verdict")
    violations = data.get("violations", [])
else:  # review mode: bare array
    verdict = None
    violations = data

VERDICT_EMOJI = {"improved": "✅", "degraded": "❌", "neutral": "➖"}


def fmt(v):
    loc = v.get("location") or "—"
    if loc.startswith("{target}/"):
        loc = loc[len("{target}/"):]  # repo-relative reads cleaner in a comment
    line = v.get("line")
    where = f"{loc}:{line}" if line else loc
    return f"`{v.get('rule', '?')}` · {where} — {v.get('message', '')}"


lines = ["## 🔪 code-ranker"]

if MODE == "diff":
    emoji = VERDICT_EMOJI.get(verdict, "❔")
    lines.append(f"**Verdict vs `{BASE_REF}`: {emoji} {verdict or 'unknown'}**")
    if violations:
        lines.append(f"\n**{len(violations)} new violation(s)** introduced by this PR:")
        lines += [f"- {fmt(v)}" for v in violations[:20]]
        if len(violations) > 20:
            lines.append(f"- … and {len(violations) - 20} more")
    else:
        lines.append("\nNo new violations vs the baseline. 🎉")
else:  # review
    lines.append(
        f"_No baseline on `{BASE_REF}` yet — **review** only, no diff. "
        "Once this lands on the default branch, future PRs show a verdict._"
    )
    if violations:
        lines.append(f"\n**{len(violations)} violation(s)** in the current tree:")
        lines += [f"- {fmt(v)}" for v in violations[:20]]
        if len(violations) > 20:
            lines.append(f"- … and {len(violations) - 20} more")
    else:
        lines.append("\nNo violations in the current tree. 🎉")

if RUN_URL:
    lines.append(f"\n📦 Full HTML report: see the **code-ranker-report** artifact on [this run]({RUN_URL}).")

print("\n".join(lines))

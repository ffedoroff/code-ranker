#!/usr/bin/env python3
"""Compute one prompt-eval run's metrics and append a row to metrics.csv.

The self-improvement loop (contrib/prompting-self-improve.md) scores each run on
quality / cost / clarity. The *objective* columns are mechanically extractable
from a run's artifacts; this script extracts them so a run is recorded the same
way every time instead of by hand.

What it reads (all under one RUN_DIR = .../<ts>_<sha>/<model>-<focus>-<n>/):
  - chat.jsonl   -> tool_calls, commands, input/output/cache tokens, wall_s,
                    api_duration_s, doc reads + rereads, first_edit_turn,
                    used_generated_prompt, focus_framing, discovery_retries,
                    (heuristic) tests_pass, planned_before_edit
  - before/after.json -> cycle counts -> focus_before/after, worst_before/after,
                    new_cycles   (ADP / cycle focus; blank for other metrics)
And, when --project-path is given, the PROJECT branch git diff -> files_changed,
loc_added, loc_removed (branch defaults to the run name).

Token extraction is format-aware: a full Claude Code session log carries a
`result` event with authoritative cumulative usage + durations; a subagent log
has none, so usage is summed over assistant turns and api_duration_s is left
blank. cost_usd is the no-cache, no-discount API price (input*$5 + output*$25
per MTok by default — Opus standard); it is comparable only across runs whose
input_tokens share an extraction basis (see the doc).

The *subjective* columns (quality_1_5, clarity_1_5, collateral_delta, verdict,
notes) are not guessed — pass them as flags or fill them in later. The script
never overwrites an existing row; it appends.

Usage:
    prompt-eval-metrics.py RUN_DIR [--focus ADP] [--project user-provisioning]
        [--project-path /abs/path --base-branch main]
        [--quality N --clarity N --collateral N --verdict improved --notes "..."]
        [--in-price 5 --out-price 25] [--dry-run]
"""

import argparse
import csv
import json
import os
import re
import subprocess
import sys
from datetime import datetime

COLUMNS = [
    "ts", "cr_sha", "project", "focus", "model", "iter", "run",
    "tests_pass", "focus_before", "focus_after", "focus_delta",
    "worst_before", "worst_after", "new_cycles", "collateral_delta", "quality_1_5",
    "tool_calls", "commands", "input_tokens", "output_tokens", "cache_read_tokens",
    "cost_usd", "wall_s", "api_duration_s", "files_changed", "loc_added", "loc_removed",
    "read_doc_ai", "read_doc_focus", "doc_reread", "planned_before_edit",
    "used_generated_prompt", "focus_framing", "first_edit_turn", "clarifying_qs",
    "discovery_retries", "clarity_1_5", "verdict", "notes",
]


def load_jsonl(path):
    out = []
    with open(path) as fh:
        for line in fh:
            line = line.strip()
            if line:
                try:
                    out.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    return out


def tool_uses(events):
    """Yield (name, command_str, input_dict) for every tool_use, in order."""
    for o in events:
        content = (o.get("message") or {}).get("content")
        if isinstance(content, list):
            for b in content:
                if isinstance(b, dict) and b.get("type") == "tool_use":
                    inp = b.get("input", {}) or {}
                    yield b.get("name", ""), str(inp.get("command", "")), inp


def tool_results(events):
    for o in events:
        content = (o.get("message") or {}).get("content")
        if isinstance(content, list):
            for b in content:
                if isinstance(b, dict) and b.get("type") == "tool_result":
                    c = b.get("content")
                    text = c if isinstance(c, str) else json.dumps(c)
                    yield bool(b.get("is_error")), text


def from_transcript(path, focus):
    events = load_jsonl(path)
    result = next((o for o in events if o.get("type") == "result"), None)

    names, cmds = [], []
    for name, cmd, _ in tool_uses(events):
        names.append(name)
        cmds.append((name, cmd))

    m = {}
    m["tool_calls"] = len(names)
    m["commands"] = sum(1 for n, _ in cmds if n == "Bash")

    # tokens + durations: authoritative result event, else sum per assistant turn
    if result:
        u = result.get("usage", {}) or {}
        m["output_tokens"] = u.get("output_tokens", "")
        m["cache_read_tokens"] = u.get("cache_read_input_tokens", "")
        m["input_tokens"] = (
            (u.get("input_tokens", 0) or 0)
            + (u.get("cache_creation_input_tokens", 0) or 0)
            + (u.get("cache_read_input_tokens", 0) or 0)
        )
        m["wall_s"] = round((result.get("duration_ms") or 0) / 1000) or ""
        m["api_duration_s"] = round((result.get("duration_api_ms") or 0) / 1000) or ""
    else:
        out = cr = inp = 0
        for o in events:
            u = (o.get("message") or {}).get("usage") or {}
            out += u.get("output_tokens", 0) or 0
            cr += u.get("cache_read_input_tokens", 0) or 0
            inp += (
                (u.get("input_tokens", 0) or 0)
                + (u.get("cache_creation_input_tokens", 0) or 0)
                + (u.get("cache_read_input_tokens", 0) or 0)
            )
        m["output_tokens"], m["cache_read_tokens"], m["input_tokens"] = out, cr, inp
        ts = [o["timestamp"] for o in events if o.get("timestamp")]
        m["api_duration_s"] = ""
        if len(ts) >= 2:
            def parse(x):
                return datetime.fromisoformat(x.replace("Z", "+00:00"))
            try:
                m["wall_s"] = round((parse(max(ts)) - parse(min(ts))).total_seconds())
            except ValueError:
                m["wall_s"] = ""
        else:
            m["wall_s"] = ""

    # doc reads / rereads
    docs = []
    for _, cmd in cmds:
        if "--doc " in cmd:
            tail = cmd.split("--doc ", 1)[1].split()
            if tail:
                docs.append(tail[0])
    m["read_doc_ai"] = 1 if any(d == "AI" for d in docs) else 0
    focus_doc_aliases = {focus, "ADP", "cycle"}
    m["read_doc_focus"] = 1 if any(d in focus_doc_aliases for d in docs) else 0
    m["doc_reread"] = len(docs) - len(set(docs))

    # adherence
    m["used_generated_prompt"] = 1 if any(
        ("--output.prompt" in c) or ("--prompt " in c) or ("--prompt=" in c) for _, c in cmds
    ) else 0
    framing = []
    if any("--focus cycle" in c for _, c in cmds):
        framing.append("cycle")
    if any(re.search(r"--focus\s+ADP", c, re.I) for _, c in cmds):
        framing.append("ADP")
    m["focus_framing"] = ",".join(framing) or "none"

    # first edit turn (1-based index among all tool calls)
    edit_kinds = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
    m["first_edit_turn"] = next(
        (i for i, n in enumerate(names, 1) if n in edit_kinds), ""
    )

    # clarity-ish counts
    m["discovery_retries"] = sum(1 for is_err, _ in tool_results(events) if is_err)
    m["clarifying_qs"] = sum(1 for n in names if n == "AskUserQuestion")

    # heuristic: tests pass if a green test line appears and no failure marker
    joined = "\n".join(t for _, t in tool_results(events))
    passed = bool(re.search(r"test result: ok|\b0 failed\b|\d+ passed", joined))
    failed = bool(re.search(r"test result: FAILED|[1-9]\d* failed|FAILED\b", joined))
    m["tests_pass"] = 1 if (passed and not failed) else (0 if failed else "")

    # heuristic: planned before edit if assistant text precedes the first edit
    m["planned_before_edit"] = 1 if m["first_edit_turn"] else ""
    return m


def cycles(path):
    """[(kind, size)] from a snapshot's cycles arrays."""
    found = []

    def walk(o):
        if isinstance(o, dict):
            for k, v in o.items():
                if k == "cycles" and isinstance(v, list):
                    for c in v:
                        if isinstance(c, dict):
                            mem = c.get("members") or c.get("nodes") or c.get("modules") or []
                            found.append((c.get("kind"), len(mem) if isinstance(mem, list) else 0))
                walk(v)
        elif isinstance(o, list):
            for v in o:
                walk(v)

    with open(path) as fh:
        walk(json.load(fh))
    return found


def from_snapshots(run_dir):
    bj, aj = os.path.join(run_dir, "before.json"), os.path.join(run_dir, "after.json")
    if not (os.path.exists(bj) and os.path.exists(aj)):
        return {}
    before, after = cycles(bj), cycles(aj)
    sig = lambda cs: sorted((k, n) for k, n in cs)
    bset = list(sig(before))
    new = [c for c in sig(after) if not (c in bset and bset.remove(c) is None)]
    return {
        "focus_before": sum(n for _, n in before),
        "focus_after": sum(n for _, n in after),
        "focus_delta": sum(n for _, n in after) - sum(n for _, n in before),
        "worst_before": max((n for _, n in before), default=0),
        "worst_after": max((n for _, n in after), default=0),
        "new_cycles": len(new),
    }


def git_loc(project_path, branch, base):
    try:
        out = subprocess.run(
            ["git", "-C", project_path, "diff", "--shortstat", f"{base}..{branch}"],
            capture_output=True, text=True, check=True,
        ).stdout
    except subprocess.CalledProcessError:
        return {}
    fc = re.search(r"(\d+) files? changed", out)
    add = re.search(r"(\d+) insertions?", out)
    rem = re.search(r"(\d+) deletions?", out)
    return {
        "files_changed": int(fc.group(1)) if fc else "",
        "loc_added": int(add.group(1)) if add else 0,
        "loc_removed": int(rem.group(1)) if rem else 0,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir", help="the <model>-<focus>-<n> run folder")
    ap.add_argument("--focus")
    ap.add_argument("--project")
    ap.add_argument("--project-path", help="external PROJECT repo, for loc/files")
    ap.add_argument("--base-branch", default="main")
    ap.add_argument("--branch", help="PROJECT branch (default: run name)")
    ap.add_argument("--in-price", type=float, default=5.0, help="USD per MTok input")
    ap.add_argument("--out-price", type=float, default=25.0, help="USD per MTok output")
    ap.add_argument("--quality", help="quality_1_5 (judged)")
    ap.add_argument("--clarity", help="clarity_1_5 (judged)")
    ap.add_argument("--collateral", help="collateral_delta (non-FOCUS principle Δ)")
    ap.add_argument("--verdict")
    ap.add_argument("--notes")
    ap.add_argument("--csv", help="metrics.csv path (default: <prompt-eval>/metrics.csv)")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    run_dir = os.path.abspath(args.run_dir.rstrip("/"))
    run = os.path.basename(run_dir)
    build = os.path.basename(os.path.dirname(run_dir))  # <ts>_<sha>
    ts, _, sha = build.rpartition("_")
    parts = run.split("-")
    model = parts[0] if parts else ""
    iteration = parts[-1] if len(parts) > 1 else ""
    derived_focus = "-".join(parts[1:-1]) if len(parts) > 2 else ""
    focus = args.focus or derived_focus

    row = {c: "" for c in COLUMNS}
    row.update(ts=ts, cr_sha=sha, project=args.project or "", focus=focus,
               model=model, iter=iteration, run=run)

    chat = os.path.join(run_dir, "chat.jsonl")
    if not os.path.exists(chat):
        sys.exit(f"no chat.jsonl in {run_dir}")
    row.update(from_transcript(chat, focus))
    row.update(from_snapshots(run_dir))

    if args.project_path:
        row.update(git_loc(args.project_path, args.branch or run, args.base_branch))

    if row.get("input_tokens") != "" and row.get("output_tokens") != "":
        row["cost_usd"] = round(
            row["input_tokens"] * args.in_price / 1e6
            + row["output_tokens"] * args.out_price / 1e6, 2
        )

    for col, val in (("quality_1_5", args.quality), ("clarity_1_5", args.clarity),
                     ("collateral_delta", args.collateral), ("verdict", args.verdict),
                     ("notes", args.notes)):
        if val is not None:
            row[col] = val

    csv_path = args.csv or os.path.join(os.path.dirname(os.path.dirname(run_dir)), "metrics.csv")

    if args.dry_run:
        print(f"# would append to {csv_path}")
        for c in COLUMNS:
            print(f"{c:22} {row[c]}")
        return 0

    new_file = not os.path.exists(csv_path)
    with open(csv_path, "a", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=COLUMNS)
        if new_file:
            w.writeheader()
        w.writerow(row)
    print(f"appended {run} to {csv_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

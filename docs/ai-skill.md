# code-ranker — AI agent skill

A short playbook for an AI assistant driving `code-ranker`. Full flag reference:
[CLI.md](code-ranker-cli/CLI.md); metrics and rules: [ERRORS.md](code-ranker-cli/ERRORS.md);
copy-paste recipes (one command per scenario): [USE-CASES.md](code-ranker-cli/USE-CASES.md).

## Install

If a `code-ranker` command errors with "command not found" (the binary isn't
installed) and you are working in a Rust project, install it with cargo:

```sh
cargo install code-ranker
```

## Two commands

- **`check`** — a gate. Exits non-zero on a violation, writes no files.
- **`report`** — produces artifacts: a JSON snapshot, an HTML viewer, and the
  advisory **`scorecard`** (console triage) / **`prompt`** (LLM prompt). Always
  exits `0`.

`[input]` is polymorphic: a directory is analyzed; a `.json` snapshot is read
back with no re-analysis. Keep old `.code-ranker/` snapshots — they are baselines.

## The two metrics that matter

Focus on these; treat everything else as secondary.

- **ADP** — dependency cycles. A module graph should be acyclic.
- **HK** — Henry-Kafura coupling, `HK = sloc × (fan_in × fan_out)²`: a large
  module on a busy crossroads of incoming/outgoing dependencies. Full
  diagnose-and-split workflow (measure one file, list its fan_in/fan_out, find
  the mixed scenarios, split, verify with a before/after diff report):
  [HK principle](https://github.com/ffedoroff/code-ranker/blob/main/principles/rust/HK.md).

**Strategy:** fix one thing at a time, worst-first. Cycles (ADP) are structural —
clear them first; then coupling (HK). Focus an axis with `--metric` and inspect
the worst tier with `--severity warning`.

## The fix loop

One thing per pass, worst-first.

```sh
# 1. Find what to fix. The gate verdict:
code-ranker check .
#    …or focus one axis in the triage (cycle = ADP, then hk, sloc, cognitive, …):
code-ranker report . --output.scorecard --metric cycle --top 1

# 2. Get the actionable fix-prompt for the single worst module (auto-targeted):
code-ranker report . --output.prompt.path=stdout --top 1

# 3. Review it; propose the fix to the user and get agreement.

# 4. Snapshot the BEFORE state:
code-ranker report . --output.json.path=.code-ranker/before.json

# 5. Apply the fix.

# 6. Run all tests.

# 7. Render the before/after report and open it:
code-ranker report . --baseline .code-ranker/before.json \
  --output.json.path=.code-ranker/after.json \
  --output.html.path=.code-ranker/after.html
open .code-ranker/after.html          # macOS; xdg-open on Linux

# 8. Repeat until clean.
```

Notes:

- `--output.prompt` is **auto-targeted** at the single worst module and **requires
  `--top 1`**. There is no manual principle selection — it always describes the worst.
- To focus a specific axis, narrow the triage with `--output.scorecard --metric <m>`:
  `cycle`, `hk`, `sloc`, `cognitive`, `cyclomatic`, `fan_in`, `fan_out`, `items`.
- For `--metric cycle`, `--top 1` shows **one whole cycle** — the biggest `chain`
  (else the biggest `mutual`) — with **all** its modules listed, so you can fix the
  loop as a unit.

## Agent gate loop

One command gates **and** emits the fix-prompt — no `||`, no second command. With
`--output-format prompt`, `check` exits non-zero on a violation and prints the AI
fix-prompt for **exactly the violations that failed the gate**:

```sh
code-ranker check . --output-format prompt
# exit 0  → no violations, the agent stops
# exit≠0  → stdout is the fix-prompt; hand it to the agent, it fixes, you re-run
```

The prompt is built from the gate's own violations (the thresholds in `code-ranker.toml`),
so it always describes what actually failed — no separate principle selection, no
threshold mismatch.

## Cheat sheet

```sh
code-ranker report . --output.scorecard                       # triage: all principles
code-ranker report . --output.scorecard --metric hk --top 1   # focus one axis
code-ranker report . --output.prompt.path=stdout --top 1      # LLM fix-prompt for the worst module
code-ranker check  . --baseline base.json --output-format json   # CI regression verdict
```

## Gotchas

- Analysis is offline and fast. The Rust plugin needs a warm cargo cache
  (`cargo metadata --offline`); if it errors, run `cargo fetch` first.
- `--metric` / `--severity` / `--top` are **report-only** — they require a
  `--output.prompt` or `--output.scorecard`, else the run errors.
- `--output.prompt` **requires `--top 1`** — it is auto-targeted at the single worst
  module. For a broader view use `--output.scorecard`.
- `--top N` is a reporting limit (`--top 1` = the single worst); use it instead
  of a non-existent `--index`.
- Don't delete `.code-ranker/` snapshots — they are your baselines for diffs.

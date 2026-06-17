# code-ranker — AI agent skill

A short playbook for an AI assistant driving `code-ranker`. Full flag reference:
[CLI.md](code-ranker-cli/CLI.md); metrics and rules: [ERRORS.md](code-ranker-cli/ERRORS.md).

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
  module on a busy crossroads of incoming/outgoing dependencies.

**Strategy:** fix them one at a time, worst-first, in `--severity warning`,
until no warnings remain — ADP first (cycles are structural), then HK.

## The fix loop

When the user says **"fix top 1 cycles" / "fix one ADP"**, run the loop below
with `--preset ADP`. When the user says **"fix HK"**, run the exact same loop
with `--preset HK`. One violation per pass.

```sh
# 1. Find the single worst warning
code-ranker report . --output.scorecard --preset ADP --severity warning --top 1

# 2. Review it; propose the fix to the user and get agreement.

# 3. Snapshot the BEFORE state
code-ranker report . --output.json.path=.code-ranker/before.json

# 4. Apply the fix.

# 5. Run all tests.

# 6. Re-check: the old #1 is gone, a new #1 surfaces (or none left)
code-ranker report . --output.scorecard --preset ADP --severity warning --top 1

# 7. Render the before/after report and open it
code-ranker report . --baseline .code-ranker/before.json \
  --output.json.path=.code-ranker/after.json \
  --output.html.path=.code-ranker/after.html
open .code-ranker/after.html          # macOS; xdg-open on Linux

# 8. Repeat until no warnings remain.
```

Notes on ADP (cycles):

- For ADP, `--top` counts **cycles**, not modules. `--top 1` prints **one whole
  cycle** — the biggest `chain` (else the biggest `mutual`) — with **all** its
  modules listed, so you see the entire loop and can fix it as a unit. **Do not
  use `--top 2`+** for ADP: it prints several separate cycles at once and
  obscures how each one connects.
- `--severity` is a no-op for ADP (every module in a cycle counts) — keep the
  flag only so the ADP and HK loops stay identical.

## Cheat sheet

```sh
code-ranker report . --output.scorecard --top 5          # triage: what to fix first
code-ranker report . --output.prompt.path=stdout         # LLM prompt for the worst principle
code-ranker check  . --baseline base.json --output-format json   # CI regression verdict
```

## Gotchas

- Analysis is offline and fast. The Rust plugin needs a warm cargo cache
  (`cargo metadata --offline`); if it errors, run `cargo fetch` first.
- `--preset` / `--severity` / `--top` are **report-only** — they require a
  `--output.prompt` or `--output.scorecard`, else the run errors.
- `--top N` is a reporting limit (`--top 1` = the single worst); use it instead
  of a non-existent `--index`.
- Don't delete `.code-ranker/` snapshots — they are your baselines for diffs.

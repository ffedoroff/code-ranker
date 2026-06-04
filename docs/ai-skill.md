# code-split — AI agent skill

A task-oriented playbook for an AI assistant driving `code-split`. Each scenario
starts from a **natural-language request** ("what the user says") and gives the
exact commands plus how to read the output and act on it. For the full flag
reference see [CLI.md](code-split-cli/CLI.md); for the metrics and rules see [ERRORS.md](code-split-cli/ERRORS.md).

## Mental model (read this first)

Two commands, split by what they emit:

- **`check`** — a **gate**. Evaluates the rules *you configure* (thresholds, cycle
  rules) and, with `--baseline`, regressions. Prints diagnostics and **exits
  non-zero** on a violation. Writes no files.
- **`report`** — produces **artifacts**: a JSON snapshot, an HTML viewer, and the
  two **advisory** refactoring-guidance formats — **`scorecard`** (a console
  triage) and **`prompt`** (an LLM prompt for one principle). Always exits `0`.

Facts an agent must keep straight:

- `[input]` is **polymorphic**: a directory is analyzed; a `.json`/`.html`
  snapshot is read back with no re-analysis (so you can "analyze once, consume
  many").
- `check` returns exit `1` for **both** a violation and a tool error. To tell
  them apart, use `--output-format json` and parse it.
- `scorecard` / `prompt` are **advisory** — they never change an exit code. The
  gate is `check`. They rank against the snapshot's built-in, language-calibrated
  thresholds (the **`warning`** ≈ top-10% and **`info`** ≈ top-50% tiers).
- Default artifact dir is `.code-split/`. **Keep old runs** — they are baselines;
  don't delete them when regenerating.
- Presets (refactoring principles): `ADP` (cycles), `SRP`, `CPX`, `OCP`, `LSP`,
  `ISP`, `DIP`, `DRY`, `KISS`, `LoD`, `MISU`, `CoI`, `YAGNI`.

Cheat sheet:

```sh
code-split report . --output.scorecard               # triage: what to fix first
code-split report . --output.prompt.path=stdout      # LLM prompt for the worst principle
code-split check  . --top 1                           # the single worst gated violation
code-split check  . --baseline base.json --output-format json   # CI regression verdict
code-split report . --baseline base.json --output.html.path=diff.html   # before/after diff
```

---

## 1. "Check my project — show the top 5 problems, briefly"

Triage overview — which principles and modules are worst, ranked by the calibrated
tiers. This is the right first move for any "check my project" request.

```sh
code-split report . --output.scorecard --top 5
```

Read the output:

- The **per-principle table** — each principle with its `⚠` (warning) / `ⓘ` (info)
  module counts and the worst offending module. A non-zero `⚠` is the strongest
  signal.
- The **WORST MODULES** list — the files breaching the most thresholds, each with
  its worst metric (e.g. `hk 4.2M`) and the other breaches (`+sloc, fan_out, cycle`).

Then summarize for the user: name the top principle, the 5 worst modules, and one
line per module on *why* it is flagged (use the metric shown — high `hk` =
coupling hotspot, `cycle` = in a dependency cycle, `sloc` = oversized, `fan_out` =
too many dependencies). To hand the user one focused fix, add the single worst
**gated** violation with its full rationale:

```sh
code-split check . --top 1          # human block: rule id, issue, why, fix
```

## 2. "Fix the dependency cycles — report before/after and open the diff"

The before/after workflow. Snapshot the current state, fix, then render a diff
viewer so the user sees coupling change with a verdict.

```sh
# 1. Capture the BEFORE state (keep it as the baseline)
code-split report . --output.json.path=.code-split/before-cycles.json

# 2. Get a ready-to-apply prompt for the cycle principle (ADP) and act on it
code-split report . --preset ADP --output.prompt.path=stdout
#    → follow the prompt: read the principle, refactor the listed modules to break the cycle

# 3. Render the AFTER↔BEFORE diff and open it
code-split report . --baseline .code-split/before-cycles.json --output.html.path=.code-split/cycles-diff.html
open .code-split/cycles-diff.html     # macOS; use xdg-open on Linux
```

Confirm the fix landed with a machine verdict (should read `improved`):

```sh
code-split check . --baseline .code-split/before-cycles.json --cycle-rule mutual=on --cycle-rule chain=on --output-format json
```

The diff viewer colours nodes added/removed/affected and highlights cycle members
in red on each side; a cycle that's gone in the current snapshot stops being red
on the Current side.

## 3. "Give me an AI prompt for what to refactor first"

The prompt for the worst-violating principle (auto-picked when `--preset` is
omitted). It is the same Markdown the HTML viewer's Prompt Generator produces:
intent, summary, a link to the full principle doc, a task checklist, the ranked
offending modules, and their connections.

```sh
code-split report . --output.prompt.path=stdout            # to the terminal / pipe
code-split report . --output.prompt.path=.code-split/fix.md  # to a file
```

Acting on it as an agent: the prompt instructs you to download & read the linked
principle, audit the listed modules, summarize findings, and save a report to
`.code-split/<YYYYMMDD-HHMMSS>-<PRESET>.md`. Do exactly that.

## 4. "Check the project against one principle (SRP / DIP / …)"

Target one principle. `--preset` fixes its own ranking metric (SRP → SLOC,
DIP → fan-out, OCP → cyclomatic, …), so you don't choose a metric.

```sh
code-split report . --preset SRP --output.prompt.path=stdout       # prompt for SRP
code-split report . --preset SRP --output.scorecard                # SRP-only triage table
```

Use `--top N` to widen/narrow the module set, or `--severity warning` to keep only
the hardest breaches.

## 5. "What to fix right now — just one thing"

The single worst item, two framings:

```sh
code-split check  . --top 1                              # the worst GATED violation (+ why/fix)
code-split report . --top 1 --output.prompt.path=stdout  # an LLM prompt for the worst single module
```

`check --top 1` is a reporting limit only — the exit code still reflects *all*
violations, so the build still fails if other violations exist.

## 6. "Gate it in CI — fail only on regressions"

Relative gate: fail only on violations the baseline didn't already have.

```sh
# On the base branch (once), store the snapshot as a CI artifact:
code-split report . --output.json.path=.code-split/main.json

# On the PR, gate against it and emit a machine verdict:
code-split check . --baseline .code-split/main.json --output-format json
```

Parse the JSON: `{ "verdict": "improved|degraded|neutral", "violations": [ … ] }`.
`degraded` (new violations) → exit non-zero → fail the PR. Each violation carries
`rule`, `group`, `graph`, `location`, `message`, `weight`. For PR annotations use
`--output-format github`; for code-scanning use `sarif`.

To distinguish "violation" (exit 1 with a JSON verdict/violations) from a "tool
error" (exit 1 with a structured error on stderr), always parse the JSON.

## 7. "Pin today's numbers as a budget (a baseline without fixing the backlog)"

Pin today's measured values so the build passes now and fails on regression —
without fixing the existing backlog first.

```sh
code-split check . --suggest-config
```

It prints paste-ready `code-split.toml` blocks: `[rules.cycles]` counts per kind
and `[rules.thresholds.file]` per-metric maxima. Copy them into `code-split.toml`,
commit, and the gate now forbids getting worse. Tighten a single rule ad hoc:

```sh
code-split check . --threshold file.cognitive=25 --threshold file.loc=300 --cycle-rule chain=7
```

## 8. "Find and prove a cycle is gone"

```sh
code-split check . --cycle-rule mutual=on --cycle-rule chain=on        # before: fails, lists the cycles
# …refactor…
code-split check . --cycle-rule mutual=on --cycle-rule chain=on        # after: passes (exit 0)
```

`--cycle-rule KIND=N` lets you keep a budget: `chain=7` allows today's 7 chain
cycles and fails on the 8th. Cycle kinds: `mutual` (A↔B), `chain` (longer SCC),
`test-embed` (off by default — a `#[cfg(test)]` back-edge).

## 9. "Produce a report to share with the team / attach to a PR"

A single self-contained HTML file (graph + metrics table + Prompt Generator, all
assets inlined — opens from `file://`, no server, no network).

```sh
code-split report . --output.html.path=docs/coupling.html
```

Add `--baseline <snap>` to make it a diff viewer with a verdict (named
`…-diff.html`). The JSON snapshot (`--output.json.path`) is the reusable,
machine-readable companion.

## 10. "Compare two branches / two snapshots without re-analyzing"

Because `[input]` is polymorphic, a snapshot stands in for the code — compare two
saved snapshots directly.

```sh
# Capture each side once
git checkout main;        code-split report . --output.json.path=.code-split/main.json
git checkout feature;     code-split report . --output.json.path=.code-split/feature.json

# Diff them without re-analyzing anything
code-split report .code-split/feature.json --baseline .code-split/main.json --output.html.path=.code-split/diff.html
code-split check  .code-split/feature.json --baseline .code-split/main.json --output-format json   # verdict
```

`{git-hash}` / `{ts}` placeholders in output names come from each snapshot's
embedded metadata (the original analysis), not the current clock.

---

## Gotchas for an agent

- **Analysis is offline and fast** (seconds). The Rust plugin needs a warm cargo
  cache (`cargo metadata --offline`); if it errors, run `cargo fetch` first.
- **Plugin auto-detection**: a directory with both `package.json` and
  `tsconfig.json` is ambiguous — pass `--plugin javascript` or `--plugin typescript`.
- **`--index` does not exist** — use `--top N` (`--top 1` = the single worst module).
- **Recommendation flags are report-only**: `--preset` / `--severity` / `--top`
  require a `--output.prompt` or `--output.scorecard`; otherwise the run errors.
- **Cycle-based principles (ADP)** have no numeric threshold — every module in a
  cycle counts, and `--severity` is ignored for them.
- **Don't delete `.code-split/`** snapshots — they are your baselines for diffs and
  regression gates.

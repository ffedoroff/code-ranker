# `code-ranker` CLI reference

Pluggable multi-language structural analysis platform.

```
code-ranker <command> [input] [options]
```

`code-ranker` is command-driven: running it with no command prints help — every action
goes through an explicit subcommand, there is no default action. Run
`code-ranker <command> --help` for per-command flags, `code-ranker --version` for the version.

Looking for a copy-paste recipe? See [USE-CASES.md](USE-CASES.md) — one scenario, one
exact command per entry (triage, CI gates, focused checks, baselines, AI prompts, …).

> **Offline & private.** code-ranker always runs entirely on your machine. It makes **no
> network calls**, sends **no telemetry or analytics**, and **never uploads your code or
> analysis results** anywhere. Generated HTML reports are self-contained — no CDN, no
> external requests, no tracking.

## Commands

| Command | What it produces |
|---|---|
| [`check`](#check) | A **verdict**: evaluates thresholds, cycle rules, and (with `--baseline`) regressions, prints diagnostics, and **exits non-zero** on violation. Writes no files. |
| [`report`](#report) | **Artifacts**: an HTML viewer and/or a JSON snapshot. With `--baseline`, the HTML becomes a diff with a verdict. Can also emit a console **scorecard** triage and an AI **prompt** (see [Recommendations](#recommendations-scorecard--prompt)). Always exits `0`. |

There are exactly two commands, split by *what they emit*: `check` produces an exit
code (a CI gate), `report` produces files (a snapshot and a viewer). Both take the same
input and share the same vocabulary below.

## Global options

`code-ranker` takes no global flags of its own beyond the clap built-ins:

| Flag | Meaning |
|---|---|
| `-h, --help` | Print help — top-level, or per-command with `code-ranker <cmd> --help`. |
| `-V, --version` | Print the version. |

Progress and timing lines are written to **stderr**, each stamped `[HH:MM:SS.mmm]`;
diagnostics and machine output go to **stdout** or files, so the two streams never mix.
A run opens with a `▶ <command>` startup line and the resolved `config:` path, logs
every external tool it shells out to with its duration to millisecond precision
(`↳ cargo metadata --offline — 28.500s`, `↳ git status --porcelain — 0.017s`,
`rustc …`), and closes with a `✓ <command> — <time>` line. The sub-command lines make
the cost of a cold cargo cache visible at a glance. All other flags are per-command and
must follow the command name.

## Input: code or snapshot

Both commands take a single positional `[input]` (default `.`). It is **polymorphic** —
its kind decides whether analysis runs:

| `[input]` | Behaviour |
|---|---|
| A **directory** (source tree) | **Analyze** it: run the plugin, build the graph, compute metrics. |
| A **`.json` snapshot** or **`.html` report** | **Read** the embedded snapshot — no analysis, no source tree or toolchain required. |

So `check .` analyzes the current directory in memory and never writes a file, while
`check snapshot.json` evaluates a snapshot produced earlier. Analysis is a built-in
capability of both commands; a JSON snapshot is written only when you explicitly ask for
one.

A JSON snapshot is an **optional artifact**, useful when you want to:

- keep a **baseline** to compare future runs against (`--baseline`);
- **analyze once, consume many** — produce a snapshot, then run cheap `check` / `report`
  passes over it without re-analyzing (handy for large repos and for CI steps that run
  without a toolchain).

```sh
# fast path — each command analyzes the code itself (analysis is seconds)
code-ranker check .
code-ranker report . --output.html.path=report.html

# analyze-once — one analysis, then cheap consumers over the snapshot
code-ranker report . --output.json.path=snap.json --output.html.path=report.html
code-ranker check  snap.json --threshold file.loc=800
code-ranker check  snap.json --baseline main.json
```

## Common analysis options

`--plugin` and `--ignore` govern analysis itself and apply **only when `[input]` is a
directory** — they are rejected with a snapshot input. `--config` is always accepted:
its rule and output keys apply to snapshots too, while analysis-only keys (e.g. `plugin`)
are ignored when reading one.

| Flag | Meaning |
|---|---|
| `--plugin <name\|auto>` | Plugin to use: `rust`, `python`, or `javascript` (covers TypeScript). `auto` (default) resolves the language automatically — see [Plugin resolution](#plugin-resolution). |
| `--config <PATH \| KEY=VALUE>` | Repeatable. Load config from a file path, **or** override one setting inline (`KEY=VALUE`). Multiple files layer in command-line order (**last wins**) over the built-in defaults; inline `KEY=VALUE` applies after all files; passing any file disables auto-discovery of `code-ranker.toml`. See [Config](#config). |
| `--ignore <glob>` | Repeatable. Glob to exclude paths from analysis. Merged with config-file globs. |
| `--git.<field> <VALUE>` | Override one of the snapshot's git metadata fields instead of reading it from `git`. See [Git metadata overrides](#git-metadata-overrides). |

### Git metadata overrides

Every snapshot records a small `git` block — `branch`, `commit`, `dirty_files`, and
the remote `origin` URL — read by shelling out to `git` in the analyzed directory.
That raw view is correct on a developer's machine but **wrong in CI**, where the
environment mangles it:

- a **detached checkout** makes `branch` come out as the literal `HEAD`;
- the untracked files a job writes *before* the analysis (the snapshot JSON, a
  fetched baseline, build outputs) inflate `dirty_files`;
- the clone uses a token-bearing URL, so `origin` is not the clean project URL.

Four flags let you inject clean values, mapped from your CI's variables:

| Flag | Overrides | Typical CI source (GitLab) |
|---|---|---|
| `--git.branch <NAME>` | `git.branch` | `$CI_MERGE_REQUEST_SOURCE_BRANCH_NAME` / `$CI_COMMIT_REF_NAME` |
| `--git.commit <HASH>` | `git.commit` | `$CI_COMMIT_SHA` |
| `--git.dirty-files <N>` | `git.dirty_files` | `0` (CI checkouts are clean before the job writes files) |
| `--git.origin <URL>` | `git.origin` | `$CI_PROJECT_URL` |

The merge is **per field**: a flag wins for its field, and any field left unset is
read from `git` as before. When `--git.branch`, `--git.commit`, and
`--git.dirty-files` are **all** supplied, `git` is **never invoked** — the fast path
that also works in a checkout with no `.git` at all (`--git.origin` is optional and
never gates this). The flags apply only when `[input]` is a directory (a snapshot
already carries its recorded git block).

```sh
# CI: inject clean values mapped from GitLab variables (git is never shelled out)
code-ranker report . \
  --git.branch="${CI_MERGE_REQUEST_SOURCE_BRANCH_NAME:-$CI_COMMIT_REF_NAME}" \
  --git.commit="$CI_COMMIT_SHA" \
  --git.dirty-files=0 \
  --git.origin="$CI_PROJECT_URL" \
  --output.json.path="code-ranker-${CI_COMMIT_SHORT_SHA}.json"

# fix just the detached-HEAD branch; commit/dirty/origin still come from git
code-ranker report . --git.branch="$CI_COMMIT_REF_NAME"
```

## `check`

The linter. Evaluates cycle rules, thresholds, and — with `--baseline` — regressions,
prints diagnostics, and **exits non-zero** when any violation is found. Writes no files.

```
code-ranker check [input] [options]
```

| Flag | Meaning |
|---|---|
| `--threshold <file.METRIC=N>` | Hard limit on a per-file metric — a breach fails the check. Scope is always `file` (a single file). METRIC: any per-file metric the engine emits (`loc`, `sloc`, `cyclomatic`, `cognitive`, `mi`, `volume`, `bugs`, `hk`, `fan_in`, `fan_out`, …); an unknown name errors. Repeatable. See [ERRORS.md](ERRORS.md#threshold-scopes). |
| `--cycle-rule <KIND=on\|off\|N>` | Configure a cycle check. KIND: `mutual`, `chain`. Value: `on` (any cycle fails), `off` (ignored), or `N` (allow up to N cycles of that kind — e.g. `chain=7` forbids an 8th). Defaults: `mutual`/`chain` on. |
| `--baseline <snapshot>` | Compare `[input]` (current) against this baseline snapshot (`.json` or `.html`) and switch to a **relative gate**: fail only on *new* violations vs the baseline; pre-existing ones are tolerated. See [`--baseline`](#--baseline-comparison). |
| `--focus <path>` | Restrict the gate to these files/folders. The whole project is still analyzed (the dependency graph needs it), but a violation outside the focused paths is dropped — neither reported nor counted toward the exit code. A folder matches everything beneath it. Repeatable. See [`--focus`](#--focus-scoping). |
| `--output-format <fmt>` | Diagnostics format: `human` (default), `json`, `github`, `sarif`, `codequality`, `prompt`. Use `github` for GitHub PR annotations, `sarif` for GitHub code scanning / GitLab ≥18.11, `codequality` for the GitLab Code Quality MR widget, `json` for generic tooling, `prompt` for a Markdown AI fix-prompt built from the gate's own violations — one command both gates and (on failure) prints the prompt, tied exactly to what failed. |
| `--top <N>` | Report only the `N` worst violations (ranked worst-first) and suppress the rest. A reporting limit only — it does **not** change the exit code. Default: all. |
| `--exit-zero` | Return exit code 0 even when violations exist. Useful in non-blocking CI checks. |
| `--suggest-config` | After the findings, also print the project's current values as a ready-to-paste `code-ranker.toml` baseline (cycle counts + per-file thresholds). Off by default; `human` output only. |

Every rule is binary: a cycle check or threshold is either **enabled** (a violation is
reported and fails the check) or **disabled** (not checked). There is no warning tier —
`check` either passes or fails. `--exit-zero` reports violations but keeps the exit code 0.

`--top N` keeps only the N worst violations, ranked by breach magnitude — threshold
breaches by how far they exceed the limit (largest overage first), cycles by size
(largest SCC first). It is a **reporting limit only**: the exit code is unchanged, so
`check` still fails when an unshown violation exists. Use `--top 1` to surface just the
single worst thing to fix (handy for handing one focused fix to a human or an AI agent).

### `--focus` scoping

A single file can't be linted in isolation — the Rust plugin needs the whole crate
graph to compute coupling and cycles. `--focus` bridges that gap: the **whole project
is analyzed**, but only violations whose location falls under one of the focused paths
are reported, and **only those count toward the exit code**. Unlike `--top` (a display
limit), `--focus` scopes the gate itself — `check` passes when the focused paths are
clean, even if the rest of the project has violations. A focus entry matches a file
exactly or, treated as a folder, anything beneath it; a leading `./` and a trailing `/`
are ignored. Locationless violations (e.g. a cycle whose breaking edge can't be placed)
can't be attributed to a path and are dropped under `--focus`. Combine with `--top` to
rank within the focused set. Repeatable.

```sh
# gate only the file you're refactoring — the rest of the repo can't fail this run
code-ranker check . --focus crates/code-ranker-plugin-api/src/plugin.rs

# focus a whole folder (matches everything beneath it)
code-ranker check . --focus crates/code-ranker-cli/src/
```

```sh
# lint the current project, fail the build on any violation
code-ranker check

# Python project: per-file budgets — cap any single file
code-ranker check ./api --plugin python \
  --threshold file.cognitive=25 --threshold file.loc=300

# CI gate with machine-readable annotations; allow up to 7 chain cycles
code-ranker check --cycle-rule chain=7 --output-format github

# regression gate: fail if the current tree got worse than the baseline
code-ranker check . --baseline .code-ranker/main.json

# useful for AI agents: surface only the single worst violation to fix
code-ranker check --top 1
```

### Diagnostics output

Every finding is identified by its dotted **rule id** — the same string used as
the config key and CLI flag — and tagged with a concern **group**: `CYC`
(dependency cycles), `CPX` (complexity), `CPL` (coupling), `SIZ` (size). Threshold
rules are `threshold.file.<metric>` — per single file. The full reference — what each rule flags,
why it matters, and how to fix it — lives in [ERRORS.md](ERRORS.md).

In the default `human` format each violation is a self-contained block, detailed
enough to paste straight into an AI assistant as a complete prompt:

```text
threshold.file.cognitive  ·  CPX  ·  files graph
  where  {target}/src/handlers.rs
  issue  cognitive complexity 67 exceeds limit 25 (2.7× over budget)
  why    Cognitive complexity weights nested and interrupted control flow by how hard a human finds it to follow…
  fix    Extract nested blocks into named helpers, use early returns to cut nesting depth…
  tune   set with --threshold file.cognitive=N   ·   rules.thresholds.file.cognitive in code-ranker.toml
  ref    https://github.com/ffedoroff/code-ranker/blob/main/docs/code-ranker-cli/ERRORS.md#group-cpx
```

The rule id and group are present in every `--output-format`: the block header
(`human`), `"rule"` + `"group"` fields (`json`), the annotation title (`github`),
and `ruleId` plus a fired-rules `tool.driver.rules` catalog (`sarif`). With
`--baseline`, the verdict (`improved` / `degraded` / `neutral`) and any regressions
are included in the diagnostics too.

The `github` and `sarif` formats also pin each finding to a **file and line** so
it lands inline in a PR: `github` adds `file=…,line=N` to the `::error` command,
`sarif` a `physicalLocation`. A cycle points at the line of the import/`use` that
closes it (the breaking edge's `line`); a whole-file metric breach has no single
line, so it pins to line 1. Run `check` from the repo root so the path resolves
repo-relative.

Each `sarif` result additionally carries a `partialFingerprints` entry
(`codeRankerRuleLocation/v1` = `<rule>:<location>`) — the same `(rule, location)`
signature `check --baseline` matches on internally, with the line number
deliberately omitted. A SARIF consumer (GitHub code scanning, IDE viewers) uses it
to recognise the *same* finding across runs even when surrounding edits shift it,
instead of reopening it as new.

### Current-values config block (`--suggest-config`)

With `--suggest-config`, the `human` output prints — after the findings — the
project's current measured values as ready-to-paste `code-ranker.toml` blocks: the
`[rules.cycles]` counts per kind, plus the per-file thresholds (the worst
single file max). Numbers use `_` separators.
Copy a block to pin today's numbers as a baseline that passes now and fails on
regression. Off by default; the machine formats (`json`/`github`/`sarif`) omit it.

```sh
code-ranker check --suggest-config
```

## `report`

Analyzes (or reads) `[input]` and writes artifacts. Without `--baseline` the HTML is a
single-snapshot viewer; with `--baseline` it becomes a diff with a verdict. `report`
always exits `0` — it produces artifacts, it does not gate.

```
code-ranker report [input] [options]
```

| Flag | Default | Meaning |
|---|---|---|
| `--output.<fmt>.path <path>` | `json` + `html` in `.code-ranker/` | Which artifacts to emit and where. `<fmt>` is `json`, `html`, `prompt`, or `scorecard`. Repeatable, one per format. See [Output paths](#output-paths). |
| `--baseline <snapshot>` | — | Baseline snapshot (`.json` or `.html`). Turns the HTML into a diff (baseline vs current) with a verdict, and names it `…-diff.html`. See [`--baseline`](#--baseline-comparison). |
| `--metric <NAME>` | all principles | Narrow the `scorecard` to one ranking axis: `hk`, `cycle`, `sloc`, `cognitive`, `cyclomatic`, `fan_in`, `fan_out`, `items`. Without it the scorecard spans every principle. `scorecard` only. See [Recommendations](#recommendations-scorecard--prompt). |
| `--severity <tier>` | all tiers | Threshold tier for the `scorecard`: `info`, `warning`, or `auto`. Repeatable to show several tiers. |
| `--top <N>` | 15 (scorecard) | `scorecard`: how many rows; `--top 1` = the single worst module. With `--metric cycle`, `--top 1` prints one entire cycle (biggest `chain` first) with **all** its members. `prompt`: **must be `--top 1`** — the prompt is auto-targeted at the single worst module. |
| `--export-full-config <PATH>` | — | Instead of analyzing, write the **full effective configuration** to `PATH` and exit. See [Inspecting the effective config](#inspecting-the-effective-config). |

`--metric`, `--severity`, and `--top` apply only when a `prompt` or `scorecard` format is
selected; passing them otherwise is an error. `--output.prompt` additionally **requires
`--top 1`** (it is auto-targeted at the single worst module); `--metric` / `--severity`
are `scorecard`-only.

### Inspecting the effective config

`--export-full-config <PATH>` dumps the configuration code-ranker would actually use —
no analysis runs — as one TOML document with two top-level sections:

- `[project]` — the merged project config: the built-in defaults (`config/defaults.toml`,
  baked into the binary) **deep-merged** with the discovered / `--config` file. Shows
  every effective `ignore` / `rules` / `output` / `levels` value, including the ones you
  did not set (inherited from the defaults).
- `[plugin]` — the active plugin's fully-merged language config (its inheritance chain
  `defaults.toml ⊕ [base] ⊕ <lang>.toml`): presets, calibrated thresholds, node/edge
  kinds, the metric-engine role tables, etc.

It honours `--plugin` and `--config`, so you can preview any combination:

```sh
# what `report` would use here, with my overrides folded in
code-ranker report . --config ci/strict.toml --export-full-config /tmp/full.toml

# the full Python plugin config (presets, thresholds, vocab)
code-ranker report . --plugin python --export-full-config /tmp/python.toml
```

It is a **diagnostic view** of every parameter you can override — because the two
sections use different schemas (and `presets` differs between the project and plugin
shapes), the file is not meant to be fed back as a single `--config`.

```sh
# default: snapshot + viewer in .code-ranker/
code-ranker report

# only the HTML viewer, to a fixed path
code-ranker report --output.html.path=report.html

# snapshot to stdout for a pipe, no HTML
code-ranker report --output.json.path=stdout

# render a diff viewer against a baseline (current = this run)
code-ranker report . --baseline .code-ranker/main.json --output.html.path=diff.html

# console triage overview — what to fix first
code-ranker report . --output.scorecard

# narrow the triage to one axis (coupling)
code-ranker report . --output.scorecard --metric hk --top 5

# AI fix-prompt for the single worst module (auto-targeted), to stdout
code-ranker report . --output.prompt.path=stdout --top 1
```

The HTML is **self-contained**: the snapshot data is embedded inline, so the single file
opens straight from disk (no server, no extra files). See [HTML viewer](#html-viewer).

## Output paths

`report` selects artifacts and their destinations through one flag family,
`--output.<fmt>.path`, where `<fmt>` is `json`, `html`, `sarif`, `codequality`,
`prompt`, or `scorecard`. The last two are the recommendation outputs — see
[Recommendations](#recommendations-scorecard--prompt) for their flags and defaults.

`sarif` and `codequality` write the **same documents** as the matching
`check --output-format` (the current rule violations, with stable per-finding
fingerprints), but as artifacts rather than to stdout — so a single `report` run
can emit the JSON snapshot, the HTML viewer, *and* a findings report for CI in one
pass. `sarif` (SARIF 2.1.0) feeds GitHub code scanning / GitLab ≥18.11; `codequality`
(CodeClimate JSON) feeds the GitLab Code Quality MR widget (GA, no flag). Like
`prompt` / `scorecard`, both are opt-in: never part of the default set, and a
`--baseline` here only diffs the HTML — it does not filter the findings.

**Which formats are written:**

- No `--output.*` flag → the default set: **both** `json` and `html`, with default
  names, into `.code-ranker/`. (`prompt` / `scorecard` are never in the default set —
  they are emitted only when explicitly named.)
- One or more `--output.<fmt>.path` given → **exactly** the listed formats, nothing else.

**The `.path` value:**

- A file path, relative to the cwd or absolute. The directory is part of the path.
- Supports [name template](#name-templates) placeholders (`{ts}`, `{git-hash}`, …),
  which are expanded before the file is written.
- The special value `stdout` (or `-`) writes that artifact to the stdout stream instead
  of a file — useful for piping the JSON snapshot in CI.

**Defaults** (when no `--output.*` is given):

```
.code-ranker/{ts}-{git-hash-3}.json
.code-ranker/{ts}-{git-hash-3}.html
```

With `--baseline`, the HTML default gains a `-diff` marker:
`.code-ranker/{ts}-{git-hash-3}-diff.html`. The JSON artifact is always the snapshot of
the current input (reusable as a future baseline), never a diff.

When selected, `sarif` defaults to `.code-ranker/{ts}-{git-hash-3}.sarif` and
`codequality` to `.code-ranker/{ts}-{git-hash-3}.codequality.json`.

The recommendation formats have their own per-format defaults: `scorecard` defaults to
**`stdout`** (it is a console overview), and `prompt` defaults to the file
`.code-ranker/{ts}-{git-hash-3}-{preset}.md`.

To pin destinations project-wide instead of passing flags every time, set them in
config:

```toml
[output.json]
path = "dist/{project-dir}-{ts}.json"

[output.html]
path = "dist/{project-dir}-{ts}.html"

[output.sarif]
path = "dist/{project-dir}-{ts}.sarif"

[output.codequality]
path = "dist/{project-dir}-{ts}.codequality.json"
```

### Name templates

`--output.<fmt>.path` values accept placeholders:

| Placeholder | Expands to | Example |
|---|---|---|
| `{project-dir}` | The analyzed directory's basename, lowercased, non-alphanumerics collapsed to `-`. | `user-provisioning` |
| `{ts}` | The run's `generated_at` as a local timestamp, `YYYYMMDD-HHMMSS`. One value per run, shared by every artifact. | `20260526-114144` |
| `{git-hash}` | The 12-char short commit hash (zeros if not a git repo). | `a3f9c21b4d5e` |
| `{git-hash-N}` | The first `N` chars of the commit hash. | `{git-hash-3}` → `a3f` |
| `{preset}` | The principle id of the auto-targeted prompt (`prompt` only). | `SRP` |

So the default `{ts}-{git-hash-3}.json` yields `20260526-114144-a3f.json`. When `[input]`
is a **snapshot**, `{git-hash}` / `{ts}` are read from the snapshot's embedded metadata —
the commit and time of the original analysis — not the current repo or clock.

The destination resolves as **`--output.<fmt>.path` flag › `[output.<fmt>] path` in
`code-ranker.toml` › built-in default**.

## Recommendations: `scorecard` & `prompt`

Two `report` output formats turn the snapshot's calibrated metric thresholds into
refactoring guidance:

- **`scorecard`** — a console triage overview answering *"what do I fix first?"*
- **`prompt`** — a ready-to-paste AI fix-prompt, **auto-targeted at the single worst
  module** (the same Markdown the HTML viewer's Prompt Generator produces).

Both rank modules with the same engine. The `scorecard` is steered by `--metric` (narrow
to one axis), `--severity` (which tier), and `--top` (how many rows). The `prompt` takes
no axis flag — it auto-targets the single worst module — and **requires `--top 1`**.

> **Advisory, not a gate.** Unlike [`check`](#check), these never fail the build and carry
> no exit code. `check` enforces the rules *you* configure; `scorecard` / `prompt` surface
> the worst hotspots against the snapshot's built-in, language-calibrated thresholds so you
> know where to start. Both also work from a snapshot input
> (`report snap.json --output.scorecard`) with no re-analysis.

### Severity tiers

Every ranking metric carries two calibrated thresholds in the snapshot — **`info`** (the
softer line; ~50 % of projects breach it) and **`warning`** (the harder line; ~10 %
breach). A module is *in a tier* when its value crosses that threshold. `--severity`
selects which tier drives the output:

| Value | Meaning |
|---|---|
| `warning` | only modules over the warning line |
| `info` | modules over the info line (a superset of `warning`) |
| `auto` | warning if any module breaches it, else info — the **`prompt` default** |

For `scorecard`, `--severity` is **repeatable** (`--severity warning --severity info`) to
show several tiers at once; with none given it shows all tiers.

Cycle-based principles (e.g. `ADP`) have **no numeric threshold** — every module in a
dependency cycle counts, ranked by HK, and `--severity` is ignored for them.

### Ranking axes (`--metric`)

The `scorecard` ranks modules by a metric. `--metric <NAME>` narrows it to one axis:
`hk` (Henry-Kafura coupling), `cycle` (dependency cycles — the ADP view), `sloc` (module
size), `cognitive` / `cyclomatic` (complexity), `fan_in` / `fan_out` (coupling direction),
`items` (interface size). An unknown name errors with the list of known metrics.

Without `--metric` the scorecard spans all principles (one row each). The principle
*catalog* still lives in the snapshot's `presets` (shared with the HTML viewer's Prompt
Generator and used for the prompt's prose) — but it is **no longer selected from the CLI**:
the `prompt` auto-targets the single worst module's principle, and the `scorecard` narrows
by metric. `cycle` has **no numeric threshold** — every module in a dependency cycle
counts, ranked by HK, and `--severity` is ignored for it.

### `scorecard` — triage overview

Defaults to **stdout**, so a bare `--output.scorecard` prints to the console. It shows a
per-principle table (warning / info counts + the worst module) followed by the worst
modules overall:

```sh
code-ranker report . --output.scorecard                     # all tiers, ~15 rows
code-ranker report . --output.scorecard --severity warning --top 20
code-ranker report . --output.scorecard.path=triage.txt     # to a file instead
code-ranker report . --output.scorecard --metric sloc       # narrow to one axis
```

```text
scorecard  (rust, 142 files)

PRESET  PRINCIPLE              ⚠  ⓘ   TOP MODULE
ADP     Acyclic Dependencies   2  2   a.rs ↔ b.rs
SRP     Single Responsibility  5 18   cli/main.rs (sloc 1832)
CPX     Reduce Complexity      3 11   cli/main.rs (cog 67)

WORST MODULES
 1 ⚠ cli/main.rs     hk 4.2M   +sloc, fan_out, cycle
 2 ⚠ snapshot.rs     sloc 1.8K +hk
 3 ⓘ plugin/rust.rs  fan_out 14

→ code-ranker report . --output.prompt.path=… --top 1
```

`--top N` caps the worst-modules list (default ~15); `--metric <NAME>` narrows the
scorecard to a single ranking axis.

### `prompt` — AI fix-prompt for the worst module

Defaults to the file `.code-ranker/{ts}-{git-hash-3}-{preset}.md` (use
`--output.prompt.path=stdout` to pipe it). It is **auto-targeted**: it emits the Markdown
fix-prompt for the **single worst module** — its principle's intent and summary, a link to
the full principle doc, a task checklist, the offending module annotated with its metric
value, and the relevant connection lists. The `{preset}` in the default filename is the
auto-selected principle id.

It **requires `--top 1`** (prompts are long, and the prompt always describes exactly one
module). There is no principle selection and no `--index`.

```sh
# fix-prompt for the single worst module, to stdout
code-ranker report . --output.prompt.path=stdout --top 1

# the same, saved to a file (name carries the auto-selected principle id)
code-ranker report . --output.prompt --top 1
```

## `--baseline` (comparison)

Both commands accept `--baseline <snapshot>` (a `.json` snapshot or a prior `.html`
report). It names the **reference point** to compare the current `[input]` against:

| Side | Source | UI label |
|---|---|---|
| **baseline** | `--baseline <snapshot>` | Baseline |
| **current** | the positional `[input]` (analyzed now, or a snapshot) | Current |

The comparison yields a top-level **verdict** — `improved` / `degraded` / `neutral` —
and a per-node state in the diff viewer: **added**, **removed**, **affected** (present in
both, but touching an added/removed edge), or **unchanged**.

- In `report`, `--baseline` turns the HTML into a diff viewer (baseline ↔ current) and
  embeds the verdict; the file is named `…-diff.html`.
- In `check`, `--baseline` switches the gate to **relative** mode: it fails only on
  *new* violations (those not already present in the baseline under the same rules), so
  pre-existing ones are tolerated. The verdict is `degraded` if there are new violations,
  `improved` if some were resolved and none added, else `neutral`. With `--output-format
  json` the verdict and the new violations are the machine output.

```sh
# human-facing diff
code-ranker report . --baseline .code-ranker/main.json --output.html.path=diff.html

# machine-readable verdict for CI
code-ranker check . --baseline .code-ranker/main.json --output-format json

# typical PR flow
code-ranker report . --output.json.path=.code-ranker/pr.json    # on the PR
git stash; git checkout main
code-ranker report . --output.json.path=.code-ranker/main.json   # on base
git checkout -; git stash pop
code-ranker report .code-ranker/pr.json --baseline .code-ranker/main.json --output.html.path=diff.html
```

Because the input is polymorphic, the last step compares **two existing snapshots**
without re-analyzing anything — the JSON/HTML snapshot stands in for the code.

## Plugin resolution

With `--plugin auto` (the default), the plugin is resolved in this order (applies only
when `[input]` is a directory):

1. **Explicit `--plugin <name>`** on the command line (any value other than `auto`) wins.
2. Otherwise the **`plugin` key in the config file** (`code-ranker.toml` /
   `Cargo.toml#metadata.code-ranker`), if set and not `auto`.
3. Otherwise **auto-detect by project markers** in the workspace root:
   - `Cargo.toml` → `rust`
   - `pyproject.toml` / `setup.py` / `setup.cfg` → `python`
   - `package.json` / `tsconfig.json` → `javascript`
4. If **more than one** marker matches, `code-ranker` errors and asks you to disambiguate
   with an explicit `--plugin`. If **no** marker matches, it errors with the same hint.

## HTML viewer

The HTML report is **self-contained**: the viewer app (Dagre graph layout, pan/zoom,
a sortable node table for the single Files view, and the prompt-generator panel whose
preset buttons are read from `snapshot.presets` — the 13 design principles ADP / SRP /
OCP / LSP / ISP / DIP / DRY / KISS / LoD / MISU / CoI / YAGNI / CPX) **and the snapshot
data** are all embedded in
the one file. External library nodes render in a distinct amber colour with dashed
edges. No network, no telemetry — `open` it straight from disk.

The data is embedded as `<script type="application/json">` tags (`cs-baseline` /
`cs-current`), which the viewer reads on load and which `--baseline` can extract back out —
so an `.html` report is interchangeable with a `.json` snapshot as a comparison input.

| Invocation | Output file | Mode | Embedded data |
|---|---|---|---|
| `report` | `{ts}-{git-hash-3}.html` | review (single snapshot) | this run (`cs-current`) |
| `report --baseline A` | `{ts}-{git-hash-3}-diff.html` | diff + verdict | `A` (`cs-baseline`) and this run (`cs-current`) |

In the header, each snapshot is a control showing its branch + commit. **Click a control
to switch which side the map and tables show** (baseline ↔ current); the **toggle** button
between the two controls — or the **`t`** key — does the same (diff mode only). Click a
control's **⚙ gear** to open its popup: the snapshot's details plus the actions that swap
snapshots from disk (each accepts a `.json` snapshot or an `.html` report) — **Replace**
that side, **Remove** it (offered while the other side remains), or **Set** the missing
side. The **Prompt Generator** button sits in the *Details* table header, to the right of
the node count.

In a diff, each node is coloured by its state — **added** (in current, not in baseline),
**removed** (in baseline, gone from current), **affected** (in both, unchanged itself but
touching an added/removed edge), or **unchanged** — while the top-level **verdict**
(`improved` / `degraded` / `neutral`) summarizes the whole diff.

Per-node modal: clicking a node opens a fullscreen card; for project files its
field list includes a **Source** link to the file on the project's git host
(GitLab/GitHub, built from `git.origin` at the snapshot's commit). Two modifier
gestures on the map skip the modal (the cursor changes while the modifier is
held): **Shift-click** a node toggles its selection just like its table
checkbox, and **⌘-click (macOS) / Ctrl-click (elsewhere)** opens that file's
source on the git host in a new tab.

## Config

Settings merge from several sources; **higher priority wins**:

1. CLI flags (`--threshold`, `--ignore`, `--output.<fmt>.path`, …)
2. `--config KEY=VALUE` inline overrides
3. `--config <file>` — repeatable; multiple files layer in command-line order
   (last wins), and any file disables the `code-ranker.toml` auto-discovery below
4. `code-ranker.toml` (cwd, then workspace root)
5. `Cargo.toml` metadata (`[workspace.metadata.code-ranker]`)
6. Built-in defaults

The inline form takes a dotted key into the config schema:

```sh
# tighten one rule in CI without editing code-ranker.toml
code-ranker check --config rules.thresholds.file.cognitive=25 \
                 --config rules.cycles.chain=7

# override an output destination inline
code-ranker report --config output.html.path=dist/report.html
```

`--ignore` globs are **merged** (union) with config globs; cycle rules and thresholds
**override** the file value. See [`docs/config.md`](config.md) for the full schema.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | `check` passed (no violations, or `--exit-zero`); `report` completed successfully. |
| 1 | Any failure — a `check` violation (cycle, threshold, or regression, without `--exit-zero`) **or** a runtime error (IO / plugin failure, ambiguous-or-undetected plugin under `auto`, malformed config, analysis flags passed with a snapshot input). |
| 2 | Argument-parsing error (unknown flag, missing required option, bad value) — emitted by the CLI parser before any work runs. |

`check` does **not** use a distinct exit code for "violation found" vs "tool
error": a violation is reported via the diagnostics on stdout, then the process
exits `1` — the same code as an error. Parse the diagnostics (`--output-format
json`/`sarif`) if you need to tell the two apart in CI.

## Plugins

Built-in (no install needed):

- `rust` — `cargo metadata` + `syn`. Builds the Rust module graph from `use`
  declarations, then collapses it to a **file graph**: every `.rs` file is one
  `file` node (inline `mod {}` modules fold into their file), and `use` / `pub use`
  edges are re-pointed to files. External crates become `external` library nodes
  (`ext:<name>`) at depth 1. Fast (seconds) — no rust-analyzer dependency.
- `python` — tree-sitter-python, native parser. Emits `file` nodes, file→file
  `uses` edges, and one `external` node per top-level package.
- `javascript` — tree-sitter-javascript / tree-sitter-typescript; one plugin handles
  `.js`, `.jsx`, `.ts`, `.tsx`. Same file + external model as Python.

All plugins are built into the `code-ranker` binary — there is nothing to install and no
external plugin processes. Adding a language means adding a built-in plugin to the binary.

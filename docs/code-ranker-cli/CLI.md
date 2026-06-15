# `code-ranker` CLI reference

Pluggable multi-language structural analysis platform.

```
code-ranker <command> [input] [options]
```

`code-ranker` is command-driven: running it with no command prints help — every action
goes through an explicit subcommand, there is no default action. Run
`code-ranker <command> --help` for per-command flags, `code-ranker --version` for the version.

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
| `--config <PATH \| KEY=VALUE>` | Repeatable. Load config from a file path, **or** override one setting inline (`KEY=VALUE`); inline values win. See [Config](#config). |
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
| `--threshold <file.METRIC=N>` | Hard limit on a per-file metric — a breach fails the check. Scope is always `file` (a single file). METRIC: `cyclomatic`, `cognitive`, `hk`, `fan_in`, `fan_out`, `loc`. Repeatable. See [ERRORS.md](ERRORS.md#threshold-scopes). |
| `--cycle-rule <KIND=on\|off\|N>` | Configure a cycle check. KIND: `mutual`, `chain`. Value: `on` (any cycle fails), `off` (ignored), or `N` (allow up to N cycles of that kind — e.g. `chain=7` forbids an 8th). Defaults: `mutual`/`chain` on. |
| `--baseline <snapshot>` | Compare `[input]` (current) against this baseline snapshot (`.json` or `.html`) and switch to a **relative gate**: fail only on *new* violations vs the baseline; pre-existing ones are tolerated. See [`--baseline`](#--baseline-comparison). |
| `--output-format <fmt>` | Diagnostics format: `human` (default), `json`, `github`, `sarif`. Use `github` for PR annotations, `sarif`/`json` for tooling. |
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
| `--preset <ID>` | worst-violating | Principle for the `prompt` / `scorecard` formats (`ADP`, `SRP`, `CPX`, …). When omitted, the principle with the most violations is chosen. See [Recommendations](#recommendations-scorecard--prompt). |
| `--severity <tier>` | `auto` (prompt) · all (scorecard) | Threshold tier: `info`, `warning`, or `auto`. Repeatable for `scorecard` to show several tiers; for `prompt` it sizes the default `--top`. |
| `--top <N>` | severity-tier size (prompt) · 15 (scorecard) | How many modules the `prompt` includes / rows the `scorecard` shows. `--top 1` = the single worst module. **For cycle presets (`ADP`) `--top` counts whole cycles, not modules** — `--top 1` (the default) prints one entire cycle (biggest `chain` first) with **all** its members; `--top 2`+ prints several separate cycles and is discouraged. |

`--preset`, `--severity`, and `--top` apply only when a `prompt` or `scorecard` format is
selected; passing them otherwise is an error.

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

# AI prompt for the worst-violating principle, to stdout
code-ranker report . --output.prompt.path=stdout

# AI prompt for the single worst SRP module
code-ranker report . --preset SRP --top 1 --output.prompt.path=stdout
```

The HTML is **self-contained**: the snapshot data is embedded inline, so the single file
opens straight from disk (no server, no extra files). See [HTML viewer](#html-viewer).

## Output paths

`report` selects artifacts and their destinations through one flag family,
`--output.<fmt>.path`, where `<fmt>` is `json`, `html`, `prompt`, or `scorecard`. The
last two are the recommendation outputs — see
[Recommendations](#recommendations-scorecard--prompt) for their flags and defaults.

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
```

### Name templates

`--output.<fmt>.path` values accept placeholders:

| Placeholder | Expands to | Example |
|---|---|---|
| `{project-dir}` | The analyzed directory's basename, lowercased, non-alphanumerics collapsed to `-`. | `user-provisioning` |
| `{ts}` | The run's `generated_at` as a local timestamp, `YYYYMMDD-HHMMSS`. One value per run, shared by every artifact. | `20260526-114144` |
| `{git-hash}` | The 12-char short commit hash (zeros if not a git repo). | `a3f9c21b4d5e` |
| `{git-hash-N}` | The first `N` chars of the commit hash. | `{git-hash-3}` → `a3f` |
| `{preset}` | The active `--preset` id (`prompt` / `scorecard` only). | `SRP` |

So the default `{ts}-{git-hash-3}.json` yields `20260526-114144-a3f.json`. When `[input]`
is a **snapshot**, `{git-hash}` / `{ts}` are read from the snapshot's embedded metadata —
the commit and time of the original analysis — not the current repo or clock.

The destination resolves as **`--output.<fmt>.path` flag › `[output.<fmt>] path` in
`code-ranker.toml` › built-in default**.

## Recommendations: `scorecard` & `prompt`

Two `report` output formats turn the snapshot's calibrated metric thresholds into
refactoring guidance:

- **`scorecard`** — a console triage overview answering *"what do I fix first?"*
- **`prompt`** — a ready-to-paste AI prompt for one principle, the same one the HTML
  viewer's Prompt Generator produces.

Both rank modules with the same engine and share three flags: `--preset`, `--severity`,
and `--top`.

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

### Presets (principles)

`--preset <ID>` selects the principle. The catalog comes from the snapshot's `presets`
(the same set as the HTML viewer's Prompt Generator): `ADP`, `SRP`, `CPX`, `OCP`, `LSP`,
`ISP`, `DIP`, `DRY`, `KISS`, `LoD`, `MISU`, `CoI`, `YAGNI`. Each preset fixes its own
ranking metric (ADP → cycles, SRP → SLOC, OCP → cyclomatic, …) and the connection lists
embedded in its prompt — so there is no separate metric/connection flag to set.

The **Rust** plugin adds four metric-lens presets that rank by a single
coupling/size metric rather than a design principle: `HK` (Henry-Kafura
coupling → `hk`), `SLOC` (module size → `sloc`), `FANIN` (afferent coupling →
`fan_in`), `FANOUT` (efferent coupling → `fan_out`). They appear in
`snapshot.presets` for Rust targets and as buttons in the viewer, and their
prompt docs live under `principles/rust/`. Because the catalog is data-driven,
any preset id in `snapshot.presets` is valid for `--preset`; an unknown id
errors with the list of known ids.

`--preset` is **optional**: when omitted, the principle with the most violations is chosen
— the one with the largest count of modules over `warning` (tie-break: over `info`), i.e.
the top row of the scorecard.

### `scorecard` — triage overview

Defaults to **stdout**, so a bare `--output.scorecard` prints to the console. It shows a
per-principle table (warning / info counts + the worst module) followed by the worst
modules overall:

```sh
code-ranker report . --output.scorecard                     # all tiers, ~15 rows
code-ranker report . --output.scorecard --severity warning --top 20
code-ranker report . --output.scorecard.path=triage.txt     # to a file instead
code-ranker report . --output.scorecard --preset SRP        # narrow to one principle
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

→ code-ranker report . --preset SRP --output.prompt.path=…
```

`--top N` caps the worst-modules list (default ~15); `--preset <ID>` narrows the whole
report to a single principle.

### `prompt` — AI prompt for one principle

Defaults to the file `.code-ranker/{ts}-{git-hash-3}-{preset}.md` (use
`--output.prompt.path=stdout` to pipe it). It emits the same Markdown the HTML viewer's
Prompt Generator produces: the principle's intent and summary, a link to the full
principle doc, a task checklist, the ranked list of offending modules (each annotated with
its metric value), and the relevant connection lists.

```sh
# worst-violating principle (preset auto-picked), to stdout
code-ranker report . --output.prompt.path=stdout

# a specific principle, default module count (the warning-tier size)
code-ranker report . --preset ADP --output.prompt.path=adp.md

# just the single worst SRP module
code-ranker report . --preset SRP --top 1 --output.prompt.path=stdout
```

`--top N` sets how many modules go into the prompt; without it the count is the size of
the active severity tier (matching the viewer's recommended count). For the cycle preset
**`ADP`, `--top N` counts whole cycles** instead: `--top 1` (the default) emits one entire
cycle — the biggest `chain`, else the biggest `mutual` — with **all** its members listed
(so the loop is visible as a unit); `--top 2`+ emits several separate cycles and is
discouraged. There is **no `--index`** — `--top 1` already yields the single worst
module/cycle, so passing `--index` is rejected with a hint to use `--top`.

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
preset buttons are read from `snapshot.presets` — ADP / SRP / OCP / LSP / ISP / DIP / DRY /
KISS / LoD / MISU / CoI / YAGNI / CPX, plus the Rust plugin's HK / SLOC / FANIN / FANOUT
metric lenses) **and the snapshot data** are all embedded in
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
3. `--config <file>`
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

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
| [`docs`](#docs) | A reference doc to stdout. Never analyzes, always exits `0` (an unknown subject exits non-zero). Bare `docs` lists languages; `docs <lang>` shows that language's subject catalog; `docs <lang> <subject>` prints the doc. `base` is a valid language for language-agnostic docs. |

There are two analysis commands, split by *what they emit*: `check` produces an exit
code (a CI gate), `report` produces files (a snapshot and a viewer). Both take the same
input and share the same vocabulary below. A third command, `docs`, reads no project and
just prints a reference doc for the `<subject>` you name. (The principle/metric doc corpus
is not published — it is embedded in the binary and printed on demand with `docs <ID>`;
see [templates.md](../templates.md).)

## Global options

`code-ranker` takes these global flags (accepted before or after the subcommand);
all other flags are per-command and must follow the command name:

| Flag | Meaning |
|---|---|
| `-h, --help` | Print help — top-level, or per-command with `code-ranker <cmd> --help`. |
| `-V, --version` | Print the version. |
| `--output.mode <quiet\|summary\|verbose>` | Verbosity of the **stderr** diagnostic stream (default `summary`). Machine output and artifacts on stdout/files are unaffected. See below. |

Progress and timing lines are written to **stderr**, each stamped `[HH:MM:SS.mmm]`;
diagnostics and machine output go to **stdout** or files, so the two streams never mix.
`--output.mode` controls how much of that stderr stream is emitted — it never changes
what lands on stdout:

| `--output.mode` | What stderr shows |
|---|---|
| `quiet` | Errors only (`error: …`); stderr is otherwise silent. Handy for scripts/CI that want a clean stream. |
| `summary` *(default)* | Errors, config-sanity warnings (`⚠ …`), written-artifact paths (`html-report=…`), and the closing `✓ <command> — <time>` line — the command name and total time, nothing more. |
| `verbose` | Everything: the `▶ <command>` startup line, the resolved `config:` path, and every external tool it shells out to with its duration to millisecond precision (`↳ cargo metadata --offline — 28.500s`, `↳ git status --porcelain — 0.017s`, `↳ rustc …`). The `↳` lines make the cost of a cold cargo cache visible at a glance. |

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

`--plugins` and `--ignore` govern analysis itself and apply **only when `[input]` is a
directory** — they are rejected with a snapshot input. `--config` is always accepted:
its rule and output keys apply to snapshots too, while analysis-only keys (e.g.
`plugins`) are ignored when reading one.

| Flag | Meaning |
|---|---|
| `--plugins <a,b,…>` | Active languages, comma-separated and/or repeatable: `rust`, `python`, `js` (covers TypeScript), … . A canonical name **or an alias** (`javascript`, `py`, `rs`, …). Overrides the `[plugins].enabled` list. Omitted everywhere ⇒ auto-detect **every** language present and analyze them all in one run — see [Plugin resolution](#plugin-resolution). |
| `--language <name>` | (`report` only) Focus the `scorecard` / `--prompt <ID>` on one language (canonical name or alias). Not required when only one language is present; required when a `--prompt`/`--focus` selector resolves across several. See [Recommendations](#recommendations-scorecard--prompt). |
| `--config plugins.<lang>.<key>=value` | Inline override of any plugin-config key (scalars / comma-lists). `plugins.base.*` targets the shared base language. `plugins.enabled=a,b` overrides the active language list. Deep tables go through a `[plugins.<lang>]` TOML block — see [Config](#config). |
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
| `--focus-path <path>` | Restrict the gate to these files/folders. The whole project is still analyzed (the dependency graph needs it), but a violation outside the focused paths is dropped — neither reported nor counted toward the exit code. A folder matches everything beneath it. Repeatable. See [`--focus`](#--focus-scoping). |
| `--focus <rule\|group>` | Restrict the gate to these rules / concern groups. Matches a full rule id (`threshold.file.hk`, `check.inline_tests_too_large`), the bare id (`inline_tests_too_large`), or a group (`TST`, `CPL`). Repeatable; combine with `--focus-path` to intersect (a violation must match both). See [`--focus`](#--focus-scoping). |
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
graph to compute coupling and cycles. The `--focus-*` flags bridge that gap: the **whole
project is analyzed**, but only violations within the focus are reported, and **only
those count toward the exit code**. Unlike `--top` (a display limit), focus scopes the
gate itself — `check` passes when the focus is clean, even if the rest of the project has
violations.

- **`--focus-path <path>`** — keep violations under a file/folder. An entry matches a file
  exactly or, treated as a folder, anything beneath it; a leading `./` and a trailing `/`
  are ignored. Locationless violations (e.g. a cycle whose breaking edge can't be placed)
  can't be attributed to a path and are dropped.
- **`--focus <rule|group>`** — keep violations of a rule or concern group. Matches a
  full rule id (`threshold.file.hk`, `check.inline_tests_too_large`), the bare id
  (`inline_tests_too_large`), or a group (`TST`, `CPL`).

Both are repeatable. With both set they **intersect** — a violation must match a path
*and* a rule. Combine with `--top` to rank within the focused set.

```sh
# gate only the file you're refactoring — the rest of the repo can't fail this run
code-ranker check . --focus-path crates/code-ranker-plugin-api/src/plugin.rs

# list only one custom linter's hits (rule id, bare id, or its group all work)
code-ranker check . --focus check.inline_tests_too_large
code-ranker check . --focus TST

# intersect: that rule, but only under one folder
code-ranker check . --focus-path crates/code-ranker-graph --focus TST
```

```sh
# lint the current project, fail the build on any violation
code-ranker check

# Python project: per-file budgets — cap any single file
code-ranker check ./api --plugins python \
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
threshold.file.cognitive  ·  rust  ·  CPX  ·  files graph
  where  {target}/src/handlers.rs
  issue  cognitive complexity 67 exceeds limit 25 (2.7× over budget)
  why    Cognitive complexity weights nested and interrupted control flow by how hard a human finds it to follow…
  fix    Run `code-ranker report --plugins rust --prompt cognitive` to generate an AI fix-prompt.
  tune   set with --threshold file.cognitive=N   ·   plugins.rust.rules.thresholds.file.cognitive in code-ranker.toml (or plugins.base for all)
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
`[plugins.base.rules.cycles]` counts per kind, plus the `[plugins.base.rules.thresholds.file]`
per-file thresholds (the worst single file max). Numbers use `_` separators.
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
| `--output.<fmt>.path <path>` | `json` + `html` in `.code-ranker/` | Which artifacts to emit and where. `<fmt>` is `json`, `html`, or `scorecard`. Repeatable, one per format. See [Output paths](#output-paths). |
| `--baseline <snapshot>` | — | Baseline snapshot (`.json` or `.html`). Turns the HTML into a diff (baseline vs current) with a verdict, and names it `…-diff.html`. See [`--baseline`](#--baseline-comparison). |
| `--focus <NAME>` | auto (all principles) | Frame the `scorecard` by a **metric** (`hk`, `cycle`, `sloc`, `cognitive`, `cyclomatic`, `fan_in`, `fan_out`, `items` — case-insensitive; also accepts the full threshold rule id `threshold.file.hk`, matched **by value** so it works whether or not the metric has a configured threshold) or a **principle** id (`LSP`, `ADP`, `SRP`, `OCP`, `DIP`, `ISP`, `DRY`, `KISS`, `LoD`, `MISU`, `CoI`, `YAGNI`, `CPX`). A metric narrows the scorecard to that metric; a principle frames it by that principle. Without it the scorecard spans every principle. Unknown names error with both namespaces listed. See [Recommendations](#recommendations-scorecard--prompt). |
| `--focus-path <PATH>` | all modules | Restrict the ranked modules to a subtree. The whole project is still analyzed (the dependency graph needs it), but only modules under one of these repo-relative paths are ranked/listed; a folder matches everything beneath it. Repeatable; combine with `--focus` to intersect. A dependency cycle is a global unit, so `--focus-path` does **not** narrow cycle members — only the node-ranked metric/breach lists. See [Recommendations](#recommendations-scorecard--prompt). |
| `--severity <tier>` | all tiers | Threshold tier for the `scorecard`: `info`, `warning`, or `auto`. Repeatable to show several tiers. |
| `--top <N>` | 15 (scorecard) | `scorecard`: how many rows; `--top 1` = the single worst module. With `--focus cycle`, `--top 1` prints one entire cycle (biggest `chain` first) with **all** its members. `--prompt <ID>`: how many modules the fix-prompt lists, ranked by the principle's sort metric. |
| `--export-full-config <PATH>` | — | Instead of analyzing, write the **full effective configuration** to `PATH` and exit. See [Inspecting the effective config](#inspecting-the-effective-config). |

`--focus`, `--focus-path`, `--severity`, and `--top` apply only when the `scorecard`
format (or `--prompt <ID>`) is selected; passing them otherwise is an error.
`--focus` and `--severity` are `scorecard`-only; `--prompt <ID>` honours `--top`,
`--focus-path`, and `--language`.

### Inspecting the effective config

`--export-full-config <PATH>` dumps the configuration code-ranker would actually use —
no analysis runs — as one TOML document with two top-level sections:

- `[project]` — the merged project config: the built-in defaults (`config/defaults.toml`,
  baked into the binary) **deep-merged** with the discovered / `--config` file. Shows
  every effective `[output]` / `[templates]` value, including the ones you did not set
  (inherited from the defaults).
- one `[plugins.<lang>]` section for **every registered language** (not only the
  active ones) — that language's fully-merged config (its inheritance chain
  `defaults.toml ⊕ [base] ⊕ <lang>.toml`, then your `[plugins.base]` /
  `[plugins.<lang>]` overrides): principles, rules, metrics, node/edge kinds, the
  metric-engine role tables, etc.

It honours `--plugins` and `--config`, so you can preview any combination:

```sh
# what `report` would use here, with my overrides folded in
code-ranker report . --config ci/strict.toml --export-full-config /tmp/full.toml

# the full Python plugin config (principles, vocab)
code-ranker report . --plugins python --export-full-config /tmp/python.toml
```

It is a **diagnostic view** of every parameter you can override — because the
project and language sections use different schemas (and `principles` differs
between the project and language shapes), the file is not meant to be fed back as a
single `--config`.

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

# narrow the triage to one metric (coupling)
code-ranker report . --output.scorecard --focus hk --top 5

# AI fix-prompt for a named principle/metric, to stdout
code-ranker report . --prompt hk --top 1
```

The HTML is **self-contained**: the snapshot data is embedded inline, so the single file
opens straight from disk (no server, no extra files). See [HTML viewer](#html-viewer).

## Output paths

`report` selects artifacts and their destinations through one flag family,
`--output.<fmt>.path`, where `<fmt>` is `json`, `html`, `sarif`, `codequality`,
or `scorecard`. `scorecard` is the recommendation output — see
[Recommendations](#recommendations-scorecard--prompt) for its flags and defaults.

`sarif` and `codequality` write the **same documents** as the matching
`check --output-format` (the current rule violations, with stable per-finding
fingerprints), but as artifacts rather than to stdout — so a single `report` run
can emit the JSON snapshot, the HTML viewer, *and* a findings report for CI in one
pass. `sarif` (SARIF 2.1.0) feeds GitHub code scanning / GitLab ≥18.11; `codequality`
(CodeClimate JSON) feeds the GitLab Code Quality MR widget (GA, no flag). Like
`scorecard`, both are opt-in: never part of the default set, and a
`--baseline` here only diffs the HTML — it does not filter the findings.

**Which formats are written:**

- No `--output.*` flag → the default set: **both** `json` and `html`, with default
  names, into `.code-ranker/`. (`scorecard` is never in the default set —
  it is emitted only when explicitly named.)
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

The recommendation format has its own default: `scorecard` defaults to
**`stdout`** (it is a console overview).

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

So the default `{ts}-{git-hash-3}.json` yields `20260526-114144-a3f.json`. When `[input]`
is a **snapshot**, `{git-hash}` / `{ts}` are read from the snapshot's embedded metadata —
the commit and time of the original analysis — not the current repo or clock.

The destination resolves as **`--output.<fmt>.path` flag › `[output.<fmt>] path` in
`code-ranker.toml` › built-in default**.

## Recommendations: `scorecard` & `prompt`

Two surfaces turn the snapshot's gate thresholds into refactoring guidance:

- **`scorecard`** (`--output.scorecard`) — a console triage overview answering
  *"what do I fix first?"*
- **`--prompt <ID>`** — a ready-to-paste AI fix-prompt for the principle or metric you
  name (the same Markdown the HTML viewer's Prompt Generator produces).

Both rank modules with the same engine. The `scorecard` is steered by `--focus`
(narrow to one metric or principle), `--focus-path` (scope to a subtree), `--severity` (which tier), and
`--top` (how many rows). `--prompt <ID>` names the target itself and honours `--top`
(how many modules it lists), `--focus-path`, and `--language`.

Both are **per language**: in a multi-language report use `--language <name>` to pick
which language the scorecard/prompt covers. It is optional when only one language is
present. When a `--focus <METRIC|PRINCIPLE>` or `--prompt <ID>` selector resolves in
two or more languages and `--language` is omitted, the command errors and lists the
matching languages (e.g. *"`HK` found in languages rust, markdown — pass `--language
<name>`"*).

> **Advisory, not a gate.** Unlike [`check`](#check), these never fail the build and carry
> no exit code. They surface the worst hotspots against **the same thresholds `check`
> enforces** — the `[plugins.<lang>.rules.thresholds.file]` limits *you* configure — so the report shows
> exactly what fails (or is about to fail) the gate. Both also work from a snapshot input
> (`report snap.json --output.scorecard`) with no re-analysis.

### Severity tiers

A ranking metric's tiers come from your gate config. **`warning`** is the
`[plugins.<lang>.rules.thresholds.file]` limit itself (the line that fails `check`); **`info`** is an
optional softer line below it, set per metric via a `[plugins.<lang>.metrics.<key>]` `info` field (kept
only when it sits below `warning`). A metric with no configured threshold has no tiers and
no breaches. A module is *in a tier* when its value crosses that line. `--severity`
selects which tier drives the output:

| Value | Meaning |
|---|---|
| `warning` | only modules over the warning line |
| `info` | modules over the info line (a superset of `warning`) |
| `auto` | warning if any module breaches it, else info — the **`--prompt` default** |

For `scorecard`, `--severity` is **repeatable** (`--severity warning --severity info`) to
show several tiers at once; with none given it shows all tiers.

Cycle-based principles (e.g. `ADP`) have **no numeric threshold** — every module in a
dependency cycle counts, ranked by HK, and `--severity` is ignored for them.

### Focus (`--focus` / `--focus-path`)

`--focus <NAME>` frames the output, resolving NAME (case-insensitive) against **two
namespaces**:

- a **metric** — the bare key `hk` (Henry-Kafura coupling), `cycle` (dependency cycles —
  the ADP view), `sloc` (module size), `cognitive` / `cyclomatic` (complexity), `fan_in` /
  `fan_out` (coupling direction), `items` (interface size), **or** the full threshold rule
  id (`threshold.file.hk`). Matched **by value**, so it works whether or not the metric has
  a configured `[plugins.<lang>.rules.thresholds.file]` threshold. This narrows the `scorecard`
  to that metric.
- a **principle** id — `LSP`, `ADP`, `SRP`, `OCP`, `DIP`, `ISP`, `DRY`, `KISS`, `LoD`,
  `MISU`, `CoI`, `YAGNI`, `CPX`. This frames the scorecard by that **design principle**.

An unknown name is a hard error that lists both namespaces (`unknown --focus '<name>'.
Metrics: …. Principles: …`).

`--focus` steers the `scorecard`. Without `--focus` it spans all principles (one row
each). To frame a fix-prompt by a metric or principle, name it with `--prompt <ID>`
instead: `--prompt hk` emits an **HK-framed** prompt (titled "HK — Henry–Kafura", no
Liskov wrapper); `--prompt LSP` emits the **Liskov-framed** one. The principle *catalog*
lives in the snapshot's `principles` (shared with the HTML viewer's Prompt Generator and
used for the prompt's prose). `cycle` has **no numeric threshold** — every module in a
dependency cycle counts, ranked by HK, and `--severity` is ignored for it.

`--focus-path <PATH>` restricts the ranked modules to a subtree (repeatable). The whole
project is still analyzed (the graph needs it), but only modules under one of these
repo-relative paths are ranked/listed; a folder matches everything beneath it. Combine with
`--focus` to intersect. A dependency cycle is a global unit, so `--focus-path` does
**not** narrow cycle members — only the node-ranked metric/breach lists.

### `scorecard` — triage overview

Defaults to **stdout**, so a bare `--output.scorecard` prints to the console. It shows a
per-principle table (warning / info counts + the worst module) followed by the worst
modules overall:

```sh
code-ranker report . --output.scorecard                     # all tiers, ~15 rows
code-ranker report . --output.scorecard --severity warning --top 20
code-ranker report . --output.scorecard.path=triage.txt     # to a file instead
code-ranker report . --output.scorecard --focus sloc   # narrow to one metric
```

```text
scorecard  (rust, 142 files)

PRESET  PRINCIPLE              WARN  INFO  TOP MODULE
ADP     Acyclic Dependencies      2     2  a.rs ↔ b.rs
SRP     Single Responsibility     5    18  cli/main.rs (sloc 1832)
CPX     Reduce Complexity         3    11  cli/main.rs (cog 67)

WORST MODULES
 1 warn cli/main.rs     hk 4.2M   +sloc, fan_out, cycle
 2 warn snapshot.rs     sloc 1.8K +hk
 3 info plugin/rust.rs  fan_out 14

→ code-ranker report . --prompt <PRINCIPLE|METRIC>
```

`--top N` caps the worst-modules list (default ~15); `--focus <NAME>` narrows the
scorecard to a single ranking metric (or frames it by a principle); `--focus-path <PATH>`
scopes the ranked modules to a subtree.

### `--prompt <ID>` — AI fix-prompt for one principle/metric by name

`--prompt <ID>` prints the fix-prompt for the principle or metric you name to stdout and
exits. You pick the target yourself — typically a principle or metric read off the
`scorecard`. It accepts a principle id (`SRP`, `ADP`) or a metric key (`hk`,
`cyclomatic`), case-insensitive, and writes no artifacts. Shape the module list with
`--top N` / `--focus-path`. If the `<ID>` resolves in more than one active language, pass
`--language <name>` to pick one (the command otherwise errors and lists the candidates).

The prompt is the same Markdown the HTML viewer's Prompt Generator produces — the
principle's intent and summary, how to read the full principle (the offline
`code-ranker docs <lang> <id>` command, no network), a task checklist, the offending
modules annotated with their metric value, and the relevant **flow** connection lists
(`uses` — structural `contains`/`reexports` are excluded). A metric id (`hk`, `cycle`, …)
frames the prompt by the **metric itself** — its own name, description, and `remediation`
doc (e.g. `plugins/base/HK.md`), with **no** SOLID design-principle wrapper.

```sh
code-ranker report . --prompt HK --top 1     # HK fix-prompt, top module
code-ranker report . --prompt HK > prompt.md # redirect to a file when you need an artifact
```

To print a **reference doc** itself (a principle's text, a metric's spec card, the AI
playbook, …) rather than a fix-prompt, use the analysis-free [`docs`](#docs) command —
e.g. `code-ranker docs rust HK` or `code-ranker docs rust ai`.

## `docs`

```
code-ranker docs [<lang> [<subject>]] [--config <PATH|KEY=VALUE>]
```

`code-ranker docs` prints reference docs to stdout. It **never analyzes** and takes **no
`[input]` positional**. The language is now the **first positional argument** — there is
**no `--plugin` flag**. Config is auto-discovered from the current directory (for
language detection only).

**Invocation forms:**

| Invocation | What it prints |
|---|---|
| `code-ranker docs` | Lists every language — detected project languages annotated — plus `base`. |
| `code-ranker docs <lang>` | That language's full subject catalog (metrics, principles, categories, …). |
| `code-ranker docs <lang> <subject>` | The subject doc for that language. |
| `code-ranker docs <lang> ai` | The AI-agent playbook for that language (full playbook + catalog). |
| `code-ranker docs base` | The language-agnostic subject catalog (`base` is a valid language). |
| `code-ranker docs base ai` | The base AI playbook (language-agnostic). |
| `code-ranker docs <subject>` (no language) | **ERROR** — lists the project's languages and points at `code-ranker docs <lang> <subject>`. |

`<subject>` selects what to print within a language:

| `<subject>` | What it prints |
|---|---|
| `ai` | The offline **AI-agent playbook** (full playbook + principle/metric catalog). Requires a language. |
| `metrics` | An **index of every metric**, grouped by category. |
| `principles` | An **index of every design principle**. |
| a metric **category** (`loc`, `complexity`, `halstead`, `maintainability`, `coupling`) | The category's label/description **plus** its member metrics. |
| a **metric** key (`sloc`, `hk`, the language's own `unsafe` / `items`, …) | The metric's **spec card** (label / name / description / category / formula). For metrics with a full prose doc (`hk`, `cyclomatic`, `cognitive`, `fan_in`, `fan_out`) the prose doc is appended after the card. |
| a **principle** id (`SRP`, `ADP`, … including project-defined principles) | The principle's **full doc** (or a synthetic card for a doc-less custom principle). |

Subject matching is **separator/case-insensitive** — `fan_in`, `Fan-in`, and `FAN in`
all resolve the same metric. An unknown subject exits non-zero.

A `<subject>` given **without a language** (e.g. `docs hk`, `docs ai`, `docs metrics`)
is always an error: it exits non-zero and prints the project's detected languages with
a hint to use `code-ranker docs <lang> <subject>`.

```sh
code-ranker docs                    # list every language (detected ones annotated) + base
code-ranker docs rust               # the rust subject catalog
code-ranker docs rust ai            # the rust AI playbook
code-ranker docs rust hk            # the HK metric card + full doc (rust)
code-ranker docs rust metrics       # the metric index for rust
code-ranker docs rust coupling      # the coupling category for rust
code-ranker docs rust unsafe        # a rust-specific metric
code-ranker docs base               # the base (language-agnostic) catalog
code-ranker docs base ai            # the base AI playbook
code-ranker docs rust SRP           # the SRP principle doc
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

`code-ranker` analyzes **all** relevant languages in one run. The set of active
languages is resolved by this precedence (low → high), where each level **fully
replaces** the lower one (no merge):

1. **Auto-detect** (lowest) — every plugin whose `detect()` matches the workspace.
2. **Config `[plugins].enabled`** — the `enabled = [...]` list in the `[plugins]` table in
   `code-ranker.toml` / `Cargo.toml#metadata.code-ranker`.
3. **Console `--plugins`** (highest) — the comma list / repeated flag.

So a list set in config **or** on the console is used verbatim; auto-detect runs
only when no list is set anywhere; if both config and console set one, the console
wins (applies only when `[input]` is a directory).

**Aliases.** Anywhere a language is named — `--plugins`, `--language`, the
`[plugins].enabled` list, a `[plugins.<lang>]` block key, and `docs <lang>` — you
may use a short **alias** instead of the canonical name; it resolves to the
canonical name (and the snapshot always records the canonical). Built-in aliases:
`rs`→`rust`, `py`→`python`, `javascript`→`js`, `typescript`→`ts`, `markdown`→`md`,
`golang`→`go`, `c++`/`cxx`→`cpp`, `cs`/`c#`→`csharp`. So `report --plugins javascript
--prompt hk` is the same as `--plugins js`. Run `code-ranker docs` to see
every language with its aliases.

**Auto-detect** runs every plugin whose `detect()` matches, evaluated against its
**effective** config — so an overridden `detect_markers` / `extensions` (via
`[plugins.<lang>]` or `--config plugins.<lang>.*`) changes what is detected. The
default markers are:

- `Cargo.toml` → `rust`
- `pyproject.toml` / `setup.py` / `setup.cfg` → `python`
- `package.json` / `tsconfig.json` → `javascript`

**Multiple matches are normal** — they are all analyzed and merged into one report;
there is no "ambiguous project" error. A language that yields an empty graph is
silently dropped.

**Invariant: one file ↔ exactly one language.** The active plugins' file sets are
disjoint.

Errors:

- **No language detected** — auto-detect matches nothing: *"could not determine any
  language in `<workspace>`; specify `[plugins] enabled = ["<name>"]` in code-ranker.toml or
  `--plugins <name>`"*.
- **Legacy `plugin` key** — the scalar `plugin = "..."` config key is not recognized;
  the error points to `[plugins] enabled = [...]`.
- **Extension conflict** — two active plugins claim the same file extension; a startup
  error (before analysis), e.g. *"extension `.h` is claimed by both `c` and `cpp` —
  adjust `extensions`/`plugins`"*.
- **Invalid `--plugins`** — an unknown language name in the list.

See [ERRORS.md](ERRORS.md) for the full diagnostics.

## HTML viewer

The HTML report is **self-contained**: the viewer app (Dagre graph layout, pan/zoom,
a sortable node table for the single Files view, and the prompt-generator panel whose
principle buttons are read from `snapshot.languages.<lang>.principles` — the 13 design
principles ADP / SRP / OCP / LSP / ISP / DIP / DRY / KISS / LoD / MISU / CoI / YAGNI /
CPX) **and the snapshot data** are all embedded in
the one file. A **language dropdown** in the header shows the active language and
switches the whole report (opening on the largest language by default); it is hidden
when the report covers a single language. External library nodes render in a distinct
amber colour with dashed edges. No network, no telemetry — `open` it straight from
disk.

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

1. CLI flags (`--plugins`, `--threshold`, `--ignore`, `--output.<fmt>.path`, …)
2. `--config KEY=VALUE` inline overrides (including `--config plugins.<lang>.<key>=value`)
3. `--config <file>` — repeatable; multiple files layer in command-line order
   (last wins), and any file disables the `code-ranker.toml` auto-discovery below
4. `code-ranker.toml` (cwd, then workspace root)
5. `Cargo.toml` metadata (`[workspace.metadata.code-ranker]`)
6. Built-in defaults

The inline form takes a dotted key into the config schema:

```sh
# tighten one rule in CI without editing code-ranker.toml
code-ranker check --config plugins.base.rules.thresholds.file.cognitive=25 \
                 --config plugins.base.rules.cycles.chain=7

# override an output destination inline
code-ranker report --config output.html.path=dist/report.html
```

`--ignore` globs are **merged** (union) with config globs; cycle rules and thresholds
**override** the file value. See [`docs/config.md`](config.md) for the full schema.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | `check` passed (no violations, or `--exit-zero`); `report` completed successfully. |
| 1 | Any failure — a `check` violation (cycle, threshold, or regression, without `--exit-zero`) **or** a runtime error (IO / plugin failure, no language detected, an extension claimed by two plugins, a cross-language `--prompt`/`--focus` needing `--language`, malformed config, analysis flags passed with a snapshot input). |
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
- `js` — tree-sitter-javascript / tree-sitter-typescript; one plugin handles
  `.js`, `.jsx`, `.ts`, `.tsx`. Same file + external model as Python.

All plugins are built into the `code-ranker` binary — there is nothing to install and no
external plugin processes. Adding a language means adding a built-in plugin to the binary.

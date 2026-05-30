# `code-split` CLI reference

Pluggable multi-language structural analysis platform.

```
code-split <command> [options] [path] [-- <plugin-args>...]
```

`code-split` is command-driven: running it with no command prints help — every action
goes through an explicit subcommand, there is no default action. Run
`code-split <command> --help` for per-command flags, `code-split --version` for the version.

> **Offline & private.** code-split always runs entirely on your machine. It makes **no
> network calls**, sends **no telemetry or analytics**, and **never uploads your code or
> analysis results** anywhere. Generated HTML reports are self-contained — no CDN, no
> external requests, no tracking.

## Commands

| Command | What it does |
|---|---|
| [`check`](#check) | Lint a workspace: analyze, evaluate thresholds & cycle rules, print diagnostics, exit non-zero on violation. Writes no files. |
| [`report`](#report) | Analyze a workspace and write artifacts — a JSON snapshot and/or an HTML viewer. Optionally compares against a baseline in the same run (`--before`). |
| [`diff`](#diff) | Compare two existing snapshots (no analysis) and write a diff report with an `improved` / `degraded` / `neutral` verdict. |

## Global options

Accepted by every command (and before the command name).

| Flag | Meaning |
|---|---|
| `--config <PATH \| KEY=VALUE>` | Load config from a file path, **or** override a single setting inline. Repeatable; inline values win. See [Config](#config). |
| `--color <when>` | `auto` (default), `always`, `never`. |
| `-v, --verbose` | More logging. |
| `-q, --quiet` | Suppress everything except diagnostics. |
| `-h, --help` / `-V, --version` | Help / version. |

## Common analysis options

Shared by `check` and `report` (the two commands that analyze a workspace).

| Flag | Meaning |
|---|---|
| `[path]` | Workspace to analyze (positional). Default `.` (current directory). |
| `--plugin <name\|auto>` | Plugin to use: `rust`, `python`, or `javascript` (covers TypeScript). `auto` (default) resolves the language automatically — see [Plugin resolution](#plugin-resolution). |
| `--graph <kinds>` | Which graphs to build. Repeatable or comma-separated: `modules`, `files`, `functions`. Default: all three. |
| `--ignore <glob>` | Repeatable. Glob to exclude paths from analysis. Merged with config-file globs. |
| `--local-only` | Skip any network-dependent step (e.g. `cargo metadata` style). |
| `-- <extra-args>` | Everything after `--` is forwarded verbatim to the plugin. |

## `check`

The linter. Analyzes the workspace, evaluates cycle rules and thresholds, prints
diagnostics, and **exits non-zero** when any violation is found. Writes no files.

```
code-split check [path] [options]
```

| Flag | Meaning |
|---|---|
| `--threshold <SCOPE[.avg].METRIC=N>` | Hard limit on a metric — a breach fails the check. SCOPE: `file` / `module` / `function` (a single unit on that graph). Add `.avg` for that scope's graph-wide average (e.g. `function.avg.cognitive`). METRIC: `cyclomatic`, `cognitive`, `hk`, `fan_in`, `fan_out`, `loc`. Repeatable. See [ERRORS.md](ERRORS.md#threshold-scopes). |
| `--cycle-rule <KIND=on\|off\|N>` | Configure a cycle check. KIND: `test-embed`, `mutual`, `chain`. Value: `on` (any cycle fails), `off` (ignored), or `N` (allow up to N cycles of that kind — e.g. `chain=7` forbids an 8th). Defaults: `test-embed` off, `mutual`/`chain` on. |
| `--output-format <fmt>` | Diagnostics format: `human` (default), `json`, `github`, `sarif`. Use `github` for PR annotations, `sarif`/`json` for tooling. |
| `--top <N>` | Report only the `N` worst violations (ranked worst-first) and suppress the rest. A reporting limit only — it does **not** change the exit code. Default: all. |
| `--exit-zero` | Return exit code 0 even when violations exist. Useful in non-blocking CI checks. |
| `--suggest-config` | After the findings, also print the project's current values as a ready-to-paste `code-split.toml` baseline (cycle counts + per-scope thresholds). Off by default; `human` output only. |

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
code-split check

# Python project: a single function and a single file get different budgets
code-split check ./api --plugin python \
  --threshold function.cognitive=25 --threshold file.loc=800

# CI gate with machine-readable annotations; also flag test-embed cycles
code-split check --cycle-rule test-embed=on --output-format github

# useful for AI agents: surface only the single worst violation to fix
code-split check --top 1
```

### Diagnostics output

Every finding is identified by its dotted **rule id** — the same string used as
the config key and CLI flag — and tagged with a concern **group**: `CYC`
(dependency cycles), `CPX` (complexity), `CPL` (coupling), `SIZ` (size). Threshold
rules are `threshold.<scope>[.avg].<metric>` — scope `file` / `module` /
`function` for a single unit, plus an optional `.avg` for that scope's graph-wide
average. The full reference — what each rule flags, why it matters, and how to fix
it — lives in [ERRORS.md](ERRORS.md).

In the default `human` format each violation is a self-contained block, detailed
enough to paste straight into an AI assistant as a complete prompt:

```text
threshold.function.cognitive  ·  CPX  ·  functions graph
  where  fn:app::handlers::process — src/handlers.rs:142
  issue  cognitive complexity 67 exceeds limit 25 (2.7× over budget)
  why    Cognitive complexity weights nested and interrupted control flow by how hard a human finds it to follow…
  fix    Extract nested blocks into named helpers, use early returns to cut nesting depth…
  tune   set with --threshold function.cognitive=N   ·   rules.thresholds.function.cognitive in code-split.toml
  ref    https://github.com/ffedoroff/code-split/blob/main/docs/ERRORS.md#group-cpx
```

The rule id and group are present in every `--output-format`: the block header
(`human`), `"rule"` + `"group"` fields (`json`), the annotation title (`github`),
and `ruleId` plus a fired-rules `tool.driver.rules` catalog (`sarif`).

### Current-values config block (`--suggest-config`)

With `--suggest-config`, the `human` output prints — after the findings — the
project's current measured values as ready-to-paste `code-split.toml` blocks: the
`[rules.cycles]` counts per kind, plus per-scope thresholds (`single` = the worst
single unit max, `.avg` = the graph-wide average). Numbers use `_` separators. Copy
a block to pin today's numbers as a baseline that passes now and fails on
regression. Off by default; the machine formats (`json`/`github`/`sarif`) omit it.

```sh
code-split check --suggest-config
```

## `report`

Analyzes the workspace and writes artifacts into `--report-path`. The analyzed state
is the **after** side. Pass `--before <snapshot>` to turn the HTML into a diff and add
a verdict — **report and comparison in a single run**.

```
code-split report [path] [options]
```

| Flag | Default | Meaning |
|---|---|---|
| `--format <kinds>` | `json,html` | Artifacts to emit: `json`, `html`. Comma-separated or repeatable (`--format json --format html`). |
| `--before <file>` | — | Baseline snapshot (`.json`, or a prior `.html` report). Makes the HTML a diff (before = this file, after = this run) with a verdict, and names it `…-diff.html`. |
| `--report-path <dir>` | `.code-split` | Output directory for all artifacts. |
| `--json-name <tpl>` | `{project-dir}-{ts}.json` | Snapshot filename template. Placeholders — see [Name templates](#name-templates). |
| `--html-name <tpl>` | `{project-dir}-{ts}.html` | HTML filename template (data embedded inline). With `--before`, `-diff` is inserted before `.html`. |

```sh
# snapshot + viewer, in .code-split/
code-split report --format json,html

# report AND compare against a baseline, one command:
# after = this analysis, before = the given snapshot, + verdict
code-split report --format json,html --before .code-split/user-provisioning-20260526-004000.json

# just the snapshot JSON, no viewer
code-split report --format json
```

The HTML is **self-contained**: the snapshot data is embedded inline, so the single file
opens straight from disk (no server, no extra files). See [HTML viewer](#html-viewer).

## `diff`

Compares two **existing** snapshots — no analysis — and writes a diff report. Use this
in CI when both sides are already built (e.g. base-branch snapshot vs PR snapshot).

```
code-split diff --before <a.json> --after <b.json> [options]
```

| Flag | Default | Meaning |
|---|---|---|
| `--before <file>` | required | Baseline snapshot (`.json`, or a prior `.html` report). |
| `--after <file>` | required | New snapshot (`.json`, or a prior `.html` report). |
| `--format <kinds>` | `html` | Artifacts to emit: `html`, `json`. Comma-separated or repeatable. JSON is the machine-readable diff for CI parsing. |
| `--report-path <dir>` | `.code-split` | Output directory. |
| `--html-name <name>` | `index.html` | HTML diff filename. |
| `--json-name <name>` | `diff.json` | JSON diff filename. |

```sh
# HTML diff for humans
code-split diff --before main.json --after pr.json

# JSON diff for CI, read the verdict
code-split diff --before main.json --after pr.json --format json
cat .code-split/diff.json | jq '.verdict'

# typical PR flow
code-split report --format json --json-name pr.json   # on the PR
git stash; git checkout main
code-split report --format json --json-name main.json  # on base
git checkout -; git stash pop
code-split diff --before .code-split/main.json --after .code-split/pr.json
```

## Plugin resolution

With `--plugin auto` (the default), the plugin is resolved in this order:

1. **Explicit `--plugin <name>`** on the command line (any value other than `auto`) wins.
2. Otherwise the **`plugin` key in the config file** (`code-split.toml` /
   `Cargo.toml#metadata.code-split`), if set and not `auto`.
3. Otherwise **auto-detect by project markers** in the workspace root:
   - `Cargo.toml` → `rust`
   - `pyproject.toml` / `setup.py` / `setup.cfg` → `python`
   - `package.json` / `tsconfig.json` → `javascript`
4. If **more than one** marker matches, `code-split` errors and asks you to disambiguate
   with an explicit `--plugin`. If **no** marker matches, it errors with the same hint.

## Name templates

`--json-name` accepts placeholders:

| Placeholder | Expands to | Example |
|---|---|---|
| `{project-dir}` | The analyzed directory's basename, lowercased, non-alphanumerics collapsed to `-`. Override with `name` in the config file. | `user-provisioning` |
| `{ts}` | Local timestamp, `YYYYMMDD-HHMMSS`. | `20260526-114144` |

So the default `{project-dir}-{ts}.json` yields `user-provisioning-20260526-114144.json`.

## HTML viewer

The HTML report is **self-contained**: the viewer app (Dagre graph layout, pan/zoom,
sortable node tables for modules / files / functions, and the prompt-generator panel with
ADP / SRP / OCP / LSP / ISP / DIP / DRY / KISS / LoD / MISU / CoI / YAGNI presets plus
*Reduce Complexity* and *Split Components*) **and the snapshot data** are all embedded in
the one file. No network, no telemetry — `open` it straight from disk.

The data is embedded as `<script type="application/json">` tags (`cs-before` / `cs-after`),
which the viewer reads on load and which `--before` / `diff` can extract back out — so an
`.html` report is interchangeable with a `.json` snapshot as a diff input.

| Invocation | Output file | Mode | Embedded data |
|---|---|---|---|
| `report --format html` | `{project-dir}-{ts}.html` | review (single snapshot) | this run |
| `report --format html --before A` | `{project-dir}-{ts}-diff.html` | diff + verdict | `A` and this run |
| `diff --before A --after B` | `index.html` | diff + verdict | `A` and `B` |

In the viewer, the **↑ change** / **↑ compare…** buttons swap in a different snapshot from
disk — accepting either a `.json` snapshot or a `.html` report.

## Config

Settings merge from several sources; **higher priority wins**:

1. CLI flags (`--threshold`, `--ignore`, …)
2. `--config KEY=VALUE` inline overrides
3. `--config <file>`
4. `code-split.toml` (cwd, then workspace root)
5. `Cargo.toml` metadata (`[workspace.metadata.code-split]`)
6. Built-in defaults

The inline form takes a dotted key into the config schema:

```sh
# tighten one rule in CI without editing code-split.toml
code-split check --config rules.thresholds.function.cognitive=25 \
                 --config rules.cycles.test-embed=true
```

`--ignore` globs are **merged** (union) with config globs; cycle rules and thresholds
**override** the file value. See [`docs/config.md`](config.md) for the full schema.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | `check` passed (or `--exit-zero`); `report` / `diff` completed. |
| 1 | Generic error: parsing, IO, plugin failure, ambiguous/undetected plugin under `auto`, or invalid arguments. |
| Non-zero (other) | `check` found a violation (cycle or threshold) without `--exit-zero`. |

## Plugins

Built-in (no install needed):

- `rust` — `cargo metadata` + `syn` for module/file graphs, optional `rust-analyzer`
  (`ra_ap_*`) for the call graph when not in `--local-only` mode and `cargo` is on PATH.
- `python` — tree-sitter-python for module/file/function graphs, native parser.
- `javascript` — tree-sitter-javascript / tree-sitter-typescript; one plugin handles
  `.js`, `.jsx`, `.ts`, `.tsx`.

### Rust plugin: analysis depth

The built-in `rust` plugin runs at different depths, controlled by the **common**
`--local-only` and `--graph` flags (there are no Rust-specific flags).

| Mode | Invocation | `cargo metadata` | Function call graph (rust-analyzer) |
|---|---|---|---|
| **Deep** (default) | `--plugin rust` | full (resolves dependencies) | built — `Calls` edges, if `cargo` is on PATH and `functions` is in `--graph` |
| **Shallow / offline** | `--plugin rust --local-only` | `--no-deps` (no resolution, no network) | skipped — functions graph has no call edges |
| **Structure only** | `--plugin rust --graph modules,files` | full | skipped (functions not requested) |

- `--local-only` makes the run fully offline and much faster: it skips both dependency
  resolution and the rust-analyzer semantic stage. Module/file graphs and complexity
  metrics are still produced — only the function **call graph** is omitted.
- If `cargo` is not on PATH, the call-graph stage is skipped automatically, so a deep run
  degrades gracefully to shallow.

```sh
# deep: module graph + function call graph (rust-analyzer)
code-split check . --plugin rust

# shallow & offline: skip dependency resolution and the call graph
code-split report . --plugin rust --local-only

# structure only: skip the (slow) call-graph stage
code-split report . --plugin rust --graph modules,files
```

All plugins are built into the `code-split` binary — there is nothing to install and no
external plugin processes. Adding a language means adding a built-in plugin to the binary.

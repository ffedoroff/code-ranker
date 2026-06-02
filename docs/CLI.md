# `code-split` CLI reference

Pluggable multi-language structural analysis platform.

```
code-split <command> [path] [options]
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

`code-split` takes no global flags of its own beyond the clap built-ins:

| Flag | Meaning |
|---|---|
| `-h, --help` | Print help — top-level, or per-command with `code-split <cmd> --help`. |
| `-V, --version` | Print the version. |

There is **no** `--color`, `--verbose`, or `--quiet` flag. Progress and timing
lines are always written to **stderr** (`[HH:MM:SS] …`); diagnostics and machine
output go to **stdout** or files, so the two streams never mix. All other flags
are per-command and must follow the command name.

## Common analysis options

Shared by `check` and `report` (the two commands that analyze a workspace);
**not** accepted by `diff`.

| Flag | Meaning |
|---|---|
| `[path]` | Workspace to analyze (positional). Default `.` (current directory). |
| `--plugin <name\|auto>` | Plugin to use: `rust`, `python`, or `javascript` (covers TypeScript). `auto` (default) resolves the language automatically — see [Plugin resolution](#plugin-resolution). |
| `--config <PATH \| KEY=VALUE>` | Repeatable. Load config from a file path, **or** override one setting inline (`KEY=VALUE`); inline values win. See [Config](#config). |
| `--ignore <glob>` | Repeatable. Glob to exclude paths from analysis. Merged with config-file globs. |
| `--local-only` | Skip any network-dependent step (e.g. `cargo metadata` dependency resolution). |
| `-- <extra-args>` | Reserved: arguments after `--` are accepted but **not currently forwarded** to any built-in plugin (no built-in plugin consumes them yet). |

## `check`

The linter. Analyzes the workspace, evaluates cycle rules and thresholds, prints
diagnostics, and **exits non-zero** when any violation is found. Writes no files.

```
code-split check [path] [options]
```

| Flag | Meaning |
|---|---|
| `--threshold <file.METRIC=N>` | Hard limit on a per-file metric — a breach fails the check. Scope is always `file` (a single file). METRIC: `cyclomatic`, `cognitive`, `hk`, `fan_in`, `fan_out`, `loc`. Repeatable. See [ERRORS.md](ERRORS.md#threshold-scopes). |
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

# Python project: per-file budgets — cap any single file
code-split check ./api --plugin python \
  --threshold file.cognitive=25 --threshold file.loc=300

# CI gate with machine-readable annotations; also flag test-embed cycles
code-split check --cycle-rule test-embed=on --output-format github

# useful for AI agents: surface only the single worst violation to fix
code-split check --top 1
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
  where  file:{target}/src/handlers.rs
  issue  cognitive complexity 67 exceeds limit 25 (2.7× over budget)
  why    Cognitive complexity weights nested and interrupted control flow by how hard a human finds it to follow…
  fix    Extract nested blocks into named helpers, use early returns to cut nesting depth…
  tune   set with --threshold file.cognitive=N   ·   rules.thresholds.file.cognitive in code-split.toml
  ref    https://github.com/ffedoroff/code-split/blob/main/docs/ERRORS.md#group-cpx
```

The rule id and group are present in every `--output-format`: the block header
(`human`), `"rule"` + `"group"` fields (`json`), the annotation title (`github`),
and `ruleId` plus a fired-rules `tool.driver.rules` catalog (`sarif`).

### Current-values config block (`--suggest-config`)

With `--suggest-config`, the `human` output prints — after the findings — the
project's current measured values as ready-to-paste `code-split.toml` blocks: the
`[rules.cycles]` counts per kind, plus the per-file thresholds (the worst
single file max). Numbers use `_` separators.
Copy a block to pin today's numbers as a baseline that passes now and fails on
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
| `--json-name <tpl>` | `{ts}-{git-hash-3}.json` | Snapshot filename template (overrides `[output] json-name` in config). Placeholders — see [Name templates](#name-templates). |
| `--html-name <tpl>` | `{ts}-{git-hash-3}.html` | HTML filename template (data embedded inline; overrides `[output] html-name`). With `--before`, `-diff` is inserted before `.html`. |

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

`--json-name` / `--html-name` accept placeholders:

| Placeholder | Expands to | Example |
|---|---|---|
| `{project-dir}` | The analyzed directory's basename, lowercased, non-alphanumerics collapsed to `-`. | `user-provisioning` |
| `{ts}` | Local timestamp, `YYYYMMDD-HHMMSS`. | `20260526-114144` |
| `{git-hash}` | The 12-char short commit hash (zeros if not a git repo). | `a3f9c21b4d5e` |
| `{git-hash-N}` | The first `N` chars of the commit hash. | `{git-hash-3}` → `a3f` |

So the default `{ts}-{git-hash-3}.json` yields `20260526-114144-a3f.json`.

The name is resolved as **`--json-name` flag › `[output] json-name` in
`code-split.toml` › built-in default**. To pin a project-wide template
(e.g. include the project name), set it in config instead of passing the flag
every time:

```toml
[output]
json-name = "{project-dir}-{ts}.json"   # → user-provisioning-20260526-114144.json
# html-name = "{project-dir}-{ts}.html"
```

## HTML viewer

The HTML report is **self-contained**: the viewer app (Dagre graph layout, pan/zoom,
a sortable node table for the single Files view, and the prompt-generator panel with
ADP / SRP / OCP / LSP / ISP / DIP / DRY / KISS / LoD / MISU / CoI / YAGNI presets plus
*Reduce Complexity* and *Split Components*) **and the snapshot data** are all embedded in
the one file. External library nodes render in a distinct amber colour with dashed
edges. No network, no telemetry — `open` it straight from disk.

The data is embedded as `<script type="application/json">` tags (`cs-before` / `cs-after`),
which the viewer reads on load and which `--before` / `diff` can extract back out — so an
`.html` report is interchangeable with a `.json` snapshot as a diff input.

| Invocation | Output file | Mode | Embedded data |
|---|---|---|---|
| `report --format html` | `{ts}-{git-hash-3}.html` | review (single snapshot) | this run |
| `report --format html --before A` | `{ts}-{git-hash-3}-diff.html` | diff + verdict | `A` and this run |
| `diff --before A --after B` | `index.html` | diff + verdict | `A` and `B` |

In the viewer, the **↑ change** / **↑ compare…** buttons swap in a different snapshot from
disk — accepting either a `.json` snapshot or a `.html` report.

Per-node modal: clicking a node opens a fullscreen card; for project files its
field list includes a **Source** link to the file on the project's git host
(GitLab/GitHub, built from `git.origin` at the snapshot's commit). To select
nodes without opening the modal, **hold Shift** over the map (the cursor
changes) and Shift-click nodes — each toggles selection just like its table
checkbox.

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
code-split check --config rules.thresholds.file.cognitive=25 \
                 --config rules.cycles.test-embed=true
```

`--ignore` globs are **merged** (union) with config globs; cycle rules and thresholds
**override** the file value. See [`docs/config.md`](config.md) for the full schema.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | `check` passed (no violations, or `--exit-zero`); `report` / `diff` completed successfully. |
| 1 | Any failure — a `check` violation (cycle or threshold, without `--exit-zero`) **or** a runtime error (IO / plugin failure, ambiguous-or-undetected plugin under `auto`, malformed config). |
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

### Rust plugin: offline mode

The built-in `rust` plugin honours the **common** `--local-only` flag (there are no
Rust-specific flags). `--local-only` passes `--no-deps` to `cargo metadata`, so the
run is fully offline — external dependencies are not enumerated. The file graph and
per-file complexity metrics are still produced.

```sh
# default: full file graph with external library nodes
code-split check . --plugin rust

# offline: skip dependency resolution
code-split report . --plugin rust --local-only
```

All plugins are built into the `code-split` binary — there is nothing to install and no
external plugin processes. Adding a language means adding a built-in plugin to the binary.

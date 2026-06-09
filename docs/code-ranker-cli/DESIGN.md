# Technical Design — Code Ranker CLI (`code-ranker-cli`)

The technical design of the `code-ranker` binary: the `code-ranker-cli` crate
(orchestrator, plugin registry/dispatch, `check` linter, `report` artifact
writer), the recommendation engine, the CLI API contracts, and the CLI
reference & examples. This is a component slice of the technical design — for
the architecture overview, principles, domain model, the plugin/extraction
crates and the plugin system see the main [DESIGN](../DESIGN.md); for the HTML
viewer assets see [`code-ranker-viewer/DESIGN.md`](../code-ranker-viewer/DESIGN.md).

<!-- toc -->

- [1. Component Model](#1-component-model)
  - [code-ranker-cli](#code-ranker-cli)
  - [code-ranker-cli recommendation engine](#code-ranker-cli-recommendation-engine)
- [2. API Contracts](#2-api-contracts)
  - [Unified CLI](#unified-cli)
  - [Report Generator](#report-generator)
  - [Check / Regression Gate](#check--regression-gate)
- [3. CLI Reference and Examples](#3-cli-reference-and-examples)

<!-- /toc -->

## 1. Component Model

### code-ranker-cli

- [x] `p1` - **ID**: `cpt-code-ranker-component-cli`

The single user-facing binary `code-ranker`. There is no default command —
a bare invocation prints help. `main()` owns two subcommands — `check` and
`report` — both taking a single polymorphic positional `[input]` (a directory
to **analyze**, or a `.json`/`.html` snapshot to **read**, via
`analyze_input` → `is_snapshot_input`):

The binary is decomposed by concern — `main()` only parses and dispatches:
`cli.rs` (the clap argument model), `analyze.rs` (input dispatch, the snapshot
path, and snapshot loading), `pipeline.rs` (the directory-analysis pipeline +
`LevelGraph` assembly, owning the `Analyzed` result), `check.rs` (`run_check`),
`report.rs` (`run_report`), `recommend.rs` (prompt/scorecard), and the `config/`
module (`model` / `load` / `ignore` / `rules` / `violations`, re-exported through
its `mod.rs` facade). `pipeline.rs` concentrates the high fan-out orchestration
behind a single caller (`analyze_input`), keeping every file's Henry-Kafura HK
low.

The shared analysis core (`analyze_input`, used by both `check` and `report`)
either reads an embedded snapshot (`.json`/`.html` input — `analyze_from_snapshot`,
which rejects `--plugin`/`--ignore` since there is nothing to analyze) or
analyzes a directory (`analyze_directory`, in `pipeline.rs`). For a directory it
loads layered config (the `config/` module — code-ranker.toml / Cargo.toml
metadata / CLI flags);
resolves the plugin name (CLI `--plugin` → config `plugin` → marker
auto-detect, all under `auto`); invokes the selected built-in plugin
(`rust` / `python` / `javascript` / `typescript`) via `plugin::analyze`, getting
a structural `api::Graph` + the plugin's `Level`s. It then runs the orchestrator
pipeline (see [§3.6 in the main DESIGN](../DESIGN.md#36-interactions--sequences)):
`code-ranker-complexity::annotate` (central metrics, while
ids are still absolute paths), `finalize_graph`, `relativize_graph` against the
detected roots, `config::apply_ignore` (language-agnostic path globs and
`dev_only_crates` via `cargo metadata`; **test-file skipping is the plugin's
job** — the CLI passes `PluginInput::ignore_tests` and each plugin drops its own
tests during the walk, since what counts as a test is language-specific), then
`annotate_cycles` +
`config::apply_cycle_rules`, `annotate_hk` and `compute_stats` over the level's
flow edges. Finally it assembles the `LevelGraph` — merging the plugin's
structural attribute specs with `code-ranker-complexity::metric_specs` and
`code-ranker-graph::coupling_specs`, then **pruning** the node/edge attribute
dictionaries, edge kinds and groups to what is actually present — and wraps it in
the snapshot's `graphs` map under `"files"`.

- **`check`** (the linter): runs the shared analysis core, then
  `config::check_violations` over cycle checks (`--cycle-rule <KIND=on|off|N>`,
  parsed into `config::CycleRule` = `Off` | `Max(n)`; a kind's cycles are reported
  only when their per-graph count exceeds its budget, so `Max(0)` is strict and
  `Max(7)` forbids the 8th) and metric thresholds (`--threshold
  <file.METRIC=N>`). No severity tiers. There is a single threshold
  scope — `file` (the files graph) — metrics written directly under
  `[rules.thresholds.file]`. `check_node_metrics` runs the per-file
  thresholds on every file node — emitting `threshold.file.<metric>`.
  Threshold values accept `_`
  separators and `K`/`M`/`G` suffixes via `config::parse_number` (CLI flags and a
  `deserialize_with` adaptor on `MetricThresholds` for quoted TOML strings); an
  invalid configuration is a hard error, never a silent fallback to defaults —
  the config structs are `#[serde(deny_unknown_fields)]`, so an unknown/stale key
  (e.g. `json-name`) fails with a field-named error rather than being ignored.
  Every `Violation` is identified
  by its dotted rule id (the config key / CLI flag, e.g. `threshold.file.loc`) and
  tagged with a concern group from the `config::RULES` catalog
  (`CYC`/`CPX`/`CPL`/`SIZ`; one entry per metric resolved by `rule_doc` — the
  trailing metric segment — with `rule_tuning` deriving the flag/config knob,
  documented in [ERRORS.md](ERRORS.md)). Prints diagnostics in the selected `--output-format`
  (`human` / `json` / `github` / `sarif`): `human` (`print_human_diagnostics`)
  renders each finding as a self-contained block (rule id, group, `where` = `id —
  path`, `issue`, `why`, `fix`, `tune`, `ref`) so it doubles as an AI prompt;
  the `ref` link and the `sarif` `helpUri` are absolute GitHub URLs (`DOCS_URL` →
  `…/blob/main/docs/code-ranker-cli/ERRORS.md#group-<g>`) so they're clickable from anywhere.
  `sarif` describes the fired rules under `tool.driver.rules`. Both machine
  formats pin each finding to a file: `github` emits `::error file=…,line=N` and
  `sarif` a `physicalLocation` (`artifactLocation` + `region.startLine`). The
  path is the violation's `{target}/rel` location stripped to a repo-relative
  path (assumes `check` ran from the repo root); the line is the cycle's breaking
  edge `line`, or `1` for a whole-file metric breach (no single line). Findings
  with no file path (a cycle whose edge couldn't be placed) stay general
  annotations / locationless results. With
  `--suggest-config`, `human` output then calls `print_current_values` — the
  current per-kind cycle counts and the per-file metric maxima
  as paste-ready `code-ranker.toml` blocks for baselining (off by default;
  machine formats omit it). Honours `--top <N>` (report only the N worst) and exits
  non-zero on any violation; `--exit-zero` suppresses the non-zero exit. Writes no
  files. With `--baseline <snapshot>` (`.json`/`.html`, loaded via `load_snapshot_any`)
  the gate switches to **relative** mode: it recomputes the baseline's violations under
  the current rules and fails only on *new* ones (those not already present under the
  same `(rule, location)` signature) — pre-existing violations are tolerated. The
  comparison yields a verdict (`degraded` if any new violations, `improved` if some were
  resolved and none added, else `neutral`), included in the diagnostics (a trailing line
  in `human`, a wrapping `{ verdict, violations }` object in `json`).
- **`report`** (`run_report`): runs the shared analysis core (analyzing the
  directory or reading the snapshot), then writes artifacts. Which formats are
  written, and where, is decided by one flag family, `--output.<fmt>[.path]`
  (`<fmt>` = `json` / `html` / `prompt` / `scorecard`), backed by `want_format`:
  a `--output.<fmt>` presence flag or a `--output.<fmt>.path` selects that
  format; for `json`/`html` the `[output.<fmt>]` config (`enabled`, else a
  configured `path`) is consulted next; if **nothing** selects anything across
  all formats, **both** `json` and `html` are written (`prompt`/`scorecard` are
  flag-only and never default). Each `.path` is a name template, or `stdout`/`-`
  to write to the stdout stream (`is_stream` / `write_artifact`). The JSON
  snapshot records `config_file` when a config was found. Names are templates
  (`render_name`) with placeholders `{project-dir}`, `{ts}`, `{git-hash}`
  (12-char short commit) and `{git-hash-N}` (first N chars) — plus `{preset}`
  for the recommendation formats. `{ts}` is the snapshot's `generated_at`
  formatted as a local timestamp — read once, not a fresh clock call per file,
  so every artifact of a run shares one stamp that matches the embedded
  `generated_at` (for a snapshot input it is the original analysis time).
  Resolved as **`--output.<fmt>.path` flag
  › `[output.<fmt>] path` config › built-in default**
  (`DEFAULT_JSON_PATH` / `DEFAULT_HTML_PATH` = `.code-ranker/{ts}-{git-hash-3}.{json,html}`;
  `DEFAULT_PROMPT_PATH` = `.code-ranker/{ts}-{git-hash-3}-{preset}.md`;
  `DEFAULT_SCORECARD_PATH` = `stdout`).
  The HTML viewer template and all assets (CSS, JS) are embedded in the binary
  via `include_str!` from `crates/code-ranker-viewer/src/assets/`, and the snapshot
  data is embedded inline in the same file as `cs-baseline` / `cs-current` JSON
  `<script>` tags (`render_html_viewer`). With `--baseline <snapshot>` the HTML
  becomes a diff view (current = this run, baseline = the file) plus a verdict,
  and its name gains a `-diff` marker before `.html`
  (`{ts}-{git-hash-3}-diff.html`); the JSON snapshot is always the current
  input (never a diff). `--baseline` accepts a `.json` snapshot or a prior
  `.html` report — the embedded snapshot is extracted via `load_snapshot_any`
  (preferring the `cs-current` tag, falling back to `cs-baseline`). `report`
  always exits `0`. The single `.html` file is fully self-contained — no
  relative-path references, no `fetch`, so it opens straight from `file://`.
  The **`prompt` / `scorecard`** formats are the refactoring-guidance outputs
  (`write_recommendations` → the `recommend` module, the console counterpart of
  the viewer's Prompt Generator): `prompt` emits the LLM Markdown for one
  principle, `scorecard` a console triage table. They share `--preset`
  (optional; default = `recommend::worst_preset`), `--severity` (`info` /
  `warning` / `auto`; repeatable for the scorecard, single for the prompt) and
  `--top`; these knobs are validated up front (rejected without a
  prompt/scorecard format, and an explicit `--index` is rejected with a hint to
  use `--top`). See [§1 `code-ranker-cli` recommendation engine](#code-ranker-cli-recommendation-engine).

**Responsibility boundary**: holds no domain logic; no analysis, no
rendering, no rules. Its sole job is argument parsing, plugin
dispatch, and artifact I/O routing.

### code-ranker-cli recommendation engine

- [x] `p2` - **ID**: `cpt-code-ranker-component-recommend`

`crates/code-ranker-cli/src/recommend.rs` is the console counterpart of the HTML
viewer's Prompt Generator (`export-popup.js`) — it derives refactoring guidance
from the snapshot's calibrated `node_attributes[*].thresholds`. It is pure
(reads a `LevelGraph` + `presets`, no I/O) and language-agnostic (it hardcodes no
metric — it reads each preset's `sort_metric` and the metric's thresholds from
the snapshot). Functions:

- `reco_for(level, metric) -> Reco` — the file nodes ranked worst-first
  (tie-broken `sloc` → `items`) plus the `warning` / `info` breach counts;
  mirrors the viewer's `recoFor`. The pseudo-metric `"cycle"` ranks the cycle
  members (by HK) and both counts equal that set's size.
- `worst_preset(level, presets)` — the principle with the most violations
  (`warning` count, tie-broken by `info`, then catalog order), used when
  `--preset` is omitted.
- `compose_prompt(level, presets, preset_id, severity, top)` — the same Markdown
  the viewer emits (`composePrompt` + `buildContent`): intent + summary +
  principle-doc link + task checklist, then the ranked offending modules, then
  the preset's connection lists (`common` / `in` / `out`, only those with edges).
- `render_scorecard(plugin, level, presets, severities, top, narrow)` — the
  console triage: a per-principle table (`warning` / `info` counts + worst
  module) and the worst modules overall (`node_breaches` ranks by selected-tier
  breach count, then HK), with a next-step hint to the worst principle.

`run_report`'s `write_recommendations` resolves the preset/severity/top, then
calls these. All of it is **advisory** — it never affects an exit code (that is
`check`'s job).

## 2. API Contracts

Interfaces are defined in [`code-ranker-cli/PRD.md`](PRD.md) (and the main PRD §7).
This section notes the implementation binding.

### Unified CLI

`cpt-code-ranker-interface-cli`

- **Technology**: Rust binary with `clap`-derived subcommands
  (`check`, `report`; no default command). Both take a polymorphic positional
  `[input]` (directory → analyze; `.json`/`.html` snapshot → read) and accept
  `--baseline <snapshot>`.
- **Location**: `crates/code-ranker-cli/src/` — `main.rs` dispatches to `cli`,
  `analyze`, `pipeline`, `check`, `report`, `recommend`, and the `config` module.
- **Output**: `report` writes a snapshot `.json` and/or an HTML viewer to the
  paths selected by `--output.<fmt>[.path]` (default
  `.code-ranker/{ts}-{git-hash-3}.{json,html}`); each `.path` is a name template
  or `stdout`/`-`, resolved as **`--output.<fmt>.path` flag › `[output.<fmt>]
  path` config › built-in default**

### Report Generator

`cpt-code-ranker-interface-report-cli`

- **Technology**: built-in Rust renderer in `code-ranker-cli`
- **Location**: `crates/code-ranker-cli/src/report.rs` (`run_report`) +
  `code-ranker-viewer` (`render_html_viewer`)
- **Template**: inline HTML string with all JS/CSS embedded; the snapshot data
  is embedded inline as `cs-baseline` / `cs-current` `<script>` tags. With
  `--baseline <snapshot>` the HTML is a baseline↔current diff named `…-diff.html`.
  The viewer assets are documented in
  [`code-ranker-viewer/DESIGN.md`](../code-ranker-viewer/DESIGN.md).

### Check / Regression Gate

`cpt-code-ranker-interface-check-cli`

- **Technology**: built-in Rust linter in `code-ranker-cli`
- **Location**: `crates/code-ranker-cli/src/check.rs` (`run_check`,
  `emit_diagnostics`)
- **Output**: diagnostics in `--output-format human|json|github|sarif` plus an
  exit code. With `--baseline <snapshot>` the gate is relative (fails only on new
  violations) and emits an `improved` / `degraded` / `neutral` verdict.

## 3. CLI Reference and Examples

The full CLI surface is documented in [CLI.md](CLI.md). The two commands are
`check` (verdict + exit code, no files) and `report` (artifacts). Both take a
polymorphic `[input]` and accept `--baseline <snapshot>`.

### Snapshots — `code-ranker report --output.json`

`report` analyzes the project (or reads a snapshot input) and writes the
snapshot to the path selected by `--output.json[.path]` (default
`.code-ranker/{ts}-{git-hash-3}.json`, e.g. `.code-ranker/20260522-112233-a3f.json`).

**Rust (built-in)**

```bash
# 1. Default snapshot only: .code-ranker/20260522-112233-a3f.json ({ts}-{git-hash-3})
code-ranker report . --plugin rust --output.json

# 2. Explicit path — for a named state
code-ranker report . --plugin rust --output.json.path=.code-ranker/before-refactor.json
```

**Python (built-in)**

```bash
# 1. Default dated snapshot
code-ranker report ~/projects/my-lib --plugin python --output.json

# 2. Explicit path for a named state
code-ranker report . --plugin python --output.json.path=.code-ranker/v2.4.0.json

# 3. Snapshot to stdout for a pipe
code-ranker report . --plugin python --output.json.path=stdout | jq '.plugin'
```

**JavaScript / TypeScript (built-in)**

```bash
# 1. Default dated snapshot
code-ranker report ~/projects/frontend --plugin javascript --output.json

# 2. Named snapshot, ignoring node_modules and dist
code-ranker report . --plugin javascript \
    --output.json.path=.code-ranker/src-only.json \
    --ignore node_modules --ignore dist
```

---

### Visualization — `code-ranker report`

With no `--output.*` flag, `report` writes the snapshot `.json` **and** the
HTML viewer together into `.code-ranker/`.

```bash
# 1. Snapshot + viewer side by side, in .code-ranker/ (default: both json + html)
code-ranker report . --plugin rust
open .code-ranker/20260522-112233-a3f.html   # default {ts}-{git-hash-3}.html

# 2. Only the HTML viewer, to docs/ for sharing with the team
code-ranker report . --plugin rust --output.html.path=docs/coupling.html

# 3. CI: artifacts into the CI folder
code-ranker report . --plugin rust \
    --output.html.path=/artifacts/code-ranker/report-pr-1234.html
```

---

### Compare against a baseline — `--baseline`

A comparison is `--baseline <snapshot>` on `report` (an HTML diff named
`…-diff.html`) or `check` (a machine verdict for CI). Because `[input]` is
polymorphic, the current side can be an already-existing snapshot, so the
comparison runs over two files without re-analyzing.

```bash
# 1. HTML diff: baseline vs the current tree
code-ranker report . --baseline .code-ranker/main.json --output.html.path=diff.html
open diff.html

# 2. HTML diff: two existing snapshots (no analysis)
code-ranker report .code-ranker/pr.json --baseline .code-ranker/main.json \
    --output.html.path=diff-refactor.html

# 3. CI: regression gate / JSON verdict for a PR comment
code-ranker check . --baseline /artifacts/code-ranker/main.json --output-format json \
    | jq '.verdict'
```

---

### Full end-to-end workflow

```bash
# Step 1+2: snapshot the baseline + open the viewer (report writes both)
code-ranker report . --plugin rust --output.json.path=.code-ranker/before.json
open .code-ranker/20260522-112233-a3f.html   # {ts}-{git-hash-3}.html, inspect the heavy nodes

# -- Step 3: the user makes changes (by hand or with an AI) --

# Step 4a: gate the change in CI against the baseline (fail only on new violations)
code-ranker check . --baseline .code-ranker/before.json --output-format json

# Step 4b: render the HTML diff against the baseline in one run
code-ranker report . --plugin rust --baseline .code-ranker/before.json
open .code-ranker/my-crate-20260522-112233-diff.html   # --baseline names it -diff.html; a diff view + verdict
```

---

**Related docs**: [PRD.md](PRD.md) (CLI requirements) ·
[CLI.md](CLI.md) (full flag reference) · [config.md](config.md) ·
[ERRORS.md](ERRORS.md) · main [DESIGN](../DESIGN.md) ·
[`code-ranker-viewer/DESIGN.md`](../code-ranker-viewer/DESIGN.md)

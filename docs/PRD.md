# PRD — Code Ranker

<!-- toc -->

- [1. Overview](#1-overview)
  - [1.1 Purpose](#11-purpose)
  - [1.2 Background / Problem Statement](#12-background--problem-statement)
  - [1.3 Goals (Business Outcomes)](#13-goals-business-outcomes)
  - [1.4 Glossary](#14-glossary)
- [2. Actors](#2-actors)
  - [2.1 Human Actors](#21-human-actors)
  - [2.2 System Actors](#22-system-actors)
- [3. Operational Concept & Workflow](#3-operational-concept--workflow)
- [4. Scope](#4-scope)
  - [4.1 Priority Tiers](#41-priority-tiers)
  - [4.2 Out of Scope (All Versions)](#42-out-of-scope-all-versions)
- [5. Functional Requirements](#5-functional-requirements)
  - [5.1 Plugin System — Step 1](#51-plugin-system--step-1)
  - [5.2 Visualization Reports — Step 2](#52-visualization-reports--step-2)
  - [5.3 Baseline Comparison — Step 4](#53-baseline-comparison--step-4)
- [6. Non-Functional Requirements](#6-non-functional-requirements)
  - [6.1 NFR Inclusions](#61-nfr-inclusions)
  - [6.2 NFR Exclusions](#62-nfr-exclusions)
- [7. Public Interfaces](#7-public-interfaces)
  - [7.1 Code Ranker Unified CLI](#71-code-ranker-unified-cli)
  - [7.2 Plugin Model](#72-plugin-model)
  - [7.3 Graph JSON Schema](#73-graph-json-schema)
- [8. Use Cases](#8-use-cases)
  - [UC-001 Analyze Rust Workspace Offline](#uc-001-analyze-rust-workspace-offline)
  - [UC-002 Before/After Refactoring Comparison](#uc-002-beforeafter-refactoring-comparison)
  - [UC-003 CI Structural Gate on Pull Request](#uc-003-ci-structural-gate-on-pull-request)
- [9. Acceptance Criteria](#9-acceptance-criteria)
- [10. Dependencies](#10-dependencies)
- [11. Assumptions](#11-assumptions)
- [12. Risks](#12-risks)

<!-- /toc -->

> **Component PRDs.** This is the product PRD — overview, actors, the
> plugin/extraction layer, the graph model and JSON schema, and the
> cross-cutting requirements. The two consumer components have their own PRDs:
> the command-line interface in [`code-ranker-cli/PRD.md`](code-ranker-cli/PRD.md)
> and the offline HTML viewer in
> [`code-ranker-viewer/PRD.md`](code-ranker-viewer/PRD.md).

## 1. Overview

### 1.1 Purpose

Code Ranker is a polyglot structural-analysis platform that (1) extracts
a file-level dependency graph from local codebases — with third-party
libraries recorded as depth-1 external nodes — via a pluggable analyzer
system, (2) visualizes the resulting graph as an interactive offline
HTML report with per-file complexity and coupling metrics, and (3)
tracks and reports architectural drift between two captured snapshots.

### 1.2 Background / Problem Statement

Developers working on large or aging codebases face two recurring
problems: they cannot see the full picture of structural coupling in a
machine-readable form, and they cannot measure whether a refactoring
actually improved that coupling. Existing tools are fragmented,
language-specific, non-exportable, or single-level.

**Target Users**:

- Developers working on local projects or monorepos (Rust at P1;
  Python, Go, JavaScript and others at P3)
- Tech leads and architects planning or validating refactors
- CI pipelines enforcing structural policies across pull requests

**Key Problems Solved**:

- No unified file dependency graph across languages in a portable
  artifact format
- No before/after coupling comparison that quantifies whether a
  refactoring improved the architecture
- Refactoring decisions rely on intuition rather than measurable data

### 1.3 Goals (Business Outcomes)

**Success Criteria**:

- Extract the file graph for a 50k-LOC Rust workspace in under 30
  seconds (typically a few seconds — no rust-analyzer)
- Generate an HTML visualization report from JSON artifacts in under
  5 seconds
- Generate a baseline-vs-current diff report between two snapshots in under 5 seconds
- Works fully offline — no network access, no LLM calls required

**Capabilities**:

- Built-in analyzer system: each language provides a plugin compiled
  into the binary that emits a standard JSON artifact
- File-graph visualization with per-file complexity + coupling metrics,
  external dependency nodes, and node sorting
- Snapshot diff for before/after refactoring quantification

### 1.4 Glossary

| Term | Definition |
|------|------------|
| Plugin | A built-in language analyzer (`rust`, `python`, or `javascript`) compiled into the `code-ranker` binary that analyzes a workspace and produces a single file graph in-process |
| Snapshot | A single self-contained JSON file combining metadata and the one `files` graph produced by a single analysis run |
| Graph | A directed graph whose nodes are source files (`file`) and third-party libraries (`external`), and whose edges are file dependencies (`uses`, `reexports`) |
| External node | A third-party library recorded at depth 1 — one node per library (`ext:<name>`), never expanded into its internals |
| Node weight | The coupling metric for a file: sum of its incoming and outgoing internal edge counts |
| Baseline / Current | The two sides of a comparison: **baseline** is the reference snapshot (`--baseline`), **current** is the positional `[input]` (analyzed now, or a snapshot) |
| Diff | A structured comparison of baseline vs current: nodes and edges added, removed, or affected |
| Verdict | The overall direction of a comparison: `improved`, `degraded`, or `neutral` |

## 2. Actors

### 2.1 Human Actors

#### Developer

**ID**: `cpt-code-ranker-actor-developer`

**Role**: Runs the plugin on a local workspace, views the HTML report,
modifies the codebase, then compares before/after snapshots.

**Needs**: Fast offline-capable tools with no mandatory LLM or network
dependency; single-command invocation per step.

#### Tech Lead

**ID**: `cpt-code-ranker-actor-tech-lead`

**Role**: Reviews HTML reports to evaluate module boundaries and
coupling hotspots; reviews diff reports to validate that refactors
improved the architecture.

**Needs**: Sortable coupling view; clear before/after delta with
magnitude; self-contained HTML that can be shared without tooling.

### 2.2 System Actors

#### CI Pipeline

**ID**: `cpt-code-ranker-actor-ci`

**Role**: Runs the plugin at pull-request time, stores snapshot
artifacts, gates the branch against the base-branch snapshot with
`check --baseline`, and attaches the `report --baseline` diff to the
pull request.

**Needs**: Non-interactive execution; deterministic artifact output;
structured exit codes.

#### PR Reviewer

**ID**: `cpt-code-ranker-actor-pr-reviewer`

**Role**: Views the diff HTML report attached to a pull request to
evaluate architectural impact without a local toolchain.

**Needs**: Self-contained HTML; clear color-coded coupling changes;
summary verdict readable in under one minute.

## 3. Operational Concept & Workflow

The platform is organized as four sequential steps. Steps 1, 2, and 4
are implemented by Code Ranker; Step 3 is the user's own modification
activity and is deliberately outside Code Ranker's scope.

```
Step 1 ─ Extract   →   Step 2 ─ Visualize   →   Step 3 ─ Modify   →   Step 4 ─ Compare
(code-ranker report)       (code-ranker report)          (User / AI)        (report --baseline)
outputs JSON            outputs HTML              (we wait)             outputs HTML
```

**Step 1 — Graph Extraction (Plugin)**: A language-specific built-in
plugin analyzes the workspace in-process when `code-ranker report` runs,
which writes a single JSON snapshot containing the file dependency graph
(with third-party libraries as depth-1 external nodes). No network access
or LLM is required. The snapshot may be stored as a CI artifact for Step
4. (For a pure CI gate that only lints and writes no files,
`code-ranker check` runs the same analysis without producing a snapshot.)

**Step 2 — Visualization (Report Generator)**: The same `code-ranker report`
run that analyzes the workspace also produces a self-contained offline
HTML viewer with interactive graph visualization and sorting by node
weight — snapshot and HTML are emitted together. No network access or
LLM is required.

**Step 3 — Modification (User Activity)**: The user reads the report,
decides what to refactor (manually or with AI assistance), and modifies
the codebase. Code Ranker does not participate in this step.

**Step 4 — Baseline Comparison**: After modification, the user re-runs
Step 1 to capture the current state (or analyzes it live). Passing the
earlier snapshot as `--baseline` compares the two: `code-ranker report
. --baseline <snapshot>` produces a baseline↔current diff HTML report
with a verdict, and `code-ranker check . --baseline <snapshot>` produces a
machine-readable verdict and gates only on *new* violations. Because the
positional input is polymorphic, `--baseline` can also compare two
existing snapshots without re-analyzing. No network access or LLM is
required.

## 4. Scope

### 4.1 Priority Tiers

#### P1 — Required for Initial Release

| Step | Scope |
|------|-------|
| Step 1 | Rust plugin only; single file-level JSON graph with external dependency nodes; no AI prompts; no CI integration |
| Step 2 | Offline HTML report with file-graph visualization and node sorting by weight |
| Step 4 | `report --baseline` offline HTML diff report and `check --baseline` machine-readable verdict comparing two snapshots |

#### P2 — Follow-On

| Step | Scope |
|------|-------|
| Step 1 | AI prompt generator (heaviest nodes → LLM prompt); CI artifact integration |
| Step 2 | CI artifact hosting |
| Step 4 | CI integration; baseline-comparison artifacts for PR review automation |
| Distribution | Multi-ecosystem binary distribution: single pre-compiled `code-ranker` binary per platform published via thin wrappers to PyPI (`pip install code-ranker`), npm (`npm install -g @code-ranker/cli`), and GitHub Releases |

#### P3 — Future

| Step | Scope |
|------|-------|
| Step 1 | Additional language plugins: Python, JavaScript, Go, C#, PHP; framework-specific plugins (Django, WordPress, etc.) with domain-specific metadata |
| Step 2 | AI prompt generation for principles review using the `principles/` corpus (per-language: `principles/rust/`, `principles/python/`, `principles/typescript/`) |

### 4.2 Out of Scope (All Versions)

- Expanding external dependencies (registry/git/npm/pypi packages
  appear as opaque depth-1 nodes; their internals are never read)
- Call-graph analysis (no `Calls` edges, no semantic call resolution).
  Per-function **metrics** are available as an opt-in `functions` level
  (`[levels] functions`), but functions are metric nodes only — they carry no
  edges and form no call graph.
- Automated code modification or refactoring suggestions
- IDE/LSP integration and interactive visualization
- Cross-language linkage (FFI/RPC boundaries are leaves)
- Database or service deployment; no server component

## 5. Functional Requirements

### 5.1 Plugin System — Step 1

> **Moved.** The unified entry-point command (`cpt-code-ranker-fr-unified-cli`)
> — the `check` / `report` subcommands, the polymorphic `[input]`, and the
> `--output.<fmt>.path` artifact selection — is specified in
> [`code-ranker-cli/PRD.md`](code-ranker-cli/PRD.md). The snapshot it writes is a
> single self-contained `.json` file; its schema is `cpt-code-ranker-fr-snapshot-meta`
> below.

#### Snapshot File Format

- [x] `p1` - **ID**: `cpt-code-ranker-fr-snapshot-meta`

Each `code-ranker report` run produces a single `.json` file
(`schema_version: "2"`). The file combines metadata and the `graphs` map (one
entry per analysis level — today only `files`) in one document. Each level
bundles its semantics dictionaries with the structural graph and computed data
(see §7.3 for the full shape):

```json
{
  "schema_version": "2",
  "generated_at": "2026-05-22T11:22:33Z",
  "command": "code-ranker report /path/to/axum-api --plugin rust",
  "workspace": "/Users/alice/projects/code-ranker",
  "target":    "/Users/alice/projects/axum-api",
  "plugin": "rust",
  "config_file": "/Users/alice/projects/axum-api/code-ranker.toml",
  "versions": { "code-ranker": "1.0.0-alpha.4", "rustc": "1.78.0" },
  "roots": {
    "registry": "/Users/alice/.cargo/registry/src/index.crates.io-abc123",
    "target":   "/Users/alice/projects/axum-api"
  },
  "git": {
    "branch": "refactor/split-handlers",
    "commit": "a3f9c21b4d5e",
    "dirty_files": 4,
    "origin": "git@gitlab.example.com:team/axum-api.git"
  },
  "timings": [
    { "stage": "rust",       "ms": 600, "detail": "547 nodes from 512 files" },
    { "stage": "complexity", "ms": 700, "detail": "512 nodes annotated" },
    { "stage": "projection", "ms": 5,   "detail": "nodes=550 edges=1320" }
  ],
  "graphs": {
    "files": {
      "edge_kinds": { ... }, "node_attributes": { ... },
      "edge_attributes": { ... }, "attribute_groups": { ... },
      "nodes": [...], "edges": [...], "cycles": [...], "stats": { ... }
    }
  }
}
```

Top-level fields:

- `schema_version` — `"2"` (the generic property-graph format)
- `generated_at` — ISO-8601 timestamp
- `command` — full command line as typed
- `workspace` — absolute path to the directory where `code-ranker` was invoked
- `target` — absolute path to the analyzed project
- `plugin` — resolved built-in plugin name (`rust` / `python` / `javascript` / `typescript`)
- `config_file` — absolute path of the config file used; omitted when none was found
- `versions` — `code-ranker` semver at minimum; the Rust plugin adds `rustc`
- `roots` — named system prefixes used to relativize node ids/paths
  (`roots[name] + "/" + rest` → absolute path). Roots that did not shorten any
  path are pruned, so a JS/TS/Python snapshot carries only `{target}` and a Rust
  snapshot `{target}` + `{registry}`
- `git` — `branch`, `commit` (12-char short SHA), `dirty_files`, and `origin`;
  the whole block omitted if not a git repository. Each field is read from `git`
  but can be overridden with a `--git.<field>` flag (for CI, where a detached
  checkout otherwise reports the branch as `HEAD` and job-written files inflate
  the dirty count); when `branch`, `commit`, and `dirty-files` are all supplied,
  `git` is not invoked at all
- `timings` — per-stage wall-clock timings (`stage`, `ms`, `detail`), in
  execution order; omitted when empty
- `graphs` — a map `level_name → level`. `files` is always present; `functions`
  (per-function metric nodes) is present when `[levels] functions` is enabled. Each
  level carries the four semantics dictionaries (`edge_kinds`,
  `node_attributes`, `edge_attributes`, `attribute_groups`) plus `nodes`,
  `edges`, `cycles`, `stats`, and a computed `ui` block (column/sort/size order
  and an optional `grouping` telling the viewer how to cluster nodes — e.g.
  `{ "key": "crate" }`)

`code-ranker report` and `code-ranker check` (with `--baseline`) read
snapshot files and embed the top-level metadata in the generated HTML as a
"Snapshot info" panel.

**Rationale**: One file per snapshot is simpler to copy, archive, and
pass between tools than a directory of four files. The timestamp in the
filename makes snapshots self-organizing without a registry.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`,
`cpt-code-ranker-actor-ci`

#### Plugin Selection

- [x] `p1` - **ID**: `cpt-code-ranker-fr-plugin-discovery`

The plugins are built into the `code-ranker` binary; the valid plugin names are
`rust`, `python`, `javascript`, and `typescript` (JS and TS are **separate**
plugins, no aliases). The `--plugin <name>` option (on `check` / `report`)
selects one of these built-ins. There is no external or dynamic plugin loading.

The plugin is resolved in the following order, stopping at the first match:

1. **Explicit `--plugin <name>`** on the command line (any value other
   than `auto`) wins.
2. Otherwise the **`plugin` key in the config file** (`code-ranker.toml` /
   `Cargo.toml#metadata.code-ranker`), if set and not `auto`.
3. Otherwise **auto-detect by project markers** in the workspace root
   (`Cargo.toml` → `rust`; `pyproject.toml` / `setup.py` / `setup.cfg`
   → `python`; `package.json` → `javascript`; `tsconfig.json` → `typescript`).
   A project carrying both `package.json` and `tsconfig.json` is ambiguous and
   requires an explicit `--plugin`.

If `--plugin` resolves to a name that is not a built-in, or if `auto`
detection finds more than one marker or none, the analyzing command MUST
exit non-zero with a human-readable error naming the valid plugins and
asking for an explicit `--plugin`.

**Rationale**: Built-in-only selection keeps the tool a single binary with
nothing to install: every supported language ships compiled in, and adding
a language means adding a built-in plugin rather than wiring up an external
process.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-ci`

#### Rust Plugin

- [x] `p1` - **ID**: `cpt-code-ranker-fr-rust-plugin`

The platform MUST ship a built-in Rust plugin (`--plugin rust`) for Cargo
workspaces. The plugin MUST:

- Derive the Rust module graph from `cargo metadata` and `mod`
  declarations / `use` statements via syntactic analysis (`syn` crate),
  then **collapse it to a file graph**: every `.rs` file becomes one
  `File` node, inline `mod {}` modules fold into their file, and
  `use` / `pub use` edges are re-pointed to the owning files. `mod foo;`
  declarations are emitted as `Contains` edges that are **kept** in the
  JSON as structural ownership metadata but not drawn and not counted in
  fan_in / HK / cycles (information flow)
- Classify each crate as local vs. external; external crates collapse to
  `External` library nodes (`ext:<name>`) recorded at depth 1, never
  expanded; edges into them are flagged `external: true`. Each `External`
  node carries the resolved `version` and its cargo-cache `path` (from
  `cargo metadata`). A dependency on another **local workspace crate** is
  resolved **submodule-precise**: `other_crate::sub::Item` walks that crate's
  library module index to the file that owns `Item` (→ its `sub.rs`); a path
  that stops at a crate-root item falls back to the root file (`lib.rs` /
  `main.rs`). A registry crate (no local library index) collapses to its
  `External` node. Resolution is **re-export-aware**, intra- and cross-crate: a
  `crate::X` / `super::X` / `other_crate::X` whose trailing segment is
  `pub use`-re-exported by the resolved module follows the re-export chain to the
  file that **defines** `X`, not the facade (`lib.rs` / `mod.rs`) — so a widely
  re-exported type lands on its defining file, not a 17-line crate-root hub.
  Module ids are namespaced **per target**, so a package
  with a library and a same-named binary (`bat` lib + `bat` bin) does not collide
  their roots (which would mis-resolve library `crate::X` onto the binary's
  `main.rs`). Each file node records its owning crate (per-target) as a `crate`
  attribute
- Capture **bare qualified paths** in expressions/types (`commands::run()`,
  `other_crate::item`, `crate::a::Alpha` with no `use`), resolved the same
  way as `use`, so both intra-crate and cross-crate dependencies referenced
  only by qualified path are not lost
- Capture **qualified paths inside `#[derive(...)]`** (e.g.
  `#[derive(serde::Serialize)]` with no `use serde`) so a crate used only
  through a derive still gets an edge, and honour **`#[path = "…"]`** on a
  `mod` (resolved relative to the declaring file's directory) so a module whose
  backing file sits at a non-default location is walked and its edges captured
- Emit a per-file **`unsafe`** count as a structural node attribute: the number
  of `unsafe { }` blocks plus `unsafe fn` / `impl` / `trait` declarations in the
  file's production code (test items excluded, like `sloc`). It is a syntactic
  count (`unsafe` inside a macro body is not seen; not type-checked) and is
  omitted when zero
- NOT emit a function-level call graph (no `Calls` edges, no
  rust-analyzer / `ra_ap_*` dependency); analysis runs in seconds
- Emit **structure only** (file + external nodes, `uses`/`contains`/`reexports`/`super`
  edges). The downstream pipeline then enriches every file node: per-file
  complexity metrics (cyclomatic, cognitive, Halstead, maintainability index, LOC
  variants) are produced by each plugin's `metrics()` step — its in-tree engine
  measures tier-1 counts (`MetricInputs`) and the `code-ranker-graph` registry
  evaluates the declarative tier-2 formulas (`metrics/builtin.toml`) via
  `write_metrics`;
  dependency cycles (Kosaraju SCC over flow edges) annotated as a `cycle` node
  attribute (`mutual` | `chain`) with `CycleGroup` entries, with
  any SCC that spans more than one crate dropped (Rust forbids circular crate
  dependencies); `reexports` is **non-flow** (a `pub use` facade is not a
  dependency), so it is excluded from cycles **and** fan-in / HK; on the map it is
  drawn **dashed** (revealed on a leaf-node hover), exactly like `contains`. A glob `use` that pulls in an **enclosing** module's
  namespace (`use super::*`, `use crate::<ancestor>::*`) is emitted as the
  separate **non-flow** kind `super` rather than `uses`: it is scope-sugar (a
  module split across files reaching back into itself), not a real outward
  dependency, so — like `contains`/`reexports` — it is kept in the data but
  excluded from cycles / fan-in / fan-out / HK; on the map it is drawn **dashed**
  (revealed on a leaf-node hover). A glob that pulls
  in a *child* module, or any **named** import of a parent item
  (`use crate::parent::Item`, `super::Item`), stays a real `uses` edge. And
  Henry-Kafura (`HK = sloc × (fan_in × fan_out)²`) — all written into the node's
  flat `attrs`. Edges to external nodes are excluded from `fan_in`/`fan_out`/`hk`
  and counted in `fan_out_external` instead. The Rust plugin additionally
  supplies language-calibrated `thresholds()` for `hk`/`sloc`/`fan_out`/`items`,
  and extends the Prompt-Generator catalog via its `presets()` hook with four
  metric-lens presets — `HK`, `SLOC`, `FANIN`, `FANOUT` — that rank modules by a
  single coupling/size metric (`hk`/`sloc`/`fan_in`/`fan_out`) rather than a
  design principle, documented under `principles/rust/`

**Rationale**: Rust is the primary use-case for the initial release.
The `code-ranker-plugin-rust` crate (cargo metadata + `syn`, including the
module→file collapse pass) implements this plugin. Removing rust-analyzer
makes the Rust path fast and the binary light.

**Actors**: `cpt-code-ranker-actor-developer`

#### File-Level Graph

- [x] `p1` - **ID**: `cpt-code-ranker-fr-file-graph`

Every plugin MUST emit a single directed **file graph**. Nodes are
`File` (project source files, carrying all per-file metrics) and
`External` (third-party libraries at depth 1, one node per library,
never expanded). Edges are `uses` and `reexports` between files, plus
`uses` edges flagged `external: true` from a file to a library node.
There is no module or function graph in the snapshot.

For Rust, the file graph is derived by collapsing the module graph (see
`cpt-code-ranker-fr-rust-plugin`); for Python/JS/TS it is built directly
from import resolution.

**Rationale**: The file is the universal unit across languages and the
level at which most refactoring and ownership decisions are made. A
single graph keeps the artifact small and the model consistent across
plugins.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`

#### Embedded Static Asset Tracking (P2)

- [ ] `p2` - **ID**: `cpt-code-ranker-fr-rust-embedded-assets`

The Rust plugin SHOULD track files embedded into the binary via macros
(`include_bytes!`, `include_str!`, `include!`, `sqlx::query_file!`, etc.)
as `File` nodes in the graph, with a dedicated `Embeds` edge kind from
the referencing module to the embedded file.

Currently these dependencies are completely invisible: a module that
embeds a TLS certificate or a SQL migration file shows no outgoing edges
to those assets, making the structural graph incomplete.

**Implementation**: In `walk_items`, detect `Item::Macro` nodes whose
path matches known embedding macros, parse the string literal argument
as a relative path, resolve it against the enclosing file's directory,
and emit a `File` node + `Embeds` edge.

**Rationale**: Embedded assets are real compile-time dependencies. SQL
files, certificates, HTML templates, and proto-generated sources that
are `include!`-ed affect correctness and security but are invisible to
structural analysis today.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`

#### Language Plugins (P3)

- [x] `p3` (Python shipped) - **ID**: `cpt-code-ranker-fr-lang-plugins`

The platform SHOULD support additional built-in language plugins for
Python, Go, JavaScript, C#, and PHP, each emitting a conformant file
graph. A built-in plugin MAY attach framework-specific information via
the `metadata` object on nodes/edges (e.g. Django, WordPress concepts);
such extensions MUST be backward-compatible with the base schema and keep
`kind` as `file` / `external`.

**Python plugin** (`--plugin python`) is shipped as a built-in in
`code-ranker-cli`. It uses `tree-sitter-python` to emit one `File` node
per `.py` file and resolve imports: imports of project files become
file→file `uses` edges (including `__init__.py` package imports pointing
at the package file), and imports that do not resolve to a project file
become `External` library nodes (`ext:<top-level-package>`, e.g.
`numpy`) reached by a `uses` edge flagged `external: true`. Per-file
complexity metrics (cyclomatic, cognitive, Halstead, MI, LOC, functions,
nexits, nargs) are annotated on each `File` node by the plugin's `metrics()`
step, which runs its in-tree `tree-sitter-python` engine (`python_ts`) and writes
the result via `code_ranker_graph::write_metrics`.

**JavaScript / TypeScript plugin** (`--plugin javascript`) is shipped as a
built-in in `code-ranker-cli`; one plugin handles `.js`, `.jsx`, `.ts`, and
`.tsx`. It uses `tree-sitter-javascript` and `tree-sitter-typescript` to
emit one `File` node per source file and resolve ES `import` statements
and CommonJS `require()` calls: imports of project files become file→file
`uses` edges, and bare-package imports become `External` library nodes
(`ext:<package>`, one per top-level package — `react`, `@scope/pkg`)
reached by a `uses` edge flagged `external: true`. Per-file complexity
metrics are annotated on each `File` node (whole-file aggregate covering
all functions, arrow functions, and methods).

Go, C#, PHP plugins remain future work (P3 deferred).

**Rationale**: The JSON contract and consumer tools are language-agnostic;
adding a new language plugin does not require changes to the report or
diff layer.

**Actors**: `cpt-code-ranker-actor-developer`

> **Moved.** The layered configuration system (`cpt-code-ranker-fr-config`) —
> source priority, `code-ranker.toml` keys, the CLI flags, rule ids and
> self-contained diagnostics — is specified in
> [`code-ranker-cli/PRD.md`](code-ranker-cli/PRD.md). See also
> [`code-ranker-cli/config.md`](code-ranker-cli/config.md) for the full schema and
> [`code-ranker-cli/ERRORS.md`](code-ranker-cli/ERRORS.md) for the rule reference.

### 5.2 Visualization Reports — Step 2

> **Moved.** The visualization / HTML report requirements are specified in
> [`code-ranker-viewer/PRD.md`](code-ranker-viewer/PRD.md): HTML report generation
> (`cpt-code-ranker-fr-html-report`), node sorting by weight
> (`cpt-code-ranker-fr-node-sorting`), the AI Prompt Generator
> (`cpt-code-ranker-fr-ai-prompts`, whose CLI counterpart is the `recommend`
> module), and principles-based prompt generation
> (`cpt-code-ranker-fr-principles-prompts`).

### 5.3 Baseline Comparison — Step 4

> **Moved — split across the two component PRDs.** The interactive HTML diff
> viewer (`cpt-code-ranker-fr-graph-diff`, `cpt-code-ranker-fr-diff-html-report`)
> is specified in [`code-ranker-viewer/PRD.md`](code-ranker-viewer/PRD.md). The
> machine gate and structured verdict (`cpt-code-ranker-fr-compare`,
> `cpt-code-ranker-fr-diff-text-report`, `cpt-code-ranker-fr-ci-diff`) are
> specified in [`code-ranker-cli/PRD.md`](code-ranker-cli/PRD.md). The diff itself
> is computed browser-side from the two embedded snapshots; the relative gate
> (`check --baseline`) is rule-based, not count-based.

## 6. Non-Functional Requirements

### 6.1 NFR Inclusions

#### Offline Operation

- [x] `p1` - **ID**: `cpt-code-ranker-nfr-offline`

All P1 components (Rust plugin, `code-ranker check`, `code-ranker report`,
and `--baseline` comparisons) MUST operate without network access. External resources (CDNs, APIs, LLM
endpoints) are forbidden at P1. All JavaScript and CSS dependencies in
generated HTML MUST be bundled into the `code-ranker` binary as embedded
assets; no CDN or external resource references in generated HTML.

**Threshold**: Zero outbound network calls during any P1 operation.

**Rationale**: Workspaces may be on air-gapped machines, private CI
runners, or laptops without connectivity. Offline-first is a hard
requirement shared by all three steps.

#### Performance

- [x] `p1` - **ID**: `cpt-code-ranker-nfr-performance`

The Rust plugin MUST complete graph extraction for a 50k-LOC workspace
in ≤ 30 seconds wall-clock on a modern developer laptop (8-core, 16 GB
RAM, SSD), measured cold-cache. The `code-ranker report` and `code-ranker check`
subcommands MUST each complete in ≤ 5 seconds for graphs with up to
10,000 nodes (including a `--baseline` comparison).

**Threshold**: ≤ 30 s for the plugin at 50k LOC; ≤ 5 s for each
subcommand at 10k nodes.

**Rationale**: Interactive use requires sub-minute turnaround.

#### Artifact Portability

- [x] `p1` - **ID**: `cpt-code-ranker-nfr-portability`

JSON snapshot artifacts MUST conform to the Graph JSON Schema
(`schema_version: "2"`) and MUST be readable by the report generator and
baseline comparison without migration within a major schema version. Generated
HTML reports MUST open correctly in Chrome, Firefox, and Safari without
installation.

**Threshold**: Zero schema-migration failures within a major version.

**Rationale**: Artifacts stored as CI artifacts must remain readable
across plugin and tool version bumps within a major version.

#### Metric Accuracy

- [ ] `p1` - **ID**: `cpt-code-ranker-nfr-metric-accuracy`

Every metric value MUST equal the true count of what it measures — no false
positives and no false negatives — for every metric the tool reports. The number
a consumer reads must mean exactly what the metric claims.

**Threshold**: For every metric, the emitted value matches ground truth: zero
false positives, zero false negatives, correct magnitude.

**Rationale**: The product output is an anomaly shortlist a human or AI agent
acts on, so a silently miscounted metric is a silently wrong ranking — and the
failure hides because the number still looks plausible. Trustworthy triage
requires every count to mean exactly what it claims.

### 6.2 NFR Exclusions

- **Accessibility**: Out of scope for v1.0.
- **Internationalization**: English-only in v1.0.
- **Regulatory Compliance**: Not applicable — the tool reads local
  source files only and produces no personal or regulated data.

## 7. Public Interfaces

### 7.1 Code Ranker Unified CLI

- [x] `p1` - **ID**: `cpt-code-ranker-interface-cli`

> **Moved.** The unified CLI interface (`cpt-code-ranker-interface-cli`) — the
> `check` / `report` subcommands, the polymorphic `[input]`, global options,
> exit codes, and the breaking-change policy — is specified in
> [`code-ranker-cli/PRD.md`](code-ranker-cli/PRD.md). The full flag reference is
> in [`code-ranker-cli/CLI.md`](code-ranker-cli/CLI.md).

### 7.2 Plugin Model

- [x] `p1` - **ID**: `cpt-code-ranker-interface-plugin-binary`

**Type**: Built-in, in-process analyzer

**Stability**: unstable (pre-1.0)

Plugins are compiled into the `code-ranker` binary and run **in-process**
when a command analyzes a workspace (`code-ranker check` / `code-ranker
report`). The plugins are `rust`, `python`, `javascript`, and `typescript`,
selected with `--plugin <name>` (see `cpt-code-ranker-fr-plugin-discovery`).
There is no subprocess invocation, no external plugin binary, and no
external/dynamic plugin loading.

Each plugin implements the `LanguagePlugin` trait (`code-ranker-plugin-api`) as a
**pure parser**: `analyze(workspace, level, input)` returns a structural `Graph`
(nodes + edges, **no metrics**), and `levels()` declares the level's semantics
dictionaries. When `input.ignore_tests` is set (`[ignore] tests`, **on by
default**), the plugin skips its own test files during the walk — what counts as
a test is language-specific (Rust `#[cfg(test)]` modules, Python
`test_*.py`/`tests/`, JS/TS `*.test.*`/`__tests__`), so the detection
(`is_test_path`) lives in the plugin, not the CLI. Per-language complexity
metrics are written by the plugin's `metrics()` step (running the matching
in-tree language engine and writing via `code_ranker_graph::write_metrics` — no
central by-extension dispatcher), while the
language-agnostic derived data (cycles / Henry-Kafura / stats in
`code-ranker-graph` over the level's flow edges) is computed centrally; all are
written into node attributes by id, and the orchestrator assembles the snapshot.
Adding a language means adding a built-in plugin crate (implementing `analyze` +
`metrics`) and one line in `plugin::registry()`.

### 7.3 Graph JSON Schema

- [x] `p1` - **ID**: `cpt-code-ranker-interface-graph-schema`

**Type**: Data format (JSON)

**Stability**: unstable (pre-1.0)

A **generic property graph**: free-form string `kind` on nodes and edges, and a
flat free-form attribute map on each. No fixed enums, no nested metric objects.
Each level carries semantics dictionaries describing its vocabulary so a consumer
can render any language/metric set without hardcoding names.

**Top-level shape** (full snapshot file):

```json
{
  "schema_version": "2",
  "generated_at":   "<ISO-8601>",
  "command":        "<full command line>",
  "workspace":      "<absolute-path>",
  "target":         "<absolute-path>",
  "plugin":         "<plugin-id>",
  "versions":       { "code-ranker": "1.0.0-alpha.4", "rustc": "1.78.0" },
  "roots":          { "target": "<abs>", "registry": "<abs>" },
  "git":            { "branch": "main", "commit": "a3f9c21b4d5e", "dirty_files": 0, "origin": "git@…:team/proj.git" },
  "timings":        [ { "stage": "rust", "ms": 0, "detail": "…" }, … ],
  "graphs": {
    "files": {
      "edge_kinds":       { "<kind>": { "flow": true, "label": "…", "description": "…" } },
      "node_attributes":  { "<key>": { "value_type": "int|float|str|bool", "label": "…",
                                       "name": "…", "short": "…", "description": "…",
                                       "formula": "…", "calc": "<eval expr>",
                                       "direction": "higher_better|lower_better",
                                       "abbreviate": true, "group": "<group?>",
                                       "thresholds": { "info": N, "warning": N } } },
      "edge_attributes":  { "<key>": { "value_type": "…", "label": "…" } },
      "attribute_groups": { "<group>": { "label": "…", "description": "…" } },
      "node_kinds":       { "<kind>": { "label": "…", "plural": "…", "fill": "#…", "stroke": "#…", "external": true } },
      "cycle_kinds":      { "<kind>": { "label": "…", "description": "…" } },
      "ui":               { "default_sort": "…", "sort_metrics": [...], "size_metrics": [...],
                            "card_metrics": [...], "columns": [...], "summary_metrics": [...] },
      "nodes": [...], "edges": [...], "cycles": [...], "stats": { ... }
    }
  },
  "presets": [ { "id": "ADP", "label": "ADP", "title": "…", "prompt": "…",
                 "doc_url": "…", "sort_metric": "cycle", "connections": ["common","out"] } ]
}
```

`graphs` is a map `level_name → level` (`files`, plus `functions` when enabled).
The dictionaries are pruned to the keys/kinds/groups actually present at that
level, and the `ui` block is computed by the orchestrator from the present
attributes. Every metric's label / name / formula / live-`calc` / direction /
threshold lives in `node_attributes`, and the Prompt-Generator principles live in
top-level `presets`, so the **viewer hardcodes no metric, kind, threshold or
prompt by name** — it renders entirely from this data (see DESIGN §3.2 HTML
assets). Optional `AttributeSpec` fields are omitted when absent.

Tier-2 metrics are **declarative data**, not hardcoded computation: each is a CEL
`formula` plus spec in `code-ranker-graph/metrics/builtin.toml`, evaluated by the
registry engine over the per-language tier-1 counts. A user adds or overrides
metrics under `[metrics.<key>]` in config (node-scope per-unit formulas, or
graph-scope `agg(…)` aggregates emitted into `stats`) with no code change; only
tier-1 counting and the graph algorithms (`fan_in`/`fan_out`/`cycle`) are in Rust.

**Node shape** — `id`, `kind`, `name`, optional `parent`, plus flat attributes:

```json
{ "id": "{target}/src/foo.rs", "kind": "file", "name": "foo.rs",
  "visibility": "public", "loc": 48, "sloc": 36, "lloc": 12, "cloc": 4, "blank": 6, "tloc": 2,
  "cyclomatic": 3, "cognitive": 2, "exits": 2, "args": 3, "unsafe": 1,
  "mi": 78.4, "mi_sei": 52.1, "length": 87, "vocabulary": 23, "volume": 312.5,
  "effort": 4820, "time": 267.8, "bugs": 0.104,
  "fan_in": 4, "fan_out": 2, "fan_out_external": 1, "hk": 1344, "cycle": "mutual" }
```

```json
{ "id": "ext:serde", "kind": "external", "name": "serde",
  "external": true, "version": "1.0.228", "path": "{registry}/serde-1.0.228" }
```

`kind` is `"file"` (a project source file — **its id IS its relativized path**,
no `file:` prefix, and it carries no `path` attribute) or `"external"` (a
3rd-party library, id `ext:<name>`, marked `external: true`; for Rust it also
carries `version` and `path` = the crate's cargo-cache directory). All
attributes are **flat** and a metric is **omitted when it rounds to zero** —
empty / default values are not stored, in both the JSON and the HTML viewer
(see `docs/DESIGN.md` for the gating rules and their one exception). Numeric
values use 3-significant-digit rounding; integral values serialize without a
decimal point.
`fan_in` / `fan_out` / `hk` count internal flow edges
only; edges whose target is external are counted in `fan_out_external`. `cycle`
(`"mutual"` / `"chain"`) is present only on nodes in a cycle. `unsafe` (Rust-only)
is a per-file count of `unsafe` usages, present only when non-zero.

**Edge shape**:

```json
{ "source": "<node-id>", "kind": "uses | reexports | contains | super", "target": "<node-id>", "line": 12 }
```

An edge is **external iff its `target` is an `ext:` node** — there is no
`edge.external` flag. Whether an edge kind is information flow vs. structural is
read from `edge_kinds[kind].flow` (e.g. `contains` is `flow: false` — kept and
shown as ownership, excluded from fan_in / HK / cycles). Edge attributes (e.g. a
Rust `reexports` edge's `visibility`) are flattened in alongside `source` /
`kind` / `target`. `line` is the optional 1-based line of the declaring
`use` / `import` statement (omitted for `contains` and unplaceable edges); `check`
uses it to point a cycle violation at a concrete edge to break.

**Stats shape** (`stats` field on a level) — a flat map of the mean of each
tracked numeric metric across the level's file nodes (zero/missing excluded; a
metric emitted only when its average is positive), e.g.:

```json
{ "cyclomatic": 1.4, "cognitive": 1.8, "fan_in": 2.25, "fan_out": 3, "hk": 864,
  "mi": 104.0, "mi_sei": 105.7, "sloc": 15.8, "cloc": 3.8, "blank": 6.8, "tloc": 4.2,
  "length": 32.2, "vocabulary": 19.6, "volume": 149.1, "effort": 1030.4,
  "time": 57.2, "bugs": 0.029 }
```

Percentiles are not stored — a viewer can compute them client-side from raw node
data.

**Breaking Change Policy**: Additive fields are minor; renames or
removals require a major-version bump and migration notes.

## 8. Use Cases

### UC-001 Analyze Rust Workspace Offline

**ID**: `cpt-code-ranker-usecase-analyze-offline`

**Actors**: `cpt-code-ranker-actor-developer`

**Preconditions**: The target directory is a valid Cargo workspace;
the `code-ranker` binary is installed.

**Main Flow**:

1. Developer runs `code-ranker report . --plugin rust` (analyzes the
   workspace and writes both a snapshot and an HTML viewer in one step)
2. `code-ranker` writes `.code-ranker/axum-api-20260522-112233.json` (the
   snapshot) and `.code-ranker/axum-api-20260522-112233.html` (the viewer)
3. Developer opens `.code-ranker/axum-api-20260522-112233.html` in a browser,
   sorts files by coupling weight
4. Developer identifies the heaviest files and decides what to refactor

(For a non-blocking lint that gates on cycles/thresholds and writes no
files, the developer can instead run `code-ranker check . --plugin rust`.)

**Postconditions**: A self-contained HTML viewer exists at
`.code-ranker/axum-api-20260522-112233.html`; no network access was required
at any step.

**Alternative Flows**:

- **Plugin fails (cargo metadata error)**: Plugin exits non-zero with
  a structured JSON error on stderr; no JSON files are written.

### UC-002 Before/After Refactoring Comparison

**ID**: `cpt-code-ranker-usecase-diff-refactor`

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`

**Preconditions**: A baseline snapshot exists from a prior run; the
developer has made structural changes to the codebase.

**Main Flow**:

1. Developer runs
   `code-ranker report . --baseline .code-ranker/snap-before.json --output.html.path=diff.html`
   (analyzes the current tree and compares it against the baseline in one run)
2. Developer opens `.code-ranker/diff.html` to see coupling changes
   color-coded by per-node diff state, with the baseline↔current verdict
3. Developer reads the machine-readable verdict with
   `code-ranker check . --baseline .code-ranker/snap-before.json --output-format json`

(Because `[input]` is polymorphic, the developer can instead capture the
current state first — `code-ranker report . --output.json.path=snap-after.json`
— then compare two existing snapshots without re-analyzing:
`code-ranker report snap-after.json --baseline .code-ranker/snap-before.json
--output.html.path=diff.html`.)

**Postconditions**: A diff HTML report exists and a machine-readable
verdict is available; the verdict quantifies whether the refactoring
improved the architecture.

**Alternative Flows**:

- **Schema version mismatch**: the comparison exits non-zero with an error
  identifying the incompatible artifact; no report is produced.

### UC-003 CI Structural Gate on Pull Request

**ID**: `cpt-code-ranker-usecase-ci-diff`

**Actors**: `cpt-code-ranker-actor-ci`, `cpt-code-ranker-actor-pr-reviewer`

**Note**: This use case is targeted at P2.

**Preconditions**: The base-branch snapshot is stored as a CI artifact;
the PR branch has been pushed.

**Main Flow**:

1. CI downloads the base-branch snapshot to `.code-ranker/snap-base.json`
2. CI runs `code-ranker check . --baseline .code-ranker/snap-base.json --output-format json`
   to gate the PR — it fails only on *new* violations versus the base
3. CI runs
   `code-ranker report . --baseline .code-ranker/snap-base.json --output.html.path=diff.html`
   to render the shareable diff viewer
4. CI attaches `.code-ranker/diff.html` to the PR and posts the verdict from
   the `check --baseline` JSON as a PR comment
5. PR Reviewer reads the coupling-change summary and diff report without
   local setup

**Postconditions**: Structural coupling changes are visible at PR time
as a self-contained HTML report.

## 9. Acceptance Criteria

- [x] Rust plugin produces a valid JSON snapshot (one `files` graph) for
  a reference workspace in ≤ 30 s on a modern laptop (typically seconds)
- [x] HTML report opens in Chrome/Firefox/Safari with interactive graph
  visualization and client-side node sorting by coupling weight
- [x] `report --baseline` produces a color-coded HTML diff from two
  snapshots; the verdict (`improved` / `degraded` / `neutral`) is present
- [x] All P1 tools operate with zero outbound network calls
- [x] Generated HTML reports contain no external resource references
- [x] JSON artifacts conform to the Graph JSON Schema (`schema_version: "2"`)
- [x] A `--baseline` comparison exits non-zero with a structured error on
  schema version mismatch
- [ ] Every metric value equals the true count of what it measures — no false
  positives and no false negatives (`cpt-code-ranker-nfr-metric-accuracy`)

## 10. Dependencies

| Dependency | Description | Priority |
|------------|-------------|----------|
| `cargo_metadata` crate | Cargo workspace enumeration (local vs. external crates) | p1 |
| `syn` crate | Rust source parsing for the module tree and `use` statements | p1 |
| `tree-sitter` (+ `-rust` / `-python` / `-javascript` / `-typescript`) | Source parsing for the in-tree per-language tier-1 metric engines (`rust_ts` / `python_ts` / `ecmascript_ts` — faithful ports of `rust-code-analysis`'s node-kind rules) and for the Python / JavaScript / TypeScript plugins | p1 |
| `cel` crate | Evaluates the declarative tier-2 metric formulas (`metrics/builtin.toml`) and user `[metrics.<key>]` formulas; the metric registry engine | p1 |
| Python 3.9+ | Runtime for the built-in Python language plugin | p3 |

## 11. Assumptions

- Target Rust workspaces have resolvable dependencies (`cargo metadata`
  succeeds) for full external-node enumeration
- Browsers rendering the HTML reports support modern JavaScript (ES2020+)
- The base-branch snapshot used for diffs was produced by the same
  major version of the Rust plugin (schema compatibility guaranteed
  within a major version)

## 12. Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| File graph too large to visualize in-browser | Medium — unusable HTML report | Cluster by directory; warn the user when node count exceeds a threshold |
| Snapshot schema divergence between plugin versions | Medium — silent diff failures | Enforce schema version check at diff time; abort with structured error on mismatch |
| Performance regressions on large workspaces | Medium — usability loss | Benchmark suite in CI on a curated 5k and 50k LOC corpus |
| P3 schema vocabulary extensions break base snapshot consumers | Low — only affects P3 adopters | Extensions use optional fields only; base consumers skip unknown fields |

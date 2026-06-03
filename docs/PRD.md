# PRD — Code Split

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
  - [7.1 Code Split Unified CLI](#71-code-split-unified-cli)
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

## 1. Overview

### 1.1 Purpose

Code Split is a polyglot structural-analysis platform that (1) extracts
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
| Plugin | A built-in language analyzer (`rust`, `python`, or `javascript`) compiled into the `code-split` binary that analyzes a workspace and produces a single file graph in-process |
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

**ID**: `cpt-code-split-actor-developer`

**Role**: Runs the plugin on a local workspace, views the HTML report,
modifies the codebase, then compares before/after snapshots.

**Needs**: Fast offline-capable tools with no mandatory LLM or network
dependency; single-command invocation per step.

#### Tech Lead

**ID**: `cpt-code-split-actor-tech-lead`

**Role**: Reviews HTML reports to evaluate module boundaries and
coupling hotspots; reviews diff reports to validate that refactors
improved the architecture.

**Needs**: Sortable coupling view; clear before/after delta with
magnitude; self-contained HTML that can be shared without tooling.

### 2.2 System Actors

#### CI Pipeline

**ID**: `cpt-code-split-actor-ci`

**Role**: Runs the plugin at pull-request time, stores snapshot
artifacts, gates the branch against the base-branch snapshot with
`check --baseline`, and attaches the `report --baseline` diff to the
pull request.

**Needs**: Non-interactive execution; deterministic artifact output;
structured exit codes.

#### PR Reviewer

**ID**: `cpt-code-split-actor-pr-reviewer`

**Role**: Views the diff HTML report attached to a pull request to
evaluate architectural impact without a local toolchain.

**Needs**: Self-contained HTML; clear color-coded coupling changes;
summary verdict readable in under one minute.

## 3. Operational Concept & Workflow

The platform is organized as four sequential steps. Steps 1, 2, and 4
are implemented by Code Split; Step 3 is the user's own modification
activity and is deliberately outside Code Split's scope.

```
Step 1 ─ Extract   →   Step 2 ─ Visualize   →   Step 3 ─ Modify   →   Step 4 ─ Compare
(code-split report)       (code-split report)          (User / AI)        (report --baseline)
outputs JSON            outputs HTML              (we wait)             outputs HTML
```

**Step 1 — Graph Extraction (Plugin)**: A language-specific built-in
plugin analyzes the workspace in-process when `code-split report` runs,
which writes a single JSON snapshot containing the file dependency graph
(with third-party libraries as depth-1 external nodes). No network access
or LLM is required. The snapshot may be stored as a CI artifact for Step
4. (For a pure CI gate that only lints and writes no files,
`code-split check` runs the same analysis without producing a snapshot.)

**Step 2 — Visualization (Report Generator)**: The same `code-split report`
run that analyzes the workspace also produces a self-contained offline
HTML viewer with interactive graph visualization and sorting by node
weight — snapshot and HTML are emitted together. No network access or
LLM is required.

**Step 3 — Modification (User Activity)**: The user reads the report,
decides what to refactor (manually or with AI assistance), and modifies
the codebase. Code Split does not participate in this step.

**Step 4 — Baseline Comparison**: After modification, the user re-runs
Step 1 to capture the current state (or analyzes it live). Passing the
earlier snapshot as `--baseline` compares the two: `code-split report
. --baseline <snapshot>` produces a baseline↔current diff HTML report
with a verdict, and `code-split check . --baseline <snapshot>` produces a
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
| Distribution | Multi-ecosystem binary distribution: single pre-compiled `code-split` binary per platform published via thin wrappers to PyPI (`pip install code-split`), npm (`npm install -g @code-split/cli`), and GitHub Releases |

#### P3 — Future

| Step | Scope |
|------|-------|
| Step 1 | Additional language plugins: Python, JavaScript, Go, C#, PHP; framework-specific plugins (Django, WordPress, etc.) with domain-specific metadata |
| Step 2 | AI prompt generation for principles review using the `principles/` corpus (per-language: `principles/rust/`, `principles/python/`, `principles/typescript/`) |

### 4.2 Out of Scope (All Versions)

- Expanding external dependencies (registry/git/npm/pypi packages
  appear as opaque depth-1 nodes; their internals are never read)
- Function-level or call-graph analysis (no `Calls` edges, no semantic
  call resolution)
- Automated code modification or refactoring suggestions
- IDE/LSP integration and interactive visualization
- Cross-language linkage (FFI/RPC boundaries are leaves)
- Database or service deployment; no server component

## 5. Functional Requirements

### 5.1 Plugin System — Step 1

#### Unified Entry-Point Command

- [x] `p1` - **ID**: `cpt-code-split-fr-unified-cli`

All user-facing operations MUST be accessible through a single binary
`code-split`. Running it with no command prints help — every action goes
through an explicit subcommand; there is no default command. There are
exactly **two** subcommands, split by *what they emit* — `check` produces
an exit code (a CI gate), `report` produces files (a snapshot and a
viewer):

```
code-split check  [input] [--plugin <name|auto>] [--baseline <snapshot>] [options]
code-split report [input] [--plugin <name|auto>] [--baseline <snapshot>] [--output.<fmt>.path <path>] [options]
```

The single positional `[input]` (default `.`) is **polymorphic**: a
**directory** is analyzed in-process (run the plugin, build the graph,
compute metrics), while a **`.json` snapshot** or **`.html` report** is
read for its embedded snapshot — no analysis, source tree, or toolchain
required. Analysis-only flags (`--plugin`, `--ignore`) are rejected with a
snapshot input.

- `check` is the linter: it evaluates cycle rules and thresholds, prints
  diagnostics, exits non-zero on any violation, and writes **no files**.
  With `--baseline <snapshot>` it switches to a **relative gate** that
  fails only on *new* violations versus the baseline (pre-existing ones
  tolerated) and emits a verdict (`improved` / `degraded` / `neutral`); a
  machine-readable verdict is produced with `--output-format json`.
- `report` writes artifacts (a JSON snapshot and/or an HTML viewer) and
  always exits `0`. Without `--baseline` the HTML is a single-snapshot
  viewer; with `--baseline <snapshot>` it becomes a baseline↔current diff
  view with a verdict, named `…-diff.html`.

`report` selects artifacts and their destinations through one flag family,
`--output.<fmt>.path <path>` (`<fmt>` is `json` or `html`). When no
`--output.*` flag is given it writes **both** formats with default names
into `.code-split/`: `{ts}-{git-hash-3}.json` and `{ts}-{git-hash-3}.html`,
e.g. `.code-split/20260526-114144-a3f.json` (`{ts}` is a local
`YYYYMMDD-HHMMSS` timestamp, `{git-hash-3}` the first three chars of the
commit). When one or more `--output.<fmt>.path` are given, **exactly** the
listed formats are written. The `.path` value is a file path (or a name
template, or `stdout`/`-` to stream the artifact); it supports placeholders
`{project-dir}` (slugified workspace name), `{ts}`, `{git-hash}` (the
12-char short commit) and `{git-hash-N}` (its first N chars). The
destination resolves as **`--output.<fmt>.path` flag › `[output.<fmt>]
path` in `code-split.toml` › built-in default**, so a project can pin its
own naming while a flag still wins for named states (e.g., `pr.json`). With
`--baseline`, the HTML default gains a `-diff` marker
(`{ts}-{git-hash-3}-diff.html`); the JSON artifact is always the current
snapshot, never a diff. No additional registry is created.

Each snapshot is a **single self-contained `.json` file** combining
metadata (command, versions, git state) and the one `files` graph. See
`cpt-code-split-fr-snapshot-meta` for the full schema.

The snapshot is written as **canonical JSON**: every object key is emitted
in alphabetical order and the `nodes` / `edges` arrays are sorted by a
stable key (node `id`; edge `from`/`to`/`kind`). Re-analyzing unchanged
code therefore yields byte-identical graph data — no churn from map
iteration order — which keeps committed snapshots (e.g. the `samples/`
goldens) diff-clean and makes a baseline comparison reflect only real changes.

A `--baseline` comparison consumes snapshot files produced by `report` and
is plugin-agnostic. Splitting into separate binaries is forbidden at
P1; the separation of concerns lives inside the binary.

**Rationale**: One file per snapshot is easier to copy, archive, attach
to CI artifacts, and pass as a `--baseline`. A timestamped, commit-stamped
filename (`{ts}-{git-hash-3}`) means users never have to think about naming
for routine snapshots while keeping per-commit runs distinct; the
`[output.<fmt>]` config sets a project-wide template and an explicit
`--output.<fmt>.path` is available for named states (e.g.,
`snap-before-refactor.json`).

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`

#### Snapshot File Format

- [x] `p1` - **ID**: `cpt-code-split-fr-snapshot-meta`

Each `code-split report` run produces a single `.json` file
(`schema_version: "2"`). The file combines metadata and the `graphs` map (one
entry per analysis level — today only `files`) in one document. Each level
bundles its semantics dictionaries with the structural graph and computed data
(see §7.3 for the full shape):

```json
{
  "schema_version": "2",
  "generated_at": "2026-05-22T11:22:33Z",
  "command": "code-split report /path/to/axum-api --plugin rust",
  "workspace": "/Users/alice/projects/code-split",
  "target":    "/Users/alice/projects/axum-api",
  "plugin": "rust",
  "config_file": "/Users/alice/projects/axum-api/code-split.toml",
  "versions": { "code-split": "1.0.0-alpha.3", "rustc": "1.78.0" },
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
- `workspace` — absolute path to the directory where `code-split` was invoked
- `target` — absolute path to the analyzed project
- `plugin` — resolved built-in plugin name (`rust` / `python` / `javascript` / `typescript`)
- `config_file` — absolute path of the config file used; omitted when none was found
- `versions` — `code-split` semver at minimum; the Rust plugin adds `rustc`
- `roots` — named system prefixes used to relativize node ids/paths
  (`roots[name] + "/" + rest` → absolute path). Roots that did not shorten any
  path are pruned, so a JS/TS/Python snapshot carries only `{target}` and a Rust
  snapshot `{target}` + `{registry}`
- `git` — `branch`, `commit` (12-char short SHA), `dirty_files`, and `origin`;
  the whole block omitted if not a git repository
- `timings` — per-stage wall-clock timings (`stage`, `ms`, `detail`), in
  execution order; omitted when empty
- `graphs` — a map `level_name → level`; today the only key is `files`. Each
  level carries the four semantics dictionaries (`edge_kinds`,
  `node_attributes`, `edge_attributes`, `attribute_groups`) plus `nodes`,
  `edges`, `cycles`, and `stats`

`code-split report` and `code-split check` (with `--baseline`) read
snapshot files and embed the top-level metadata in the generated HTML as a
"Snapshot info" panel.

**Rationale**: One file per snapshot is simpler to copy, archive, and
pass between tools than a directory of four files. The timestamp in the
filename makes snapshots self-organizing without a registry.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`,
`cpt-code-split-actor-ci`

#### Plugin Selection

- [x] `p1` - **ID**: `cpt-code-split-fr-plugin-discovery`

The plugins are built into the `code-split` binary; the valid plugin names are
`rust`, `python`, `javascript`, and `typescript` (JS and TS are **separate**
plugins, no aliases). The `--plugin <name>` option (on `check` / `report`)
selects one of these built-ins. There is no external or dynamic plugin loading.

The plugin is resolved in the following order, stopping at the first match:

1. **Explicit `--plugin <name>`** on the command line (any value other
   than `auto`) wins.
2. Otherwise the **`plugin` key in the config file** (`code-split.toml` /
   `Cargo.toml#metadata.code-split`), if set and not `auto`.
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

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`

#### Rust Plugin

- [x] `p1` - **ID**: `cpt-code-split-fr-rust-plugin`

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
  `cargo metadata`). A dependency on another **local workspace crate**
  becomes a file→file edge to that crate's root file (`lib.rs` / `main.rs`)
- Capture **bare qualified paths** in expressions/types (`commands::run()`,
  `other_crate::item`, `crate::a::Alpha` with no `use`), resolved the same
  way as `use`, so both intra-crate and cross-crate dependencies referenced
  only by qualified path are not lost
- NOT emit a function-level call graph (no `Calls` edges, no
  rust-analyzer / `ra_ap_*` dependency); analysis runs in seconds
- Compute per-file code complexity metrics (cyclomatic, cognitive,
  Halstead, maintainability index, LOC variants, functions, closures,
  nexits, nargs) for each `File` node via the shared `code-split-plugin`
  complexity engine; metrics are stored in the `complexity` field of the node and
  serialized into the snapshot
- Detect dependency cycles (Kosaraju SCC) in the file graph; annotate
  each node in a cycle with `cycle_kind` (`TestEmbed` | `Mutual` |
  `Chain`) and store `CycleGroup` entries in `Graph.cycles`
- Compute Henry-Kafura complexity (`HK = LOC × (fan_in × fan_out)²`)
  for every file node from **internal** file→file edges; store in
  `complexity.coupling` (`fan_in`, `fan_out`, `fan_out_external`, `hk`).
  Edges to `External` nodes are excluded from `fan_in`/`fan_out`/`hk`
  and counted in `fan_out_external` instead

**Rationale**: Rust is the primary use-case for the initial release.
The `code-split-plugin-rust` crate (cargo metadata + `syn`, including the
module→file collapse pass) implements this plugin. Removing rust-analyzer
makes the Rust path fast and the binary light.

**Actors**: `cpt-code-split-actor-developer`

#### File-Level Graph

- [x] `p1` - **ID**: `cpt-code-split-fr-file-graph`

Every plugin MUST emit a single directed **file graph**. Nodes are
`File` (project source files, carrying all per-file metrics) and
`External` (third-party libraries at depth 1, one node per library,
never expanded). Edges are `uses` and `reexports` between files, plus
`uses` edges flagged `external: true` from a file to a library node.
There is no module or function graph in the snapshot.

For Rust, the file graph is derived by collapsing the module graph (see
`cpt-code-split-fr-rust-plugin`); for Python/JS/TS it is built directly
from import resolution.

**Rationale**: The file is the universal unit across languages and the
level at which most refactoring and ownership decisions are made. A
single graph keeps the artifact small and the model consistent across
plugins.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### Embedded Static Asset Tracking (P2)

- [ ] `p2` - **ID**: `cpt-code-split-fr-rust-embedded-assets`

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

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### Language Plugins (P3)

- [x] `p3` (Python shipped) - **ID**: `cpt-code-split-fr-lang-plugins`

The platform SHOULD support additional built-in language plugins for
Python, Go, JavaScript, C#, and PHP, each emitting a conformant file
graph. A built-in plugin MAY attach framework-specific information via
the `metadata` object on nodes/edges (e.g. Django, WordPress concepts);
such extensions MUST be backward-compatible with the base schema and keep
`kind` as `file` / `external`.

**Python plugin** (`--plugin python`) is shipped as a built-in in
`code-split-cli`. It uses `tree-sitter-python` to emit one `File` node
per `.py` file and resolve imports: imports of project files become
file→file `uses` edges (including `__init__.py` package imports pointing
at the package file), and imports that do not resolve to a project file
become `External` library nodes (`ext:<top-level-package>`, e.g.
`numpy`) reached by a `uses` edge flagged `external: true`. Per-file
complexity metrics (cyclomatic, cognitive, Halstead, MI, LOC, functions,
nexits, nargs) are annotated on each `File` node via the shared
`code-split-plugin` complexity engine using `rust-code-analysis`'s `PythonParser`.

**JavaScript / TypeScript plugin** (`--plugin javascript`) is shipped as a
built-in in `code-split-cli`; one plugin handles `.js`, `.jsx`, `.ts`, and
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

**Actors**: `cpt-code-split-actor-developer`

#### Configuration System

- [x] `p1` - **ID**: `cpt-code-split-fr-config`

The analyzing commands (`check` / `report`) MUST load a layered
configuration from multiple sources. Priority order (highest wins for
scalars; `ignore.paths` is merged):

| Priority | Source |
|---|---|
| 1 | CLI flags (`--ignore`, `--cycle-rule`, `--threshold`, `--plugin`, `--output.<fmt>.path`) |
| 2 | `--config KEY=VALUE` inline overrides (dotted key into the config schema) |
| 3 | `--config <file>` |
| 4 | `code-split.toml` in cwd, then in target directory |
| 5 | `Cargo.toml` `[workspace.metadata.code-split]` / `[package.metadata.code-split]` |
| 6 | Built-in defaults |

**Config file keys** (`code-split.toml` or `Cargo.toml` metadata section):

```toml
plugin = "auto"          # default plugin; "auto" detects by project markers, overridden by --plugin

[ignore]
paths        = ["**/generated/**"]  # glob patterns matched against node path
tests        = true      # strip test files from the graph (legacy alias: test_modules)
dev_only_crates = true   # strip crates reachable only via [dev-dependencies]
                         # (uses `cargo metadata` for transitive accuracy)

[rules.cycles]
test-embed = false       # default: off  (Rust #[cfg(test)] back-edge)
mutual     = true        # default: on
chain      = true        # default: on

[rules.thresholds.file]      # a single file (files graph)
loc        = 800
hk         = 500_000
cyclomatic = 10

[output.json]                # default JSON snapshot destination (report command)
path    = "{project-dir}-{ts}.json"   # placeholders: {project-dir} {ts} {git-hash} {git-hash-N}
enabled = true               # whether to write this format by default

[output.html]                # default HTML viewer destination (report command)
path    = "{project-dir}-{ts}.html"   # a --output.html.path flag still overrides
enabled = true
```

**CLI flags**:

- `--plugin <NAME|auto>` — override default plugin (`auto` detects by markers)
- `--output.<fmt>.path <PATH>` (`report`; `<fmt>` is `json` or `html`) — select
  that artifact format and set its destination (a path, a name template with
  placeholders `{project-dir}`, `{ts}`, `{git-hash}`, `{git-hash-N}`, or
  `stdout`/`-`); wins over `[output.<fmt>] path`; built-in default
  `{ts}-{git-hash-3}`. Presence of any `--output.*` flag selects exactly the
  listed formats; with none, both `json` and `html` are written
- `--baseline <SNAPSHOT>` (`check` / `report`) — compare the current `[input]`
  against this baseline snapshot (`.json` or `.html`); on `check` it switches
  to a relative gate (fail only on new violations), on `report` it turns the
  HTML into a baseline↔current diff with a verdict
- `--config <PATH | KEY=VALUE>` — load config from an explicit file path, or
  override a single setting inline via a dotted key (repeatable; inline wins)
- `--ignore <GLOB>` — add a path glob (repeatable, merged with file)
- `--cycle-rule <KIND=on|off|N>` — configure a cycle check: `on` (any cycle of
  that kind fails), `off` (ignored), or an integer `N` (allow up to `N`, fail on
  the `N+1`-th — e.g. `chain=7` to pin today's count and forbid new ones)
- `--threshold <file.METRIC=N>` — set a per-file threshold (e.g.
  `file.loc=800`, `file.cyclomatic=10`); a breach fails the check (`check`
  only). The scope is always `file` (a single source file). `N` accepts `_`
  separators and `K`/`M`/`G` suffixes (e.g. `file.hk=5M`)
- `--top <N>` — report only the `N` worst violations (`check` only); reporting
  limit, does not change the exit code
- `--exit-zero` — exit 0 even when violations are found (`check` only,
  collect-only mode)
- `--suggest-config` — also print the current values as a ready-to-paste
  `code-split.toml` baseline (`check` only; off by default)

**No severity levels**: there is no warning tier — `check` either passes or fails.
A threshold is set or unset; a cycle kind is off, strict (`on`/`0`), or carries a
count budget `N` (up to `N` cycles of that kind allowed). A budget lets teams pin
today's cycle count and fail only on regressions, without fixing the backlog first.

**Rule ids and self-contained diagnostics**: every violation is identified by its
dotted rule id — the same string used as the config key and CLI flag (e.g.
`threshold.file.loc`) — and tagged with a concern group: `CYC` (dependency
cycles), `CPX` (complexity), `CPL` (coupling), `SIZ` (size). The full reference is
documented in [ERRORS.md](ERRORS.md). The default `human` output renders each
finding as a self-contained block — rule id, group, location (`id — path:line`),
measurement, rationale, fix, and the flag/config key that tunes the rule — so a
single block copied from the terminal is a complete prompt for an AI assistant.
The rule id and group are carried in every `--output-format` (block header,
`json` `rule`/`group` fields, `github` annotation title, `sarif` `ruleId` plus a
fired-rules `tool.driver.rules` catalog).

**Current-values config block (`--suggest-config`)**: with `--suggest-config`,
`human` output prints — after the findings — the project's current measured values
as ready-to-paste `code-split.toml` blocks: the `[rules.cycles]` counts per kind,
and the per-file thresholds (the worst single unit). A team copies the block to pin today's numbers as a baseline that passes
now and fails on regression. Off by default; the machine formats
(`json`/`github`/`sarif`) omit it.

The path of the config file actually used is recorded in the snapshot as `config_file`.

**Invalid configuration is fatal**: a malformed config file, an unknown threshold
scope/metric, or a bad inline `--config` / `--threshold` / `--cycle-rule` value
aborts the command with a non-zero exit and a clear message — the tool never
silently falls back to defaults, which would drop the user's rules and let
`check` pass when it should fail (a false green for a CI gate).

**Rationale**: Teams need to suppress expected patterns (e.g. `test-embed`
cycles, dev-only crate noise) and enforce structural budgets in CI without
modifying source code.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`

### 5.2 Visualization Reports — Step 2

#### HTML Report Generation

- [x] `p1` - **ID**: `cpt-code-split-fr-html-report`

The `code-split report` subcommand MUST analyze the workspace and, when the
`html` artifact is selected (the default set is both `json` and `html`),
generate a single self-contained offline HTML file alongside the snapshot
`.json`. The HTML MUST include:

- Interactive file-graph visualization, with `external` library nodes
  shown in a distinct amber colour (dashed edges)
- A coupling metrics table showing node weight (fan-in + fan-out) for
  each file
- All JavaScript and CSS inlined (no CDN or external resources)

**Rationale**: A self-contained HTML file requires no server, no
internet, and no installed dependencies to view — maximizing
accessibility for developers and reviewers on any machine.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### Node Sorting by Weight

- [x] `p1` - **ID**: `cpt-code-split-fr-node-sorting`

The HTML report MUST allow the user to sort files by coupling weight
(fan-in + fan-out edge count). The report MUST display the top-N
heaviest files prominently. Sorting MUST be performed client-side within
the HTML (no server required).

**Rationale**: The heaviest nodes are the most likely candidates for
refactoring. Surfacing them first reduces the time to actionable insight.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### AI Prompt Generator (P2)

- [x] `p2` - **ID**: `cpt-code-split-fr-ai-prompts`

The HTML report SHOULD include a UI control that generates a prompt for
an LLM, pre-populated with the top-N heaviest nodes and their coupling
context, asking for refactoring recommendations. The prompt format MUST
be copyable as plain text for direct paste into any LLM interface.

**Rationale**: Connecting structural data to an LLM's reasoning closes
the loop between measurement and advice without coupling the offline
tool to a specific LLM provider.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### Principles-Based Prompt Generation (P3)

- [x] `p3` - **ID**: `cpt-code-split-fr-principles-prompts`

The HTML report SHOULD support a principles-audit prompt mode that maps
the top coupling findings to the canonical principle corpus under
`principles/<language>/` (currently `rust/`, `python/`, `typescript/`)
and instructs the LLM to audit the codebase against each principle.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

### 5.3 Baseline Comparison — Step 4

#### Graph Diff Engine

- [ ] `p1` - **ID**: `cpt-code-split-fr-graph-diff`

> **Deferred — not implemented in this release.** The structured
> snapshot-to-snapshot diff (`compare_snapshots`: nodes/edges added / removed /
> affected) was tied to the pre-redesign model; it is stubbed (returns a
> not-implemented error) and will be reintroduced against the generic model
> later, together with the diff HTML viewer (the JS assets are not yet migrated
> to schema `"2"`). The **relative `check --baseline` gate** below
> (`cpt-code-split-fr-compare`) is implemented by a different mechanism
> (re-evaluating the rules against the baseline snapshot), and does not depend on
> this engine. The remainder of this FR describes the deferred viewer behavior.

With `--baseline <snapshot>`, `code-split report` will compute a structured
diff between the baseline snapshot and the current `[input]`: nodes and
edges added, removed, or affected. The diff includes an overall
verdict: `improved`, `degraded`, or `neutral`. The interactive
diff HTML uses Graphviz WASM (bundled in the binary) for client-side
DOT→SVG layout with directory cluster grouping; there is a single Files
view (no level switcher). The map is laid out **once** from the **union**
of both snapshots (Graphviz computes a single set of node positions); the
`[data-side]` Baseline/Current buttons are then a pure CSS visibility flip —
current-only (added) elements are hidden on the Baseline side, baseline-only
(removed) elements on the Current side — so every file present on both sides
keeps its exact position and never moves when toggling. **Current is shown by
default.** In the metric node-size modes (SLOC/HK) each circle is resized
to the active side's value around its fixed centre (a file that grew or
shrank changes size, not position). The active side is reflected
throughout: the `side=baseline|current` URL parameter, the node-table title
(`Details` / `Details Baseline` / `Details Current`), and a `Baseline` /
`Current` badge on the node-popup and Prompt-Generator headers. The two header
slots are the **current** (right) — the primary snapshot the report is
about, always present and **not removable** — and an optional **baseline**
(left, editable, removable). With no baseline it is
single-snapshot **review** mode: the baseline slot is an empty, editable
placeholder (`↑ Set baseline`) and the Baseline/Current buttons are hidden;
loading a baseline turns the report into a diff. Each header slot's hover
tooltip is labelled `Baseline` / `Current` and notes which side is currently
shown; that slot is also highlighted in the header. Two buttons swap in a
different snapshot from disk (each accepts a `.json` snapshot or an `.html`
report): **↑ Replace current** changes the evaluated snapshot, **↑ Set
baseline** loads a reference to diff against. Cycle detection
(Tarjan SCC) runs in-browser and annotates nodes/edges for red-stroke
highlighting (solid red, no dasharray); the highlight is **side-aware** —
a `baseline-only` cycle is red only on the Baseline side, `current-only` only
on Current, `both` on either, so a cycle removed in the current snapshot
stops being red once you switch to Current. Internal `file` nodes render
blue; `external` library nodes render amber with dashed edges. The node
table column order is: checkbox, Name, Kind, Cycle, Status, LOC, HK,
Fan-in, Fan-out, H.vol, H.bugs, H.effort, H.time, H.len, H.vocab,
Cyclomatic, Cognitive, MI, MI SEI, Logical, Comments, Blank. A checkbox column
(leftmost) enables persistent multi-node selection (shared across
Baseline/Current by node id — a file present in both snapshots stays selected
when toggling; the selected-row count reflects the active side): clicking a checkbox
highlights the row (yellow) and the corresponding SVG node (yellow fill
- amber stroke); shift-click selects a range; the header checkbox
selects or deselects all visible rows (indeterminate when partial).
Selection also works directly on the map: **holding Shift** turns the main
diagram into a selection surface (the cursor changes over the SVG), and
Shift-clicking an SVG node toggles its selection — exactly like ticking its
table checkbox, kept in sync — instead of opening the modal. Holding the
**"open source" modifier** — **⌘ on macOS, Ctrl elsewhere** (Ctrl is left
alone on macOS, where it maps to right-click) — likewise changes the cursor
and turns a node click into "open source": it opens the file on the project's
git host (from `git.origin`) in a new tab instead of the modal (project files
only). While either modifier is held — or the cursor hovers the right edge — the map's
right-side controls (zoom and node-size) and a bottom-left shortcut legend are
revealed; the legend spells out the active keys for the platform (⌘ on macOS,
Ctrl elsewhere).
The modal popup opened by clicking a row or an SVG node is fullscreen
(locks body scroll); it includes a synced selection checkbox, fields in
order id (⎘ copy) → path (⎘ copy, filename bold) → source (a link to the
file on the project's git host, built from `git.origin`; project files
only) → kind → visibility → items/methods → cycle info → status → metric
sections in a single column. Hover highlight (blue drop-shadow) takes CSS
priority over selection highlight. **Space** toggles the selection checkbox
while the popup is open. The popup's neighbourhood diagram mirrors the map's
gestures — Shift-click toggles a node's selection, ⌘/Ctrl-click opens its
source — and shows the same yellow highlight for nodes already selected; its
3rd-party (external) cards and arrows are drawn grey and are inert (not
selectable, no source, no ⌘-navigation).

**Rationale**: The diff is the quantitative answer to "did my
refactoring reduce coupling?" Without it, the user must compare two
static visualizations manually.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`,
`cpt-code-split-actor-pr-reviewer`

#### Diff HTML Report

- [ ] `p1` - **ID**: `cpt-code-split-fr-diff-html-report`

> **Deferred — not implemented in this release** (the HTML viewer is not yet
> migrated to schema `"2"`; `report --baseline` currently embeds the current
> snapshot but renders no diff). Target behavior:

`code-split report --baseline` will generate a single self-contained
offline HTML report displaying:

- Added / removed / affected files and edges, color-coded by per-node diff
  state (added, removed, affected, unchanged)
- Cycle detection: files/edges in dependency cycles annotated with
  `baseline-only` / `current-only` / `both` / `none` status and red-stroke
  highlighting
- `external` library nodes shown in a distinct amber colour with dashed
  edges to distinguish them from internal file edges
- Diff summary table: node/edge counts and cycle counts (SCCs, nodes in
  cycles), baseline vs current with Δ
- All JavaScript and CSS bundled locally (no CDN or external resources)

**Rationale**: Self-contained HTML is viewable without tooling and
suitable for attaching to PRs or sharing with stakeholders.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`,
`cpt-code-split-actor-pr-reviewer`

#### Machine-Readable Comparison Verdict

- [x] `p1` - **ID**: `cpt-code-split-fr-compare`

`code-split check --baseline <snapshot> --output-format json` MUST compare
the current `[input]` against the baseline snapshot and emit a
machine-readable verdict and new-violation summary to stdout. The verdict is
`improved` (some violations resolved, none added), `degraded` (new violations),
or `neutral`; the gate is **relative** — it fails only on violations not already
present in the baseline (matched by `(rule, location)` signature). It is
implemented by re-evaluating the configured rules against the baseline snapshot
— **not** by the (deferred) structured graph diff — so it needs no
`compare_snapshots` engine.

JSON summary — a `verdict` wrapper around the new-violations list:

```json
{
  "verdict": "degraded",
  "violations": [
    { "rule": "threshold.file.hk", "group": "CPL", "graph": "files",
      "location": "{target}/src/a.rs", "message": "…", "weight": 2.1 }
  ]
}
```

> The earlier count-based summary (node/edge added/removed/affected, SCC counts)
> is part of the deferred `cpt-code-split-fr-graph-diff` engine and is not
> emitted in this release.

The human-facing counterpart, `code-split report --baseline` (the interactive
diff HTML viewer), is **deferred** with the rest of the viewer
(`cpt-code-split-fr-diff-html-report`).

**Rationale**: `check --baseline` is the machine gate (an exit code and a
JSON verdict for CI); the shareable human diff viewer follows once the viewer is
migrated.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`,
`cpt-code-split-actor-pr-reviewer`

#### Text Change Report

- [x] `p1` - **ID**: `cpt-code-split-fr-diff-text-report`

`code-split check --baseline <snapshot> --output-format json` emits a
structured JSON summary (see `cpt-code-split-fr-compare`) embeddable in CI
logs and PR comments. The JSON contains the verdict, node/edge counts and
delta per level, plus cycle SCC counts.

**Actors**: `cpt-code-split-actor-ci`, `cpt-code-split-actor-pr-reviewer`

#### CI Diff Integration (P2)

- [x] `p2` - **ID**: `cpt-code-split-fr-ci-diff`

`code-split check --baseline <snapshot>` SHOULD act as a CI regression
gate: exit non-zero when the current tree introduces *new* violations
versus the baseline (e.g. new cycles added, HK degraded beyond a limit).
The base-branch snapshot is fetched from a stored CI artifact; the verdict
JSON (`--output-format json`) and the `report --baseline` diff HTML are
attached to the pull request automatically.

**Actors**: `cpt-code-split-actor-ci`, `cpt-code-split-actor-pr-reviewer`

## 6. Non-Functional Requirements

### 6.1 NFR Inclusions

#### Offline Operation

- [x] `p1` - **ID**: `cpt-code-split-nfr-offline`

All P1 components (Rust plugin, `code-split check`, `code-split report`,
and `--baseline` comparisons) MUST operate without network access. External resources (CDNs, APIs, LLM
endpoints) are forbidden at P1. All JavaScript and CSS dependencies in
generated HTML MUST be bundled into the `code-split` binary as embedded
assets; no CDN or external resource references in generated HTML.

**Threshold**: Zero outbound network calls during any P1 operation.

**Rationale**: Workspaces may be on air-gapped machines, private CI
runners, or laptops without connectivity. Offline-first is a hard
requirement shared by all three steps.

#### Performance

- [x] `p1` - **ID**: `cpt-code-split-nfr-performance`

The Rust plugin MUST complete graph extraction for a 50k-LOC workspace
in ≤ 30 seconds wall-clock on a modern developer laptop (8-core, 16 GB
RAM, SSD), measured cold-cache. The `code-split report` and `code-split check`
subcommands MUST each complete in ≤ 5 seconds for graphs with up to
10,000 nodes (including a `--baseline` comparison).

**Threshold**: ≤ 30 s for the plugin at 50k LOC; ≤ 5 s for each
subcommand at 10k nodes.

**Rationale**: Interactive use requires sub-minute turnaround.

#### Artifact Portability

- [x] `p1` - **ID**: `cpt-code-split-nfr-portability`

JSON snapshot artifacts MUST conform to Graph JSON Schema v1 and MUST
be readable by the report generator and baseline comparison without migration
for all v1.x releases. Generated HTML reports MUST open correctly in
Chrome, Firefox, and Safari without installation.

**Threshold**: Zero schema-migration failures within a major version.

**Rationale**: Artifacts stored as CI artifacts must remain readable
across plugin and tool version bumps within a major version.

### 6.2 NFR Exclusions

- **Accessibility**: Out of scope for v1.0.
- **Internationalization**: English-only in v1.0.
- **Regulatory Compliance**: Not applicable — the tool reads local
  source files only and produces no personal or regulated data.

## 7. Public Interfaces

### 7.1 Code Split Unified CLI

- [x] `p1` - **ID**: `cpt-code-split-interface-cli`

**Type**: Single CLI binary (`code-split`)

**Stability**: unstable (pre-1.0)

**Subcommands**: bare `code-split` prints help — there is no default
command; every action is an explicit subcommand.

```
# Lint — gate on cycle rules & thresholds; writes no files
code-split check  [input] [--plugin <name|auto>] [--threshold ...] [--cycle-rule ...] [--baseline <snapshot>] [--output-format <human|json|github|sarif>] [--exit-zero]

# Steps 1+2 — analyze (or read) the input and write a snapshot and/or HTML viewer
code-split report [input] [--plugin <name|auto>] [--output.<fmt>.path <path>] [--baseline <snapshot>]
```

The positional `[input]` (default `.`) is polymorphic: a directory is
analyzed, while a `.json` snapshot or `.html` report is read for its
embedded snapshot (no analysis). Step 4 is `--baseline <snapshot>`, accepted
by both commands: `report --baseline` writes a baseline↔current diff HTML
viewer with a verdict, and `check --baseline` is a relative CI gate (fail
only on new violations) whose verdict is machine-readable with
`--output-format json`.

Global options accepted by every command: `--config <PATH | KEY=VALUE>`
(repeatable; inline wins), `--color <when>`, `-v/--verbose`, `-q/--quiet`,
`-h/--help`, `-V/--version`.

**Exit codes**: 0 = `check` passed (or `--exit-zero`), `report`
completed; non-zero = generic failure, or `check` found a violation;
failures emit a structured JSON error on stderr.

**Breaking Change Policy**: Adding flags or subcommands is minor;
renaming or removing flags, changing JSON artifact schema, or changing
exit-code semantics requires a major-version bump.

### 7.2 Plugin Model

- [x] `p1` - **ID**: `cpt-code-split-interface-plugin-binary`

**Type**: Built-in, in-process analyzer

**Stability**: unstable (pre-1.0)

Plugins are compiled into the `code-split` binary and run **in-process**
when a command analyzes a workspace (`code-split check` / `code-split
report`). The plugins are `rust`, `python`, `javascript`, and `typescript`,
selected with `--plugin <name>` (see `cpt-code-split-fr-plugin-discovery`).
There is no subprocess invocation, no external plugin binary, and no
external/dynamic plugin loading.

Each plugin implements the `LanguagePlugin` trait (`code-split-plugin-api`) as a
**pure parser**: `analyze(workspace, level, input)` returns a structural `Graph`
(nodes + edges, **no metrics**), and `levels()` declares the level's semantics
dictionaries. The orchestrator computes all metrics centrally
(`code-split-complexity` by file extension; cycles / Henry-Kafura / stats in
`code-split-graph` over the level's flow edges), writing them into node
attributes by id, and assembles the snapshot. Adding a language means adding a
built-in plugin crate and one line in `plugin::registry()`.

### 7.3 Graph JSON Schema

- [x] `p1` - **ID**: `cpt-code-split-interface-graph-schema`

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
  "versions":       { "code-split": "1.0.0-alpha.3", "rustc": "1.78.0" },
  "roots":          { "target": "<abs>", "registry": "<abs>" },
  "git":            { "branch": "main", "commit": "a3f9c21b4d5e", "dirty_files": 0, "origin": "git@…:team/proj.git" },
  "timings":        [ { "stage": "rust", "ms": 0, "detail": "…" }, … ],
  "graphs": {
    "files": {
      "edge_kinds":       { "<kind>": { "flow": true, "label": "…", "hint": "…" } },
      "node_attributes":  { "<key>": { "value_type": "int|float|str|bool", "label": "…", "group": "<group?>" } },
      "edge_attributes":  { "<key>": { "value_type": "…", "label": "…" } },
      "attribute_groups": { "<group>": { "label": "…", "hint": "…" } },
      "nodes": [...], "edges": [...], "cycles": [...], "stats": { ... }
    }
  }
}
```

`graphs` is a map `level_name → level`; today only `files`. The four semantics
dictionaries are always present (possibly empty), pruned to the keys/kinds/groups
actually present at that level.

**Node shape** — `id`, `kind`, `name`, optional `parent`, plus flat attributes:

```json
{ "id": "{target}/src/foo.rs", "kind": "file", "name": "foo.rs",
  "visibility": "public", "loc": 48, "sloc": 42, "lloc": 12, "cloc": 4, "blank": 6,
  "cyclomatic": 3, "cognitive": 2, "exits": 2, "args": 3,
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
attributes are **flat** and a metric is **omitted when it rounds to zero**.
Numeric values use 3-significant-digit rounding; integral values serialize
without a decimal point. `fan_in` / `fan_out` / `hk` count internal flow edges
only; edges whose target is external are counted in `fan_out_external`. `cycle`
(`"mutual"` / `"chain"` / `"test_embed"`) is present only on nodes in a cycle.

**Edge shape**:

```json
{ "source": "<node-id>", "kind": "uses | reexports | contains", "target": "<node-id>" }
```

An edge is **external iff its `target` is an `ext:` node** — there is no
`edge.external` flag. Whether an edge kind is information flow vs. structural is
read from `edge_kinds[kind].flow` (e.g. `contains` is `flow: false` — kept and
shown as ownership, excluded from fan_in / HK / cycles). Edge attributes (e.g. a
Rust `reexports` edge's `visibility`) are flattened in alongside `source` /
`kind` / `target`.

**Stats shape** (`stats` field on a level) — a flat map of the mean of each
tracked numeric metric across the level's file nodes (zero/missing excluded; a
metric emitted only when its average is positive), e.g.:

```json
{ "cyclomatic": 1.4, "cognitive": 1.8, "fan_in": 2.25, "fan_out": 3, "hk": 864,
  "mi": 104.0, "mi_sei": 105.7, "sloc": 15.8, "cloc": 3.8, "blank": 6.8,
  "length": 32.2, "vocabulary": 19.6, "volume": 149.1, "effort": 1030.4,
  "time": 57.2, "bugs": 0.029 }
```

Percentiles are not stored — a viewer can compute them client-side from raw node
data.

**Breaking Change Policy**: Additive fields are minor; renames or
removals require a major-version bump and migration notes.

## 8. Use Cases

### UC-001 Analyze Rust Workspace Offline

**ID**: `cpt-code-split-usecase-analyze-offline`

**Actors**: `cpt-code-split-actor-developer`

**Preconditions**: The target directory is a valid Cargo workspace;
the `code-split` binary is installed.

**Main Flow**:

1. Developer runs `code-split report . --plugin rust` (analyzes the
   workspace and writes both a snapshot and an HTML viewer in one step)
2. `code-split` writes `.code-split/axum-api-20260522-112233.json` (the
   snapshot) and `.code-split/axum-api-20260522-112233.html` (the viewer)
3. Developer opens `.code-split/axum-api-20260522-112233.html` in a browser,
   sorts files by coupling weight
4. Developer identifies the heaviest files and decides what to refactor

(For a non-blocking lint that gates on cycles/thresholds and writes no
files, the developer can instead run `code-split check . --plugin rust`.)

**Postconditions**: A self-contained HTML viewer exists at
`.code-split/axum-api-20260522-112233.html`; no network access was required
at any step.

**Alternative Flows**:

- **Plugin fails (cargo metadata error)**: Plugin exits non-zero with
  a structured JSON error on stderr; no JSON files are written.

### UC-002 Before/After Refactoring Comparison

**ID**: `cpt-code-split-usecase-diff-refactor`

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

**Preconditions**: A baseline snapshot exists from a prior run; the
developer has made structural changes to the codebase.

**Main Flow**:

1. Developer runs
   `code-split report . --baseline .code-split/snap-before.json --output.html.path=diff.html`
   (analyzes the current tree and compares it against the baseline in one run)
2. Developer opens `.code-split/diff.html` to see coupling changes
   color-coded by per-node diff state, with the baseline↔current verdict
3. Developer reads the machine-readable verdict with
   `code-split check . --baseline .code-split/snap-before.json --output-format json`

(Because `[input]` is polymorphic, the developer can instead capture the
current state first — `code-split report . --output.json.path=snap-after.json`
— then compare two existing snapshots without re-analyzing:
`code-split report snap-after.json --baseline .code-split/snap-before.json
--output.html.path=diff.html`.)

**Postconditions**: A diff HTML report exists and a machine-readable
verdict is available; the verdict quantifies whether the refactoring
improved the architecture.

**Alternative Flows**:

- **Schema version mismatch**: the comparison exits non-zero with an error
  identifying the incompatible artifact; no report is produced.

### UC-003 CI Structural Gate on Pull Request

**ID**: `cpt-code-split-usecase-ci-diff`

**Actors**: `cpt-code-split-actor-ci`, `cpt-code-split-actor-pr-reviewer`

**Note**: This use case is targeted at P2.

**Preconditions**: The base-branch snapshot is stored as a CI artifact;
the PR branch has been pushed.

**Main Flow**:

1. CI downloads the base-branch snapshot to `.code-split/snap-base.json`
2. CI runs `code-split check . --baseline .code-split/snap-base.json --output-format json`
   to gate the PR — it fails only on *new* violations versus the base
3. CI runs
   `code-split report . --baseline .code-split/snap-base.json --output.html.path=diff.html`
   to render the shareable diff viewer
4. CI attaches `.code-split/diff.html` to the PR and posts the verdict from
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
- [x] JSON artifacts conform to Graph JSON Schema v1 and pass schema
  validation
- [ ] A `--baseline` comparison exits non-zero with a structured error on
  schema version mismatch

## 10. Dependencies

| Dependency | Description | Priority |
|------------|-------------|----------|
| `cargo_metadata` crate | Cargo workspace enumeration (local vs. external crates) | p1 |
| `syn` crate | Rust source parsing for the module tree and `use` statements | p1 |
| `petgraph` crate | In-memory graph representation in the Rust plugin | p1 |
| `rust-code-analysis` crate | Tree-sitter-based multi-language metrics library (cyclomatic, cognitive, Halstead, MI, LOC, NOM, nexits, nargs); used via fork `ffedoroff/rust-code-analysis` | p1 |
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

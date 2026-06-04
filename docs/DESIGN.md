# Technical Design — Code Split

<!-- toc -->

- [1. Architecture Overview](#1-architecture-overview)
  - [1.1 Architectural Vision](#11-architectural-vision)
  - [1.2 Architecture Drivers](#12-architecture-drivers)
  - [1.3 Architecture Layers](#13-architecture-layers)
- [2. Principles & Constraints](#2-principles--constraints)
  - [2.1 Design Principles](#21-design-principles)
  - [2.2 Constraints](#22-constraints)
- [3. Technical Architecture](#3-technical-architecture)
  - [3.1 Domain Model](#31-domain-model)
  - [3.2 Component Model](#32-component-model)
  - [3.3 API Contracts](#33-api-contracts)
  - [3.4 Internal Dependencies](#34-internal-dependencies)
  - [3.5 External Dependencies](#35-external-dependencies)
  - [3.6 Interactions & Sequences](#36-interactions--sequences)
  - [3.7 Plugin System](#37-plugin-system)
  - [3.8 CLI Reference and Examples](#38-cli-reference-and-examples)
- [4. Additional Context](#4-additional-context)
- [5. Traceability](#5-traceability)

<!-- /toc -->

> **Component designs.** This is the product technical design — architecture,
> principles, domain model, the plugin/extraction crates and the plugin system.
> The two consumer components have their own design docs: the orchestrator
> binary in [`code-split-cli/DESIGN.md`](code-split-cli/DESIGN.md) and the
> offline HTML viewer in
> [`code-split-viewer/DESIGN.md`](code-split-viewer/DESIGN.md).

## 1. Architecture Overview

### 1.1 Architectural Vision

Code Split is a pipeline: **extract → evaluate / visualize → (user
modifies) → compare**. The platform is built around a single portable
JSON artifact format that decouples the extraction layer (plugins) from
the consumption layer (the `check` linter and `report` artifact writer).
Either layer can evolve independently as long as the schema version is
respected.

At P1 the platform ships three components:

- **Rust Plugin** (`code-split-rust`): a Cargo workspace analyzer built
  on `syn` (syntactic analysis). It builds the Rust module graph and
  collapses it to a single **file graph**; produces a single snapshot
  per run
- **Check** (`code-split check`): built into `code-split-cli`; analyzes (or
  reads) the input, evaluates cycle rules and thresholds, prints diagnostics,
  and exits non-zero on violation. With `--baseline <snapshot>` it switches
  to a **relative gate** — failing only on *new* violations vs the baseline —
  and emits a verdict (`improved` / `degraded` / `neutral`). Writes no files
- **Report** (`code-split report`): built into `code-split-cli`; analyzes (or
  reads) the input and writes artifacts — a snapshot `.json` and/or a single
  self-contained offline HTML viewer; all JS/CSS assets embedded in the binary
  via `include_str!`. With `--baseline <snapshot>` the HTML becomes a
  baseline↔current diff with a verdict (one shared union layout where the
  Baseline/Current toggle is a CSS visibility flip so common nodes never move),
  named `…-diff.html`. It also emits two refactoring-guidance formats
  (`--output.prompt` / `--output.scorecard`) — the console counterpart of the
  viewer's Prompt Generator, computed by the `recommend` module

The three pillars of the design are:

1. **JSON-first artifact contract** — the single snapshot is the
   sole handoff between all components; any plugin can feed any
   consumer
2. **Offline-first** — every P1 component runs without network access;
   generated HTML reports inline all assets
3. **Pluggable extraction layer** — the built-in plugins (`rust`,
   `python`, `javascript`) all produce the same JSON artifact, so new
   languages can be added as built-in plugins without touching the
   consumer tools

### 1.2 Architecture Drivers

#### Functional Drivers

| Requirement | Design Response |
|-------------|-----------------|
| `cpt-code-split-fr-rust-plugin` | Implemented by the `code-split-plugin-rust` crate (cargo metadata + `syn`), which collapses the module graph to a file graph. Dispatched in-process by `code-split-cli`'s `plugin` registry. Outputs a single snapshot `.json`. |
| `cpt-code-split-fr-lang-plugins` (Python, JS/TS) | Python: `code-split-plugin-python` using `tree-sitter-python`. JS/TS: `code-split-plugin-javascript` using `tree-sitter-javascript` / `tree-sitter-typescript`, supporting both ESM and CommonJS. Both emit `File` nodes + file→file `uses` edges + `External` library nodes, and annotate per-file complexity via the shared `code-split-plugin` crate. |
| `cpt-code-split-fr-file-graph` | All plugins emit a single file graph: `File` nodes with `uses` / `reexports` edges between files, plus `External` library nodes at depth 1 reached by `uses` edges flagged `external: true`. The Rust plugin derives it by collapsing its module graph; Python/JS/TS build it directly from import resolution. |
| `cpt-code-split-fr-html-report` | Built-in Rust renderer in `code-split-cli`: `report` analyzes (or reads) the input, then renders an HTML template with inline assets alongside the JSON snapshot. |
| `cpt-code-split-fr-node-sorting` | Node weight (fan-in + fan-out) is computed at render time and embedded in the HTML; client-side JavaScript sorts the table on user interaction. |
| `cpt-code-split-fr-ai-prompts` | HTML: the viewer's Prompt Generator (`export-popup.js`). CLI: the `recommend` module (`code-split-cli/src/recommend.rs`) drives the `report --output.prompt` (LLM Markdown for one principle) and `--output.scorecard` (console triage) formats from the snapshot's calibrated thresholds — advisory, no exit code. |
| `cpt-code-split-fr-graph-diff` | Browser-side diff in the HTML viewer (`diff.js` `computeDiff`/`computeCycles`): node/edge set difference on the file graph and `affected` propagation, from the two embedded snapshots. The `check --baseline` regression gate is rule-based (re-evaluates rules on the baseline), not a structured graph diff. |
| `cpt-code-split-fr-diff-html-report` | With `report --baseline <snapshot>` the viewer becomes a self-contained diff with color-coded baseline/current views and a verdict; all assets inlined; the file is named `…-diff.html`. |
| `cpt-code-split-fr-diff-text-report` | `check --baseline <snapshot> --output-format json` emits the machine-readable verdict (`improved` / `degraded` / `neutral`) and the list of new violations for CI parsing. |

#### NFR Allocation

| NFR ID | Summary | Allocated To | Design Response |
|--------|---------|--------------|-----------------|
| `cpt-code-split-nfr-offline` | Zero outbound network calls | All components | Rust plugin: no HTTP; `code-split check` / `code-split report`: HTML assets embedded in binary, no CDN references in generated output. |
| `cpt-code-split-nfr-performance` | ≤ 30 s @ 50k LOC (plugin); ≤ 5 s @ 10k nodes (check/report) | `code-split-plugin-rust`, `code-split-plugin`, `code-split-cli` | Syntactic analysis + the module→file collapse run in seconds (no rust-analyzer); `check` / `report` process a snapshot in a single pass. |
| `cpt-code-split-nfr-portability` | JSON artifacts stable within a major version | All components | Schema version field in `meta`; consumers abort on mismatch; additive-only changes within a major version. |

### 1.3 Architecture Layers

```mermaid
flowchart TD
    subgraph step1["Language plugins (Rust binary)"]
        cli["code-split-cli<br/>(registry + dispatch via trait)"]
        api["code-split-plugin-api<br/>(LanguagePlugin trait)"]
        pr["code-split-plugin-rust<br/>(cargo metadata + syn, module→file collapse)"]
        pp["code-split-plugin-python<br/>(tree-sitter)"]
        pj["code-split-plugin-javascript<br/>(tree-sitter)"]
        plug["code-split-plugin<br/>(complexity, finalize, logging)"]
        core["code-split-graph<br/>(graph types + JSON schema)"]
        cli -->|"dyn LanguagePlugin"| api
        pr -.implements.-> api
        pp -.implements.-> api
        pj -.implements.-> api
        pr --> plug
        pp --> plug
        pj --> plug
        api --> core
        plug --> core
        pr --> core
        pp --> core
        pj --> core
    end

    core -->|"snapshot (one `files` graph)"| artifacts["In-memory or JSON snapshot"]

    subgraph consume["Consumers (built-in Rust)"]
        check["code-split check<br/>(code-split-cli)"]
        report["code-split report<br/>(code-split-cli + code-split-viewer)"]
    end

    artifacts --> check
    artifacts --> report
    check -->|"diagnostics + exit code (verdict with --baseline)"| gate["CI gate"]
    report -->|"{ts}-{git-hash-3}.{json,html} (-diff.html with --baseline)"| html_report["Snapshot + HTML viewer"]
```

| Layer | Responsibility | Technology |
|-------|---------------|------------|
| Plugin — Presentation | Argument parsing, output routing, artifact writing | `clap`, `anyhow` (Rust) |
| Plugin — Application | Dispatch language plugins, assemble the snapshot | `code-split-cli` (Rust) |
| Plugin — Domain | Generic graph model + operations (cycles/hk/stats/snapshot) | `code-split-plugin-api`, `code-split-graph`, `serde` (Rust) |
| Plugin — Contract | The `LanguagePlugin` trait every language plugin implements; the CLI works only against it | `code-split-plugin-api` (Rust) |
| Plugin — Infrastructure | Per-language analysis (one crate each, behind the trait) on a shared utility layer (complexity, finalize, logging) | `code-split-plugin-rust`/`-python`/`-javascript`, `code-split-plugin`, `syn`, `tree-sitter`, `rust-code-analysis` (Rust) |
| Check | Analyze (or read) input, evaluate rules and (with `--baseline`) regressions, print diagnostics, exit non-zero on violation | `code-split-cli` (Rust) |
| Report | Analyze (or read) input, write snapshot JSON + offline HTML viewer (a diff with `--baseline`) | `code-split-cli` + `code-split-viewer` (Rust), Graphviz WASM bundled in binary, assets embedded via `include_str!` |

## 2. Principles & Constraints

### 2.1 Design Principles

#### JSON Artifact Contract as the Sole Integration Surface

- [x] `p1` - **ID**: `cpt-code-split-principle-json-contract`

The single JSON snapshot (one `files` graph plus metadata) is the
ONLY handoff between the plugin layer and the consumer layer. No
in-process coupling between the analysis crates and the report
rendering code is permitted. This contract is versioned via
`schema_version`; consumers abort on a version mismatch.

#### Offline-First

- [x] `p1` - **ID**: `cpt-code-split-principle-offline-first`

Every P1 component must work without network access. Generated HTML
files must contain no external resource references. This is a design
constraint, not a preference — it must be verified in CI.

#### Files-Only Graph Model

- [x] `p1` - **ID**: `cpt-code-split-principle-files-only`

The model is a **generic property graph** (free-form node/edge `kind` +
attribute maps), but today the snapshot carries exactly one level: **files**.
Node kinds in output are `"file"` (a project source file, carrying all metrics)
and `"external"` (a third-party library at depth 1 — one node per library, never
expanded). The one **information-flow** edge kind is `uses` between files; the
structural `contains` (module ownership), `reexports` (a `pub use` facade), and
`super` (a glob `use super::*` / `use crate::<ancestor>::*` namespace pull from an
enclosing module) are recorded but non-flow — excluded from fan-in / HK / cycles
and not drawn. An edge is external iff its target is an
`external` node. There is no module, function, or call graph yet: plugins resolve
everything to file→file dependencies before the snapshot is written. The `graphs`
map and the per-level semantics dictionaries leave room for `module` / `function`
levels later with no model change.

#### Internal Coupling Excludes External Libraries

- [x] `p1` - **ID**: `cpt-code-split-principle-internal-coupling`

`fan_in`, `fan_out`, and Henry-Kafura (`HK = sloc × (fan_in × fan_out)²`)
are computed from **internal** file→file edges only. Edges to `External`
library nodes are excluded from these counts and from HK, and are
surfaced separately in `coupling.fan_out_external`. Rationale: HK
measures internal architectural coupling, not the breadth of 3rd-party
library usage, which would otherwise drown out real structural signal.

#### Pluggable Extraction, Stable Consumers

- [x] `p1` - **ID**: `cpt-code-split-principle-pluggable`

The `check` linter and `report` artifact writer are schema consumers, not
language-aware tools. Adding a new language plugin MUST NOT require
changes to any consumer tool. All language-specific knowledge lives
exclusively in the plugin.

### 2.2 Constraints

#### Stable Rust Toolchain

- [x] `p1` - **ID**: `cpt-code-split-constraint-stable-rust`

The Rust plugin must build on stable Rust. `rustc_private` and
nightly-only features are prohibited.

#### Python 3.9+ Minimum

- [x] `p3` - **ID**: `cpt-code-split-constraint-python`

The built-in Python language plugin targets Python 3.9+ as the minimum
version to analyze. No Python runtime is required by the `code-split`
binary itself; the constraint applies to the target workspace being
analyzed, not the execution environment.

## 3. Technical Architecture

### 3.1 Domain Model

**Technology**: a **generic property-graph** model. The contract types live in
`code-split-plugin-api`; the serializable snapshot and computed-data types live
in `code-split-graph`.

The model is deliberately language-agnostic: there are **no** `NodeKind` /
`EdgeKind` / `Visibility` enums and no fixed metric field set. A node has a
free-form string `kind` and a free-form attribute map; a source file is just
`kind == "file"`. The core never interprets `kind`; it reads only the attribute
keys it understands, described per level by the semantics dictionaries.

| Entity | Description | Location |
|--------|-------------|----------|
| Graph | Pure structure: `nodes: Vec<Node>` + `edges: Vec<Edge>`. No computed data. What a plugin's `analyze` returns. | `crates/code-split-plugin-api/src/graph.rs` |
| Node | `id: NodeId`, `kind: String` (`"file"` / `"external"` today), `name: String`, `parent: Option<NodeId>`, `attrs: Attributes` (flattened into the JSON object). | `crates/code-split-plugin-api/src/node.rs` |
| Edge | `source: NodeId`, `target: NodeId`, `kind: String` (`"uses"` / `"contains"` / `"reexports"` / `"super"`), `attrs: Attributes` (usually empty; e.g. a Rust `reexports` edge carries `visibility`). An edge is **external iff its `target` node is external** — there is no `edge.external` flag. | `crates/code-split-plugin-api/src/edge.rs` |
| Attributes | `BTreeMap<String, AttrValue>` (alphabetical → byte-stable). Plugins fill **structural** keys (`path`, `loc`, `visibility`, `version`, `items`, `external`, `crate` — the Rust plugin's per-target owning crate, e.g. `bat` / `bat (bin)`, …); the orchestrator adds **computed** keys (`cyclomatic`, `cognitive`, `sloc`, `lloc`, `cloc`, `blank`, `mi`, `mi_sei`, `length`, `vocabulary`, `volume`, `effort`, `time`, `bugs`, `fan_in`, `fan_out`, `fan_out_external`, `hk`, `cycle`) into the same map by node id. All flat — no nesting. Zero-valued metrics are omitted. | `crates/code-split-plugin-api/src/attrs.rs` |
| AttrValue | Untagged scalar: `Bool` / `Int` / `Float` / `Str` (serialized to its natural JSON form). Metric producers round to 3 significant digits and store an integral value as `Int` so e.g. `1.0` serializes as `1`. | `crates/code-split-plugin-api/src/attrs.rs` |
| NodeId | Stable string key. A **file node's id IS its relativized path** — `{target}/src/a.rs` (no `file:` prefix); an external node is `ext:{name}` (`ext:serde`). During analysis a file id is its absolute path; the orchestrator relativizes it against the named roots. | `crates/code-split-plugin-api/src/node.rs` |
| Level | What a plugin can produce, with its semantics dictionaries: `name`, `edge_kinds`, `node_attributes` / `edge_attributes`, `attribute_groups`, plus `node_kinds: BTreeMap<String, NodeKindSpec>` and `cycle_kinds: BTreeMap<String, CycleKindSpec>` (seeded from `default_node_kinds()` / `default_cycle_kinds()`, overridable), plus an optional `grouping: Grouping` telling the viewer how to cluster nodes — `{ key }` (group by a node attribute's value, e.g. `crate`) or `{ function }` (a named viewer grouper, e.g. `dir`). The orchestrator merges the plugin's structural attribute specs with the central complexity + coupling specs, overlays the plugin's `thresholds()`, computes the `ui` block, then prunes everything to what is actually present. The `node_attributes` dictionary keeps every key carried by *any* node (external included, so the viewer can still label e.g. an external node's `path`), while the `ui` render lists are filtered more tightly to keys present on at least one **internal** (non-external) node — those surfaces (table, summary, sort) never show external rows. | `crates/code-split-plugin-api/src/level.rs` |
| EdgeKindSpec | `flow: bool` (single source of truth — counted/drawn when `true`; structural like `contains` when `false`), plus optional `label` / `description`. | `crates/code-split-plugin-api/src/level.rs` |
| AttributeSpec | Everything the UI needs to render a metric from data: `value_type`, `label`, `name` (tooltip title), `short` (table header), `description`, `formula` (display), `calc` (an `eval`-able JS expression over sibling attrs — the live derivation), `direction` (`higher_better`/`lower_better`, for delta colour), `abbreviate` (K/M), `group`, `thresholds {info, warning}`. All optional but `value_type`. | `crates/code-split-plugin-api/src/level.rs` |
| NodeKindSpec / CycleKindSpec | Per-kind UI semantics. `NodeKindSpec`: `label`/`plural`/`fill`/`stroke`/`external`. `CycleKindSpec`: `label`/`description`. Generic defaults from `default_node_kinds()` / `default_cycle_kinds()` in `code-split-plugin-api`. | `crates/code-split-plugin-api/src/level.rs` |
| Thresholds | `{ info: f64, warning: f64 }` — two-tier per-metric thresholds; language-calibrated, returned by a plugin's `thresholds()` and overlaid onto the matching `AttributeSpec`. | `crates/code-split-plugin-api/src/level.rs` |
| Preset | A Prompt-Generator principle: `id`, `label`, `title`, `prompt`, `doc_url?`, `sort_metric`, `connections`. The orchestrator builds a generic default catalog (`code-split-cli/src/presets.rs`) and a plugin's `presets(defaults, input)` hook may pass through / edit / extend it. Stored top-level in the snapshot. | `crates/code-split-plugin-api/src/plugin.rs` |
| CycleGroup | SCC with ≥ 2 nodes: `kind: String` (`"test_embed"` / `"mutual"` / `"chain"`), `nodes: Vec<NodeId>`. Each member node also carries a `cycle` attribute. | `crates/code-split-graph/src/level_graph.rs` |
| LevelUi | Computed UI hints: `default_sort`, `sort_metrics`, `size_metrics`, `card_metrics`, `columns`, `summary_metrics` — each a curated metric order filtered to the attributes present on internal nodes, so the viewer renders them verbatim and hardcodes none of it — plus an optional `grouping` (carried through from the level spec, pruned to a usable attribute) telling the viewer how to cluster diagram nodes. | `crates/code-split-graph/src/level_graph.rs` |
| LevelGraph | One analysis level in the snapshot: the semantics dictionaries (`edge_kinds`/`node_attributes`/`edge_attributes`/`attribute_groups`/`node_kinds`/`cycle_kinds`) + `nodes` + `edges` + `cycles: Vec<CycleGroup>` + `stats: BTreeMap<String, AttrValue>` (flat averages) + `ui: LevelUi`. | `crates/code-split-graph/src/level_graph.rs` |
| Snapshot | The `.json` artifact: `schema_version: "2"`, `generated_at`, `command`, `workspace`, `target`, `plugin`, `config_file?`, `versions`, `roots`, `git?`, `timings`, `graphs: BTreeMap<String, LevelGraph>`, and top-level `presets: Vec<Preset>`. Serialized via `to_canonical_string_pretty` — **canonical JSON** (alphabetical keys; `nodes`/`edges` sorted). | `crates/code-split-graph/src/snapshot.rs` |
| StageTime | Per-stage timing entry: `stage`, `ms`, `detail`. Stored in `Snapshot.timings` in execution order. | `crates/code-split-graph/src/snapshot.rs` |

**Relationships**:

- `Node` → `Node`: linked via `Edge` (`source` → `target`).
- `Graph` → `Node`/`Edge`: ownership; a node carries an optional `parent`
  pointing to a containing node (unused on file nodes today — diagram clustering
  is driven by the level's `ui.grouping`, not the `parent` field).
- Snapshot diff (`--baseline`) is computed **browser-side** by the viewer
  (`diff.js`) from the two embedded snapshots; there is no server-side
  `compare_snapshots` / `CompareSummary` (removed). `check --baseline` gates
  by re-evaluating rules on the baseline, not by a structured graph diff.

### 3.2 Component Model

#### code-split-graph

- [x] `p1` - **ID**: `cpt-code-split-component-core`

Operations **over** the generic model (defined in `code-split-plugin-api`):
cycle detection, Henry-Kafura coupling, aggregate stats, id relativization, and
the serializable `Snapshot` / `LevelGraph` types. Language-agnostic, zero I/O.
Depends on `code-split-plugin-api`, `serde`, `chrono`, `anyhow` only — no
`petgraph`, `cargo_metadata`, `syn`, or analyzers. Which edge kinds count as
information flow is **not hardcoded** — every pass takes a `flow_kinds: HashSet<String>`
the orchestrator derives from the level's `EdgeKindSpec.flow`.

Modules:

- **`cycles.rs`** — `annotate_cycles(graph, flow_kinds) -> Vec<CycleGroup>`:
  Kosaraju SCC over flow edges (today just `uses`). `contains`,
  `reexports`, and `super` are non-flow (`EdgeKindSpec.flow = false`), so a `mod foo;`
  parent/child pair, a `pub use` facade hub (`lib.rs` / `mod.rs`), and a
  `use super::*` namespace pull never
  fabricate cycles. An SCC whose members span **more than one crate** is dropped
  — Rust forbids circular crate dependencies, so it can only be a resolution
  artifact (crate identity read from the node `crate` attribute, falling back to
  the path). Classifies each surviving SCC `"test_embed"` (any member under
  `tests/` / `test_support/` / a `*_test[s]` file) / `"mutual"` / `"chain"` and
  writes a `cycle` attribute on each member node.
- **`hk.rs`** — `annotate_hk(graph, flow_kinds)`: writes `fan_in` / `fan_out` /
  `fan_out_external` / `hk` (`hk = sloc × (fan_in × fan_out)²`) into each
  internal node's `attrs`. `fan_in` / `fan_out` count unique **internal** flow
  partners only; edges whose target is external are counted into
  `fan_out_external` instead. Zero values are omitted.
- **`stats.rs`** — `compute_stats(graph) -> BTreeMap<String, AttrValue>`: the
  mean of each tracked numeric metric across the file nodes (zero/missing values
  excluded; a metric emitted only when its average is positive).
- **`finalize.rs`** — `finalize_graph`: drop self-loops, dedup edges on
  `(source, target, kind)`, prune unreferenced external nodes, sort.
- **`snapshot.rs`** — the top-level `Snapshot` artifact (`schema_version`,
  header, `graphs` map, `presets`) plus its header types `GitInfo` / `StageTime`.
- **`level_graph.rs`** — the widely-imported per-level payload types: `LevelGraph`
  (graph + semantics dictionaries + computed cycles/stats/UI), `LevelUi`, and
  `CycleGroup`. Split out from `snapshot.rs` so their fan-in lands here, not on
  the artifact module (keeps each file's Henry-Kafura low).
- **`relativize.rs`** — `relativize_graph` / `relativize_level`: map absolute
  file paths to `{target}` / `{registry}` / … tokens (following edges, parents
  and cycle node lists) and drop a file node's redundant `path`.
- **`serialize.rs`** — **canonical serialization** (`to_canonical_string` /
  `to_canonical_string_pretty`): round-trips through `serde_json::Value` (a
  `BTreeMap`, so keys come out alphabetical) and sorts `nodes` by `id` and
  `edges` by `source`/`target`/`kind`, byte-stable for unchanged input. Generic
  over any `Serialize`; depends on nothing else in the crate.
- **`attrs.rs`** — the shared attribute helpers every enrichment pass pulls in:
  `round_sig3` / `num_attr` (numeric rounding + the `f64 → AttrValue` bridge) and
  the `attr_f64` / `is_external` reads/predicate. A **leaf** module that depends
  only on the plugin API, never on the crate root — so the passes import helpers
  from here instead of `use crate::…`, which would otherwise close a
  `submodule → lib.rs → submodule` dependency cycle.
- **`lib.rs`** — declares the submodules (`pub mod attrs; … pub mod stats;`) and
  holds `coupling_specs()` (the coupling/cycle `AttributeSpec`s + the `coupling`
  group, merged in by the orchestrator). It carries **no `pub use` re-exports** —
  consumers import each item from its owning submodule path
  (`code_split_graph::snapshot::Snapshot`, `…::cycles::annotate_cycles`,
  `…::attrs::num_attr`, …). Keeping the crate root off the flow graph (no
  re-export hub) is deliberate: a flat prelude gave `lib.rs` a flow edge to every
  submodule (`fan_out`), and Henry-Kafura — `sloc × (fan_in × fan_out)²` —
  squares that, so the root scored a large false-positive HK. (There is no
  server-side snapshot-diff module — `--baseline` diffing is done browser-side by
  the viewer's `diff.js`.)

#### code-split-plugin-api

- [x] `p1` - **ID**: `cpt-code-split-component-plugin-api`

The **foundation** crate: it defines the generic model (`Node` / `Edge` /
`Graph` / `Attributes` / `AttrValue` / `Level` + the `EdgeKindSpec` /
`AttributeSpec` / `AttributeGroup` spec types) and the single trait,
`LanguagePlugin`. It depends on **nothing** of ours (only `serde` + `anyhow`)
and re-exports nothing; every other crate depends on it. The model lives in
topic submodules (`attrs` / `edge` / `graph` / `level` / `node` / `plugin`) and
consumers import from those paths (`code_split_plugin_api::node::Node`,
`…::graph::Graph`, …) rather than a flat crate-root prelude — the root has no
`pub use` hub, which keeps its `fan_out` (and therefore Henry-Kafura HK) low for
a crate that everything else depends on.

`LanguagePlugin` is a **pure parser** contract — `name`, `detect(ws, input)`
(can-parse, replacing markers), `levels` (the levels + their semantics
dictionaries), `analyze(ws, level, input) -> Graph` (**structure only**, no
metrics), `versions` (e.g. `rustc`). `PluginInput` carries `ignore` globs +
free-form `options`, so input can grow without trait changes.

The CLI works **only** against `dyn LanguagePlugin`. The single place that names
concrete plugins is `code-split-cli`'s `plugin::registry() -> Vec<Box<dyn
LanguagePlugin>>`; dispatch (`analyze`), `detect`, and `versions` all iterate
that list. Adding a language is: implement the trait in a new crate, add one
line to `registry()` — nothing else changes.

#### code-split-plugin-rust

- [x] `p1` - **ID**: `cpt-code-split-component-syn`

The Rust language plugin (implements `LanguagePlugin`; analysis in `analyze`),
dispatched by `code-split-cli`. It produces the Rust module graph via syntactic
analysis and collapses it to a file graph (see §3.7) before returning a generic
`api::Graph` — **structure only, no metrics** (complexity is added centrally by
the orchestrator). It builds with a crate-local typed model
(`src/internal.rs` — `Node` / `Edge` / `Visibility`) for the syn/collapse passes
and converts to the generic model at the boundary, so it depends on
`code-split-plugin-api` only (not `code-split-graph`, not `rust-code-analysis`).
Calls `cargo metadata` **with `--offline`** (code-split never hits the network —
it resolves from the warm cargo cache, surfacing an actionable error otherwise);
classifies crates as local vs. external; walks local source trees with `syn` to
extract the module hierarchy and `use` / `pub use` statements, emitting internal
crate / module / trait nodes and `contains` / `uses` / `reexports` / `super` edges
(`super` = a glob `use super::*` / `use crate::<ancestor>::*` reaching back into an
enclosing module — non-flow scope-sugar, not a real dependency). It also runs a `syn::visit`
path collector over each file to capture **bare qualified paths** in
expressions/types (≥ 2 segments, no `use`) **and qualified paths inside
`#[derive(...)]`** (e.g. `serde::Serialize`), resolved through the same
full resolver as `use` statements.

Resolution runs in **two phases** so cross-crate edges can be submodule-precise:
phase A walks every workspace crate, building all module nodes and a per-crate
library module index; phase B then resolves the collected `use` / bare-path
references against (1) the owning crate's index (intra-crate / `crate` / `self`
/ `super`), (2) the **workspace libraries** (each a module index + `pub use`
re-export table) — a cross-crate `other_crate::sub::Item` walks the dependency
crate's index to the file that owns `Item` (→ its `sub.rs`), falling back to the
crate root when the path stops at a root item — and (3) the extern-crate map
(registry deps → one crate-root / `External` node). `std`/`core`/keyword-only
paths are ignored; external crates are added as `Crate` nodes with
`external = true` and their source is never read. Resolution is
**re-export-aware**, both intra- and cross-crate: when a path's trailing segment
is not a submodule but a symbol the resolved module re-exports
(`pub use error::DomainError`, or another crate's `pub use access_scope::AccessScope`),
it follows the `pub use` chain to the file that **defines** the symbol instead of
anchoring on the facade — so `crate::X` / `super::X` and `other_crate::X` land on
the definer, not on a 17-line `lib.rs` / `mod.rs` hub (which would otherwise
collect a huge false `fan_in`). Module
node ids are namespaced **per target** (`mod:{pkg}::{kind}:{name}::…`): a package
exposing a library and a same-named binary (`bat` lib + `bat` bin) keeps two
distinct module trees, so `crate::X` in the library never mis-resolves onto the
binary's `main.rs` (which would otherwise invent an impossible library→binary
edge and an inflated `main.rs` fan_in). Each module records its owning crate,
surfaced on the collapsed file node as the per-target `crate` attribute. A
per-package `visited_files` `HashSet<PathBuf>` guard prevents double-walking
source files when a workspace has both `lib` and `bin` targets declaring the
same modules. A `mod` with `#[path = "…"]` is resolved via that
attribute (relative to the declaring file's directory) before the default
`name.rs` / `name/mod.rs` lookup.

These module-level nodes are **internal**: the Rust plugin's collapse
pass (see §3.7) folds them down to file nodes (`kind == "file"`, id = the
file's absolute path) and external library nodes (`ext:{name}`, carrying
`external: true`, the resolved `version`, and the dependency's source `path`)
before returning. The orchestrator then relativizes the absolute file ids to
`{target}/…` and the external `path` to `{registry}/…`.

**Edge sources & remaining blind spots**: file→file / file→library edges
come from three sources — (1) `use` / `pub use` statements; (2) bare
qualified paths in expressions/types (`commands::run()`, `other_crate::item`,
`crate::a::Alpha`); (3) qualified paths inside `#[derive(...)]`
(`serde::Serialize`) — all resolved the same way as `use`. A `mod foo;`
declaration emits a `Contains` edge that is kept in the JSON but treated as
structural ownership only — not drawn, not counted in fan_in / HK / cycles.
What remains uncaptured: any path **inside a macro body** (the macro's own path
is recorded, e.g. `anyhow::bail!`, but the tokens it wraps are not — macros are
never expanded), an old-style `extern crate foo;` (no path), and a cross-crate
reference into a registry dependency (no local module index) which still
collapses onto the single `External` node. A file reached only via `Contains`
(e.g. a module declared with `mod foo;` but never referenced by path or `use`)
has `fan_in` 0 and can appear isolated on the map.

#### code-split-complexity

- [x] `p1` - **ID**: `cpt-code-split-component-complexity`

The **central, language-agnostic** complexity pass — the single place that knows
`rust-code-analysis` (Mozilla, via the `ffedoroff/rust-code-analysis` fork on
branch `patch/update-tree-sitter-0.26.8`). Plugins emit structure only; this
crate computes metrics. Depends on `code-split-plugin-api` (the model) and
`code-split-graph` (for `num_attr`).

**Interface**: `annotate(graph: &mut Graph) -> usize`. It iterates every file
node (`kind == "file"`, whose `id` is the file's absolute path at this stage),
reads the file, picks a parser **by extension** (`rs` → Rust, `py` → Python,
`ts`/`mts`/`cts` → TypeScript, `tsx` → Tsx, `js`/`jsx`/`mjs`/`cjs` → JavaScript),
computes the file's root `FuncSpace`, and writes the metrics into the node's
`attrs` as flat keys. There is no per-plugin extension list or parser callback —
the dispatch lives here. Whole-file aggregate, so all functions, methods, arrow
functions and closures roll up into the file's single node.

`metric_specs()` exposes the metric `AttributeSpec`s + their groups (complexity /
halstead / loc / maintainability), which the orchestrator merges into each
level's dictionaries and then prunes to the keys actually present.

**Metrics written per file** (flat `attrs` keys; each omitted when it rounds to
zero; the LOC block is gated on `sloc > 0`, the Halstead block on `volume > 0`):

| Group | Keys |
|----------|------|
| complexity | `cyclomatic`, `cognitive`, `exits`, `args`, `closures` |
| maintainability | `mi`, `mi_sei` |
| loc | `sloc`, `lloc`, `cloc`, `blank` |
| halstead | `length`, `vocabulary`, `volume`, `effort`, `time`, `bugs` |

Coupling (`fan_in` / `fan_out` / `fan_out_external` / `hk`) and `cycle` are added
later by `code-split-graph` (`annotate_hk` / `annotate_cycles`), not here.

#### code-split-plugin-python (built-in)

- [x] `p3` - **ID**: `cpt-code-split-component-python-plugin`

In-process Python plugin implemented in `code-split-plugin-python/src/lib.rs`.
Uses `tree-sitter-python` (already a transitive dep via `rust-code-analysis`)
for AST traversal and `walkdir` for file discovery.

**Pipeline**:

1. **Scan** — walk all `.py` files under the workspace, skipping `.venv`,
   `__pycache__`, `node_modules`, and any dot-prefixed directory.
2. **Module index** — derive dotted module paths from file paths:
   `parser/shops/amazon/pdp.py` → `parser.shops.amazon.pdp`;
   `parser/shops/amazon/__init__.py` → `parser.shops.amazon`.
3. **Per-file node** — emit one `File` node per `.py` file.
4. **Import resolution** — resolve `import_statement` and
   `import_from_statement` nodes. Imports that resolve to a project file
   emit a file→file `uses` edge (including `__init__.py` package imports,
   which point at the package's `__init__.py` file); relative imports
   (`.`, `..`, `.submodule`) are resolved against the current module's
   package path. Imports that do not resolve to a project file produce an
   `External` library node (`ext:<top-level-package>`, one per top-level
   package such as `numpy`) reached by a `uses` edge flagged
   `external: true`.

It implements `LanguagePlugin` (`analyze` returns a generic `api::Graph`,
structure only) and depends on `code-split-plugin-api` only. `detect` matches
`pyproject.toml` / `setup.py` / `setup.cfg`.

**ID scheme**:
- File: the file's absolute path (relativized to `{target}/...` by the
  orchestrator; no `file:` prefix)
- External library: `ext:numpy` (carries `external: true`)

**Visibility heuristic**: emitted as a `visibility` string attr — `__name`
(no trailing dunder) → `private`; `_name` → `restricted`; otherwise → `public`.

**Complexity** is added centrally by `code-split-complexity` (by `.py`
extension) — the plugin computes none.

#### code-split-plugin-javascript (built-in)

- [x] `p3` - **ID**: `cpt-code-split-component-js-plugin`

In-process JavaScript plugin implemented in
`code-split-plugin-javascript/src/lib.rs` (`name = "javascript"`, scans
`.js`/`.jsx`/`.mjs`/`.cjs` via `tree-sitter-javascript`, `detect` = a
`package.json` marker). It implements `LanguagePlugin` (`analyze` returns a
generic `api::Graph`, structure only) and **also exposes the shared ECMAScript
logic** the TypeScript plugin reuses by composition: `analyze_ecmascript(ws,
exts, lang_for_ext, candidate_exts_order)` (the walker / import-specifier
extractor / resolver — the tree-sitter node kinds are identical across the JS
and TS grammars), `ecmascript_level(name)`, `detect_with_marker`, and
`external_package`. Depends on `code-split-plugin-api` + `tree-sitter` +
`tree-sitter-javascript` (no `tree-sitter-typescript`). Uses `walkdir` for file
discovery.

**Source root detection**: if `src/` exists in the workspace, scans from
`src/`; otherwise scans from the workspace root. This avoids picking up
non-source `.js` files (config, scripts, test fixtures) in projects that
follow the `src/` layout convention.

**Pipeline**:

1. **Scan** — walk `.ts`, `.tsx`, `.js`, `.jsx` files from source root,
   skipping `node_modules`, `dist`, `.venv`, dotfile directories,
   `.gen.ts`, `.config.ts/js`.
2. **File index** — map each file's relative path to its absolute path.
3. **Per-file node** — emit one `File` node per source file.
4. **Import resolution** — resolve ES `import` statements and CommonJS
   `require()` calls. Imports that resolve to a project file emit a
   file→file `uses` edge; handles the `@/` path alias (→ source root),
   relative paths, and index-file collapsing (extensions tried in order:
   `.ts`, `.tsx`, `.js`, `.jsx`, `index.ts`, `index.tsx`, `index.js`,
   `index.jsx`). Imports that do not resolve to a project file produce an
   `External` library node (`ext:<package>`, one per top-level package —
   `react`, `@scope/pkg`) reached by a `uses` edge flagged
   `external: true`.

**ID scheme**:
- File: the file's absolute path (relativized to `{target}/...`; no `file:` prefix)
- External library: `ext:react`, `ext:@scope/pkg` (carries `external: true`)

**Visibility**: JS/TS have no visibility; every file node gets `visibility:
"public"`.

**Complexity** is added centrally by `code-split-complexity` (by extension) —
the plugin computes none.

#### code-split-plugin-typescript (built-in)

- [x] `p3` - **ID**: `cpt-code-split-component-ts-plugin`

In-process TypeScript plugin (`name = "typescript"`, scans
`.ts`/`.tsx`/`.mts`/`.cts`, `detect` = a `tsconfig.json` marker). It does **not**
duplicate parsing logic: it depends on `code-split-plugin-javascript` and drives
the shared `analyze_ecmascript` helper, passing the `tree-sitter-typescript`
grammars (`LANGUAGE_TYPESCRIPT` for `.ts`/`.mts`/`.cts`, `LANGUAGE_TSX` for
`.tsx`) and a TS-first candidate-extension order, plus `ecmascript_level` /
`detect_with_marker`. This is the "ts inherits js via composition" arrangement
(two crates; `-typescript` depends on `-javascript`). Same id scheme,
visibility, and central-complexity rules as the JS plugin.

#### code-split-cli · recommendation engine

> **Moved.** The orchestrator binary (`cpt-code-split-component-cli`) — plugin
> registry/dispatch, the shared analysis core, the `check` linter and `report`
> artifact writer — and the recommendation engine
> (`cpt-code-split-component-recommend`, the console counterpart of the viewer's
> Prompt Generator) are documented in
> [`code-split-cli/DESIGN.md`](code-split-cli/DESIGN.md).

#### HTML assets (`crates/code-split-viewer/src/assets/`)

> **Moved.** The viewer assets (`cpt-code-split-component-html-assets`) — the
> data-driven `schema.js` layer, `diff.js` / `layout.js` / `app.js` /
> `node-table.js` / `diagram.js` / `export-popup.js` and the rest, plus the
> affected-status / cycle-detection / offline-guarantee notes — are documented
> in [`code-split-viewer/DESIGN.md`](code-split-viewer/DESIGN.md).

### 3.3 API Contracts

Interfaces are defined in PRD §7. This section notes the implementation
binding.

#### Unified CLI (`cpt-code-split-interface-cli`)

> **Moved.** The implementation binding for the unified CLI is in
> [`code-split-cli/DESIGN.md`](code-split-cli/DESIGN.md#unified-cli).

#### Plugins (built-in, in-process)

Plugins are not external binaries. The three plugins — `rust`, `python`,
`javascript` — are compiled into the `code-split` binary and invoked
in-process; each writes its graphs directly into the shared `GraphBuilder`.
See [§3.7 Plugin System](#37-plugin-system).

#### Report Generator (`cpt-code-split-interface-report-cli`) · Check / Regression Gate (`cpt-code-split-interface-check-cli`)

> **Moved.** The implementation bindings for the `report` generator and the
> `check` / regression gate are in
> [`code-split-cli/DESIGN.md`](code-split-cli/DESIGN.md#report-generator). The
> HTML rendering side (`render_html_viewer` and the embedded assets) is in
> [`code-split-viewer/DESIGN.md`](code-split-viewer/DESIGN.md).

#### Graph JSON Schema (`cpt-code-split-interface-graph-schema`)

- **Location**: defined by `Snapshot`, `Node`, `Edge` structs in
  `crates/code-split-graph/src/`
- **Versioning**: `schema_version: "2"`; additive fields are minor;
  breaking changes require a major-version bump

### 3.4 Internal Dependencies

| Consumer | Dependency | Interface |
|----------|------------|-----------|
| `code-split-cli` | `code-split-plugin-api` | `LanguagePlugin` trait — the only contract the CLI uses to talk to plugins |
| `code-split-cli` | `code-split-plugin-{rust,python,javascript,typescript}` | one `Box<dyn LanguagePlugin>` each, listed in `plugin::registry()` |
| `code-split-cli` | `code-split-complexity` | `annotate()` (central metrics) + `metric_specs()` |
| `code-split-cli` | `code-split-graph` | `Snapshot`/`LevelGraph`, `annotate_cycles`/`annotate_hk`/`compute_stats`, `relativize_graph`, `finalize_graph`, `coupling_specs`, canonical serialization |
| `code-split-cli` | `code-split-viewer` | `render_html_viewer()`, `extract_embedded_snapshot()` |
| `code-split-plugin-{rust,python,javascript,typescript}` | `code-split-plugin-api` | `impl LanguagePlugin` (name/detect/levels/analyze/versions) |
| `code-split-plugin-typescript` | `code-split-plugin-javascript` | shared ECMAScript walker/resolver (`analyze_ecmascript`, `ecmascript_level`, …) |
| `code-split-complexity` | `code-split-plugin-api`, `code-split-graph` | the model; `num_attr` |
| `code-split-graph` | `code-split-plugin-api` | the generic model it operates on |
| `code-split-viewer` | `code-split-graph` | `Snapshot`, `to_canonical_string` |
| `code-split-cli` (`run_report`) | the analyzed snapshot (+ optional `--baseline`) | top-level metadata + `graphs` map; rendered via `code-split-viewer` |
| `code-split-cli` (`run_check`) | the analyzed snapshot | `graphs` map; per-rule violation evaluation (relative gate re-evaluates the baseline's rules) |

**Rules**:

- No circular dependencies among the Rust crates; **everything depends on
  `code-split-plugin-api`** (the bottom of the graph).
- **The `LanguagePlugin` trait (in `code-split-plugin-api`) is the only contract
  between the CLI and the language plugins.** The sole place that names concrete
  plugins is `code-split-cli`'s `plugin::registry()` — a `Vec<Box<dyn LanguagePlugin>>`.
  Everything else (dispatch, marker-based auto-detect, version metadata) iterates
  that list and never hardcodes a language. Adding a language = add a crate +
  one line in `registry()`.
- Only `code-split-plugin-rust` may depend on `cargo_metadata` and `syn`.
- Only `code-split-complexity` may depend on `rust-code-analysis` — it is the
  single, central, by-extension metrics pass; plugins emit structure only.
- Language-specific code/names (markers, parsers, `rustc` version) live **only**
  in the `code-split-plugin-*` crates; `code-split-complexity`,
  `code-split-plugin-api` and `code-split-graph` are language-agnostic.
- `code-split-graph` has zero I/O and zero analyzer dependencies.
- The stderr progress/timing log (`code-split-plugin-api::log`) lives in the
  foundation crate so both the CLI's stage timers and the plugins' sub-command
  shell-outs (`cargo metadata`, `rustc`) emit one consistent `[HH:MM:SS.mmm]`
  format. `log::timed(label, f)` wraps every external invocation and prints its
  duration to millisecond precision; `code-split-cli`'s `logger` delegates its
  formatting here.
- The Rust plugin's module→file collapse lives in `code-split-plugin-rust/src/lib.rs`.
- `code-split-cli` orchestrates: it dispatches the language plugins (through the
  trait) and hands the snapshot to `code-split-viewer` for rendering.

### 3.5 External Dependencies

| Dependency | Interface | Purpose |
|------------|-----------|---------|
| `cargo_metadata` crate | `MetadataCommand::exec()` | Enumerate workspace crates and path-dependencies |
| `syn` crate | `syn::parse_file`, `syn::visit::Visit` | Parse Rust source for module hierarchy and `use` statements |
| `rust-code-analysis` (fork: `ffedoroff/rust-code-analysis`, branch `patch/update-tree-sitter-0.26.8`) | `metrics(&parser, path) -> Option<FuncSpace>` | Tree-sitter-based multi-language complexity metrics |
| `serde` + `serde_json` | derive macros, `to_writer_pretty` | JSON serialization |
| `clap` | derive macros | CLI argument parsing |
| Python stdlib | `json`, `pathlib`, `argparse` | JSON processing, file I/O, CLI parsing in Python tools |

### 3.6 Interactions & Sequences

#### Step 1 — Plugin Dispatch and Artifact Write

**ID**: `cpt-code-split-seq-extract`

```mermaid
sequenceDiagram
    participant User
    participant CLI as code-split report (orchestrator)
    participant Plugin as Rust plugin (code-split-plugin-rust)
    participant Cx as code-split-complexity
    participant G as code-split-graph
    participant FS as Filesystem

    User ->> CLI: code-split report . --plugin rust --output.json
    CLI ->> Plugin: analyze(ws, "files", input)
    Note over Plugin: syn + cargo metadata → collapse to files (STRUCTURE ONLY)
    Plugin -->> CLI: api::Graph (abs-path file ids, ext:* nodes) + Level specs
    CLI ->> Cx: annotate(&mut graph)  (central metrics, by extension)
    Cx -->> CLI: N nodes annotated (flat attrs written by id)
    CLI ->> G: finalize_graph + relativize_graph (abs → {target}/{registry})
    CLI ->> CLI: apply_ignore (globs / tests / dev-only)
    CLI ->> G: annotate_cycles + annotate_hk + compute_stats (flow edges)
    CLI ->> CLI: assemble LevelGraph (merge + prune specs) → graphs["files"]
    CLI ->> FS: write {ts}-{git-hash-3}.json (metadata + timings + files level)
    CLI -->> User: exit 0
```

#### Step 2 — Report Generation

**ID**: `cpt-code-split-seq-report`

`report` re-analyzes the workspace (the same plugin pipeline as Step 1) and
then writes artifacts.

```mermaid
sequenceDiagram
    participant User
    participant Report as code-split report (built-in Rust)
    participant FS as Filesystem

    User ->> Report: code-split report . (default: both json + html)
    Report ->> Report: run analysis pipeline (syn → complexity → module-to-file collapse, see Step 1)
    Report ->> Report: compute node weights (fan-in + fan-out)
    Report ->> FS: write {ts}-{git-hash-3}.json snapshot (when json selected)
    Report ->> Report: embed snapshot data inline as cs-baseline / cs-current JSON script tags
    Report ->> FS: write {ts}-{git-hash-3}.html (self-contained: assets + data embedded)
    Report -->> User: exit 0
```

#### Step 4 — Compare against a baseline

**ID**: `cpt-code-split-seq-baseline`

A comparison is `--baseline <snapshot>` on `report` (an HTML diff) or `check` (a
machine verdict / regression gate). The current side is the positional `[input]`
— analyzed now, or an already-existing snapshot, so the comparison can run over
two snapshot files without re-analyzing anything.

```mermaid
sequenceDiagram
    participant User
    participant Report as code-split report --baseline (built-in Rust)
    participant FS as Filesystem

    User ->> Report: code-split report current.json --baseline baseline.json --output.html.path diff.html
    Report ->> FS: load_snapshot_any(baseline.json) and read current.json
    Report ->> Report: embed both snapshots inline as cs-baseline / cs-current JSON script tags
    Note over Report: diff (computeDiff/computeCycles, verdict) runs browser-side in diff.js
    Report ->> FS: write diff.html (self-contained: assets + both snapshots embedded), named …-diff.html
    Report -->> User: exit 0
```

```mermaid
sequenceDiagram
    participant User
    participant Check as code-split check --baseline (built-in Rust)

    User ->> Check: code-split check . --baseline baseline.json --output-format json
    Check ->> Check: analyze current input + recompute baseline violations under current rules
    Check ->> Check: new = current violations absent from baseline (same rule+location)
    Check ->> Check: verdict = degraded (new) / improved (resolved, none new) / neutral
    Check -->> User: diagnostics { verdict, violations } + exit non-zero if new violations
```

### 3.7 Plugin System

#### Plugin Resolution

All plugins are built into the `code-split` binary; there is no external
or dynamic plugin loading. Resolution only selects which built-in plugin
to run.

The plugin defaults to `auto`. When `--plugin auto`, the analysis core
(behind `check` / `report`) resolves the plugin *name* in this order,
stopping at the first match:

```
1. Explicit flag    --plugin <name> (≠ auto) on the command line
                    → use that built-in plugin

2. Config           the `plugin` key in code-split.toml /
                    Cargo.toml metadata (if set and ≠ auto)
                    → use that built-in plugin

3. Auto-detect      each plugin's `detect(ws, input)` over `registry()`:
                    Cargo.toml → rust;
                    pyproject.toml / setup.py / setup.cfg → python;
                    package.json → javascript;
                    tsconfig.json → typescript
```

The resolved name must be one of the four compiled-in plugins — `rust`,
`python`, `javascript`, or `typescript` — which is then invoked in-process. JS
and TS are now **separate** plugins (no aliases): `detect` auto-selects, and a
project carrying both a `package.json` and a `tsconfig.json` is ambiguous → error
asking for an explicit `--plugin`. Multiple matching markers or none → the same
error.

#### Snapshot File Format

`code-split report` writes the snapshot to the path selected by
`--output.json[.path]`, defaulting to `.code-split/{ts}-{git-hash-3}.json`
(timestamp + first 3 chars of the commit) in the current working directory:

```
.code-split/<YYYYMMDD-HHMMSS>-<hash3>.json
```

The name template is resolved as **`--output.json.path` flag › `[output.json]
path` in config › built-in default**, with placeholders `{project-dir}`
(slugified workspace name), `{ts}`, `{git-hash}` (12-char short commit) and
`{git-hash-N}` (first N chars). Example: `code-split report /path/to/axum-api
--plugin rust --output.json.path=.code-split/{ts}-{git-hash-3}.json` →
`.code-split/20260522-112233-a3f.json` (or `axum-api-20260522-112233.json` if
`[output.json] path = "{project-dir}-{ts}.json"`).

The file combines metadata and the `graphs` map (one entry per analysis level;
today only `files`) in one document. Each level bundles its semantics
dictionaries with the structural graph and the computed cycles/stats:

```json
{
  "schema_version": "2",
  "generated_at":   "2026-05-22T11:22:33Z",
  "command":        "code-split report /path/to/axum-api --plugin rust",
  "workspace":      "/Users/alice/projects/code-split",
  "target":         "/Users/alice/projects/axum-api",
  "plugin":         "rust",
  "versions": { "code-split": "1.0.0-alpha.4", "rustc": "1.78.0" },
  "roots": {
    "registry": "/Users/alice/.cargo/registry/src/index.crates.io-abc123",
    "target":   "/Users/alice/projects/axum-api"
  },
  "git": { "branch": "…", "commit": "a3f9c21b4d5e", "dirty_files": 4, "origin": "git@…:team/axum-api.git" },
  "timings": [ { "stage": "rust", "ms": 0, "detail": "17 nodes from 8 files" }, … ],
  "graphs": {
    "files": {
      "edge_kinds":       { "uses": { "flow": true, "label": "uses", "description": "…" }, "contains": { "flow": false, … } },
      "node_attributes":  { "cyclomatic": { "value_type": "int", "label": "Cyclomatic", "name": "Cyclomatic complexity", "short": "Cyclomatic", "formula": "branches + 1", "direction": "lower_better", "group": "complexity" }, "hk": { "value_type": "float", "calc": "sloc * (fan_in * fan_out) ** 2", "thresholds": { "info": 150000, "warning": 10000000 }, … }, … },
      "edge_attributes":  { "visibility": { "value_type": "str", "label": "Visibility" } },
      "attribute_groups": { "complexity": { "label": "Complexity", "description": "…" }, … },
      "node_kinds":       { "file": { "label": "File", "fill": "#dbe9f4", "stroke": "#4d6f9c" }, "external": { "external": true, … } },
      "cycle_kinds":      { "mutual": { "label": "Mutual", "description": "…" } },
      "ui":               { "default_sort": "hk", "columns": [...], "summary_metrics": [...], "sort_metrics": [...], "size_metrics": [...], "card_metrics": [...] },
      "nodes": [
        { "id": "{target}/src/a.rs", "kind": "file", "name": "a.rs", "sloc": 30, "cyclomatic": 1, "hk": 480, "cycle": "mutual", "visibility": "public", … },
        { "id": "ext:serde", "kind": "external", "name": "serde", "external": true, "version": "1.0.228", "path": "{registry}/serde-1.0.228" }
      ],
      "edges": [ { "source": "{target}/src/a.rs", "kind": "uses", "target": "ext:serde", "line": 15 }, … ],
      "cycles": [ { "kind": "mutual", "nodes": ["{target}/src/a.rs", "{target}/src/b.rs"] } ],
      "stats": { "cyclomatic": 1, "hk": 240, "sloc": 26, … }
    }
  },
  "presets": [ { "id": "ADP", "title": "…", "prompt": "…", "doc_url": "…", "sort_metric": "cycle", "connections": ["common","out"] }, … ]
}
```

All node/edge attributes are **flat** (no nested `complexity`/`coupling`
objects); a file node carries no `path` (its id IS its path); an edge is
external iff its `target` is an `ext:` node (no `edge.external`). Every metric's
label/name/formula/`calc`/direction/threshold is in `node_attributes`, node/cycle
kinds in `node_kinds`/`cycle_kinds`, column/sort ordering in `ui`, and the
Prompt-Generator principles in top-level `presets` — so the viewer renders
entirely from this data and hardcodes none of it.

`workspace` is the directory where `code-split` was invoked (cwd). `target`
is the analyzed project path. `roots` are named prefixes for path
resolution: `roots[name] + "/" + rest` → absolute path. All node `path`
values and file node ids (which **are** the relativized path — no `file:`
prefix) use `{name}/…` notation referencing these roots.

The Rust plugin populates roots automatically via `detect_roots()`:

| Root | Source | Example |
|------|--------|---------|
| `target` | analyzed project path | `/path/to/my-crate` |
| `cargo` | `$CARGO_HOME` or `~/.cargo` | `/Users/alice/.cargo` |
| `registry` | first `index.crates.io-*` dir under cargo | `.../registry/src/index.crates.io-abc123` |
| `rustup` | `$RUSTUP_HOME` or `~/.rustup` | `/Users/alice/.rustup` |
| `rust-src` | `rustc --print sysroot` + `/lib/rustlib/src/rust/library` | `.../toolchains/stable-aarch64-apple-darwin/.../library` |

`rust-src` is only added when the path exists on disk; omitted otherwise.
It shortens stdlib paths like `{rustup}/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/alloc/src/vec/mod.rs`
to `{rust-src}/alloc/src/vec/mod.rs`.

**Assembly**: the built-in plugin returns a structural `api::Graph` with
absolute file-path ids. The orchestrator runs the central complexity pass, then
`relativize_graph` — which rewrites node ids (and edge `source`/`target`, node
`parent`, and the external `path` attr) from absolute paths to `{target}/…` /
`{registry}/…` tokens, and drops a file node's redundant `path`. It then runs
cycles/hk/stats, assembles the `LevelGraph`, prepends all metadata, prunes roots
to those actually used (so a JS/TS/Python snapshot carries only `target`, a Rust
snapshot `target` + `registry`), and writes the final snapshot.

`versions` records the `code-split` binary version plus any toolchain versions
the plugin reports (e.g. `rustc` for the Rust plugin).

The `git` fields are collected by `code-split` before invoking the plugin
(`crates/code-split-cli/src/git.rs`):

| Field | Source |
|-------|--------|
| `branch` | `git -C <workspace> rev-parse --abbrev-ref HEAD` |
| `commit` | `git -C <workspace> rev-parse --short=12 HEAD` (12-char short SHA) |
| `dirty_files` | `git -C <workspace> status --porcelain \| wc -l` |
| `origin` | `git -C <workspace> config --get remote.origin.url` (omitted if empty/absent) |

Each field can be overridden via a `--git.<field>` flag (`GitOverride`), so a CI
job can inject clean values mapped from its platform variables — a detached CI
checkout otherwise reports the branch as `HEAD`, and files the job writes inflate
the dirty count. The merge is per field (a flag wins, the rest fall back to the
commands above); when `branch`, `commit`, and `dirty-files` are **all**
overridden, the commands are not run at all (the CI fast path, which also works
in a tree with no `.git`).

If branch/commit fail (and are not overridden) the `git` key is omitted entirely — no error is raised.
`origin` feeds the HTML viewer's per-node "Source" link (`git@host:grp/proj.git`
or `https://…` is normalized to a web blob URL — GitLab `/-/blob/`, GitHub
`/blob/` — at the commit).

`code-split report` embeds this metadata in the generated HTML as a visible
"Snapshot info" panel — for both snapshots when `--baseline` is given.

#### Built-in Plugin: Rust

At P1 the only built-in plugin is `rust`. It is compiled directly into
the `code-split` binary and invoked in-process, so no sub-process overhead
is incurred. Its internal structure is the `code-split-plugin-rust` crate
(cargo metadata + `syn`), which also collapses the module graph to a file
graph; it is not a separate binary on disk. There is no rust-analyzer
dependency, so the run completes in seconds.

##### Analysis Modes and Prerequisites

The Rust plugin has two modes selected by flags on the analyzing commands
(`code-split check` / `code-split report`):

| Mode | Flag | `cargo` required | Network / registry | External nodes |
|------|------|------------------|--------------------|----------------|
| Full | *(none)* | yes | no — `cargo metadata --offline`, cache-only | yes |

The project does NOT need to compile — `syn` parses source syntactically and
tolerates errors. Only dependency resolution (`cargo metadata`, run with
`--offline`) is required; it reads from the already-warm cargo cache, so
code-split makes no network calls. If the cache isn't populated, the plugin
emits an actionable error telling you to warm it (e.g. `cargo fetch`).

##### Full Mode — Step-by-Step

```
code-split report /path/to/my-crate --plugin rust
```

1. `code-split-cli` resolves the output path(s) from `--output.<fmt>[.path]` /
   config / the built-in `.code-split/` default, creating parent directories
   as needed when the artifacts are written.
2. Collects git state (`branch`, `commit`, `dirty_files`) from
   `/path/to/my-crate`.
3. Runs the syntactic stage (`code-split-plugin-rust`):
   a. Runs `cargo metadata --offline --format-version=1` inside the workspace.
   b. Identifies all local packages (those with a `path` source).
   c. For each local package, locates the crate root (`lib.rs` /
      `main.rs` / `[lib] path`).
   d. Recursively follows `mod foo;` declarations using `syn`, building
      the module tree and collecting all `use` / `pub use` statements.
   e. Emits `Crate`, `Module`, `Trait` nodes and `Contains`, `Uses`,
      `Reexports` edges into `GraphBuilder`.
   f. External crates are added as opaque `Crate` nodes with
      `external = true`; their source is never read.
4. Annotates complexity (`code-split-plugin`, with a `RustParser`):
   a. Walks all `.rs` files in the workspace with `walkdir`.
   b. For each file, parses it with `rust-code-analysis` to obtain a
      `FuncSpace` tree.
   c. Annotates each file-backed `Module` node with whole-file metrics
      from the root space (matched by canonical path).
5. Collapses the module graph to a **file graph** (in `code-split-plugin-rust`):
   a. Every `.rs` file becomes one `File` node (its id is the file's
      path — no `file:` prefix); inline
      `mod {}` modules fold into their containing file.
   b. `Uses` / `Reexports` / `Contains` edges are re-pointed from module
      ids to the file ids that own them, so file→file connections are
      preserved. Cross-file `Contains` edges (`mod foo;` declarations) are
      **kept** in the snapshot as structural ownership metadata, but are
      not drawn / not counted in fan_in / HK / cycles; self-edges and
      crate→crate `Contains` are dropped.
   c. External crates collapse to `External` library nodes (`ext:<name>`)
      at depth 1; edges into them are flagged `external: true`. Each carries
      its cargo-cache `path` (the crate's `Cargo.toml` directory, e.g.
      `{registry}/tokio-1.49.0`), later relativized to a `{registry}`/`{cargo}`
      root.
   d. Cross-crate references are **submodule-precise**: `use
      other_crate::sub::Item` resolves (in the module-graph stage) to the
      module node for `sub`, which collapses to a file→file edge to that
      crate's `sub.rs`. A path that stops at a crate-root item resolves to the
      crate node, which maps to the crate-root file (`lib.rs` / `main.rs`) — so
      root-level items still produce a file→file edge to the root. Crate→crate
      dependency edges (from `cargo metadata`) are dropped as crate-level meta.
6. Runs `annotate_all_cycles` (SCC → `CycleKind`) and `annotate_hk`
   (internal `fan_in`/`fan_out`/`hk`; `fan_out_external` separately) on
   the file graph, then `annotate_stats`. Named roots that did not shorten
   any node path are pruned (`prune_unused_roots`), so the header lists only
   relevant roots.
7. Writes the final snapshot `.json` (metadata + `timings` + `files` graph)
   via `to_canonical_string_pretty` — canonical, byte-stable JSON.

##### Failure Modes

| Situation | Behavior |
|-----------|----------|
| `cargo` not on `$PATH` | exit 1 — "cargo not found" (the Rust plugin requires `cargo` for `cargo metadata`) |
| `cargo metadata` fails (dependency resolution error) | exit 1 — cargo stderr forwarded verbatim, including the underlying dependency resolution failure |
| Workspace member glob matches no directories | warning logged; zero crates emitted for that glob |
| A source file has a syntax error | `syn` parse failure logged as a warning; file is skipped; analysis continues |
| Output directory not writable | exit 1 before analysis starts |

#### P3 Framework-Specific Plugins

Framework plugins (Django, WordPress, etc.) MAY emit additional metadata
beyond the base schema vocabulary by using the `metadata` object on nodes
and edges. The `kind` field MUST remain one of the base kinds (`file`,
`external`) so base consumers can still process the graph.
Framework-specific semantics are expressed in `metadata.<plugin>.*` keys
and are ignored by base consumers.

Example — a Django-specific annotation on a file node:

```json
{
  "id": "{target}/myapp/signals.py",
  "kind": "file",
  "name": "signals.py",
  "metadata": {
    "django": {
      "signal": "post_save",
      "sender": "User"
    }
  }
}
```

(A file node's id IS its relativized path — no `file:` prefix and no separate
`path` attribute. The `metadata` object is an illustrative P3 framework
extension, not part of the base flat-attribute schema.)

### 3.8 CLI Reference and Examples

> **Moved.** The CLI reference and worked examples (snapshots, visualization,
> baseline comparison, the full end-to-end workflow) are in
> [`code-split-cli/DESIGN.md`](code-split-cli/DESIGN.md). The full flag surface
> is documented in [`code-split-cli/CLI.md`](code-split-cli/CLI.md).

## 4. Additional Context

**Repository layout**:

```
code-split/
  crates/
    code-split-graph/             # Rust — graph types, JSON schema, StageTime, cycles/hk/diff
    code-split-plugin-api/        # Rust — the LanguagePlugin trait (plugin contract)
    code-split-plugin/            # Rust — shared plugin utils: complexity, finalize, logging
    code-split-plugin-rust/       # Rust — Rust analysis: cargo metadata + syn, module→file collapse
    code-split-plugin-python/     # Rust — Python analysis: tree-sitter
    code-split-plugin-javascript/ # Rust — JS/TS analysis: tree-sitter
    code-split-viewer/            # Rust — HTML viewer: assets + render_html_viewer
    code-split-cli/               # Rust — orchestrator, plugin registry/dispatch, check linter, report
      src/
        plugin/            # Built-in plugins: rust.rs (incl. module→file collapse), python.rs, javascript.rs, finalize.rs (file-graph normalizer for Python/JS), mod.rs
        presets.rs         # Generic Prompt-Generator preset catalog (principles)
        recommend.rs       # Recommendation engine: scorecard + prompt formats (CLI counterpart of the viewer's Prompt Generator)
        assets/            # HTML/CSS/JS assets embedded via include_str!
          index.html       # Shell template (single Files view); cs-baseline / cs-current JSON script tags embedded inline at render time
          index.css        # Node/edge/nav styling (external nodes amber)
          graphviz.umd.js  # Graphviz WASM (~802 KB, offline)
          snarkdown.umd.js # Markdown→HTML renderer (~2 KB, offline) for the prompt preview
          layout.js        # buildDOT — DOT graph construction (external nodes amber/dashed)
          panzoom.js       # Pan/zoom logic
          schema.js        # Snapshot data-access layer (specs, evalCalc/calcDisplay)
          app.js           # Entry point, event wiring
          diff.js          # Browser-side diff + cycle computation
          node-table.js    # Sortable node table
          summary.js       # Diff/review summary table
          modal.js         # Node modal overlay
          diagram.js       # Popup fan-in/fan-out SVG diagram (column layout)
          export-popup.js  # Prompt-generator popup
          nav.js           # openModalForNode — node popup navigation
          utils.js         # Shared helpers
          ui.js            # (empty — baseline/current visibility handled in app.js on the union layout)
  docs/
    PRD.md                 # Product PRD (overview, actors, plugin layer, schema, NFRs)
    DESIGN.md              # Product technical design (architecture, domain model, plugins)
    code-split-cli/        # CLI component docs: PRD, DESIGN, CLI.md, config.md, ERRORS.md
    code-split-viewer/     # HTML viewer component docs: PRD, DESIGN
  principles/              # Principle corpus (used at P3 for prompt generation)
    rust/                  # Rust-specific principle docs
    python/                # Python-specific principle docs
    typescript/            # TypeScript/JavaScript principle docs
```

**Out of scope for this revision (deferred to P2/P3)**:

- Recommender rules catalog and `Finding` types (removed from P1)
- LLM prompt generation (P2 for AI prompts, P3 for principles prompts)
- CI artifact integration (P2)
- Additional language plugins (P3)

**Module→file collapse trade-off**: Rust dependencies are expressed via
module paths (`use crate::foo::bar`), not file paths. Rather than emit a
separate module graph, the Rust plugin builds the module tree and then
collapses it to files — folding inline `mod {}` modules into their file
and re-pointing `use`/`pub use` edges to files — so file→file
connections are fully preserved in the single graph model. There is no
function-level call graph: resolving call sites semantically would
require rust-analyzer (slow, volatile), which has been removed.

**HTML asset bundling**: All JS/CSS assets for the `code-split report` viewer
(single-snapshot or `--baseline` diff) are embedded into the binary via
`include_str!`. The
Graphviz WASM bundle is committed under
`crates/code-split-viewer/src/assets/` and never fetched at runtime.
Generated HTML files work offline via `file://` with no network access.

## 5. Traceability

- **PRD**: [PRD.md](PRD.md)
- **CLI component**: [`code-split-cli/PRD.md`](code-split-cli/PRD.md) ·
  [`code-split-cli/DESIGN.md`](code-split-cli/DESIGN.md)
- **Viewer component**: [`code-split-viewer/PRD.md`](code-split-viewer/PRD.md) ·
  [`code-split-viewer/DESIGN.md`](code-split-viewer/DESIGN.md)
- **Graph JSON Schema**: defined by the `Snapshot` / `LevelGraph` / `Node` /
  `Edge` structs in `crates/code-split-graph/src/`; node/edge reference in
  [node_schema.md](node_schema.md)
- **ADRs**: pending — `docs/ADR/0001-files-only-graph.md` and
  `docs/ADR/0002-four-step-pipeline.md` to be authored after this
  design is accepted

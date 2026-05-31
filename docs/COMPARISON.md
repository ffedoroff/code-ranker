# How `code-split` compares

A look at `code-split` next to the well-known structural / complexity analyzers it
overlaps with: **rust-code-analysis**, **Lizard**, **Radon**, **escomplex**,
**cargo-modules**, and **knip**.

## TL;DR

Most of these tools answer one of two questions:

- **"How complex is each unit?"** — per-function / per-file metrics (rust-code-analysis,
  Lizard, Radon, escomplex).
- **"How is the code wired together?"** — dependency / module structure
  (cargo-modules, knip).

`code-split` is the only one that does **both at once, across Rust / Python / JS / TS,
and then tracks the delta over time**: it builds a file-level dependency graph (with
third-party libraries shown as depth-1 external nodes), attaches per-file complexity
*and* coupling metrics to every file node, detects cycles, and diffs two snapshots
into an `improved` / `degraded` / `neutral` verdict — all offline, behind a single
plugin protocol.

> **Note on rust-code-analysis:** `code-split` is not a rival to it — it is *built on
> it*. The `code-split-complexity` crate uses the `rust-code-analysis` fork
> (`rust-code-analysis-code-split`) for cyclomatic / cognitive / Halstead / MI / LOC.
> code-split's contribution is the graph, coupling, cycles, diff, report, and CI
> layers wrapped around those metrics and unified across languages.

## Scope & workflow

Legend: ✓ first-class · ~ partial / indirect / via companion · ✗ none

| Capability | code-split | rust-code-analysis | Lizard | Radon | escomplex | cargo-modules | knip |
|---|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| Languages | Rust, Py, JS, TS | many (tree-sitter) | many | Python only | JS (+TS fork) | Rust only | JS / TS |
| File dependency graph | ✓ | ✗ | ✗ | ✗ | ~ | ~ | ~ |
| External (3rd-party) deps as graph nodes | ✓ | ✗ | ✗ | ✗ | ~ | ✗ | ~ |
| Coupling: fan-in / fan-out | ✓ | ✗ | ✗ | ✗ | ~ | ✗ | ✗ |
| Henry–Kafura (`hk`) | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Cycle detection — files | ✓ | ✗ | ✗ | ✗ | ✗ | ✓ | ✗ |
| Before/after diff + verdict | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Interactive offline HTML report | ✓ | ✗ | ~ | ✗ | ~ (Plato) | ~ (DOT) | ✗ |
| Machine-readable JSON artifact | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| CI linter (thresholds + exit codes) | ✓ | ✗ | ✓ | ~ (Xenon) | ✗ | ~ (`--acyclic`) | ✓ |
| One plugin protocol, multi-language | ✓ | ~ (library) | ✗ | ✗ | ✗ | ✗ | ✗ |
| Install download size* | ~6–7 MB | ~2 MB | 0.1 MB | 0.05 MB | 0.06 MB | source | 1.6 MB |

\* Download to install. Native-binary tools bundle everything; package tools are tiny
but need a separate runtime — see [Distribution footprint](#distribution-footprint).

## Per-unit code metrics

| Metric | code-split | rust-code-analysis | Lizard | Radon | escomplex | cargo-modules | knip |
|---|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| Cyclomatic | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ | ✗ |
| Cognitive | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Halstead (volume/effort/bugs…) | ✓ | ✓ | ✗ | ✓ | ✓ | ✗ | ✗ |
| Maintainability Index | ✓ (`mi` + `mi_sei`) | ✓ | ✗ | ✓ | ✓ | ✗ | ✗ |
| LOC breakdown (sloc/lloc/cloc/blank) | ✓ | ✓ | ~ (NLOC) | ✓ | ~ | ✗ | ✗ |
| Parameter / argument count | ✗ | ✓ | ✓ | ✗ | ✓ | ✗ | ✗ |
| Method count (NOM) | ~ (traits) | ✓ | ✗ | ✗ | ~ | ✗ | ✗ |

## The tools in detail

### rust-code-analysis (Mozilla)

A tree-sitter-based **metrics library + CLI** covering many languages. Computes
cyclomatic, cognitive, Halstead, MI, LOC, NOM, NARGS, NEXITS per "space" (file /
function / class), emitting JSON/YAML/TOML/CBOR.

- **Overlap:** the entire per-unit metric set — because code-split *uses it* for exactly
  that.
- **Gap:** no cross-file dependency graph, no coupling (fan-in/out), no cycles, no diff,
  no report, no CI gating. It hands you numbers per code unit and stops.
- **Reach for it instead when:** you want raw metrics for a language code-split has no
  plugin for, or you are building your own tooling on top of the metric engine.

### Lizard

A lightweight, multi-language **cyclomatic-complexity gate** for CI. Reports CCN, NLOC,
token count, and parameter count per function, and warns/fails on thresholds.

- **Overlap:** per-function cyclomatic + a CI threshold gate.
- **Gap:** no cognitive, no Halstead, no MI, no dependency graph, no coupling, no diff.
  Function-local only.
- **Reach for it instead when:** you want a zero-config, drop-in "fail the build if any
  function exceeds CCN N" check across an unusual language mix, and nothing else.

### Radon

The standard **Python-only** metrics CLI: cyclomatic (with A–F grades), raw LOC,
Halstead, and Maintainability Index. Often paired with **Xenon** (gating) and **Wily**
(history tracking).

- **Overlap:** cyclomatic, Halstead, MI, LOC — for Python.
- **Gap:** Python only; no cognitive complexity; no dependency graph / coupling; no
  built-in before/after verdict (Wily approximates trend tracking separately).
- **Reach for it instead when:** you live entirely in Python and want the established,
  battle-tested grades + Xenon gating without the graph layer.

### escomplex (typhonjs-escomplex / Plato)

A **JavaScript/TypeScript** metrics engine: cyclomatic, Halstead, MI per function and
module, plus module-level dependency lists and aggregate coupling/density figures.
Plato renders historical HTML dashboards from it.

- **Overlap:** per-unit JS/TS metrics, some module coupling aggregates, and (via Plato)
  an HTML view.
- **Gap:** JS/TS only; no navigable file dependency graph with per-file coupling; no
  cognitive complexity; no structured snapshot diff/verdict. The core libraries are
  largely unmaintained.
- **Reach for it instead when:** you specifically want the classic Plato dashboard for a
  JS codebase.

### cargo-modules

A **Rust-only structure tool**: renders the module tree and `uses`/`owns` graph as a
terminal tree or Graphviz DOT, flags orphan modules, and can fail on cycles
(`--acyclic`).

- **Overlap:** Rust dependency graph + cycle checking.
- **Gap:** Rust only; module-tree view rather than a metric-annotated file graph; no
  complexity or coupling metrics; no before/after diff; rendering needs an external
  Graphviz step.
- **Reach for it instead when:** you only need to see/print a single Rust crate's module
  tree and don't care about metrics or history.

### knip

A **JS/TS dead-code finder**: builds an internal reachability graph to report unused
files, exports, types, and dependencies, and exits non-zero on findings.

- **Overlap:** an internal dependency/reachability graph and a CI exit gate — for JS/TS.
- **Gap:** answers "what is unused", not "how is it structured / how complex is it". No
  complexity metrics, no coupling metrics, no visualization, no before/after verdict.
- **Reach for it instead when:** your goal is pruning unused code/deps in a JS/TS repo —
  it is excellent at that and complementary to code-split.

## Where `code-split` is unique

- **One artifact, both axes.** A single snapshot carries a file dependency graph with
  *both* complexity (cyclomatic, cognitive, Halstead, MI, LOC) and structural coupling
  (fan-in, fan-out, Henry–Kafura) attached to every file node, plus third-party
  libraries as depth-1 external nodes. No other tool here unifies complexity *and*
  coupling on a navigable cross-language file graph.
- **Architectural drift over time.** The before/after diff with an `improved` /
  `degraded` / `neutral` verdict turns "did this refactor help?" from intuition into a
  measurement. None of the others ship this.
- **Same model across languages.** Rust, Python, JS, and TS produce the same node/edge
  schema behind one plugin protocol — you compare and gate them identically. The
  per-language tools each speak only their own dialect.
- **Cycle classification.** Cycles are typed (`test_embed` / `mutual` / `chain`) with
  per-kind severity rules, not just a yes/no acyclic check.
- **Portable, shareable output.** A self-contained offline HTML viewer plus a
  machine-readable JSON snapshot with stable, machine-independent path roots.

## Where the others still win

Being honest about the trade-offs:

- **Language reach:** rust-code-analysis and Lizard cover far more languages out of the
  box than code-split's four plugins.
- **Maturity of gates:** Lizard, Radon+Xenon, and knip are mature, narrowly-focused CI
  gates with years of production use.
- **Specialized depth:** knip's dead-code analysis and cargo-modules' Rust module
  rendering go deeper in their niche than code-split aims to.
- **Extra per-unit metrics:** rust-code-analysis exposes some metrics (NARGS, NEXITS,
  full NOM) that code-split does not currently surface in its snapshot, even though the
  engine computes them.

These are complementary, not mutually exclusive: e.g. run **knip** to prune dead JS/TS,
then **code-split** to measure and gate what remains.

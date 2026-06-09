# Node JSON Schema

Reference for the node objects emitted in code-ranker snapshot files
(`.code-ranker/{ts}-{git-hash-3}.json`, `schema_version: "2"`), under
`graphs.files.nodes`. There is a single graph level — `files` — so every node is
either a source `file` or a third-party `external` library.

The model is a **generic property graph**: a node has a free-form string `kind`,
a `name`, and a **flat attribute map** (no nested `complexity` / `coupling` /
`loc` / `halstead` objects). Every metric's label, formula, live derivation
(`calc`), direction and calibrated thresholds are described by the level's
`node_attributes` dictionary, so a consumer can render any metric without
hardcoding it — see the main [DESIGN](DESIGN.md) §3.1/§3.7 and [PRD](PRD.md) §7.3.

## Full example

A `file` node (Rust — carries the per-target `crate` and `items`):

```json
{
  "id": "{target}/src/a.rs",
  "kind": "file",
  "name": "a.rs",
  "visibility": "public",
  "crate": "rust-sample",
  "items": 2,
  "cyclomatic": 1,
  "loc": 30, "sloc": 14, "lloc": 1, "cloc": 11, "blank": 5,
  "length": 69, "vocabulary": 32, "volume": 345, "effort": 4413.97,
  "time": 245.22, "bugs": 0.0896,
  "mi": 85.054, "mi_sei": 87.531,
  "fan_in": 2, "fan_out": 2, "fan_out_external": 1, "hk": 224,
  "cycle": "mutual"
}
```

An `external` node (Rust — carries `version` + the cargo-cache `path`):

```json
{ "id": "ext:once_cell", "kind": "external", "name": "once_cell",
  "external": true, "version": "1.21.4", "path": "{registry}/once_cell-1.21.4" }
```

All attributes are **flat**, and a metric is **omitted when it rounds to zero**.
Numeric values use 3-significant-digit rounding; an integral value serializes
without a decimal point (`1.0` → `1`). Python / JS / TS file nodes carry the same
keys minus `crate` / `items` (Rust-only); their `external` nodes carry neither
`version` nor `path` (no on-disk package is resolved).

---

## Identity & structure

### `id` — string, required

Stable unique key. The scheme depends on the node kind:

| kind | scheme | example |
|------|--------|---------|
| `file` | **the relativized path itself** (no `file:` prefix) | `{target}/src/api/auth.ts` |
| `external` | `ext:{name}` | `ext:tokio`, `ext:numpy`, `ext:@scope/pkg` |

A file node's id **is** its project-relative path, so it carries **no separate
`path` attribute** (the path falls back to the id). IDs contain no line numbers
or byte offsets and stay stable across code moves. Paths use named-root prefixes
so they are portable across machines:

| prefix | resolves to |
|--------|-------------|
| `{target}` | the analyzed project root |
| `{workspace}` | the code-ranker workspace root (cwd) |
| `{registry}` | Cargo registry cache |
| `{cargo}` | Cargo home (`$CARGO_HOME`, holds `git/checkouts/…`) |
| `{rustup}` | rustup toolchain root |
| `{rust-src}` | rustc sysroot `library/` (added only when present) |

Roots that did not shorten any path are pruned, so a JS/TS/Python snapshot
carries only `{target}` and a Rust snapshot `{target}` + `{registry}`.

### `kind` — string, required

| value | description |
|-------|-------------|
| `file` | A source file in the analyzed project — carries all per-file metrics |
| `external` | A third-party library the project depends on, recorded at depth 1 (one node per library, never expanded into its internals; carries no metrics) |

### `name` — string, required

Short human-readable name. For `file` nodes, the file basename (`"a.rs"`,
`"setup.ts"`). For `external` nodes, the library name (`"tokio"`, `"numpy"`,
`"@scope/pkg"`).

### `external` — bool, on external nodes

`true` on `external` library nodes. Absent on `file` nodes. (There is no
`edge.external` flag — an edge is external **iff its `target` is an `ext:`
node**.)

### `version` — string, optional (Rust external nodes)

Resolved package version (semver), from `cargo metadata` (e.g. `"1.21.4"`).
Present on Rust `external` nodes; omitted on file nodes and on Python/JS/TS
external nodes.

### `path` — string, optional (Rust external nodes)

The library's cargo-cache directory — the directory of its `Cargo.toml`, e.g.
`{registry}/once_cell-1.21.4` for a registry crate (the directory name encodes
the resolved version) or a `{cargo}/git/checkouts/…` path for a git dependency.
**File nodes carry no `path`** (their id is the path); Python/JS/TS external
nodes carry none either.

### `crate` — string, optional (Rust file nodes)

The owning crate (compilation unit) of a `file` node, from `cargo metadata`.
Per-target: a library uses the package name (`"bat"`), a binary gets a suffix
(`"bat (bin)"`, or `"bat (bin <name>)"` when the binary name differs). Omitted on
`external` nodes and on plugins that do not resolve crates (Python/JS/TS). Drives
diagram clustering via the level's `ui.grouping` (see DESIGN §3.2).

### `items` — number, optional (Rust file nodes)

Count of top-level items the file declares. Used for tie-breaking in the
worst-first rankings. Rust-only.

### `visibility` — string, optional

Declared visibility, as a plain string attribute:

| value | meaning |
|-------|---------|
| `"public"` | visible to everyone (`pub`) — the default value carried by file nodes |
| `"private"` | visible only within the current module |
| `"crate"` | visible within the current crate (`pub(crate)`) |
| `"super"` | visible to the parent module (`pub(super)`) |

Python uses a name heuristic (`__name` → `private`, `_name` → `restricted`, else
`public`); JS/TS have no visibility and always emit `"public"`.

### `cycle` — string, optional

Set **only** on a node that participates in a dependency cycle; absent otherwise.
The matching SCC is also listed in the level's `cycles` array.

| value | meaning |
|-------|---------|
| `"mutual"` | two nodes that directly depend on each other (SCC size = 2) |
| `"chain"` | cycle involving three or more nodes (SCC size ≥ 3) |

> **Renamed.** This attribute was `cycle_kind` in the old nested schema; in
> schema `"2"` it is the flat key `cycle`.

---

## Metric attributes (flat)

All metrics are flat keys on the node — there is no `complexity` wrapper object.
Present on `file` nodes only (external libraries are never read). Each is omitted
when it rounds to zero; the LOC keys are gated on `sloc > 0` and the Halstead
keys on `volume > 0`. Complexity / Halstead / LOC / maintainability metrics come
from the central `code-ranker-complexity` pass (rust-code-analysis by file
extension); coupling and `cycle` are added by `code-ranker-graph`.

### Complexity — `cyclomatic`, `cognitive`, `exits`, `args`, `closures`

| key | metric | notes |
|-----|--------|-------|
| `cyclomatic` | **Cyclomatic complexity** (McCabe) — `branches + 1`. | Min 1. For Rust this is whole-file and typically ≈ 1. |
| `cognitive` | **Cognitive complexity** (SonarSource) — penalises nesting. | Min 0; often absent for Rust files. |
| `exits` | Number of exit points (`return` / `throw`). | |
| `args` | Number of function / closure arguments. | |
| `closures` | Number of closures defined in the file. | |

Whole-file aggregates: all functions, methods, arrow functions and closures roll
up into the file's single node.

### Lines of code — `loc`, `sloc`, `lloc`, `cloc`, `blank`

| key | meaning |
|-----|---------|
| `loc` | **Total lines** in the file (everything — including any test code). |
| `sloc` | **Source lines** — lines with at least one non-whitespace, non-comment character (rust-code-analysis `ploc`). The main size metric; it is the `sloc` used by HK and MI. **In Rust, lines inside `#[cfg(test)]` / `#[test]` items are excluded** (see `tloc`), so `sloc` (and everything derived from it — `hk`, `mi`, the Halstead/complexity metrics) reflects **production** code only, not inline unit tests. `loc` stays the raw file count. |
| `tloc` | **Test lines** — lines inside `#[cfg(test)]` / `#[test]` / `#[bench]` items (Rust only; absent/omitted elsewhere). The complement of `sloc`: tests are removed *first*, then the production remainder is counted, so `loc = sloc + cloc + blank + tloc`. |
| `lloc` | **Logical lines** — statements/expressions rather than physical lines (Rust: production only). |
| `cloc` | **Comment-only lines** (inline comments on code lines are not counted; Rust: production only). |
| `blank` | Empty or whitespace-only lines (Rust: production only). |

### Maintainability — `mi`, `mi_sei`

| key | formula | notes |
|-----|---------|-------|
| `mi` | `171 − 5.2·ln(volume) − 0.23·cyclomatic − 16.2·ln(sloc)` | Higher is better. >85 easy, 65–85 moderate, <65 hard; can go negative. |
| `mi_sei` | `MI + 50·sin(√(2.4 × comment-ratio))` | SEI variant: a bonus for comment density. Equals `mi` when `cloc = 0`. |

### Halstead — `length`, `vocabulary`, `volume`, `effort`, `time`, `bugs`

Halstead treats a program as operators (η₁ unique / N₁ total) and operands
(η₂ unique / N₂ total).

| key | formula | meaning |
|-----|---------|---------|
| `length` | `N₁ + N₂` | Program length — total operator + operand occurrences. |
| `vocabulary` | `η₁ + η₂` | Distinct operators + operands. |
| `volume` | `length × log₂(vocabulary)` | Algorithm size in bits. The primary Halstead size metric. |
| `effort` | `volume × difficulty` | Mental effort to implement. |
| `time` | `effort ÷ 18` | Estimated implementation time, in **seconds** (18 = Stroud number). |
| `bugs` | `effort^⅔ ÷ 3000` | Estimated delivered bugs (the engine's actual definition, **not** the classic `volume ÷ 3000`). A relative ranking, not an absolute count. |

### Coupling — `fan_in`, `fan_out`, `fan_out_external`, `hk`

Derived from the dependency graph (edges), not source code. `fan_in` / `fan_out`
/ `hk` count **internal** file→file flow (`uses`) partners only; edges to
`external` library nodes are excluded and counted in `fan_out_external` instead.
Non-flow edges (`contains`, `reexports`, `super`) are excluded from all of these.

| key | meaning |
|-----|---------|
| `fan_in` | Number of distinct project files that **depend on** this file (incoming internal `uses` edges). High fan-in → many dependents → risky to change. |
| `fan_out` | Number of distinct project files **this file depends on** (outgoing internal `uses` edges). High fan-out → broad responsibilities. |
| `fan_out_external` | Number of distinct **external libraries** this file uses (outgoing edges to `ext:` nodes). Tracked separately so 3rd-party usage doesn't inflate internal coupling or HK. |
| `hk` | **Henry-Kafura** complexity: `hk = sloc × (fan_in × fan_out)²`. Combines size with internal coupling; external edges are excluded. A small isolated file has no `hk` (omitted); a large hub reaches the millions. Use as a relative ranking within a project. |

---

## Edges

Edges live in `graphs.files.edges`, each a flat object:

```json
{ "source": "<node-id>", "kind": "uses | reexports | contains | super", "target": "<node-id>", "line": 12 }
```

| `kind` | flow? | drawn? | counted in fan-in / HK / cycles? |
|--------|:--:|:--:|:--:|
| `uses` | yes | yes | yes |
| `reexports` | no | no | no — a `pub use` facade is not a dependency |
| `contains` | no | no | no — structural module ownership (`mod foo;`), kept as metadata |
| `super` | no | no | no — a glob `use super::*` / `use crate::<ancestor>::*` namespace pull (Rust). Usually scope-sugar; but when the child really uses a parent item via the glob it is a real back-dependency (a low-priority cycle), kept non-flow because the two are indistinguishable without name resolution — see [principles/rust/what-is-cycle.md](../principles/rust/what-is-cycle.md) |

An edge is **external iff its `target` is an `ext:` node** (no `edge.external`
flag). Edge-level attributes (e.g. a Rust `reexports` edge's `visibility`) are
flattened in alongside `source` / `kind` / `target`. Which kinds count as
information flow is read from the level's `edge_kinds[kind].flow`.

`line` is the 1-based line in the **source** node's file where the dependency is
declared (the `use` / `import` / `require` statement). It is **optional** —
omitted for structural `contains` edges and for edges the plugin can't place
(e.g. Rust bare-path references). When several imports collapse onto one
deduplicated edge, the first one's line is kept. `check` uses it to point a cycle
violation at a concrete spot to break (see the `github` / `sarif` annotations in
[CLI.md](code-ranker-cli/CLI.md)).

---

**Related docs**: [PRD.md](PRD.md) §7.3 (the full Graph JSON Schema) ·
[DESIGN.md](DESIGN.md) §3.1 Domain Model / §3.7 Snapshot File Format. The schema
is defined by the `Node` / `Edge` structs in `crates/code-ranker-plugin-api/src/`
and the `Snapshot` / `LevelGraph` structs in `crates/code-ranker-graph/src/`.

# Node JSON Schema

Reference for the node objects emitted in code-split snapshot files
(`.code-split/<project>-<timestamp>.json`), under `graphs.files.nodes`.
There is a single graph level — `files` — so every node is either a
source `file` or an `external` library.

## Full example

```json
{
  "id": "file:{target}/src/test/setup.ts",
  "kind": "file",
  "name": "setup.ts",
  "path": "{target}/src/test/setup.ts",
  "visibility": "public",
  "cycle_kind": null,
  "complexity": {
    "cyclomatic": 1,
    "cognitive": 0,
    "coupling": {
      "fan_in": 2,
      "fan_out": 3,
      "fan_out_external": 1,
      "hk": 144
    },
    "maintainability": {
      "mi": 95.867,
      "mi_sei": 63.319
    },
    "loc": {
      "total": 16,
      "source": 15,
      "physical": 14,
      "logical": 7,
      "comments": 0,
      "blank": 1
    },
    "halstead": {
      "length": 63,
      "vocabulary": 27,
      "volume": 299.557,
      "effort": 1100.875,
      "time": 61.159,
      "bugs": 0.0355
    }
  }
}
```

All optional fields are omitted when null or not applicable to the node kind.
Numeric fields inside `complexity` use 3-significant-digit serialization.

---

## Top-level fields

### `id` — string, required

Stable unique key for this node. The scheme depends on the node kind:

| kind | scheme | example |
|------|--------|---------|
| `file` | `file:{path}` | `file:{target}/src/api/auth.ts` |
| `external` | `ext:{name}` | `ext:tokio`, `ext:numpy`, `ext:@scope/pkg` |

IDs contain no line numbers or byte offsets and remain stable across
code moves.

### `kind` — string, required

The structural category of this node. One of:

| value | description | plugins |
|-------|-------------|---------|
| `file` | A source file in the analyzed project — carries all per-file metrics | rust, python, js |
| `external` | A third-party library the project depends on, recorded at depth 1 (one node per library, never expanded into its internals; carries no metrics). For Rust, `path` holds the crate's cargo-cache location | rust, python, js |

### `name` — string, required

Short human-readable name. For `file` nodes, the file basename
(`"setup.ts"`). For `external` nodes, the library name (`"tokio"`,
`"numpy"`, `"@scope/pkg"`).

### `path` — string, optional

Physical location of the node. On `file` nodes, the source file. On Rust
`external` nodes, the crate's cargo-cache directory — the directory of its
`Cargo.toml`, e.g. `{registry}/tokio-1.49.0` for a registry crate (the
directory name encodes the resolved version) or a `{cargo}/git/checkouts/…`
path for a git dependency. Omitted on Python/JS external nodes (no
on-disk crate path is resolved). Uses named-root prefixes so paths are
portable across machines:

| prefix | resolves to |
|--------|-------------|
| `{target}` | the analyzed project root |
| `{workspace}` | the code-split workspace root |
| `{registry}` | Cargo registry cache |
| `{cargo}` | Cargo home (`$CARGO_HOME`, holds `git/checkouts/…`) |
| `{rustup}` | rustup toolchain root |

Examples: `{target}/src/api/auth.ts`, `{target}/src/lib.rs`,
`{registry}/serde-1.0.228`.

### `version` — string, optional

Resolved package version (semver), from `cargo metadata`. Present on Rust
`external` library nodes (e.g. `"1.0.228"`); also available for the
analyzed crates internally. Omitted on file nodes and on Python/JS external
nodes (no version is resolved).

### `crate` — string, optional (Rust)

The owning crate (compilation unit) of a `file` node, from `cargo metadata`.
A package can produce several crates — a library plus one or more binaries —
so this is **per-target**: the library uses the package name (`"bat"`), a
binary gets a suffix (`"bat (bin)"`, or `"bat (bin <name>)"` when the binary
name differs from the package name). Omitted on `external` nodes and on
plugins that do not resolve crates (Python/JS/TS). Drives diagram clustering
via the level's `ui.grouping` (see DESIGN §3.2).

### `visibility` — string or object, optional

Declared visibility of the node.

Simple cases are represented as a plain string:

| value | meaning |
|-------|---------|
| `"public"` | visible to everyone (`pub`) |
| `"private"` | visible only within the current module (default in Rust) |
| `"crate"` | visible within the current crate (`pub(crate)`) |
| `"super"` | visible to the parent module (`pub(super)`) |

When visibility is path-restricted, an object is used instead:

```json
"visibility": { "restricted": "crate::services::platform_client" }
```

`null` for nodes that have no inherent visibility (e.g. `external` nodes).

### `cycle_kind` — string, optional

Set when this node participates in a dependency cycle. `null` otherwise.

| value | meaning |
|-------|---------|
| `"test_embed"` | cycle caused by a `#[cfg(test)]` back-edge (Rust only) |
| `"mutual"` | two nodes that directly depend on each other (SCC size = 2) |
| `"chain"` | cycle involving three or more nodes (SCC size ≥ 3) |

---

## `complexity` — object, optional

All code and structural metrics for this node. Present on `file` nodes;
omitted entirely for `external` library nodes (their internals are never
read) and for files with no measurable metrics.

### `complexity.cyclomatic` — number

**Cyclomatic complexity** (McCabe, 1976). Counts the number of linearly
independent paths through the code: `branches + 1`. Each `if`, `else if`,
`for`, `while`, `match` arm, `&&`, `||` adds 1.

- Minimum value: **1** (a straight-line function with no branches)
- Good range: 1–5; review at >10; refactor at >20
- Computed from: AST branch nodes

### `complexity.cognitive` — number

**Cognitive complexity** (SonarSource, 2018). Measures how difficult the
code is to *read and understand*, not just to test. Unlike cyclomatic,
it penalises nesting: an `if` inside a loop inside another `if` costs
more than three flat `if` statements.

- Minimum value: **0**
- More sensitive to deeply nested code than cyclomatic
- Computed from: AST structure with nesting weights

### `complexity.coupling` — object, optional

Structural coupling metrics derived from the dependency graph (edges),
not from source code. Present on `file` nodes. `fan_in` / `fan_out` /
`hk` count **internal** file→file edges only; edges to `external`
library nodes are excluded from these and counted separately in
`fan_out_external`.

#### `coupling.fan_in` — number

Number of other project files that **depend on** this file (incoming
`uses` edges). A high fan_in means many dependents — changing this file
is risky.

#### `coupling.fan_out` — number

Number of project files that **this file depends on** (outgoing `uses`
edges, internal only). A high fan_out means broad responsibilities — the
file knows too much.

#### `coupling.fan_out_external` — number

Number of **distinct external libraries** this file depends on (outgoing
`uses` edges with `"external": true`, one per top-level library). Tracked
separately from `fan_out` so third-party usage is visible without
inflating the internal-coupling metrics or HK.

#### `coupling.hk` — number

**Henry-Kafura complexity** (1984):

```
hk = loc × (fan_in × fan_out)²
```

Combines size with coupling. `fan_in` and `fan_out` here are the
**internal** file→file counts — external library edges are excluded, so
HK measures internal architectural coupling rather than 3rd-party
library usage. A small isolated file has `hk = 0`; a large hub file can
reach values in the millions. Use as a relative ranking within a project
rather than an absolute threshold.

### `complexity.maintainability` — object, optional

Composite indices that estimate how easy the code is to maintain.
Both are derived from `halstead.volume`, `cyclomatic`, and LOC.

#### `maintainability.mi` — number

**Maintainability Index** (Oman & Hagemeister, 1992):

```
MI = 171 − 5.2 × ln(halstead.volume) − 0.23 × cyclomatic − 16.2 × ln(loc.source)
```

Higher is better. Rough thresholds: >85 — easy to maintain;
65–85 — moderate effort; <65 — difficult. Can go negative for very
complex files.

#### `maintainability.mi_sei` — number

**MI (SEI variant)** (Carnegie Mellon SEI, 1997). Adds a bonus term
for comment density:

```
MI_SEI = MI + 50 × sin(√(2.4 × comment_ratio))
```

When `loc.comments = 0` the bonus is zero and `mi_sei` equals `mi`.
A well-documented file can score ~25 points higher than its raw `mi`.

### `complexity.loc` — object, optional

Line-of-code breakdown. Multiple LOC definitions coexist because each
answers a different question.

#### `loc.total` — number

Total lines in the file, including everything.
Same as `ploc` in legacy notation.

#### `loc.source` — number

**Source lines** — lines that contain at least one non-whitespace,
non-comment character. The most common LOC metric.
(`sloc` in legacy notation.)

#### `loc.physical` — number

**Physical lines** — same as `total` for most tools; in some
implementations excludes the last blank line. (`ploc` in legacy notation.)

#### `loc.logical` — number

**Logical lines** — counts statements and expressions rather than
physical lines. A one-liner with three statements counts as 3.
(`lloc` in legacy notation.)

#### `loc.comments` — number

Lines that consist entirely of comments (inline comments on code lines
are not counted). (`cloc` in legacy notation.)

#### `loc.blank` — number

Empty or whitespace-only lines.

### `complexity.halstead` — object, optional

**Halstead metrics** (Halstead, 1977) treat a program as a sequence of
operators (keywords, punctuation, operators) and operands (identifiers,
literals). Two raw counts drive all derived metrics:

- **n1** — number of *unique* operators  
- **n2** — number of *unique* operands  
- **N1** — total operator occurrences  
- **N2** — total operand occurrences

#### `halstead.length` — number

Program length: `N = N1 + N2`. Total count of all operator and operand
tokens in the code unit.

#### `halstead.vocabulary` — number

Program vocabulary: `n = n1 + n2`. Number of distinct operators and
operands. Grows logarithmically with program size.

#### `halstead.volume` — number

Program volume: `V = N × log₂(n)`. Represents the information content
of the program in bits. The primary Halstead size metric.

Typical ranges: trivial function ~10, average function ~500,
complex file ~50 000+.

#### `halstead.effort` — number

Mental effort required to write the program:
`E = (n1/2n2) × N × log₂(n)`.

Correlates strongly with development time. Used as input for `mi`.

#### `halstead.time` — number

Estimated programming time in **seconds**: `T = E / 18`.
The divisor 18 is an empirical constant (Stroud number).
Treat as a rough order-of-magnitude estimate only.

#### `halstead.bugs` — number

Estimated number of latent bugs delivered: `B = V / 3000`.
The divisor 3000 is empirical. More useful as a relative
ranking than an absolute count.

# Metric catalog

Every metric the tool emits, grouped by **how it is produced**. Keys are **flat**:
the name in this catalog (`vocabulary`, `cyclomatic`, `hk`, `sloc`, …) is exactly
the attribute key on a node and in the JSON — there is no prefix or namespace.

## How a metric is produced

| group | produced by | where |
|---|---|---|
| **Measured from the AST** | the shared generic engine counts it during the tree walk | `code-ranker-plugins/src/engine/` (per-language `Dialect`) → a `MetricInputs` |
| **Measured from the graph** | a graph pass over the dependency edges | `code-ranker-graph` (`hk.rs`, `cycles.rs`) |
| **Derived** | a CEL `formula_cel` over measured values | `code-ranker-graph/metrics/builtin.toml`, run by the registry engine (`registry.rs`) |
| **Aggregated** | a mean / percentile / … over all nodes | `stats.rs` (built-in means) + registry graph-scope `agg()` |

Only the **measured** values and the graph passes are Rust. Everything **derived**
or **aggregated** is data (a CEL formula + spec) — edit the formula, change the
metric, no code change. Users add their own under `[metrics.<key>]`. The same
registry runs per **unit**, so it serves file nodes and (with `[levels] functions`
on) function nodes alike. How the registry orders and evaluates formulas (e.g. why
`mi` runs after `volume`) is in
[`metric-correctness.md` → How derived metrics are computed](metric-correctness.md#how-derived-metrics-are-computed-the-cel-registry-engine).

## Status legend

- **✓ emitted** — written onto the node and present in the JSON report.
- **◦ intermediate** — computed and used as a formula input only, not emitted.
  (No metric is `◦` today: every measured input is now also emitted so the viewer
  can show each derived metric's live "formula = numbers" derivation line.)

---

## Measured from the AST

> "AST" here is tree-sitter's *concrete* syntax tree (CST) — every token is a
> node, not a classic AST. We say "AST" loosely; the point is *counted from
> syntax nodes, not text*.

The engine produces these as a `MetricInputs` for each unit. The Halstead base
counts and the structural counters feed the derived formulas **and are emitted**
on the node — so the viewer can render each derived metric's live "formula = this
node's numbers" line (e.g. `length = N₁ + N₂` shown with the actual N₁/N₂). They
carry a display spec but are kept out of the default table columns.

### Halstead base counts (`compute_halstead`; operands distinguished by text)

| key | status | meaning |
|---|---|---|
| `eta1` | ✓ | unique operators (η₁) |
| `eta2` | ✓ | unique operands (η₂) |
| `n1` | ✓ | total operators (N₁) |
| `n2` | ✓ | total operands (N₂) |

### Structural counts (`walk` / `cog_walk`)

| key | status | meaning |
|---|---|---|
| `spaces` | ✓ | unit count: `source_file` (1) + each `function`/`impl`/`trait`/closure space |
| `branches` | ✓ | `if`/`for`/`while`/`loop`/`match_arm`/`try`/`&&`/`\|\|` |
| `span_sloc` | ✓ | `end_row − start_row` of the unit (the MI input) |
| `exits` | ✓ | `return` + `try` + (fn declaring `-> T`) |
| `args` | ✓ | parameters of a fn/closure (punctuation/attributes excluded) |
| `closures` | ✓ | number of closures |
| `cognitive` | ✓ | nesting/depth/lambda-weighted accumulator + boolean runs |

### LOC counts (`compute_loc`, keyed on node `row`)

| key | status | meaning |
|---|---|---|
| `sloc` | ✓ | distinct code-bearing rows (`ploc`) |
| `lloc` | ✓ | number of statement nodes |
| `cloc` | ✓ | comment lines (`only_comment + code_comment`) |
| `blank` | ✓ | `span − ploc − only_comment` |
| `tloc` | ✓ | test lines removed by `strip_cfg_test` (Rust only) |

### Plugin structural attributes (per-language plugin walk)

| key | status | meaning |
|---|---|---|
| `loc` | ✓ | raw line count of the file |
| `items` | ✓ | top-level item count |
| `unsafe` | ✓ | `unsafe` block/fn count (Rust) |

## Measured from the graph

The source is the **flow edge set** (`EdgeKindSpec.flow == true` — `uses` in Rust;
`contains`/`reexports`/`super` are non-flow). Counts are over `HashSet`s of
partners (duplicate edges collapse). External nodes carry no coupling metrics.

| key | status | meaning |
|---|---|---|
| `fan_in` | ✓ | unique internal nodes that depend on this one |
| `fan_out` | ✓ | unique internal nodes this one depends on (external excluded) |
| `fan_out_external` | ✓ | unique external libraries this one depends on |
| `cycle` | ✓ | cycle kind: `"mutual"` (2-node SCC) / `"chain"` (3+); cross-crate SCCs dropped |

## Derived (CEL formulas in `builtin.toml`)

Each is a CEL `formula_cel` evaluated by the registry engine over the measured values
(`log2`/`ln`/`pow`/`sqrt`/`sin` are host functions — the exact `f64` ops). Editing
a formula changes the metric with no Rust change. `hk` combines a graph metric
with a code metric, so it depends on `fan_in`/`fan_out` being computed first.

| key | formula | inputs |
|---|---|---|
| `length` | `n1 + n2` | `n1`, `n2` |
| `vocabulary` | `eta1 + eta2` | `eta1`, `eta2` |
| `volume` | `length · log2(vocabulary)` | `length`, `vocabulary` |
| `effort` | `D · volume`, `D = (eta1/2)·(n2/eta2)` | `eta1`, `n2`, `eta2`, `volume` |
| `time` | `effort / 18` | `effort` |
| `bugs` | `effort^(2/3) / 3000` | `effort` |
| `cyclomatic` | `spaces + branches` | `spaces`, `branches` |
| `mi` | `171 − 5.2·ln(volume) − 0.23·cyclomatic − 16.2·ln(span_sloc)` | `volume`, `cyclomatic`, `span_sloc` |
| `mi_sei` | `171 − 5.2·log2(volume) − 0.23·cyclomatic − 16.2·log2(span_sloc) + 50·sin(√(2.4·(cloc/span_sloc)))` | `volume`, `cyclomatic`, `span_sloc`, `cloc` |
| `hk` | `sloc · (fan_in · fan_out)²` | `fan_in`, `fan_out`, `sloc` (no `sloc` → no `hk`) |

Henry–Kafura (`hk`) is emitted only when `> 0`. `cyclomatic` is omitted at its
floor `1` (a function-less file); the rest omit at `0`.

## Aggregated (the `stats` block)

A per-graph map of one value per tracked metric, over all internal (file) nodes.

- **Built-in means** (`stats.rs`): the mean of a fixed key set (`builtin.toml`
  `stat = [...]` + coupling keys), emitted under the flat metric key (e.g.
  `cyclomatic`), zeros/missing excluded.
- **Declared aggregates** (registry graph-scope): a user metric with
  `scope = "graph"` whose formula calls `agg(key, reducer, population)` and lands
  in `stats` under its own key. `reducer` ∈
  `sum`/`avg`/`min`/`max`/`count`/`median`/`p<q>` (percentile by numpy R-7);
  `population` ∈ `not_empty` (value ≠ floor) / `all` (missing counted at the
  floor).

## Not built in

There is no built-in composite "ranker score". It is expressible as a user metric
— a node-scope CEL formula over other metrics, optionally normalised by injected
aggregates.

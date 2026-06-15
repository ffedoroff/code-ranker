# Metric tiers — the identifier catalog

A flat, machine-usable namespace for every metric, keyed by **tier** (distance
from a file's source text) and **source** (the raw material it comes from). Use
these dotted ids as stable input keys.

## Identifier grammar

```
t<tier><source>.<metric>[.<agg>]
```

- `<tier>` — `1`..`5` (see below).
- `<source>` — `code` or `graph`, present only where it disambiguates (tiers 1–2).
  Tiers 3–5 omit it (mixed, or operate over the whole graph).
- `<metric>` — the metric name (the field as emitted on a node).
- `<agg>` — aggregation, **tier 4 only** (`mean`, planned: `p50` / `p90` / `max` / …).

| prefix | meaning | source | computed in |
|---|---|---|---|
| `t1code.` | measured directly from a file's AST | code | per-language engine + plugin structural walk |
| `t1graph.` | measured directly from the dependency graph | graph | `code-ranker-graph` (`hk.rs`, `cycles.rs`) |
| `t2code.` | pure function of `t1code.*` | code | per-language engine |
| `t3.` | combines a graph metric with a code metric | code × graph | `code-ranker-graph` (`hk.rs`) |
| `t4.` | aggregated across all nodes | all nodes | `code-ranker-graph` (`stats.rs`) |
| `t5.` | final ranker score | all nodes | *(not yet implemented)* |

(There is no `t2graph.*`: no metric is a pure function of graph primitives alone.)

## Status legend

- **✓ emitted** — written onto the node and present in the JSON report.
- **◦ intermediate** — computed and used as an input, but not emitted on its own.
- **⧗ planned** — part of this scheme but **not implemented today**.

---

## t1code — measured from the AST

### Halstead base counts (`compute_halstead`; operands distinguished by text)

| id | status | meaning |
|---|---|---|
| `t1code.eta1` | ◦ | unique operators (η1) |
| `t1code.eta2` | ◦ | unique operands (η2) |
| `t1code.n1` | ◦ | total operators (N1) |
| `t1code.n2` | ◦ | total operands (N2) |

### Structural counts (`walk` / `cog_walk`)

| id | status | meaning |
|---|---|---|
| `t1code.spaces` | ◦ | `source_file` (1) + `function_item` + `impl_item` + `trait_item` + `closure_expression` |
| `t1code.branches` | ◦ | `if`/`for`/`while`/`loop`/`match_arm`/`try`/`&&`/`\|\|` |
| `t1code.exits` | ✓ | `return` + `try` + (fn declaring `-> T`) |
| `t1code.args` | ✓ | parameters of a fn/closure (punctuation/attributes excluded) |
| `t1code.closures` | ✓ | number of `closure_expression` |
| `t1code.cognitive` | ✓ | nesting/depth/lambda-weighted accumulator + boolean runs |

### LOC counts (`loc_walk`, keyed on node `row`)

| id | status | meaning |
|---|---|---|
| `t1code.sloc` | ✓ | distinct code-bearing rows (`ploc`) |
| `t1code.lloc` | ✓ | number of statement nodes |
| `t1code.cloc` | ✓ | comment lines (`only_comment + code_comment`) |
| `t1code.blank` | ✓ | `span − ploc − only_comment` |
| `t1code.tloc` | ✓ | test lines removed by `strip_cfg_test` (Rust only) |
| `t1code.span` | ◦ | `root.end_row − root.start_row` (unit span sloc) |

### Plugin structural attributes (per-language plugin walk)

| id | status | meaning |
|---|---|---|
| `t1code.loc` | ✓ | raw line count of the file |
| `t1code.items` | ✓ | top-level item count |
| `t1code.unsafe` | ✓ | `unsafe` block/fn count (Rust) |

## t1graph — measured from the dependency graph

The source is the **flow edge set** (`EdgeKindSpec.flow == true` — `uses` in Rust;
`contains`/`reexports`/`super` are non-flow). Edges are built by the plugin's
import resolver (`use`/`mod`), so this is source-derived but one level removed
from the AST. Counts are over `HashSet`s of partners (duplicate edges collapse).
External nodes carry no coupling metrics.

| id | status | meaning |
|---|---|---|
| `t1graph.fan_in` | ✓ | unique internal nodes that depend on this one |
| `t1graph.fan_out` | ✓ | unique internal nodes this one depends on (external excluded) |
| `t1graph.fan_out_external` | ✓ | unique external libraries this one depends on |
| `t1graph.cycle` | ✓ | cycle kind: `"mutual"` (2-node SCC) / `"chain"` (3+); cross-crate SCCs dropped |

## t2code — derived from `t1code`

| id | status | formula | inputs |
|---|---|---|---|
| `t2code.length` | ✓ | `n1 + n2` | `t1code.n1`, `t1code.n2` |
| `t2code.vocabulary` | ✓ | `eta1 + eta2` | `t1code.eta1`, `t1code.eta2` |
| `t2code.volume` | ✓ | `length · log2(vocabulary)` | n1, n2, eta1, eta2 |
| `t2code.effort` | ✓ | `D · volume`, `D = (eta1/2)·(n2/eta2)` | eta1, n2, eta2, volume |
| `t2code.time` | ✓ | `effort / 18` | effort |
| `t2code.bugs` | ✓ | `effort^(2/3) / 3000` | effort |
| `t2code.cyclomatic` | ✓ | `spaces + branches` | `t1code.spaces`, `t1code.branches` |
| `t2code.mi` | ✓ | `171 − 5.2·ln(volume) − 0.23·cyclomatic − 16.2·ln(span)` | volume, cyclomatic, span |
| `t2code.mi_sei` | ✓ | `171 − 5.2·log2(volume) − 0.23·cyclomatic − 16.2·log2(span) + 50·sin(√(2.4·(cloc/span)))` | volume, cyclomatic, span, cloc |

## t3 — combined (code × graph)

| id | status | formula | inputs |
|---|---|---|---|
| `t3.hk` | ✓ | `sloc · (fan_in · fan_out)²` | `t1graph.fan_in`, `t1graph.fan_out`, `t1code.sloc` (falls back to `t1code.loc`) |

Henry–Kafura; emitted only when `> 0`.

## t4 — aggregated across all nodes

Grammar `t4.<metric>.<agg>`, where `<metric>` is any numeric node metric from
tiers 1–3. Computed in `stats.rs` over all internal (file) nodes.

| id | status | meaning |
|---|---|---|
| `t4.<metric>.mean` | ✓ | mean of `<metric>` across all internal nodes (emitted only when `> 0`; zeros/missing excluded) |
| `t4.<metric>.p50` | ⧗ | median — **not implemented** |
| `t4.<metric>.p90` | ⧗ | 90th percentile — **not implemented** |
| `t4.<metric>.max` | ⧗ | maximum — **not implemented** |

Today `stats.rs` emits only the mean, under a flat key (e.g. `cyclomatic`), not
the `t4.cyclomatic.mean` form. The dotted form here is the target naming.

## t5 — final ranker score

| id | status | meaning |
|---|---|---|
| `t5.ranker_score` | ⧗ | final per-node score — **not implemented**; no single score field exists yet |

# Customizing code-ranker — metrics, aggregates, thresholds & report views

Almost everything code-ranker reports is **data, not code**: you can add metrics,
aggregate them, retune thresholds, and reshape the report's columns / card /
stats lists — all from TOML, no recompilation, no Rust.

This guide covers the full syntax with copy-pasteable examples, and a worked
end-to-end case: a **TLOC/SLOC ratio** metric plus an **aggregate that only
averages large files (loc > 300)**, surfaced in the JSON `stats` block, the table,
and (where the viewer supports it) the SVG map.

There are two places config lives — know which one you want:

| Layer | File | Who edits it | What it controls |
|---|---|---|---|
| **Project** | `code-ranker.toml` (in your repo) | you, per project | custom `[metrics]`, `[rules]` thresholds/cycles/**checks**, `[report]` views, `[presets]`, `[ignore]`, `[levels]`, plugin/output |
| **Language** | `<lang>.toml` (shipped in the binary) | a language plugin author | the node-kind vocabulary, presets, default thresholds, and the **report list overrides** (`[report]`) |

Custom metrics and thresholds are **project** config (runtime). The report
list-override DSL is **language** config (compiled into the plugin). Both are
described below.

---

## 1. Project config (`code-ranker.toml`)

### 1.1 Custom node metrics — `[metrics.<key>]`

A node metric is a [CEL](https://github.com/cel-spec/cel-spec) formula over the
values already on each file node. It is computed at analysis time and emitted with
a full display spec — it lands in the JSON, in the level's `node_attributes`
dictionary, and in the node popup, and is sortable / delta-coloured wherever it is
shown.

> **It is not added to the default table `columns` or card.** Those lists come
> from the metric catalog plus a `[report]` override. To show your metric as a
> column / card number, add it with a **project `[report]`** in `code-ranker.toml`
> (§1.6) — `[report] columns = { add = ["tsr"] }`.

```toml
[metrics.comment_ratio]
formula        = "sloc > 0.0 ? cloc / sloc * 100.0 : 0.0"  # CEL; guard against /0
formula_pretty = "cloc / sloc * 100"   # readable form shown in tooltips/popup
# calc        = "cloc / sloc * 100"    # JS for the live "= numbers" line; defaults to `formula`
label          = "Comments %"          # column / card label
name           = "Comment density"     # tooltip title
short          = "Cmt%"                # compact table header
description    = "Comment lines per source line, as a percentage."  # tooltip body + check `why`
remediation    = "Add doc comments to the public API and explain non-obvious logic."  # check `fix` line
direction      = "higher_better"       # higher_better | lower_better (colours deltas)
group          = "loc"                 # concern group: loc | coupling | complexity | …
warning        = 25.0                  # two-tier severity thresholds (scorecard / badges)
info           = 15.0
# also optional: value_type ("float"|"int"|"bool"|"str"), omit_at
```

Only `formula` is required; every spec field is optional (a quick metric needs
just the formula). The metric is **omitted** from output when its value rounds to
`omit_at` (default `0`) — so a guard like `sloc > 0.0 ? … : 0.0` both avoids
divide-by-zero and drops the metric on files where it has no signal.

**What each spec field drives in the UI** (so the tooltip / column read well):

| Field | Where it shows |
|---|---|
| `label` | the default column / card label, and the fallback for `name` / `short` |
| `name` | the **tooltip title** (hover a column header or a popup value) |
| `short` | the compact **table-header** text (falls back to `label`, then key) |
| `description` | the **tooltip body** — write a full sentence; this is the docs for the metric |
| `formula_pretty` | the human-readable formula shown as the **first tooltip line** (the executable `formula` is CEL and isn't shown raw) |
| `calc` | JS the viewer re-runs with the node's values to show the **second line** — the same formula filled with this file's numbers (`81 / 105 = 0.771`), exactly like the built-in `hk`. Defaults to the CEL `formula`, so a plain-arithmetic metric needs nothing; set it explicitly only if the formula uses CEL-only functions (`log2`/`pow`/…), which the viewer can't run. |
| `direction` | colours deltas green/red (`higher_better` / `lower_better`) |
| `warning` / `info` | two-tier **severity thresholds** the scorecard counts and the viewer badges against (see §1.7); a missing tier mirrors the other |

**Inputs a formula may read** (the values present on a file node):

- Halstead base counts: `eta1` `eta2` `n1` `n2`
- structural: `spaces` `branches` `cognitive` `exits` `args` `closures`
- LOC: `sloc` `lloc` `cloc` `blank` `tloc` `loc`
- derived (built-ins): `cyclomatic` `volume` `effort` `time` `bugs` `length`
  `vocabulary` `mi` `mi_sei`
- coupling: `fan_in` `fan_out` `hk`
- any earlier `[metrics.<key>]` you defined (they evaluate in dependency order)
- language-specific attrs the plugin emits (e.g. Rust `unsafe`, `items`)

CEL math host functions: `log2` `ln` `pow` `sqrt` `sin` (plus the usual ternary
`?:`, arithmetic, and comparison operators).

### 1.2 Aggregates — a graph-scope metric → the `stats` block

Set `scope = "graph"` and the formula reduces the whole project to one number,
emitted into the report's per-graph `stats` block (the summary numbers). Use the
`agg` reducer:

```toml
[metrics.cognitive_p90]
scope   = "graph"
formula = "agg('cognitive', 'p90', 'not_empty')"
label   = "Cognitive (p90)"
```

`agg(metric, reducer, population)`:

- **reducer**: `sum` `avg`/`mean` `min` `max` `count` `median` `p<q>` (any
  percentile, e.g. `p50` `p90` `p99`), and `top<N>` / `top<N>_<reducer>` — keep
  the N largest values then reduce (default `avg`): `top10` `top10_avg` `top5_sum`
  `top10_max`.
- **population**: `not_empty` — only nodes whose value carries signal (≠ the
  metric's `omit_at` floor); `all` — every internal node, missing values counted
  at the floor.

A graph formula may combine several `agg(...)` calls and reference earlier graph
metrics by name, so you can express ratios of aggregates, etc.

### 1.3 Worked example — TLOC/SLOC ratio, aggregated over large files only

Goal: a per-file **test-to-source ratio** `tloc / sloc`, plus a project aggregate
that **reduces the top 10 of the large files (`loc > 300`) to one number**, and
shows the per-file ratio as a table column right after `hk`. The full config is
[`custom-field-example.toml`](./custom-field-example.toml) next to this doc.

```toml
# 1) The per-file ratio. Guarded so files with no source don't divide by zero;
#    its value is dropped (omit_at = 0) where it has no signal.
[metrics.tsr]
formula   = "sloc > 0.0 ? tloc / sloc : 0.0"
label     = "TLOC/SLOC"
direction = "lower_better"
group     = "loc"

# 2) The same ratio, but ONLY on large files — zero elsewhere. Because the value
#    is 0 (the omit floor) on files with loc <= 300, the `not_empty` population
#    excludes them automatically. No special "filter" syntax is needed: a guarded
#    metric + `not_empty` *is* the population filter.
[metrics.tsr_big]
formula   = "loc > 300.0 ? (sloc > 0.0 ? tloc / sloc : 0.0) : 0.0"
label     = "TLOC/SLOC (loc>300)"
direction = "lower_better"
group     = "loc"

# 3) The aggregate: the worst ratio among the TOP 10 of those large files.
#    `top10_max` = keep the 10 largest values, then take their max. Into `stats`.
#    (Swap the reducer freely: `top10_avg` averages the same 10, `p90`, `sum`, …)
[metrics.tsr_big_avg]
scope   = "graph"
formula = "agg('tsr_big', 'top10_max', 'not_empty')"
label   = "TLOC/SLOC avg (top 10, loc>300)"

# 4) Show the per-file ratios as table columns — right after `hk` — and feature
#    `tsr` on the node card. (A project [report] override; §1.6.)
[report]
columns = { after = { hk = ["tsr", "tsr_big"] } }
card    = { add = ["tsr"] }
```

The full [`custom-field-example.toml`](./custom-field-example.toml) next to this doc also adds the
tooltip / `formula_pretty` fields (§1.1), a `check` threshold on `tsr` (§1.4), and a
`TSR` Prompt-Generator preset (§1.7) — so the one file demonstrates every feature
in this guide.

Run it. With no `--output.*.path`, artifacts land in the default `.code-ranker/`
directory (git-ignored), named `<timestamp>-<git-hash>.{json,html}`:

```sh
# from the repo root — JSON snapshot + the HTML viewer into .code-ranker/
code-ranker report . --config docs/customization/custom-field-example.toml --output.json --output.html

# gate on the custom-metric threshold (exits non-zero on a breach):
code-ranker check . --config docs/customization/custom-field-example.toml

# triage scorecard ranked by the custom `TSR` preset (warning tier, worst file):
code-ranker report . --config docs/customization/custom-field-example.toml \
  --output.scorecard --preset TSR --severity warning --top 1

# or send the JSON to an explicit path / stdout:
code-ranker report . --config docs/customization/custom-field-example.toml --output.json.path=-
```

The JSON `stats` block carries the aggregate:

```json
"stats": { "…": 0, "tsr_big_avg": 3.318 }
```

`tsr` and `tsr_big` are also emitted on every file node where they have signal
(`"tsr": 0.771`, …) with their display specs, so they show in the node popup and
are sortable wherever surfaced. (They are not in the *default* `ui.columns` — see
the note in §1.1.)

> **Why the guard-and-`not_empty` trick?** Populations today are `all` /
> `not_empty` — there is no `agg(..., where loc > 300)` predicate. Encoding the
> predicate **in the node metric** (`loc > 300.0 ? … : 0.0`) and letting
> `not_empty` drop the zeros gives you exactly "aggregate over files matching a
> condition" with the primitives that exist. The same pattern works for any
> predicate (`fan_in > 5`, `cyclomatic > 20`, …).

### 1.4 Thresholds & cycle rules — `[rules]`

```toml
[rules.thresholds.file]            # values accept _ separators and K/M/G suffixes
hk         = 300K                  # Henry-Kafura budget per file
cyclomatic = 200
sloc       = 800

[rules.cycles]
mutual = "off"                     # off | a max budget (e.g. mutual = 0, chain = 2)
chain  = 2
```

A breach becomes a `check` violation. Thresholdable keys are the built-in
metrics — `sloc` `loc` `lloc` `cloc` `blank` `cyclomatic` `cognitive` `hk`
`fan_in` `fan_out` `mi` `volume` `bugs` … plus the structural `items` / `unsafe`
— **and any custom `[metrics.<key>]` you defined**:

```toml
[metrics.tsr]
formula = "sloc > 0.0 ? tloc / sloc : 0.0"
group   = "loc"

[rules.thresholds.file]
tsr = 1.5      # `check` now flags every file whose test-to-source ratio > 1.5
```

`check` reads the metric off each node (the key doubles as the attribute key) and
the breach inherits the metric's concern `group` (`loc` → the `SIZ` size group), so
a custom-metric violation slots in beside the built-ins. A genuinely unknown
key — a typo, or a metric you never defined — is still rejected at load. This is a
**single-tier** gate; the **two-tier** `warning` / `info` thresholds on the metric
itself (§1.1) drive the scorecard and viewer badges instead.

### 1.5 Other project keys (brief)

```toml
plugin = "rust"                    # or "auto" (marker detection)
[ignore]
paths = ["generated/**"]           # globs pruned before metrics/cycles
tests = true                       # drop the language's test files
dev_only_crates = true             # (rust) drop dev-only dependency nodes
[levels]
functions = true                   # also emit the per-function level
```

### 1.6 Report views — `[report]` (surface a custom metric in the table / card / stats)

A custom `[metrics.<key>]` lands in the JSON and node popup but **not** in the
default table columns, card, or stats — those lists come from the metric catalog.
Add a project-level `[report]` to your `code-ranker.toml` to patch them. It uses
exactly the **list-override DSL** described in §2.1 (`add` / `remove` / `replace` /
`after` / `before` / `prepend` / `clear`), layered on top of the catalog **and**
the active language's own `[report]`:

```toml
[report]
columns = { after = { hk = ["tsr", "tsr_big"] } }  # place after `hk` in the table
card    = { add = ["tsr"] }                        # feature on the node card
stats   = { add = ["tsr_big_avg"] }                # add to the JSON stats block
size    = { add = ["tsr"] }                        # SVG circle-size mode (§3)
filter  = { add = ["tsr_big"] }                    # SVG node filter (§3)
```

- `columns` drives the HTML table and the JSON `ui.columns` (and the derived sort
  / summary lists). With `after = { hk = [...] }` the columns appear immediately
  after `hk` rather than at the end.
- `card` drives the node card's featured numbers.
- `stats` patches the aggregate keys averaged into the JSON `stats` block (a
  graph-scope metric is already emitted there; this is for re-ordering / pruning).
- `size` / `filter` drive the SVG map's circle-size modes and node filters (§3).

Only keys that actually exist on a node survive (the orchestrator prunes the
patched list), so listing a metric the current language doesn't emit is harmless.

### 1.7 Prompt-Generator presets — `[presets.<ID>]`

A **preset** is a refactoring lens: it ranks files by one metric and ships a
ready-to-paste AI prompt. The plugin catalog has the usual SOLID / complexity
presets; add your own (over a custom metric) with `[presets.<ID>]` — the table key
is the preset id. It feeds the `--preset` recommendation, the `scorecard`, and the
viewer's Prompt-Generator buttons:

```toml
[presets.TSR]
title       = "TSR — Trim inline test bulk"  # heading of the generated prompt
sort_metric = "tsr"                          # the metric the worst-first list ranks by
prompt      = """
These files carry more test lines than source lines. Move their inline test
modules into sibling test files, keeping coverage identical.
"""
# optional: label (button text, defaults to id), doc_url, connections = ["in","out","common"]
```

Only `sort_metric` is essential (the lens the preset *is*); `label` / `title`
default to the id. A project preset with the **same id** as a plugin preset
overrides it; a new id appends. Run it:

```sh
# scorecard narrowed to the preset, warning tier, worst file:
code-ranker report . --config code-ranker.toml --output.scorecard --preset TSR --severity warning --top 1

# or generate the refactoring prompt for the worst files:
code-ranker report . --config code-ranker.toml --preset TSR --output.prompt
```

For the `--severity` counts to be meaningful the metric should carry `warning` /
`info` thresholds (§1.1); without them the scorecard falls back to `hk`'s tiers for
the count, though the narrowed worst-file list still ranks by the metric.

### 1.8 Custom checks — `[rules.checks.<id>]` (write a linter rule in config)

`[rules.thresholds.file]` only expresses `metric > limit`. A **custom check** is
the general form: a CEL **boolean** `when` predicate over each file node, plus a
`message`. When the predicate is true for a file, `check` reports a violation
pinned to that file — a config-only linter rule, no Rust.

> **Full CEL reference:** [`cel-reference.md`](./cel-reference.md) documents the
> language, every built-in function, and exactly what is in scope here vs in a
> `[metrics]` formula — written for both humans and AI agents.

The predicate sees more than one number. Every node value is in scope under its
own key — numeric (`tloc`, `sloc`, `loc`, `cyclomatic`, `unsafe`, …), boolean, or
string — **plus** derived path fields and a small standard library:

| In scope | What it is |
|---|---|
| any attribute key | the node's value (`tloc`, `sloc`, `loc`, `unsafe`, `fan_in`, …) |
| `derives` `macros` `attrs` `imports` `types` `traits` | Rust-plugin syntactic facts — comma-joined strings (production code only); check via `contains(derives, "Serialize")` / `matches(...)`. `derives`/`macros`/`attrs`/`imports` are "uses X" sets; `types`/`traits` are names defined in the file |
| `path` | repo-relative file path, e.g. `crates/a/src/handler.rs` |
| `name` | basename, e.g. `handler.rs` |
| `stem` | basename without the final extension, e.g. `handler` |
| `ext` | final extension, e.g. `rs` |
| `dir` | everything before the basename, e.g. `crates/a/src` |
| `s.contains(x)` `s.startsWith(x)` `s.endsWith(x)` | CEL-native substring tests → bool |
| `s.matches(re)` | CEL-native regex match → bool |
| `n.double()` | cast an integer attribute to float — CEL's bare `/` is integer division and rejects mixed int/float, so `tloc.double() / sloc.double()` is how you take a ratio |

Because `check` runs as a **second pass over the fully-built graph**, the
predicate can also reach the *edges* and the *file set* — not just the node
itself:

| Graph in scope | What it is |
|---|---|
| `deps` | list of labels this file depends on (a path like `crates/a/infra.rs`, or `ext:<crate>` for an external dependency) |
| `rdeps` | list of labels that depend on this file (reverse edges) |
| `files` | list of every project file path (bound only when referenced) |
| `siblings` | list of files in the same folder (excluding this one) |
| `depends_on(s)` / `depended_on_by(s)` | does any out- / in-neighbour label contain `s` → bool |
| `file_exists(p)` | is `p` one of the project files → bool |
| `.size()` `.exists(x, …)` `.all(x, …)` `.filter(x, …)` | CEL list macros over any list above |

```toml
[rules.checks.test_source_ratio]
# Flag files whose inline test code outweighs their production code — but only on
# files over 100 lines (small files are noise). `.double()` makes `/` a real
# (float) division; bare int `/` would truncate the ratio to 0.
when    = "loc > 100 && sloc > 0 && tloc.double() / sloc.double() > 0.5"
message = "{tloc} inline test lines vs {sloc} source lines ({loc}-line file) — test/source ratio too high"
group   = "TST"          # concern label in diagnostics (free-form; default "LNT")
why     = "When inline tests outgrow the production code, the file is dominated by test bulk."
fix     = "Move the inline `#[cfg(test)]` tests into a sibling test module."

[rules.checks.no_direct_sqlx]
# A dependency/layer rule over the edges: this file must not import the sqlx crate.
when    = 'depends_on("ext:sqlx")'
message = "depends directly on the `sqlx` crate"
group   = "DEP"

# Reusable named helpers, expanded into a check's `when` (a helper may use an
# earlier one). Add reuse/readability, not new power.
[rules.defs]
is_test_file = 'name.endsWith("_tests.rs") || path.contains("/tests/")'
```

- **`when`** is required — any CEL boolean expression (`&&` / `||` / `!` / `? :`,
  comparisons, the functions/lists above, and `[rules.defs]` helpers). Evaluated
  per file node; a predicate that errors or yields a non-boolean simply doesn't
  fire (never panics). A `when` that fails to *compile* (or a cyclic `defs` set)
  becomes a loud violation so a typo can't pass silently.
- **`message`** is required; `why` / `fix` / `title` are optional diagnostic copy.
  All four interpolate `{key}` from the node's values (any attribute or a derived
  path field).
- **`group`** is a free-form concern label (shown in the diagnostic and the
  summary breakdown); it defaults to `LNT`.

Each fired check is a `check.<id>` rule in every output format (human, JSON,
GitHub annotations, SARIF, Code Quality) and counts toward the gate's exit code,
exactly like a threshold or cycle violation. The snippets above are
copy-pasteable into a project `code-ranker.toml` as-is.

> **What a check can't see.** The predicate reads the node's *measured values*,
> its *path*, and its *graph edges* — but not source text. So rules about
> declarations *inside* a file (a `derive`, a macro call, an attribute, a type or
> trait name) aren't expressible: nothing in the node models them. Metrics, path,
> and dependency/collection rules are the sweet spot.

---

## 2. Language config (`<lang>.toml`) — for plugin authors

A language's `<lang>.toml` **inherits** the common `defaults.toml` and overrides
only the diffs (node-kind vocabulary, presets, default thresholds, …). A language
that belongs to a **family** inherits an extra base layer in between — the chain is
`defaults.toml ⊕ [base].toml ⊕ <lang>.toml` (via `config::load_chain`): JavaScript
and TypeScript inherit `ecmascript/config.toml` (the shared engine vocab); C and C++
inherit `cfamily/config.toml`. So `c/config.toml` / `cpp/config.toml` carry only what
truly differs, and the shared bits live once in the base. Two override mechanisms
matter here.

### 2.1 The list-override DSL

Any inherited **list** can be patched instead of restated. A plain array still
**replaces** the inherited list wholesale (the default); an **op-table** mutates
it in place:

```toml
# replace wholesale (historical behaviour)
skip_dirs = ["node_modules", "dist"]

# OR patch the inherited list with an op-table:
skip_dirs = { add = ["vendor"], remove = ["dist"] }
test_dirs = { replace = { "test" = "spec" } }   # swap one element, in place
columns   = { clear = true, add = ["kind", "hk"] }  # wipe then rebuild
```

Operations, applied in this order, result de-duplicated (order stable):

| Op | Meaning |
|---|---|
| `clear = true` | start from an empty list (delete all) |
| `remove = [..]` | drop these elements (delete one or many) |
| `replace = { old = "new" }` | swap an element **in place**, keeping its position |
| `after = { anchor = [..] }` | insert right **after** the `anchor` element |
| `before = { anchor = [..] }` | insert right **before** the `anchor` element |
| `prepend = [..]` | insert at the front |
| `add = [..]` | append at the end |

The ops apply in the order above (`clear` → `remove` → `replace` → `after` /
`before` → `prepend` → `add`), and the result is de-duplicated keeping the first
occurrence. This is how you "change one element", "delete all / one", "insert at a
position", or "add" without copying the whole inherited list. If an `after` /
`before` anchor isn't present, those items fall through to the end (treated as
`add`).

### 2.2 Report list overrides — `[report]`

The report's table **columns**, card-featured metrics, and JSON **stats** keys
are inherited from the global metric catalog. A language patches them with the
same DSL — the orchestrator applies the patch over the catalog list, then prunes
to keys actually present on a node (so a language-only metric only ever surfaces
for that language):

```toml
[report]
# drop five columns, add the Rust-only `unsafe` count:
columns = { remove = ["volume", "effort", "time", "length", "vocabulary"], add = ["unsafe"] }
# also report project-wide mean `unsafe` in the JSON stats:
stats   = { add = ["unsafe"] }
# swap a card-featured metric in place:
card    = { replace = { "sloc" = "unsafe" } }
```

`columns` drives both the HTML table and the JSON `ui.columns` (and the derived
sort / summary lists); `card` drives the node card's featured numbers; `stats`
patches the aggregate keys averaged into the JSON `stats` block; `size` and
`filter` drive the SVG map's circle-size modes and node filters (§3).

> The **same `[report]` section works in the project `code-ranker.toml`** (§1.6) —
> the patches layer: catalog → language `[report]` → project `[report]`. A plugin
> author sets the language defaults; a project tweaks them on top without forking
> the plugin.

---

## 3. Viewer integration — sizing & filtering the SVG map by a metric

The **table, node card, and JSON stats** reflect your custom metrics via the `ui`
lists (`columns`, `card`, `summary`) that `[report]` patches (§1.6 /
§2.2). The **SVG map** is driven the same way — its size-mode and filter buttons are
built entirely from two more `ui` lists, so the viewer hardcodes none of them:

- **Circle sizing by a metric — `[report] size`.** Each key in `ui.size`
  becomes a button on the map; clicking it draws every node as a circle whose area
  scales with that metric (re-click to return to box mode). The built-in modes are
  `sloc` and `hk`; add your own:

  ```toml
  [report]
  size = { add = ["tsr"] }   # a "TLOC/SLOC" circle-size mode next to SLOC / HK
  ```

  Built-in metrics keep their calibrated scale; a custom metric is scaled against
  the median of the rendered population, so a ratio (~1) and a line count (~1000s)
  both spread sensibly.

- **Filter the map to a metric's population — `[report] filter`.** Each key in
  `ui.filter` becomes an on/off toggle that keeps only the nodes where that
  metric has signal — exactly how the built-in `cycle` filter isolates cycle
  members. So filtering on `tsr_big` (zero on small files; see §1.3) shows only the
  `loc > 300` files that feed the aggregate:

  ```toml
  [report]
  filter = { add = ["tsr_big"] }   # toggle: show only the large files
  ```

  The built-in default is `cycle`; a button is only shown when its metric is
  actually present on a node (so `cycle` is offered only when the project has
  cycles).

Both lists are pruned to keys present on an internal node and are patched with the
same DSL as `columns` / `card` / `stats`. The worked example
([`custom-field-example.toml`](./custom-field-example.toml)) wires both: open its
HTML report and the map offers a **TLOC/SLOC** size mode and a **tsr_big** filter.

---

## Quick reference

```text
node metric     [metrics.k] formula="<CEL over node values>"   (+ label/direction/group/…)
  ui fields     name=tooltip-title  short=table-header  description=tooltip-body
                formula_pretty=readable-formula   warning=/info= two-tier severity thresholds
aggregate       [metrics.k] scope="graph" formula="agg('m','reducer','population')"
  reducers      sum avg|mean min max count median p<q>  top<N> top<N>_<reducer>
  populations   not_empty (signal only)  |  all (missing = floor)
  "where cond"  put the predicate in a node metric: cond ? value : 0  → not_empty drops the rest
check threshold [rules.thresholds.file] k = <limit>   (K/M/G suffixes; built-ins AND custom metrics)
custom check    [rules.checks.ID] when="<CEL bool>" message="…"  (runs as a 2nd pass over the built graph)
  node in scope any attribute key  ·  path/name/stem/ext/dir  ·  CEL-native contains/startsWith/endsWith/matches  ·  n.double()
  graph in scope deps/rdeps (edge label lists)  ·  files/siblings  ·  depends_on/depended_on_by/file_exists  ·  list macros .size()/.exists/.all/.filter
  copy          message/why/fix/title  (interpolate {key})  ·  group (free-form, default LNT)
  helpers       [rules.defs] name="<cel expr>"  expanded into when (reuse; a helper may use an earlier one)
list patch      key = { clear=true, remove=[..], replace={old="new"},
                        after={anchor=[..]}, before={anchor=[..]}, prepend=[..], add=[..] }
report views    [report] columns|card|stats = <list patch>   (works in <lang>.toml AND code-ranker.toml)
map controls    [report] size|filter = <list patch>   (SVG circle-size modes / node filters; built-ins sloc,hk / cycle)
preset          [presets.ID] sort_metric="k" title="…" prompt="…"   (--preset ID / scorecard / prompt)
```

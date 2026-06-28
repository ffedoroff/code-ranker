# CEL in code-ranker — a complete reference

code-ranker evaluates [CEL](https://github.com/google/cel-spec) (Common
Expression Language, the cel-rust 0.13 implementation) in **two** config places.
This page is the full, self-contained reference: the language, every built-in
function, and exactly what is in scope in each place. It is written so an AI agent
can author correct CEL from this page alone.

> **TL;DR for agents.** Two contexts, different scopes:
> 1. **`[metrics.<key>] formula_cel`** — arithmetic over a file's **numeric** values
>    (+ `path`/`name`/… so a formula can branch on location). Inputs are floats,
>    so `/` is real division. Math helpers (`pow`, `log2`, …) available.
>    Graph-scope variant uses `agg(...)` → the `stats` block.
> 2. **`[rules.checks.<id>] when`** — a **boolean** predicate per file. Sees the
>    node's attributes (numbers/strings/bools), path fields, the dependency graph,
>    and `agg(...)` for relative thresholds. Numeric attributes are **integers**
>    here, so use `x.double()` for a ratio. Math helpers available too.
>
> Common to both: the CEL language + the cel-rust standard library below.

---

## 1. The two contexts at a glance

| | `[metrics.<key>]` `formula_cel` | `[rules.checks.<id>]` `when` |
|---|---|---|
| Result type | number | boolean |
| Evaluated | once per node (or once per graph if `scope = "graph"`) | once per file node |
| Variables | numeric inputs **+ path fields** (§4.1) | node attributes + path fields + graph lists (§4.2) |
| Number type | **float** (so `tloc / sloc` is a real ratio) | attribute's own type — **int** for counts (so use `tloc.double() / sloc.double()`) |
| Math helpers (`pow`/`log2`/…) | **yes** (§4.1) | **yes** (§4.1) |
| Graph access (`deps`/`depends_on`/…) | no | **yes** (§4.2) |
| `agg(...)` reducer | yes, when `scope = "graph"` (→ `stats`) | **yes** — for a relative threshold (node vs project distribution, §4.2) |
| CEL stdlib (§3) | yes | yes |

A formula/predicate that fails to compile is a hard config error. At runtime, a
metric formula that errors or yields a non-number is **omitted** for that node; a
check predicate that errors or yields a non-boolean simply **does not fire**
(never panics).

---

## 2. The CEL language

### Types & literals

| Type | Literal examples |
|---|---|
| int (64-bit signed) | `0`, `42`, `-7` |
| uint | `10u` |
| double (float) | `1.5`, `0.0`, `3.0` |
| bool | `true`, `false` |
| string | `"text"`, `'text'`, `"with \"quote\""` |
| list | `[1, 2, 3]`, `["a", "b"]` |
| map | `{"k": 1}` |
| null | `null` |

### Operators

- arithmetic: `+` `-` `*` `/` `%` (see §2.1 for the division gotcha)
- comparison: `==` `!=` `<` `<=` `>` `>=`
- logical: `&&` `||` `!`
- ternary: `cond ? a : b`
- membership: `x in [a, b, c]` → bool
- index / field: `list[0]`, `map["k"]`

String `+` concatenates: `dir + "/" + name`.

### 2.1 The division gotcha (read this)

CEL division follows the operand types:

- `int / int` → **integer** division (truncates): `tloc / sloc` where both are ints
  gives `0` for `40 / 100`. `int / float` (mixed) is a **type error**.
- `float / float` → real division.

Consequences per context:

- In a **`[metrics]` formula** every input is already a float, so
  `tloc / sloc` is a real ratio — nothing extra needed.
- In a **`[rules.checks]` predicate** numeric attributes are integers, so write
  `tloc.double() / sloc.double() > 0.5` (the native `double()` cast, §3). A bare
  `tloc / sloc` would truncate to 0/1.

Always guard a denominator: `sloc > 0 && tloc.double() / sloc.double() > 0.5`.

---

## 3. cel-rust standard library (available in BOTH contexts)

These come from cel-rust's `Context::default()`. String/collection functions are
callable **either as a method or as a function** — `path.endsWith(".rs")` ≡
`endsWith(path, ".rs")`.

### Strings

| Call | Meaning |
|---|---|
| `s.contains(x)` | `s` contains substring `x` → bool |
| `s.startsWith(x)` | prefix test → bool |
| `s.endsWith(x)` | suffix test → bool |
| `s.matches(re)` | full **regex** match (RE2 syntax) → bool. A malformed pattern is a runtime error (the check just doesn't fire). |
| `size(s)` | length in bytes → int |

### Type conversions

| Call | Meaning |
|---|---|
| `x.double()` / `double(x)` | int/uint → float (the ratio cast) |
| `x.int()` / `int(x)` | → int (truncates a float) |
| `x.uint()` | → uint |
| `string(x)` | → string |
| `bytes(s)` | string → bytes |

### Numbers & collections

| Call | Meaning |
|---|---|
| `size(list)` | element count → int |
| `list.contains(x)` | membership → bool (same as `x in list`) |
| `max(a, b, …)` / `min(a, b, …)` | extreme of the arguments |

### List comprehension macros

Over any list (`deps`, `files`, a literal `[…]`):

| Macro | Meaning |
|---|---|
| `list.exists(x, pred)` | some element satisfies `pred` → bool |
| `list.all(x, pred)` | every element satisfies `pred` → bool |
| `list.exists_one(x, pred)` | exactly one satisfies → bool |
| `list.filter(x, pred)` | sublist of matching elements |
| `list.map(x, expr)` | list of `expr` over each element |
| `has(map.field)` | field presence → bool |

Example: `deps.exists(d, d.startsWith("ext:")) && deps.filter(d, d.contains("/infra/")).size() == 0`.

(cel-rust also ships `optional.*` and, with timestamps, `duration`/`timestamp`
helpers — rarely needed here; see the cel-rust docs.)

---

## 4. What each context adds

### 4.1 `[metrics.<key>]` formulas

**Variables** — the file's measured values, all **floats**:

- Halstead base counts: `eta1` `eta2` `n1` `n2`
- structural: `spaces` `branches` `cognitive` `exits` `args` `closures` `span_sloc`
- LOC: `sloc` `lloc` `cloc` `blank` `tloc` `loc`
- derived built-ins: `cyclomatic` `volume` `effort` `time` `bugs` `length`
  `vocabulary` `mi` `mi_sei`
- coupling: `fan_in` `fan_out` `hk`
- any **earlier** `[metrics.<key>]` you defined (they evaluate in dependency order)
- language-specific numeric attrs a plugin emits — Rust adds `unsafe` (count of
  `unsafe` blocks/fns/impls/traits) and `items` (count of top-level items:
  `fn`/`struct`/`enum`/`impl`/`trait`/`mod`/`const`/…). These are Rust-only because
  only the Rust plugin computes them; another language's nodes simply don't carry
  the key (a predicate referencing an absent attribute just doesn't fire).
- **derived path fields** (strings): `path` `name` `stem` `ext` `dir` — so a
  formula can branch on the file's location, e.g.
  `path.contains("/generated/") ? 0.0 : hk` to blank a metric for codegen. (Same
  fields as in checks, §4.2.)

**Math host functions** (available in both contexts — metrics and checks): `log2`
`ln` `log10` `pow` `sqrt` `sin` `cos` `abs` `min2(a,b)` `max2(a,b)`. (`min2`/`max2`
are the two-argument forms; cel's variadic `min`/`max` from §3 also work.)

**Graph-scope aggregates** — set `scope = "graph"` and use `agg(metric, reducer,
population)` to reduce the whole project to one number (lands in the `stats`
block):

- **reducer**: `sum` · `avg`/`mean` · `min` · `max` · `count` · `median` ·
  `p<q>` (any percentile, e.g. `p50` `p90` `p99`) · `top<N>` / `top<N>_<reducer>`
  (keep the N largest, then reduce — default `avg`: `top10`, `top10_max`, `top5_sum`).
- **population**: `not_empty` (only nodes whose value ≠ the metric's `omit_at`
  floor) · `all` (every internal node, missing counted at the floor).

```toml
[plugins.base.metrics.comment_ratio]
formula_cel = "sloc > 0.0 ? cloc / sloc * 100.0 : 0.0"   # float division, guarded

[plugins.base.metrics.cognitive_p90]
scope       = "graph"
formula_cel = "agg('cognitive', 'p90', 'not_empty')"
```

> Strings/paths are **not** in scope in a metric formula — only numbers. Path and
> string predicates belong in `[rules.checks]`.
>
> **Per-language metrics.** A metric under `[plugins.base.metrics.<key>]` applies to
> every language. To define a metric for one language only, put it under
> `[plugins.<lang>].metrics` (e.g. `[plugins.rust.metrics.unsafe_density]`);
> to share one across all languages as a base, use `[plugins.base.metrics]`. The
> CEL, scope, and field semantics are identical — only the scope of the override
> differs. (See [config-resolution.md](config-resolution.md) for the full
> per-language precedence.)

### 4.2 `[rules.checks.<id>]` predicates

**Node attributes** — every attribute on the file node, under its own key, in its
own type:

- numeric (int): `tloc` `sloc` `loc` `cloc` `blank` `cyclomatic` `cognitive` `hk`
  `fan_in` `fan_out` `mi` `volume` `bugs` … and any custom `[metrics.<key>]`
- Rust-plugin string facts (production code only, comma-joined; check with
  `contains`): `derives` `macros` `attrs` `imports` `types` `traits`
  *(Rust-only — other languages don't emit these.)*
- `visibility`, `crate` (strings), `unsafe`, `items` (Rust)

**Derived path fields** (strings, always present):

| Field | Example (`crates/a/src/handler.rs`) |
|---|---|
| `path` | `crates/a/src/handler.rs` |
| `name` | `handler.rs` |
| `stem` | `handler` |
| `ext` | `rs` |
| `dir` | `crates/a/src` |

**Dependency graph (lists of labels)** — a label is a file path, or `ext:<crate>`
for an external dependency:

| Variable / function | Meaning |
|---|---|
| `deps` | list this file depends on (out-edges) |
| `rdeps` | list of files that depend on this file (in-edges) |
| `files` | every project file path |
| `siblings` | files in the same folder (excluding this one) |
| `depends_on(s)` | any out-neighbour label contains `s` → bool |
| `depended_on_by(s)` | any in-neighbour label contains `s` → bool |
| `file_exists(p)` | is `p` one of the project files → bool |

**Relative thresholds — `agg(metric, reducer, population)`** — the same reducer
as in graph-scope metrics (§4.1), but usable *inside a predicate* to compare this
node against the **whole project's distribution** — a threshold no fixed number
can express portably:

```toml
[plugins.base.rules.checks.complexity_outlier]
# Flag files in the project's worst 10% by cyclomatic complexity.
when    = "cyclomatic.double() > agg('cyclomatic', 'p90', 'not_empty')"
message = "{name}: cyclomatic {cyclomatic} is in the project's top 10%"

[plugins.base.rules.checks.oversized]
# More than twice the median file size.
when    = "loc.double() > 2.0 * agg('loc', 'median', 'not_empty')"
```

The aggregate is computed once over every internal node; reducers and populations
are exactly those listed in §4.1. (A `[plugins.base.metrics]` entry with `scope="graph"` writes a
*project* number into `stats`; here the same number is compared *per file*.)

**Helpers — `[plugins.base.rules.defs]`** — name a reusable sub-expression, expanded into
`when` before compilation (a helper may use an earlier one; a cycle is an error):

```toml
[plugins.base.rules.defs]
is_test_file = 'name.endsWith("_tests.rs") || path.contains("/tests/")'
in_domain    = 'path.contains("/domain/")'

[plugins.base.rules.checks.no_infra_in_domain]
when    = 'in_domain && deps.exists(d, d.contains("/infrastructure/"))'
message = "{path}: a domain file depends on infrastructure"
group   = "ARCH"
```

**Message interpolation** — `message`, `why`, `fix`, `title` substitute `{key}`
with any node attribute or derived path field: `"{tloc} test lines in {name}"`.
An unknown `{key}` is left verbatim.

> The math host functions (`pow`/`log2`/`sqrt`/…, §4.1) are registered here too,
> so a predicate can do real arithmetic over node values
> (`sqrt(hk.double()) > 10.0`). For a value you reference in several rules, prefer
> computing it once as a `[metrics.<key>]` and thresholding/referencing that.

---

## 5. Worked examples

```toml
# Metric: a guarded ratio (floats → real division)
[plugins.base.metrics.tsr]
formula_cel = "sloc > 0.0 ? tloc / sloc : 0.0"

# Check: test bulk over a production file, exempting test files
[plugins.base.rules.checks.inline_test_bulk]
when    = 'tloc > 100 && !name.endsWith("_tests.rs") && !path.contains("/tests/")'
message = "{tloc} lines of inline test code in {name}"

# Check: a ratio in a predicate (ints → .double())
[plugins.base.rules.checks.too_much_test]
when    = "loc > 100 && sloc > 0 && tloc.double() / sloc.double() > 0.5"
message = "test/source ratio too high ({tloc}/{sloc})"

# Check: forbidden dependency (edges)
[plugins.base.rules.checks.no_sqlx]
when    = 'depends_on("ext:sqlx")'
message = "imports the sqlx crate directly"

# Check: a list-comprehension macro over the dependency set. `filter` is the
# macro; `size()` is the collection function that counts the result. (A bare
# `deps.size() > 20` needs no macro and just equals `fan_out > 20`.)
[plugins.base.rules.checks.wide_ext_hub]
when    = 'deps.filter(d, d.startsWith("ext:")).size() > 20'
message = "{name}: depends on many external crates — a coupling hub"

# Metric: the same macro in a formula. Graph lists (`deps`/`files`/…) are
# checks-only (§4.2), so a metric macro runs over a *literal* list — here the
# file's own complexity signals — counting how many exceed a floor.
[plugins.base.metrics.complexity_signals]
formula_cel = "[cyclomatic, cognitive, branches].filter(x, x > 10.0).size().double()"

# Check: relative threshold (this node vs the project distribution)
[plugins.base.rules.checks.complexity_outlier]
when    = "cyclomatic.double() > agg('cyclomatic', 'p90', 'not_empty')"
message = "{name}: cyclomatic {cyclomatic} is in the project's worst 10%"

# Metric: branch on path (blank the metric for generated code)
[plugins.base.metrics.real_hk]
formula_cel = 'path.contains("/generated/") ? 0.0 : hk'

# Metrics: size-normalized complexity — branching *per 100 source lines*. A raw
# `cognitive`/`cyclomatic` count just tracks size; dividing by `sloc` measures
# DENSITY, in intuitive units (e.g. 42 = 42 points of cognitive load per 100 lines).
# Guard the divide (`sloc == 0 -> 0`).
[plugins.base.metrics.cognitive_per_100sloc]
formula_cel = "sloc > 0.0 ? cognitive / sloc * 100.0 : 0.0"

[plugins.base.metrics.cyclomatic_per_100sloc]
formula_cel = "sloc > 0.0 ? cyclomatic / sloc * 100.0 : 0.0"

# Check: a SHORT-but-DENSE file — the most complexity packed into the fewest lines,
# judged RELATIVE to this repo (no fixed number ports across codebases). Custom
# metrics are aggregatable, so we threshold each density against its own p90:
#   1. top-decile cognitive density    cognitive_per_100sloc  > p90
#   2. top-decile branching density     cyclomatic_per_100sloc > p90
#   3. genuinely short                   sloc < project median  → true density, not bulk
# (3) is what excludes large-and-dense files: a 200-line file can top the density
# deciles yet isn't "short". A multi-line `when` (TOML `'''…'''`) stays readable —
# CEL ignores the newlines; a node missing an attr just doesn't fire (never errors).
[plugins.base.rules.checks.dense_complexity]
when = '''
  cognitive_per_100sloc  > agg('cognitive_per_100sloc',  'p90', 'not_empty') &&
  cyclomatic_per_100sloc > agg('cyclomatic_per_100sloc', 'p90', 'not_empty') &&
  sloc.double() < agg('sloc', 'p50', 'not_empty')
'''
message = "{name}: dense complexity — {cognitive} cognitive / {cyclomatic} cyclomatic packed into {sloc} sloc (top-decile density for this repo)"
why     = "High branching crammed into few lines reads as clever but is hard to follow and test."
fix     = "Extract the nested branches into named helpers — trade a few more lines for lower per-line complexity."
group   = "SRP"
```

---

## 6. Checklist for agents

1. **Pick the context.** A number per file → `[plugins.base.metrics]`. A pass/fail rule →
   `[plugins.base.rules.checks]`.
2. **Ratios:** float-divide. In a `metrics` formula just `a / b`; in a `rules.checks`
   predicate write `a.double() / b.double()`, and guard `b > 0`.
3. **Edges/collections** (`deps`/`depends_on`/`files`/…) are checks-only.
   **Path fields, math functions, and `agg(...)`** work in both contexts.
   **Relative threshold?** use `agg(metric, reducer, 'not_empty')` in a check —
   e.g. `hk.double() > agg('hk','p90','not_empty')`.
4. **String tests:** prefer the method form — `path.endsWith(".rs")`,
   `path.contains("/api/")`, `name.matches("^v[0-9]+$")`.
5. **Dependency rules:** `depends_on("ext:<crate>")` for an external crate,
   `deps.exists(d, d.contains("/layer/"))` for an internal layer.
6. **Don't divide by zero**; don't mix int and float (`int / float` errors — cast
   with `.double()`).
7. A check `message`/`why`/`fix` can interpolate `{attr}` — use it to make the
   diagnostic specific.

See [`README.md`](./README.md) for the full customization guide and
[`custom-field-example.toml`](./custom-field-example.toml) for a runnable
metrics/threshold/principle example.

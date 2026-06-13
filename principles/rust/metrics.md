# How a line is counted (in Rust)

**TL;DR**: Code Ranker classifies every physical line of a Rust file into exactly
one LOC bucket — `sloc`, `cloc`, `blank`, or `tloc`. Inline tests are split off
*first* (`#[cfg(test)]` / `#[test]` / `#[bench]`), so they never inflate the
production size, HK, or complexity of a file. The walk-through below counts a
14-line hello-world by hand.

## What "correct" means (normative)

This file is the **source of truth** for *what each metric counts* in Rust — the
definition the **Metric Accuracy** goal (`cpt-code-ranker-nfr-metric-accuracy`)
and its tests assert against (see [`docs/metric-correctness.md`](../../docs/metric-correctness.md)).
Three rules hold for **every** metric below, so the per-metric sections only list
what *does* count:

- **Counted from the parsed AST, never from text.** A metric keyword that appears
  only as a look-alike — inside an identifier (`super_unsafe_fn`), a comment, a
  string / char literal, a doc-comment, an attribute, or an unexpanded macro body
  — **does not count**. No false positives from text.
- **Production code only.** `#[cfg(test)]` / `#[test]` / `#[bench]` items are
  stripped first, so nothing inside them counts toward any production metric
  (their lines become `tloc`).
- **Macros are not expanded.** A construct generated *inside* a macro invocation
  is invisible — a deliberate non-goal, not a missed count.

**Keyword look-alike guard set.** These are the construct keywords/operators a
complexity or `unsafe` metric can key on; the FP tests inject each *only* as a
look-alike (comment / string / identifier / …) and assert no metric moves. The
set may be a superset of the analyzer's exact increment triggers — guarding a
non-trigger keyword is harmless, missing a real one is not:
`if`, `else`, `match`, `while`, `for`, `loop`, `return`, `unsafe`, `&&`, `||`,
`?`. (The FP matrix in `code-ranker-complexity` iterates this exact list, and a
test asserts the list it uses is documented here — so the two cannot drift.)

Per-function metrics (`cyclomatic`, `cognitive`, `exits`, `args`, `closures`) are
**summed over the file's functions** and **omitted at their no-signal value**
(`omit_at` — `1` for `cyclomatic`, `0` for the rest; see `node_schema.md`). The
per-construct increment rules for these come from `rust-code-analysis` (our
analyzer of record); the guarantees this spec adds on top are the three rules
above plus summation and omission. Reconciling the analyzer's definitions against
independent tools is the differential layer in `docs/metric-correctness.md`.

## The example

Each line carries its own checkbox matrix in a trailing comment: the first
column is the line number `#`, then exactly one of `sloc`/`cloc`/`blank`/`tloc`
is ticked. Every line additionally counts toward `loc`.

```rust
//                                                 | #  | sloc | cloc | blank | tloc |
// Greet the world.                             // | 1  |      |  x   |       |      |
fn greet() -> &'static str {                    // | 2  |  x   |      |       |      |
    "hello, world"                              // | 3  |  x   |      |       |      |
}                                               // | 4  |  x   |      |       |      |
                                                // | 5  |      |      |   x   |      |
#[cfg(test)]                                    // | 6  |      |      |       |  x   |
mod tests {                                     // | 7  |      |      |       |  x   |
    use super::*;                               // | 8  |      |      |       |  x   |
    // check the greeting                       // | 9  |      |      |       |  x   |
                                                // | 10 |      |      |       |  x   |
    #[test]                                     // | 11 |      |      |       |  x   |
    fn greets() {                               // | 12 |      |      |       |  x   |
        assert_eq!(greet(), "hello, world");    // | 13 |      |      |       |  x   |
    }                                           // | 14 |      |      |       |  x   |
}                                               // | 15 |      |      |       |  x   |
```

Reading the matrix: line 1 is comment-only (`C`), lines 2–4 are real code
including the bare `}` (`S`), line 5 is whitespace (`B`), and everything from
the `#[cfg(test)]` attribute through the module's closing brace is test code
(`T`). Note lines 9 and 10 inside the test region: the comment `// check the
greeting` does **not** count as `cloc`, and the blank line does **not** count
as `blank` — both fall into `tloc`. The whole region is removed by line range
*first*, so neither comment nor blank lines inside it are ever classified as
production; only the comment on line 1 (outside any test) is `cloc`.

## The totals

| Metric | Value | Definition |
|--------|------:|------------|
| `sloc` | **3** | Source lines — code with a non-whitespace, non-comment character. Production only. |
| `cloc` | **1** | Comment-only lines. |
| `blank` | **1** | Empty / whitespace-only lines. |
| `tloc` | **10** | Test lines — the *whole* `#[cfg(test)]` region (lines 6–15), attribute, comment, blank, and braces included. |
| `loc` | **15** | Raw file line count (every line, tests included). |

`sloc + cloc + blank + tloc = loc` → `3 + 1 + 1 + 10 = 15`. The four production
buckets partition the file, and `tloc` is the complement carved out before any
production measurement happens.

## Why tests are split off first

The pass strips `#[cfg(test)]` / `#[test]` / `#[bench]` items (via a `syn` AST
walk) *before* measuring anything. So the production source the analyzer
actually sees is just lines 1–5:

```rust
// Greet the world.
fn greet() -> &'static str {
    "hello, world"
}
```

Everything downstream — `sloc`, `cloc`, `blank`, the Halstead block, cyclomatic
/ cognitive complexity, and `hk = sloc × (fan_in × fan_out)²` — is computed on
*this* remainder. A file with a huge inline test suite is not penalized for it;
those lines live in `tloc` and nowhere else.

Two consequences worth remembering:

- A comment or blank line *inside* a test region is `tloc`, not `cloc` / `blank`
  (lines 9–10). The region is removed wholesale, by line range, before comments
  and blanks are counted.
- `cfg(feature = "test")` is **not** a test — only a bare `test` *identifier*
  inside `cfg(...)` (including `cfg(all(test, …))` / `cfg(any(test, …))`)
  triggers the split. A string literal `"test"` is left as production code.

## The metrics that aren't per-line

`sloc` / `cloc` / `blank` / `tloc` map one-to-one onto physical lines. Every
other metric is *derived* over the production remainder (lines 1–5 in our
example) and belongs to the whole unit, not to a single line. They come from
two sources: **`rust-code-analysis`** (complexity, Halstead, MI) and the
**dependency graph** (`fan_in` / `fan_out` / `hk`).

### `lloc` — logical lines of code

Counts **statements**, not physical lines. A statement spread over three
physical lines is one `lloc`; three statements on one line are three. So `lloc`
ignores formatting and measures "how much is happening". Our `greet` body has a
single expression statement, so `lloc = 1` while `sloc = 3`.

### `cyclomatic` — independent paths

Start at **1** (the single straight-line path), then **+1 for every branch
point**. In Rust the analyzer counts each of: `if`, `for`, `while`, `loop`,
match arm, `?` (try), `&&`, `||`. No branches → `1`.

```rust
fn classify(n: i32) -> &'static str {   //   base                 = 1
    if n < 0 {                          //   if                  +1  → 2
        "neg"
    } else if n == 0 || n == 1 {        //   else-if +1, ||  +1  → 4
        "small"
    } else {
        "big"
    }
}                                       //   cyclomatic           = 4
```

A `cyclomatic` of N means you need at least N test cases to cover every path.
The example above is one function; the **file-level** `cyclomatic` is the **sum
over every function** in the file, so it tracks the file's total branching
burden. A file with no functions (a pure type or `clap` declaration) carries no
function-level paths, so the metric is **omitted** rather than reported as a
bare `1`.

### `cognitive` — how hard it is to *read*

Like cyclomatic, but weighted by **nesting depth** and biased toward
control-flow that humans find confusing. A branch at the top level costs 1; the
same branch nested two levels deep costs 1 + 2 = 3. A flat sequence of `if`s is
cheap; a deeply nested pyramid is expensive even at the same cyclomatic count.
Linear code, early returns, and `&&`/`||` chains add little; `break`/`continue`
to a label and re-nesting add a lot. Like `cyclomatic`, the file-level value is
the **sum over every function**; a function-less file omits it.

### `exits`, `args`, `closures` — structural counts

Per-function tallies, **summed over the file's functions** (like `cyclomatic` —
not read from the file root, which is why a function-less file omits them):

- **`exits`** — explicit exit points: each `return` and each `?` (try) operator.
- **`args`** — parameter count, summed over functions **and** closures.
- **`closures`** — number of closures (`|…| …`) defined.

A function-less file omits all three (their no-signal value is `0`). As with the
others, a `return`/`?`/`|` appearing in a comment, string, or identifier does not
count — these are AST counts, not text matches.

### Halstead — the operator/operand dictionaries

This is where the **dictionaries** come in. `rust-code-analysis` walks the
syntax tree and fills two maps:

- **operators** — keyed by token *kind* (`+`, `=`, `if`, `(`, `fn`, …). Counts
  how many distinct operators appear and how often.
- **operands** — keyed by the literal *text* of identifiers and literals (`a`,
  `greet`, `2`, `"hello"`, …).

From the two maps come four raw counts:

| Symbol | Meaning | From |
|--------|---------|------|
| **η₁** | distinct operators | `operators.len()` |
| **N₁** | total operator occurrences | sum of operator counts |
| **η₂** | distinct operands | `operands.len()` |
| **N₂** | total operand occurrences | sum of operand counts |

Everything else is arithmetic on those four. Worked on the expression
`x = a + a * 2` (illustrative tokenization):

```
operators: =, +, *        → η₁ = 3,  N₁ = 3   (each used once)
operands:  x, a, a, 2     → η₂ = 3,  N₂ = 4   (a appears twice)
```

| Metric | Formula | Plugged in | Value |
|--------|---------|-----------|------:|
| **`vocabulary`** | η₁ + η₂ | 3 + 3 | **6** |
| **`length`** | N₁ + N₂ | 3 + 4 | **7** |
| **`volume`** | length × log₂(vocabulary) | 7 × log₂6 | **18.1** |
| *difficulty* | (η₁ ÷ 2) × (N₂ ÷ η₂) | 1.5 × 1.33 | 2.0 |
| **`effort`** | difficulty × volume | 2.0 × 18.1 | **36.2** |
| **`time`** | effort ÷ 18 | 36.2 ÷ 18 | **2.0 s** |
| **`bugs`** | effort^(2/3) ÷ 3000 | 36.2^0.667 ÷ 3000 | **0.0037** |

So `vocabulary` is simply "how many *different* symbols the code uses", and
`length` is "how many symbols total". `volume` reads them as the bits needed to
encode the program; `time` (the 18 is Halstead's empirical "Stroud number")
estimates implementation seconds; `bugs` estimates delivered defects.

### `mi` / `mi_sei` — maintainability index

A single 0–100 score (higher = more maintainable) folding volume, branching,
and size together:

```
mi      = 171 − 5.2·ln(volume) − 0.23·cyclomatic − 16.2·ln(sloc)
mi_sei  = 171 − 5.2·log₂(volume) − 0.23·cyclomatic − 16.2·log₂(sloc)
                + 50·sin(√(2.4 × comment_ratio))        comment_ratio = cloc ÷ sloc
```

`mi` punishes big (`sloc`), complex (`cyclomatic`), and dense (`volume`) code.
`mi_sei` is the SEI variant: same skeleton on a log₂ basis, plus a bonus for
comment density — well-documented code scores higher.

### `fan_in` / `fan_out` — graph coupling

These come from the **dependency graph**, not the file's text. Over the flow
edges (real `use`/path/derive dependencies — `pub use` re-exports are excluded),
for each internal node we count **unique** partners:

- **`fan_in`** — how many distinct modules depend *on* this one.
- **`fan_out`** — how many distinct internal modules this one depends on.

Two things decide whether an import counts. First it's **resolved** to the file
that defines the item (following `pub use` re-exports). Then only edges of a
**flow kind** are tallied: of the four edge kinds the Rust plugin emits, just
`uses` is `flow: true` — `contains` (the `mod` tree), `reexports` (`pub use`
facades), and `super` (glob pulls from an ancestor) are all `flow: false` and
never reach `fan_in` / `fan_out` / `hk`. So "resolves to X" and "counts toward
coupling" are separate gates; an edge must pass both.

Worked on `parser.rs`. First, what `parser.rs` itself imports (`fan_out`):

```rust
// parser.rs
use crate::ast::{Node, Expr};   // uses → ast.rs        → fan_out +1
use crate::lexer::Token;        // uses → lexer.rs       → fan_out +1
use crate::ast::Stmt;           // uses → ast.rs (same file) — already counted
use serde::Serialize;           // uses → serde, but external → fan_out_external, not fan_out
                                 //                       → fan_out = 2
```

`ast.rs` and `lexer.rs` are two distinct internal files, so `fan_out = 2`. The
imports resolve to the *defining file*, so `Node`, `Expr`, and `Stmt` (all
defined in `ast.rs`) collapse to one partner — partners are **unique**, so
importing ten items from one file is still `1`. (Had `Stmt` instead been a
submodule living in its own file, it would resolve to *that* file and count
separately.) `serde` resolves fine and its edge is even `flow: true`, but the
target is an external crate, so HK routes it to `fan_out_external` rather than
`fan_out`.

Now, who depends on `parser.rs` (`fan_in`) — looking across the other files:

```rust
// mod.rs — declares the module
mod parser;                      // contains  → parser.rs  — flow: false, NOT counted (structure)

// lib.rs — crate facade
pub use crate::parser::Parser;   // reexports → parser.rs  — flow: false, NOT counted (facade)

// repl.rs
use crate::parser::Parser;       // uses      → parser.rs  → counts for parser.rs: fan_in +1
use crate::parser::parse;        // uses      → parser.rs (same file) — already counted

// main.rs
use crate::parser::parse;        // uses      → parser.rs  → counts for parser.rs: fan_in +1
                                 //                         → parser.rs fan_in = 2
```

Only the two `uses` edges count, so `parser.rs` has `fan_in = 2` (from `repl.rs`
and `main.rs`). The other two edges into `parser.rs` resolve to it but are
non-flow and dropped: `mod parser;` in `mod.rs` is a `contains` edge (module
ownership — structure, not a dependency), and `pub use` in `lib.rs` is a
`reexports` edge (a facade). This is exactly why hub files like `mod.rs` and
`lib.rs` don't accumulate false coupling. `repl`'s second import is another
`uses` edge to the same file, so the uniqueness rule collapses it to one.

> **Identity: `Σ fan_in = Σ fan_out` across the whole project.** Every internal
> edge adds +1 to its source's `fan_out` *and* +1 to its target's `fan_in`, so
> summed over all nodes both equal the number of unique internal dependency
> edges — the directed-graph "sum of in-degrees = sum of out-degrees" handshake.
> It holds because the same edge set feeds both metrics symmetrically: external
> edges are excluded from both (they live in `fan_out_external`), and pruning
> never leaves a dangling edge. Note this is only true for the *totals* — any
> single node usually has different `fan_in` and `fan_out` (that asymmetry is
> exactly what `hk` rewards), and `fan_out_external` is outside the balance.

Dependencies on **external libraries** (std, third-party crates) are *not*
counted toward `fan_out`. They're tracked apart because we measure how coupled a
module is *within this codebase* — those are the edges you can actually
refactor. A dependency on `serde` is a fixed cost; a dependency on a sibling
module is a design choice that drives `hk` and splitting decisions.

### `hk` — Henry-Kafura coupling

Combines size with how central the module is in the graph. Worked on a node B
with 4 source lines, imported by 3 modules and importing 2:

```
hk = sloc × (fan_in × fan_out)²
   = 4    × (3      × 2      )²  = 4 × 36 = 144
```

The coupling term is **squared**, so a small file wired into many collaborators
on both sides scores far higher than a large but isolated one.

External-only dependencies don't count (they land in `fan_out_external`), and a
node with no internal coupling on one side (`fan_in` or `fan_out` = 0) gets
`hk = 0`, which is dropped. See [henry-kafura-coupling.md](henry-kafura-coupling.md)
for the full rationale.

### Project averages (the `stats` block)

Finally, the pipeline emits a per-project **mean** of each tracked metric
(`cyclomatic`, `cognitive`, `fan_in`, `fan_out`, `hk`, `mi`, `mi_sei`, `sloc`,
`cloc`, `blank`, `tloc`, and the Halstead set) over all internal file nodes.
Zero and missing values are excluded from a metric's average, and a metric is
emitted only when its average is positive — so a project with no inline tests
simply has no `tloc` average rather than a misleading `0`.

## Where these formulas come from

Each metric traces back to a published source; Code Ranker just implements them
(via `rust-code-analysis`) over the production remainder.

- **Halstead** (`vocabulary`, `length`, `volume`, `effort`, `time`, `bugs`) —
  Maurice H. Halstead, *Elements of Software Science*, Elsevier, 1977. This is
  where operators/operands, the η/N counts, and `V = N·log₂η`, `E = D·V`,
  `T = E/18` originate. The constants `18` (Stroud number) and `3000` (mental
  discriminations per delivered bug) are Halstead's empirical values — the
  `rust-code-analysis` implementation cites them inline.[^impl]
- **`cyclomatic`** — Thomas J. McCabe, "A Complexity Measure", *IEEE
  Transactions on Software Engineering*, SE-2(4), 1976, pp. 308–320. The
  "edges − nodes + 2" graph definition that reduces to "branches + 1".
- **`cognitive`** — G. Ann Campbell, "Cognitive Complexity: A new way of
  measuring understandability", SonarSource white paper, 2018 (and the
  companion paper "Cognitive Complexity — An Overview and Evaluation",
  *TechDebt 2018*). The nesting-weighted model that deliberately breaks from
  McCabe's.
- **`mi` / `mi_sei`** — Paul Oman & Jack Hagemeister, "Metrics for assessing a
  software system's maintainability", *ICSM 1992*. The original
  `171 − 5.2·ln(V) − 0.23·G − 16.2·ln(LOC) + 50·sin(√(2.4·CM))`. The `mi_sei`
  log₂ variant is from the SEI *C4 Software Technology Reference Guide*, 1997.
- **`fan_in` / `fan_out` / `hk`** — Sallie Henry & Dennis Kafura, "Software
  Structure Metrics Based on Information Flow", *IEEE Transactions on Software
  Engineering*, SE-7(5), 1981, pp. 510–518.

[^impl]: The `18` and `3000` constants are documented in the
`rust-code-analysis` source (`src/metrics/halstead.rs`), which cites a
[GeeksforGeeks summary](https://www.geeksforgeeks.org/software-engineering/software-engineering-halsteads-software-metrics/)
and a [Purdue technical report](https://docs.lib.purdue.edu/cgi/viewcontent.cgi?article=1145&context=cstech)
for the derivations.

## Related

- [Henry-Kafura coupling](henry-kafura-coupling.md) — how `sloc` feeds `hk`.
- [Module size](module-size.md) — what a healthy `sloc` looks like.

# HK — Henry-Kafura Coupling (in Rust)

**TL;DR**: Henry-Kafura "information flow" complexity scores a module by how
much it sits in the middle of the dependency graph and how big it is:
`HK = sloc × (fan_in × fan_out)²`. A high HK module is large *and* a busy
crossroads — the most expensive place in the codebase to change.

<!-- doc:base "What it measures" -->
<!-- doc:base "Why it matters" -->

## In Rust

Fan-in and fan-out are counted over real code dependencies (`use` paths,
qualified paths, derives) — the flow edges, not structural `mod`/`pub use`
relationships. A Rust module scores high HK when it is both widely imported
and imports widely:

- A `lib.rs` or `mod.rs` facade that re-exports and also orchestrates.
- A `types.rs` / `model.rs` that every layer imports *and* that itself pulls
  in serialization, validation, and persistence concerns.
- A `utils.rs` junk drawer that accumulates helpers used everywhere.
<!-- doc:base "Reducing it" -->

## When a hub is legitimate (accept, don't game)

Not every high-HK file should be split. A few are *irreducible by design* —
their coupling **is** the architecture, not an accident:

- **A core contract / trait** that every implementor depends on. Its `fan_in`
  grows with each implementation by definition, and it references the types its
  own signatures use (`fan_out`). The number is the cost of having one contract
  instead of many ad-hoc ones.
- **A top-level orchestrator** that wires every subsystem together. High
  `fan_out` is its whole job; pushing those dependencies elsewhere only moves
  the crossroads, it does not remove it.

Before accepting one, *prove* it is irreducible — apply the Step-4 test: would a
split **dissolve** coupling or merely **relocate** it? If every candidate
extraction either leaves `fan_in × fan_out` unchanged (you only shaved `sloc`)
or *raises* `fan_out` (you moved out a type the file's own signatures mention),
the hub is load-bearing and splitting it is metric-gaming, not decoupling.

When that holds, **accept it explicitly**: raise the `hk` threshold to sit just
above the hub, and record *why* right next to the value in config — name the
file, its role, and the factor that makes it irreducible. That turns a silent
suppression into a reviewed, documented decision, and keeps the gate meaningful
for the *next* file that crosses the line.

This is the exception, not the default. Raise the ceiling only for a NEW,
genuine hub you have proven irreducible; for everything else, prefer the split.
<!-- doc:base "How code-ranker surfaces it" -->

## A workflow: dissecting and splitting a high-HK file

A repeatable loop for taking one hotspot file from "everything routes through
it" to a clean split — measure, understand *why* it is a crossroads, split
along the right seam, then prove the coupling actually fell.

### Step 1 — Measure HK for one file

Run the gate to find which file is over budget, then analyze once to a JSON
snapshot and read the file's `hk` and the three factors that produce it
(`HK = sloc × (fan_in × fan_out)²`):

```bash
# Gate on HK: flag every file whose hk exceeds a budget N, worst-first (add
# --top 1 for just the single worst). Each finding prints a self-contained
# where/issue/why/fix block — paste one straight into an AI assistant to act on:
code-ranker check <path/to/project> --threshold file.hk=100000 --top 1

# Analyze once to a JSON snapshot, reused by every jq query below:
code-ranker report <path/to/project> --output.json.path=.code-ranker/hk.json

# The exact numbers for the file you want to dissect (matched by id suffix —
# use enough of the path to be unique):
F=src/foo.rs
jq --arg f "$F" '
  .graphs.files.nodes[] | select(.id | endswith($f))
  | {id, sloc, fan_in, fan_out, hk}
' .code-ranker/hk.json
```

Knowing the breakdown tells you which lever to pull: the coupling term is
squared, so a unit dropped from `fan_in` or `fan_out` moves HK far more than
trimming `sloc`. Chase the product, not the line count.

### Step 2 — List every fan_in and fan_out edge

Fan-in/out are the real code-dependency (`uses`) edges in the graph. List both
sides for the one file — who depends on it, and what it depends on:

```bash
jq -r --arg f "$F" '
  (.graphs.files.nodes[] | select(.id | endswith($f)) | .id) as $id
  | .graphs.files.edges[] | select(.kind == "uses")
  | if   .target == $id then "fan_in   <- \(.source)"
    elif .source == $id then "fan_out  -> \(.target)"
    else empty end
' .code-ranker/hk.json
```

The edge list says *which* modules couple; it does not say *why*. For that,
open each fan-in dependant and look at exactly which symbols it imports from
the hotspot — that single fact is what exposes the mixed scenarios in Step 3:

```bash
# For one dependant, see precisely what it pulls from the hotspot's module:
rg -n 'use .*(::foo::|crate_name::)' path/to/dependant.rs
```

### Step 3 — Analyze for mixed scenarios (audiences)

A file earns high HK when it serves several *unrelated audiences* from one
place: every consumer of any one concern draws an edge to the whole file. Read
the Step-2 results and group the dependants by what they actually use:

- Tabulate `dependant → symbols imported`.
- Cluster the symbols into **concerns / audiences** — e.g. "the core
  contract", "a data type only the reporting layer reads", "a helper used by
  two leaves", "registration/wiring".
- Flag the **wrong-audience** edges: dependants that reach the file for *only
  one* concern and never touch the rest. Those are the edges a split removes.

If most of the fan_in is one audience reaching for a slice that has nothing to
do with the file's main job, that slice is the thing to move out.

### Step 4 — Split along the seam

Pick the canonical Rust technique that matches the seam you found:

- **Extract a focused submodule + re-export.** Move the cohesive group into a
  new `mod`, keep the public path stable with `pub use`
  (`mod preset; pub use preset::Preset;`). Callers don't churn; the hub's
  `sloc` drops and the moved item's narrow dependants detach from the hub.
- **Move a pure data type (DTO) to its own module.** A `serde`/plain struct
  with no internal dependencies has `fan_out = 0`, so its own HK is `0`. When
  many consumers reach the hub only for a DTO, this is the biggest, safest win.
- **Segregate the trait (ISP).** If disjoint caller groups use disjoint
  methods, split one fat trait into focused traits; each consumer then depends
  only on the facet it uses, lowering the fan_in of any one unit. See
  [ISP](ISP.md).
- **Invert a dependency (DIP).** Define an abstraction the hub depends on and
  implement it in the leaf, cutting the hub's `fan_out`. See
  [DIP](DIP.md).
- **Separate facade from orchestration.** For a `mod.rs`/`lib.rs` that both
  re-exports and contains logic, move the logic into a sibling and leave a thin
  re-export facade. Re-exports (`pub use`) are structural, not flow edges, so
  they do not count toward fan_in/fan_out.

Two rules that decide whether a split *dissolves* coupling or merely *moves*
it:

- **Dissolve, don't relocate.** Extracting an item with ~zero `fan_out`
  removes its HK contribution outright. Extracting an item the hub *still
  references* adds an outgoing edge back to the hub — you traded `sloc` for
  `fan_out`, and the square punishes that. Prefer moving leaf data and helpers,
  not types the hub's own signatures mention.
- **Keep the contract's argument/return types beside it.** Moving a type that
  the trait takes or returns out of the file does not cut fan_in (the
  implementors still depend on the trait) but *raises* fan_out. Split by
  audience, not by line count.

Re-export the moved items at the crate root so call sites stay short.

### Step 5 — Verify with a before/after diff report

Prove the coupling fell and that behaviour is unchanged. Snapshot **before**
the split, apply it, run the test suite, then diff **after** against the
baseline and render the HTML report:

```bash
# BEFORE — baseline snapshot (keep .code-ranker/ snapshots; they are baselines):
code-ranker report <path/to/project> --output.json.path=.code-ranker/before.json

#   …apply the split, then run the full test suite (cargo test / nextest)…

# AFTER — diff against the baseline + render the HTML diff report:
code-ranker report <path/to/project> \
  --baseline .code-ranker/before.json \
  --output.json.path=.code-ranker/after.json \
  --output.html.path=.code-ranker/after.html

# Confirm the hotspot's HK actually dropped (and no sibling rose past it):
jq --arg f "$F" '
  .graphs.files.nodes[] | select(.id | endswith($f)) | {sloc, fan_in, fan_out, hk}
' .code-ranker/after.json

# Or let the gate confirm it: the file no longer breaches the same budget (exit 0):
code-ranker check <path/to/project> --threshold file.hk=100000
```

Then **surface the report to the user**: print its absolute path and offer to
open it, so the before/after crossroads is one click away.

```bash
echo "HK diff report: $(cd .code-ranker && pwd)/after.html"
# Suggest opening it:
#   open      .code-ranker/after.html   # macOS
#   xdg-open  .code-ranker/after.html   # Linux
```

Read the result, don't assume it: if the file's HK did not drop, or a sibling's
HK rose past it, the split **relocated** coupling instead of dissolving it —
reconsider what you moved (Step 4's two rules). Re-measure after every change
and let the numbers overrule intuition.
<!-- doc:base "Related principles" -->
<!-- doc:base "References" -->

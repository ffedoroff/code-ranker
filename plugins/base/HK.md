# HK — Henry–Kafura

**TL;DR**: Henry-Kafura "information flow" complexity scores a module by how
much it sits in the middle of the dependency graph and how big it is:
`HK = sloc × (fan_in × fan_out)²`. A high HK module is large *and* a busy
crossroads — the most expensive place in the codebase to change.

## What it measures

Henry and Kafura's 1981 metric combines three signals into one number:

- **fan_in** — how many modules depend on this one (incoming edges).
- **fan_out** — how many modules this one depends on (outgoing edges).
- **sloc** — the module's size in source lines of code.

Code Ranker computes `HK = sloc × (fan_in × fan_out)²`. The `(fan_in × fan_out)`
product is squared, so coupling dominates: a small file wired into many
collaborators on both sides scores far higher than a large but isolated file.
The intuition is that information flowing *through* a module — in from its
dependants, out to its dependencies — is where integration cost concentrates.

## Why it matters

A high-HK module is the worst kind of change target:

- It is **load-bearing** (high fan_in): breaking it breaks many dependants.
- It is **fragile** (high fan_out): it breaks when any of its many
  dependencies change.
- It is **large** (high sloc): the surface area for both is wide.

The square on the coupling term is deliberate — it pushes the "god module"
that everything routes through to the top of the list, ahead of merely large
files. Those are the modules where splitting pays off the most.

## What high HK looks like

Fan-in and fan-out are counted over real code dependencies (import paths,
qualified paths) — the flow edges, not structural module-ownership or
re-export relationships. A module scores high HK when it is both widely imported
and imports widely:

- A package facade that re-exports and also orchestrates.
- A types/model module that every layer imports *and* that itself pulls
  in serialization, validation, and persistence concerns.
- A "utils" junk drawer that accumulates helpers used everywhere.

## Reducing it

You lower HK by attacking whichever factor dominates:

- **Shrink it** (sloc): extract cohesive groups of items into focused
  sibling modules. The split halves the size and usually the coupling too.
- **Cut fan_out**: depend on fewer, more abstract collaborators — invert a
  dependency (see [DIP](DIP.md)), or move a
  responsibility that drags in unrelated imports elsewhere.
- **Cut fan_in**: narrow the public surface so fewer modules need this one;
  if different callers use disjoint parts, split it
  (see [ISP](ISP.md)).

Because the coupling term is squared, even a modest reduction in fan_in or
fan_out moves HK a lot — prefer those over chasing line count.

## When a hub is legitimate (accept, don't game)

Not every high-HK file should be split. A few are *irreducible by design* —
their coupling **is** the architecture, not an accident:

- **A core contract / interface** that every implementor depends on. Its
  `fan_in` grows with each implementation by definition, and it references the
  types its own signatures use (`fan_out`). The number is the cost of having one
  contract instead of many ad-hoc ones.
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

## How code-ranker surfaces it

HK is a first-class node metric (`hk`), the default sort, and the `HK` principle
in the Prompt Generator. The principle ranks modules worst-first by HK and
pre-selects both incoming and outgoing connections, so the generated prompt
shows the full crossroads around each hotspot.

## A workflow: dissecting and splitting a high-HK file

A repeatable loop for taking one hotspot file from "everything routes through
it" to a clean split — measure, understand *why* it is a crossroads, split
along the right seam, then prove the coupling actually fell.

### Step 1 — Find the worst HK files

Rank the project by HK — no JSON to parse, the CLI does it for you. Either gate on
a budget (worst-first; `--top 1` for just the single worst), each finding a
self-contained where/issue/why/fix block you can paste into an AI assistant:

```bash
code-ranker report --output.scorecard --focus hk
```

HK is `sloc × (fan_in × fan_out)²`: the coupling term is squared, so a unit dropped
from `fan_in` or `fan_out` moves HK far more than trimming `sloc` — chase the
product, not the line count. (The exact `sloc` / `fan_in` / `fan_out` breakdown for
any node is in the HTML viewer's node popup, `report --output.html`.)

### Step 2 — See the crossroads (fan_in / fan_out)

The HK prompt lists each hotspot worst-first **with its connections** — who depends
on it (`Connections — in`) and what it depends on (`Connections — out`), the real
code-dependency (`uses`) edges that drive fan_in/fan_out. No `jq`, no snapshot to
query:

```bash
code-ranker report --prompt HK --top 1
```

The connection list says *which* modules couple; it does not say *why*. For that,
open each fan-in dependant and look at exactly which symbols it imports from the
hotspot — that single fact is what exposes the mixed scenarios in Step 3.

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

Pick the canonical technique that matches the seam you found:

- **Extract a focused submodule + re-export.** Move the cohesive group into a
  new module, keep the public path stable with a re-export
  (`import module principle; re-export principle.Principle`). Callers don't churn; the
  hub's `sloc` drops and the moved item's narrow dependants detach from the hub.
- **Move a pure data type (DTO) to its own module.** A plain data structure
  with no internal dependencies has `fan_out = 0`, so its own HK is `0`. When
  many consumers reach the hub only for a DTO, this is the biggest, safest win.
- **Segregate the interface (ISP).** If disjoint caller groups use disjoint
  methods, split one fat interface into focused interfaces; each consumer then
  depends only on the facet it uses, lowering the fan_in of any one unit. See
  [ISP](ISP.md).
- **Invert a dependency (DIP).** Define an abstraction the hub depends on and
  implement it in the leaf, cutting the hub's `fan_out`. See
  [DIP](DIP.md).
- **Separate facade from orchestration.** For a package/module entry point that
  both re-exports and contains logic, move the logic into a sibling and leave a
  thin re-export facade. Re-exports are structural, not flow edges, so
  they do not count toward fan_in/fan_out.

Two rules that decide whether a split *dissolves* coupling or merely *moves*
it:

- **Dissolve, don't relocate.** Extracting an item with ~zero `fan_out`
  removes its HK contribution outright. Extracting an item the hub *still
  references* adds an outgoing edge back to the hub — you traded `sloc` for
  `fan_out`, and the square punishes that. Prefer moving leaf data and helpers,
  not types the hub's own signatures mention.
- **Keep the contract's argument/return types beside it.** Moving a type that
  the interface takes or returns out of the file does not cut fan_in (the
  implementors still depend on the interface) but *raises* fan_out. Split by
  audience, not by line count.

Re-export the moved items at the package root so call sites stay short.

### Step 5 — Verify with a before/after diff report

Prove the coupling fell and that behaviour is unchanged. Snapshot **before**
the split, apply it, run the test suite, then diff **after** against the
baseline and render the HTML report:

```bash
# BEFORE — baseline snapshot (keep .code-ranker/ snapshots; they are baselines):
code-ranker report --output.json.path=.code-ranker/before.json

#   …apply the split, then run the full test suite…

# AFTER — diff against the baseline + render the HTML diff report:
code-ranker report \
  --baseline .code-ranker/before.json \
  --output.json.path=.code-ranker/after.json \
  --output.html.path=.code-ranker/after.html

# Confirm the hotspot's HK actually dropped (and no sibling rose past it) —
# re-rank with the scorecard, or let the gate decide (exit 0 = no breach):
code-ranker report --output.scorecard --focus hk
code-ranker check --threshold file.hk=100000
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

## Related principles

- [DIP](DIP.md) — inverting dependencies cuts fan_out.
- [ISP](ISP.md) — segregating interfaces cuts fan_in.
- [SRP](SRP.md) — single-responsibility modules stay
  small and loosely coupled, keeping HK low.

## References

1. Henry, S. and Kafura, D. "Software Structure Metrics Based on Information
   Flow". *IEEE Transactions on Software Engineering*, SE-7(5), 1981.

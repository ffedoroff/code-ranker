# HK — Henry-Kafura Coupling (in Rust)

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

## In Rust

Fan-in and fan-out are counted over real code dependencies (`use` paths,
qualified paths, derives) — the flow edges, not structural `mod`/`pub use`
relationships. A Rust module scores high HK when it is both widely imported
and imports widely:

- A `lib.rs` or `mod.rs` facade that re-exports and also orchestrates.
- A `types.rs` / `model.rs` that every layer imports *and* that itself pulls
  in serialization, validation, and persistence concerns.
- A `utils.rs` junk drawer that accumulates helpers used everywhere.

## Reducing it

You lower HK by attacking whichever factor dominates:

- **Shrink it** (sloc): extract cohesive groups of items into focused
  sibling modules. The split halves the size and usually the coupling too.
- **Cut fan_out**: depend on fewer, more abstract collaborators — invert a
  dependency (see [DIP](solid-dependency-inversion.md)), or move a
  responsibility that drags in unrelated imports elsewhere.
- **Cut fan_in**: narrow the public surface so fewer modules need this one;
  if different callers use disjoint parts, split it
  (see [ISP](solid-interface-segregation.md)).

Because the coupling term is squared, even a modest reduction in fan_in or
fan_out moves HK a lot — prefer those over chasing line count.

## How code-ranker surfaces it

HK is a first-class node metric (`hk`), the default sort, and the `HK` preset
in the Prompt Generator. The preset ranks modules worst-first by HK and
pre-selects both incoming and outgoing connections, so the generated prompt
shows the full crossroads around each hotspot.

## Related principles

- [DIP](solid-dependency-inversion.md) — inverting dependencies cuts fan_out.
- [ISP](solid-interface-segregation.md) — segregating interfaces cuts fan_in.
- [SRP](solid-single-responsibility.md) — single-responsibility modules stay
  small and loosely coupled, keeping HK low.

## References

1. Henry, S. and Kafura, D. "Software Structure Metrics Based on Information
   Flow". *IEEE Transactions on Software Engineering*, SE-7(5), 1981.

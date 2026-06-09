# SLOC — Module Size (in Rust)

**TL;DR**: Source lines of code is a blunt but honest signal. Size is not a
defect by itself, but the largest files are almost always the ones that have
quietly accumulated several responsibilities — and they are the hardest to
read, test, and review. Treat SLOC as a "look here first" lens, not a verdict.

## What it measures

`sloc` counts the module's physical source lines that carry real code —
blank lines and comment-only lines are excluded. It is the rawest measure of
"how much is in this file". Code Ranker ranks modules largest-first.

## Why it matters

Large files impose concrete costs:

- **Reading**: you must hold more context at once to understand any one part.
- **Reviewing**: a large diff in a large file is hard to reason about; bugs
  hide in the noise.
- **Testing**: a module doing several things needs combinatorially more tests,
  and its tests are harder to isolate.
- **Merging**: more lines means more conflict surface for concurrent work.

Crucially, oversized modules are a *symptom*. The real problem is usually a
violation of the [Single Responsibility Principle](solid-single-responsibility.md):
the file grew because unrelated decisions kept landing in the same place.

## In Rust

Rust makes splitting cheap and safe — the compiler verifies every move:

- A module is just a file or a `mod` block; extracting items into a sibling
  module is a mechanical, checked refactor.
- `pub use` lets you split the implementation across files while keeping the
  public path stable, so callers do not break.
- Visibility (`pub(crate)`, `pub(super)`) lets the split expose only what the
  rest of the crate actually needs.

Common offenders: a `lib.rs` that grew an entire subsystem inline, a
`handlers.rs` holding every HTTP route, a `models.rs` with every domain type
plus their serialization and validation.

## Reducing it

For each oversized module:

1. List the distinct responsibilities it currently holds.
2. Group items (functions, types, impls) by responsibility.
3. Move each group into a focused sibling module with a single clear purpose.
4. Re-export from the original path with `pub use` if callers depend on it,
   so external behaviour does not change.

Stop when each module has one reason to change — not when you hit an arbitrary
line count. The goal is cohesion, not a smaller number.

## How code-ranker surfaces it

`sloc` is a first-class node metric, a sort option, and the `SLOC` preset in
the Prompt Generator (largest-first, no connections pre-selected — the focus
is the file's own contents). It complements the principle presets: SLOC finds
the biggest files; SRP/DRY explain *why* a given one is too big.

## Related principles

- [SRP](solid-single-responsibility.md) — the usual reason a file is large.
- [KISS](kiss.md) — large files are often accidentally complex.
- [DRY](dry.md) — size sometimes hides duplicated knowledge worth extracting.

# Adding a custom per-file metric (worked example: `unsafe`)

**Goal of this doc:** learn the end-to-end path for emitting a *custom,
plugin-computed, per-file* attribute so it lands in the JSON snapshot and shows
up in the HTML viewer. We use a count of Rust `unsafe` usages as the worked
example.

**Explicitly out of scope here** (deliberately deferred — do *not* build them in
this slice):

- per-100KLOC / any project-level normalization or rate
- project-level aggregation in `stats` (mean/sum)
- the same metric on other languages (Python / JS / TS)
- a composite 0–100 score

We only want one thing working: **`some_file.rs` → `"unsafe": 3` in its node**.

## The key insight: this already exists

The plugin already ships two custom per-file integer attributes computed by the
Rust analyzer and carried through to the snapshot: **`loc`** ("Lines") and
**`items`** ("Items"). They are *not* produced by the central
`code-ranker-complexity` pass — the Rust plugin computes them during its `syn`
walk and writes them onto the node.

So a new `unsafe` metric is **not new machinery** — it rides the exact same four
touchpoints that `loc` / `items` already use. Find every place `items` is
mentioned in `code-ranker-plugin-rust` and you have the full map.

The split to keep in mind:

- **The attribute *value*** flows to JSON purely because it sits in
  `node.attrs` (touchpoints 1–3). Skipping touchpoint 4 still gets you the value
  in the JSON; it just won't be a labelled/sortable metric in the viewer.
- **The attribute *spec*** (touchpoint 4) is what makes the viewer render it as a
  named metric (label, tooltip, column, delta colour). The viewer hardcodes no
  metric by name — it renders entirely from the per-level `node_attributes`
  dictionary.

## The four touchpoints

All paths are in `crates/code-ranker-plugin-rust/src/`.

### 1. Carry the count on the internal node model

`internal.rs` — the crate-local typed `Node` (`struct Node`, ~line 49). Add a
field next to the existing `loc` / `item_count`:

```rust
pub item_count: Option<u32>,
pub unsafe_count: Option<u32>,   // NEW: count of `unsafe` usages (production only)
```

Update every place that constructs `internal::Node` (it's a plain struct, so the
compiler will list the missing-field sites for you — default the new field to
`None`).

### 2. Count during the `syn` walk

`module_graph.rs`, function `walk_file` (~line 299). It already:

- parses the file into a `syn::File` (`parsed`),
- runs a `syn::visit::Visit` collector over **non-test** top-level items
  (the `for item in &parsed.items { if ignore_tests && is_test_item(item) { continue; } … }`
  loop, ~lines 336–341),
- writes per-file facts onto the owning module node (`node.loc = Some(loc);`
  `node.item_count = Some(item_count);`, ~lines 327–329).

Add a small visitor (near `CratePathCollector`, ~line 232):

```rust
/// Counts `unsafe` usages in a parsed file: `unsafe { }` blocks plus
/// `unsafe fn` / `unsafe impl` / `unsafe trait` declarations.
#[derive(Default)]
struct UnsafeCounter {
    count: u32,
}

impl<'ast> syn::visit::Visit<'ast> for UnsafeCounter {
    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.count += 1;
        syn::visit::visit_expr_unsafe(self, node);
    }
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if node.sig.unsafety.is_some() {
            self.count += 1;
        }
        syn::visit::visit_item_fn(self, node);
    }
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if node.unsafety.is_some() {
            self.count += 1;
        }
        syn::visit::visit_item_impl(self, node);
    }
    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if node.unsafety.is_some() {
            self.count += 1;
        }
        syn::visit::visit_item_trait(self, node);
    }
}
```

Run it over the **same test-filtered items** as the existing collector loop, so
`unsafe` inside `#[cfg(test)]` / `#[test]` code never counts (consistent with how
`sloc`/complexity already exclude tests):

```rust
let mut unsafe_counter = UnsafeCounter::default();
for item in &parsed.items {
    if ignore_tests && is_test_item(item) {
        continue;
    }
    syn::visit::Visit::visit_item(&mut unsafe_counter, item);
}
```

Then write it onto the owning module node, right where `loc` / `item_count` are
set (~lines 322–330):

```rust
node.loc = Some(loc);
node.item_count = Some(item_count);
node.unsafe_count = Some(unsafe_counter.count);   // NEW
```

(Optional optimisation: fold the count into the existing collector loop instead
of a second pass — `syn::visit::Visit` lets one visitor do both. Two passes is
fine and clearer for a first cut.)

### 3. Carry it through the module→file collapse

`lib.rs`, the collapse pass that converts internal nodes to generic
`api::Node`s. `loc` / `items` are written in **two** branches — the *insert*
branch that first creates the file node (~lines 416–432) and the *update* branch
that touches an already-created file node (~lines 442–461). Mirror `items` in
**both**:

```rust
// insert branch (~426)
if let Some(items) = node.item_count {
    attrs.insert("items".to_string(), AttrValue::Int(items as i64));
}
if let Some(u) = node.unsafe_count {              // NEW
    if u > 0 {                                    // omit zero — see note below
        attrs.insert("unsafe".to_string(), AttrValue::Int(u as i64));
    }
}
```

```rust
// update branch (~455)
if let Some(items) = node.item_count {
    n.attrs.insert("items".to_string(), AttrValue::Int(items as i64));
}
if let Some(u) = node.unsafe_count {              // NEW
    if u > 0 {
        n.attrs.insert("unsafe".to_string(), AttrValue::Int(u as i64));
    }
}
```

**Zero-omission:** the schema convention is that a metric is omitted when it is
zero (cf. `hk` / `cycle`). Gating on `u > 0` means files with no `unsafe` simply
carry no `unsafe` key, instead of a noisy `"unsafe": 0` on every node. `loc` is
always present because it's never zero; `unsafe` should follow the zero-omission
rule.

### 4. Declare the attribute spec (so the viewer renders it)

`lib.rs`, `fn levels()` (~line 111), in the `node_attributes` block (~lines
188–194) next to `loc` / `items`:

```rust
node_attributes.insert("items".into(), aspec(ValueType::Int, "Items"));

// NEW
let mut unsafe_spec = aspec(ValueType::Int, "Unsafe");
unsafe_spec.short = Some("Unsafe".into());
unsafe_spec.description = Some(
    "Count of `unsafe` blocks, `unsafe fn`/`impl`/`trait` declarations in \
     production code (test items are excluded). Syntactic count — see limitations."
        .into(),
);
unsafe_spec.direction = Direction::LowerBetter;   // higher = worse → red delta
node_attributes.insert("unsafe".into(), unsafe_spec);
```

`AttributeSpec` has all-public fields and only a minimal `new(value_type, label)`
constructor (`code-ranker-plugin-api/src/level.rs:109`), so set the extra fields
directly after `aspec(...)`. Import `Direction` from
`code_ranker_plugin_api::level` (the `use` block at the top of `lib.rs` already
pulls `AttributeSpec, EdgeKindSpec, Grouping, Level, Thresholds` from there — add
`Direction`).

The orchestrator merges this spec into the level dictionary and then prunes it to
keys actually present on internal nodes, so no further wiring is needed.

**Group (optional, skipped in this slice):** to file `unsafe` under a "Safety"
group in the viewer, you'd also register an `AttributeGroup` (via the `group()`
helper) in the level's `attribute_groups` and set `unsafe_spec.group =
Some("safety".into())`. Not required for the value to appear — defer until there
is more than one safety metric.

## Verify

1. Build and run `report` against a Rust crate that actually contains `unsafe`
   (a crate with FFI or `unsafe` blocks — running on `code-ranker` itself may
   yield few/none):

   ```sh
   cargo run -p code-ranker-cli -- report /path/to/unsafe-heavy-crate
   ```

2. Grep the emitted snapshot for the new key:

   ```sh
   grep -o '"unsafe": [0-9]*' .code-ranker/*-*.json | sort | uniq -c
   ```

   Expect non-zero counts on files with `unsafe`, and no key on files without.

3. Open the generated `.html`: `unsafe` should appear as a sortable column /
   tooltip metric, labelled "Unsafe", with a red (worse) delta direction in a
   `--baseline` diff.

4. Add a focused unit test in `module_graph.rs` (mirror the existing
   `count_items` / collapse tests): feed a small source string with one
   `unsafe { }` block and one `unsafe fn`, assert `unsafe_count == 2`, and assert
   that an `unsafe` block inside a `#[cfg(test)] mod tests { … }` is **not**
   counted.

## Known limitations (document them, don't fix them here)

- **Purely syntactic.** This counts the syntactic appearance of `unsafe` — no
  type or semantic analysis (consistent with the rest of the Rust plugin; this is
  why rust-analyzer is intentionally absent).
- **Macros are not expanded.** An `unsafe` block produced *inside* a macro body
  is invisible — `syn` does not parse the tokens of a macro invocation. Same
  blind spot already documented for bare-path resolution.
- **Test exclusion is top-level.** `is_test_item` filters `#[cfg(test)]` /
  `#[test]` / `#[bench]` at the item level being walked, matching the existing
  collector. A `#[cfg(test)]` attribute on a deeply nested item is out of this
  slice's scope.

## Snapshot / golden tests will change

Adding `unsafe` to node output changes the JSON the e2e/sample tests assert
against. Per repo convention, **patch the new `unsafe` field into the sample
goldens surgically** rather than full-regenerating them (the sample goldens carry
a hand-frozen header). Never delete prior `.code-ranker/` run snapshots when
regenerating.

## Checklist

- [ ] `internal.rs`: `Node.unsafe_count: Option<u32>` + default it at every
      construction site
- [ ] `module_graph.rs`: `UnsafeCounter` visitor + run it over test-filtered
      items in `walk_file` + `node.unsafe_count = Some(…)`
- [ ] `lib.rs` collapse: write `"unsafe"` in both the insert and update branches,
      gated on `> 0`
- [ ] `lib.rs` `levels()`: `node_attributes` spec for `"unsafe"` (label, short,
      description, `Direction::LowerBetter`)
- [ ] unit test for counting + test-exclusion
- [ ] update sample goldens surgically
- [ ] `cargo run … report` on an unsafe-bearing crate → grep JSON → eyeball HTML

## Generalizing later

Once this single per-file integer works end-to-end, the same four touchpoints
take any further Rust marker (`unwrap`/`expect`, `panic!`/`todo!`, …) — each is
just another counter in the visitor and another `node_attributes` spec. Project
normalization (per-100KLOC), `stats` rollup, the cross-language "safety" markers,
and the composite score are separate, later layers and are intentionally not part
of this slice.

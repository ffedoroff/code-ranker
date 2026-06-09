# Refactor plan — splitting the oversized viewer assets

A plan for breaking the largest viewer assets and `DESIGN.md` into smaller,
concern-aligned files.

**Progress (2026-06-08):**
- ✅ **Phase 1 (JS split)** — done. `diagram.js` → `source-links.js` /
  `node-popup.js` / `modal-content.js` / `map-interactions.js` / `map-render.js`;
  `tooltip.js` extracted from `node-table.js`; `app.js` → `view-state.js` /
  `snap-controls.js` / thin `app.js`; `grouping.js` added (the grouping ladder).
- ✅ **Phase 2 (CSS split)** — done. `index.css` → `base.css` / `map.css` /
  `modal.css` / `tables.css` / `export.css` / `snap.css` / `map-svg.css`,
  concatenated in `lib.rs` in source order (verified byte-identical).
- ✅ **Relative zoom + cycle visibility** — done (see forward-plan section below).
- ⏳ **In/out cluster diagonal placement** — pending (needs visual iteration).
- ⏳ **Phase 3 (DESIGN/PRD docs)** — DESIGN.md being updated to the new layout.

## Constraints (read first)

The viewer assets are **plain global-scope scripts**, not ES modules. They are
loaded sequentially (see `index.html` `<script>` order) and embedded into the
`code-ranker` binary by `crates/code-ranker-viewer/src/lib.rs`:

1. each asset is an `include_str!` const (`lib.rs:25–41`),
2. `render_html_viewer` inlines each by replacing its `<script src="…">` /
   `<link>` tag with the file contents (`lib.rs:69–135`).

Therefore **every new file costs three mechanical edits**: a const +
`include_str!` in `lib.rs`, a `.replace(...)` in `render_html_viewer`, and a
`<script>`/`<link>` line in `index.html` (in the right order).

Two correctness rules for the split:

- **Top-level execution order matters; function-body references do not.**
  Cross-file calls go through `window.*` and run at `DOMContentLoaded` (or later
  on hover/click), by which point every script is loaded. So a function may call
  a global defined in a file that loads *after* it. The only thing that must not
  break is any code that *runs at load time* (top-level consts/IIFEs).
- **Preserve every `window.*` export.** The current files already mark their
  public surface with `window.foo = foo`. Keep those exact names so nothing
  downstream breaks.

Current load order (after `data.js`): `schema → diff → layout → utils → modal →
panzoom → diagram → ui → summary → export-popup → node-table → nav → app`.

---

## Target architecture (conceptual layers)

```
vendor   graphviz.umd.js · snarkdown.umd.js           (untouched)
data     schema.js · diff.js · utils.js               (pure, no DOM)
graph    layout.js (buildDOT) · map-render            (DOT → SVG)
map      map-interactions.js · panzoom.js             (behaviour on the map)
popup    modal.js · node-popup.js · modal-content.js · source-links.js
shared   tooltip.js                                   (the #tt engine)
tables   node-table.js · summary.js
export   export-popup.js
shell    nav.js · view-state.js · snap-controls.js · app.js
```

---

## Phase 1 — JS monoliths (highest value, lowest risk)

### 1a. `diagram.js` (1162 lines) → 5 files

Function-to-file map (line ranges are from the current file):

| New file | Functions (current lines) | ~Lines | Notes |
|---|---|---|---|
| `source-links.js` | `gitWebBase` (485), `gitSourceUrl` (502), `nodeSourceUrl` (516), `connSourceLine` (536), `absPath` (551) | ~75 | Pure URL builders. Keeps `window.nodeSourceUrl` / `window.connSourceLine`. No DOM. |
| `node-popup.js` | `buildDiagramSVG` (1–484), `markPopupSelected` (719–729) | ~500 | The neighbour SVG diagram. Keeps `window.markPopupSelected`. Candidate for a *further* internal split later (collectConns / allocCols / card rendering) — out of scope for this pass. |
| `modal-content.js` | `buildModalContent` (559–718) | ~160 | Left field-table HTML of the modal. |
| `map-interactions.js` | `toggleNodeSelected` (730), `isOpenSrcClick` (760), `kbdHintsHtml` (764), `drillIntoGroup` (794), `drillOutOfGroup` (809), `statusLineFor` (821), `statusLineForGroup` (842), `setupEdgeHighlight` (853), `setupTooltips` (984) | ~375 | All map behaviour. Keeps `window.isOpenSrcClick`, `window.kbdHintsHtml`. **`setupEdgeHighlight` must still be called before `setupTooltips`** — keep that ordering at the call site (it's a runtime ordering, not a load ordering). |
| `diagram.js` (remainder) | `drawSVG` (1105), `renderSVGNow` (1135) | ~58 | The render entry point. Too small to stand alone — **decision needed**: keep as `diagram.js`, rename to `map-render.js`, or fold into `map-interactions.js`. Recommendation: rename to `map-render.js` and drop the now-misleading `diagram.js` name. |

Dependencies to respect: `buildModalContent` / `node-popup.js` call
`source-links.js` and `tooltip.js` functions → place `source-links.js` and
`tooltip.js` **before** `node-popup.js`/`modal-content.js` in load order (safe
default; see order below).

### 1b. `node-table.js` (521 lines) → 2 files

| New file | Functions (current lines) | ~Lines |
|---|---|---|
| `tooltip.js` | `renderTooltip` (356), `renderDescTooltip` (370), `renderNodeTooltip` (386), `renderGroupTooltip` (404), `setupTooltip` (415) + `SHOW_DELAY` | ~165 |
| `node-table.js` (remainder) | `attachModalCheckbox` (1), `setupNodeTable` (37) | ~355 |

`tooltip.js` is the **shared `#tt` engine** used by the map (`map-interactions`),
the popup (`node-popup`), the summary, and the table. It is conceptually a
`shared` layer, not part of the table. Load it **early** (right after `utils.js`)
so every later consumer can reference it.

### 1c. `app.js` (639 lines) → 3 files

| New file | Functions (current lines) | ~Lines | Notes |
|---|---|---|---|
| `view-state.js` | `activeSnap` (6), `viewMode` (15), `viewModeSuffix` (20), `activeGraph` (24), `activeLocalGraph` (31), `unionGraph` (46), `applySideVisibility` (54), `applySideSizing` (72), `setViewSide` (105), `toggleViewSide` (135), `recomputeAll` (239), `renderView` (265), `applyViewState`* | ~280 | *`applyViewState` referenced in `nav.js`/`popstate`; locate it (it lives in app.js's DOMContentLoaded scope today — must become a top-level/`window` fn during extraction). |
| `snap-controls.js` | `flyoutHeader` (154), `updateWarnCount` (187), `updateHeader` (194), `buildSnapPopupHTML` (319), `setupSnapPopup` (392), `updateActiveSnapGroup` (469), `readEmbeddedSnapshot` (477), `extractSnapshotFromText` (486), `setupFileControls` (498), `setupModeToggle` (139) | ~340 | |
| `app.js` (remainder) | `DOMContentLoaded` bootstrap + `updateFilesTab` (236) | ~60 | Just wires everything; loads last. |

⚠️ **Watchpoint:** much of `app.js` currently lives *inside* the
`DOMContentLoaded` closure (local scope). Extracting those into separate files
requires promoting them to top-level functions (or `window.*`) and confirming
they don't close over `DOMContentLoaded`-local variables. This is the only place
in Phase 1 that is more than a copy-paste — budget review time here.

### Resulting load order after Phase 1

```
schema → diff → layout → utils → tooltip → source-links → modal → panzoom →
node-popup → modal-content → map-interactions → map-render → ui → summary →
export-popup → node-table → nav → view-state → snap-controls → app
```

(`tooltip` and `source-links` moved early; the 5 diagram-derived files replace
`diagram`; `view-state`/`snap-controls` inserted before `app`.)

### lib.rs / index.html edits for Phase 1

For each new file: add `const ASSET_X = include_str!("assets/x.js");`, a
`.replace(r#"<script src="./x.js"></script>"#, &format!("<script>{}</script>",
ASSET_X))`, and the matching `<script>` line in `index.html` at the position
above. Net: +9 new files, ≈27 mechanical edits across `lib.rs` + `index.html`.
Keep `ui.js` (still inlined-by-name; the empty-but-present contract is
documented).

---

## Phase 2 — `index.css` (722 lines) → 6 files

The file is already divided by `/* ── … ── */` section banners, so the cut is
mechanical. Because the stylesheet is inlined into a single `<style>` to keep the
**offline guarantee**, assemble it by **concatenating consts in `lib.rs`** (not
`@import`, which would add a `file://` request).

| New file | Source sections (by current banner) |
|---|---|
| `base.css` | title/branding, mode toggle, stats bar, Views, Control panel |
| `map.css` | SVG frame, drag-to-pan cursor, graphviz nodes/edges, visibility toggles, cycle override + highlights, selection, hover, SVG status bar, edge-highlight, modifier cursors, shortcut legend |
| `modal.css` | node modal, popup side/main cards, copied-message |
| `tooltip.css` | tooltip (`.tt`, `.tt-desc`, `.tt-code`, formula lines) |
| `tables.css` | node table, summary, collapsible diff summary, too-many-nodes guard, footer |
| `export.css` | export popup, md preview, presets, recommendation badges |
| `snap.css` | snap popup, review mode |

(`map.css` will be the largest; if it stays unwieldy, split selection/hover/cycle
into `map-state.css` later.) In `lib.rs`, replace the single
`<style>{ASSET_CSS}</style>` with `<style>{base}{map}{modal}…</style>`.

---

## Phase 3 — documentation

`DESIGN.md` (96 lines / 40 KB) is itself the monolith problem in prose: one
mega-table with rows of 5 000+ chars (`diagram.js`, `export-popup.js`,
`index.css`). Plan:

1. **Replace the flat file-table with layer-grouped sections** mirroring the new
   code layout: `data` / `graph` / `map` / `popup` / `shared` / `tables` /
   `export` / `shell`. One short paragraph per file.
2. **Re-home the giant rows** onto the files they split into (e.g. the
   `diagram.js` row's content distributes across `node-popup` / `modal-content` /
   `map-interactions` / `source-links` / `map-render`).
3. If a layer's prose stays large, extract sub-docs (`DESIGN-map.md`,
   `DESIGN-popup.md`) and link them — the doc already does this for
   `code-ranker-cli/DESIGN.md`.
4. Update the `<!-- toc -->` and the "migrated to schema 2" status note's file
   list (`diff.js, layout.js, app.js, …`) to the new names.

`PRD.md` (230 lines) is organised by **feature**, not file — leave it. Run the
`/ud` doc-sync check after Phases 1–2 land so DESIGN reflects reality.

---

## Forward plan — multi-level zoom navigation (NOT YET implemented)

This is the next feature after the split. It is documented here **now** so the
Phase-1 module boundaries anticipate it and don't have to be re-cut later. **Do
not implement during the split** — only carve the seams so it drops in cleanly.

### Two orthogonal axes (don't conflate them)

- **`level`** = the analysis *graph* from the snapshot (`snapshot.graphs[level]`,
  read via `schema.js` `levelSpec`; `nav.js` `currentLevel`/`switchToLevel`). This
  is a **data** axis — what the backend computed.
- **zoom / drill state** = how we *group and filter one graph* for display
  (today: `window.drillGroup` + `window.nodeSizeMode`). This is a **view** axis.

The zoom ladder below lives entirely on the **view** axis over the **files**
graph (`groups = crates`, `nodes = files` for Rust). Crates and folders are
*derived groupings* of file nodes, not separate backend graphs — so the whole
ladder is computable client-side by varying the **grouping function** and a
**path filter**. (Possible exception: level 0 may need each crate's workspace
path; check whether it's derivable from node ids before asking the backend.)

### Relative zoom — the interaction model (Rust: group=crate, subgroup=folder)

Navigation is a **global level-of-detail (LOD) zoom**, not a drill-into-one-group.
There is a single relative integer `zoom`, **centred on crates at `zoom = 0`**.
Pressing zoom in / out re-renders the **whole map** at one coarser/finer
granularity — every group expands or collapses together; the user is *not*
focused on one crate. The grouping tiers, coarse → fine:

```
workspace-subfolder ◄── crate ──► crate + folder ──► … ──► file
       zoom −1            zoom 0        zoom +1
```

- **`zoom = 0`** (default) — one node per **crate**, inter-crate edges. *(exists today)*
- **`zoom = −1`** — crates collapsed into their **parent workspace folder**; show
  those folders and the connections between them. *(not yet)*
- **`zoom = +1`** — show crates **and the folders inside them**: each crate becomes
  a **cluster** wrapping its folder nodes (nested), edges drawn between folders.
  *(not yet)*
- **`zoom = +2…`** — folders expand to sub-folders / files the same way (nesting
  one more tier each step); the deepest tier is the single **file** node, whose
  detail is the existing per-node **popup**. *(file popup exists; intermediate
  nesting not yet)*

Key property: a coarser tier **wraps** the finer tier as a nested cluster, so
zoom +1 shows *both* levels at once (crates as clusters, folders as nodes) — it
is not "replace crates with folders". This is the main difference from the old
drill model.

**Global zoom vs. focus (two separate axes).** The LOD `zoom` above changes the
*whole* map. **Clicking a group node drills into that one group** (focus): the
map then shows only that crate's insides — exactly today's `drillIntoGroup` /
`drillGroup` behaviour, generalised to push onto a `focusPath`. Back / zoom-out
at the boundary pops it.

- **Click a group element** (crate / folder) → drill into that one (push
  `focusPath`); restricts the *node set* to its insides.
- **Click a leaf element** (a file — the deepest tier) → open the per-node
  **detail popup** (today's behaviour; nothing to drill into).
- **Zoom in / out** → change the *granularity* of whatever is shown (global LOD).

The two compose: drill into crate X, then zoom in/out within X. Keep them as
distinct state (`window.zoom` integer + an optional `window.focusPath`), **not**
one conflated `drillGroup`.

| Today | Means | Future equivalent |
|---|---|---|
| `drillGroup === null` | crate overview | `zoom = 0`, no focus |
| `drillGroup === "X"` | files of crate X (dir sub-clusters + callers/deps) | focus = `[crate=X]`, zoom at folder/file tier |
| node popup | one file's detail | deepest tier / focus = `[crate=X, dir=Y, file=Z]` |

### Generalisation required (the data-model change)

1. **Add `window.zoom: int` (relative LOD, default 0)** as the primary nav state,
   plus an optional **`window.focusPath: Array<{dim, value}>`** for click-to-focus
   (this is what `drillGroup` becomes — a stack, empty by default). `recomputeAll`
   resets `zoom = 0`, `focusPath = []`.
2. **`grouping.js` owns a tier sequence** `[subfolder, crate, dir1, dir2, …, file]`
   and two pure helpers: `tiersForZoom(zoom)` → which tiers are visible and how
   they nest, and `nodeKeyAtTier(node, tier)` → that node's group key at a tier.
   `makeGroupOf` (today) is the `crate`-tier special case.
3. **`layout.js` `buildDOT` → emit nested clusters.** Its binary
   `if (drillGroup === null) {…} else {…}` collapses into one parametric path:
   *filter nodes by `focusPath`, then for each visible tier (per `tiersForZoom`)
   emit a graphviz **cluster** wrapping the next-finer tier, leaf tier = real
   nodes.* Inter-group edges are deduped **per visible tier boundary**. The
   callers/dependencies clusters generalise to "edges crossing the focus
   boundary".
4. **`nav.js` URL state** — add `zoom=` (signed int) and, when focused, a focus
   path (`focus=foo/src/net` or repeated `f=` params); `navViewState` /
   `navViewUrl` / `applyViewState` carry both. Breadcrumb shows the focus path
   (one chip per segment, Back pops one); zoom is its own control.
5. **Zoom controls** — add semantic zoom buttons (in = `zoom+1`, out = `zoom−1`),
   separate from the existing pixel `+`/`−`/`fit` panzoom; decide whether they
   share UI or sit beside the size-mode row.

### In/out cluster placement (layout, NOT YET implemented)

In the focus/drilled view the **callers** cluster (IN, green `#edf7ed`) and the
**dependencies** cluster (OUT, orange `#fdf3e3`) are today pinned to the extreme
left / right ranks (`{rank=min}` / `{rank=max}` under `rankdir=LR`), with their
member nodes stacked **vertically** in a single column each. With more members
this column grows tall and wastes width.

Goal: place IN and OUT **diagonally opposite** and lay each cluster out in the
shape that draws best, rather than as a rigid vertical column:

- Keep IN and OUT on **opposite sides** so the flow reads across the central
  group (IN feeds in, OUT flows out) — e.g. **IN top-left, OUT bottom-right**.
- Inside each cluster, let the nodes spread in the **more compact direction**
  (grid / wrap) instead of one tall column — choose whatever minimises the
  cluster's bounding box and edge crossings to the centre.
- This is a `layout.js` `buildDOT` concern: the current `{rank=min}` / `{rank=max}`
  rank-constraint subgraphs and the per-cluster node ordering are what changes;
  may need per-cluster `rank`/`rankdir` or anchor placement rather than a global
  min/max. Treat exact graphviz mechanism as open — the requirement is "opposite
  corners, compact, optimal for rendering", not a specific attribute.

Note this interacts with the nested-cluster work above (zoom +1): the same
"place a cluster optimally, not as a vertical column" rule should apply to the
crate→folder nested clusters too.

### How this reshapes the Phase-1 split

Carve a dedicated **`grouping.js`** (data layer, no DOM) during Phase 1a — even
though today it holds just `makeGroupOf` + the dir-grouper. Move `makeGroupOf`
there out of `layout.js`. This is the seam where the ladder will live, so
`layout.js` (DOT emission) and `map-interactions.js` (drill nav) both *consume*
grouping instead of *owning* it. Keep `drillGroup` as-is for now, but write
`drillIntoGroup`/`drillOutOfGroup` and the URL helpers so that swapping the
single value for a stack touches only `grouping.js` + `nav.js`, not the renderer.

Updated Phase-1 file list adds one file:

| New file | Holds now | Will hold |
|---|---|---|
| `grouping.js` | `makeGroupOf`, dir-grouper (moved from `layout.js`) | the grouping ladder + `drillPath` model |

Load it right after `schema.js` (it's pure data, consumed by `layout.js`).

### Cycles surfaced at every zoom level (NOT YET implemented)

"Cycles" = circular dependencies — the elements drawn with a **red stroke**
today. Source of truth is the backend `graph.cycles` array; `diff.js`
`computeCycles` derives per-side status and tags nodes/edges with
`cycle-status-{baseline-only|current-only|both|none}`, which `map-svg.css` colours
red (side-gated). **(Implemented — see the progress banner at the top.)** Cycles
now show at every level: real status on file nodes/edges and aggregated onto
group nodes.

We want cycle membership **visible at every zoom level**, including the new group
levels (crates by subfolder, crates, folders). When file nodes are collapsed
into a group node (a crate or folder), that group must surface that it *contains*
cycle members — otherwise zooming out hides every cycle in the project.

Generalisation required:

1. **Aggregate cycle status up the grouping ladder.** A group node is
   "in-cycle" if any member node is (and carries the strongest per-side status
   among its members). Compute alongside the grouping in `grouping.js` /
   `diff.js`, not in the renderer. Render the group node with the same red
   stroke + a **count badge** ("3 in cycle"), and report it in the status bar /
   tooltip (`statusLineForGroup`).
2. **Distinguish intra- vs inter-group cycles.** At an overview level, a cycle
   may sit entirely *inside* one group (mark that group red) or *span* groups
   (the inter-group edges that close the loop should be red, like the file-level
   red cycle edges). The group-overview edge dedup in `buildDOT` must preserve a
   "this aggregated edge participates in a cross-group cycle" flag.
3. **A cycle-focus mode at any level.** Reuse the **dormant `show-cycle-*`
   chips** (DESIGN notes they "remain in the file but are unused") as a real
   toggle that filters/dims the map to only cycle participants — working at every
   zoom level, not just files. Wire it through the same view-state + URL plumbing
   as `nodeSizeMode` / `drillPath`.

Split impact: cycle aggregation is **data** — it belongs next to `grouping.js`
and `diff.js`, consumed by `layout.js` (group node/edge styling) and
`map-interactions.js` (status bar / badge). Keep the file-level red-stroke
behaviour exactly as-is during the split; only ensure the seam exists so the
aggregation drops into `grouping.js`/`diff.js` later.

### PRD / DESIGN follow-up

When implementing: add a PRD feature section "Semantic zoom across grouping
levels" (the ladder table above, per-language since other languages will have
different dimension sequences — e.g. packages▸modules▸classes), with a sub-point
on **cycle visibility aggregating up the levels**, and a DESIGN section
describing the `drillPath` model, `grouping.js`, and the cycle-aggregation rule.
For now this plan document is the single source.

---

## Recommended sequencing & verification

1. **Phase 1a** (`diagram.js`) — biggest single win. After: `cargo build`, render
   a report (`code-ranker report <dir> --output.html.path=out.html`), open it,
   smoke-test map hover/drill/selection/source-links/popup.
2. **Phase 1b** (`tooltip.js`) and **1c** (`app.js`) — verify tooltips everywhere
   and header/snap-popup/file-upload after each.
3. **Phase 2** (CSS) — purely visual; diff the rendered HTML's `<style>` block
   against the pre-split build to confirm byte-equivalence (order preserved).
4. **Phase 3** (docs) — last, once the code names are final.

Each phase is independently shippable and reviewable. The split is behaviour-
preserving: the inlined output should be functionally identical, only
re-partitioned.

**Open decision:** the `diagram.js` remainder (`drawSVG`/`renderSVGNow`) — keep
the `diagram.js` name, rename to `map-render.js`, or fold into
`map-interactions.js`? Recommendation: `map-render.js`.

---

**Related docs**: [DESIGN.md](DESIGN.md) · [PRD.md](PRD.md)

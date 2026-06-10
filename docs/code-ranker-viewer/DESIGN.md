# Technical Design — Code Ranker Viewer (`code-ranker-viewer`)

The technical design of the offline HTML viewer: the `code-ranker-viewer` crate
and its static assets (embedded into the `code-ranker` binary), the data-driven
rendering layer, the browser-side diff and cycle computation, the tier/focus/
reveal-depth navigation, and the offline guarantee. This is a component slice of the technical
design — for the architecture overview, principles, domain model, the
plugin/extraction crates and the plugin system see the main
[DESIGN](../DESIGN.md); for the CLI crate that drives rendering (`run_report` →
`render_html_viewer`) see [`code-ranker-cli/DESIGN.md`](../code-ranker-cli/DESIGN.md).

<!-- toc -->

- [HTML assets (`crates/code-ranker-viewer/src/assets/`)](#html-assets-cratescode-ranker-viewersrcassets)
- [Asset layers](#asset-layers)
- [Navigation: tier, focus & reveal depth](#navigation-tier-focus--reveal-depth)
- [Affected status](#affected-status)
- [Cycle detection](#cycle-detection)
- [Offline guarantee](#offline-guarantee)

<!-- /toc -->

## HTML assets (`crates/code-ranker-viewer/src/assets/`)

- [x] `p1` - **ID**: `cpt-code-ranker-component-html-assets`

Static assets for the `code-ranker report` HTML output (a single-snapshot viewer,
or a baseline↔current diff with `--baseline`). Every asset is `include_str!`-ed
into a const in `lib.rs` and inlined by `render_html_viewer` (each `<script>` /
`<link>` placeholder in `index.html` is `.replace`d with the file contents), so
the report is a single self-contained `.html`.

> **Fully data-driven (schema `"2"`).** `schema.js` is the single data-access
> layer; every consumer reads from the snapshot dictionaries — flat node `attrs`,
> `edge.source/target`, `node.cycle`, per-level `node_attributes` / `edge_kinds`
> / `node_kinds` / `cycle_kinds` / `attribute_groups` / `ui`, and top-level
> `presets`. **No metric/kind/colour/threshold/prompt is hardcoded by name.**
> Metric formulas come from `AttributeSpec.formula`; the live derivation is
> `eval`-ing `AttributeSpec.calc` over the node's attributes (`schema.js`
> `calcDisplay`). Preview a real report with
> `code-ranker report <dir> --output.html.path=out.html` (self-contained).

The JS is plain global-scope scripts (no ES modules) loaded in order; top-level
`function`/`const` declarations share one global lexical scope across files, so a
function may call a global defined in a later-loaded file as long as the call
happens at runtime (DOMContentLoaded or later), not at load time. Adding/removing
an asset is three edits: an `include_str!` const + a `.replace` in `lib.rs`, and a
`<script>`/`<link>` line in `index.html`.

## Asset layers

Files are grouped by concern (the load order in `index.html` roughly follows this
top-to-bottom). The viewer was split out of three former monoliths (`diagram.js`,
`app.js`, `node-table.js`).

### Vendor

| File | Purpose |
|------|---------|
| `graphviz.umd.js` | Graphviz compiled to WASM (`@hpcc-js/wasm`, ~802 KB, self-contained); renders DOT→SVG in-browser. |
| `snarkdown.umd.js` | Tiny (~2 KB) Markdown→HTML renderer (`window.snarkdown`), vendored for offline use; renders the generated prompt preview. |

### Data (pure, no DOM)

| File | Purpose |
|------|---------|
| `schema.js` | The single data-access layer over the snapshot dictionaries (readers for `node_attributes` / `edge_kinds` / `node_kinds` / `cycle_kinds` / `attribute_groups` / `ui` / `presets`, plus `evalCalc`/`calcDisplay`). |
| `grouping.js` | The **grouping ladder** the reveal depth indexes into: `grouperForDig(level, dig)` / `groupKeyAtDig` (a tier ladder spanning synthetic crate-folders → crate tier → folders-under-crate → files, via `crateRoots`/`crateDirs`/`maxCrateDepth`; the reveal-depth lens indexes into it via `window.dig` (overview) / `window.drillDig` (drilled). `viewTier` picks the dimension (crate vs file); the **file tier** uses an absolute directory ladder anchored on `maxFileDepth`/`digFloor`, with `overviewBaseDig` the file-tier landing; `crateKeyToFileKey`/`fileKeyToCrateKey` map a focus key across dimensions for tier switching), plus `groupLabel` (box label — the **full** folder path: crate dir + absorbed source dir + folders when digging in, the collapsed crate-dir path when digging out), `groupCountAtDig` (group-box count at a dig level — powers the dig-control +/- previews), `nodeFullDir` (full workspace-relative dir of a node, e.g. `/crates/foo/src` — drilled sub-cluster labels), `crateRelDir` (crate-relative dir for neighbour labels), `aggCycleStatus`, `clampDig`. Derives every tier from file-id paths + the crate attribute — no extra backend data. |
| `diff.js` | Browser-side diff: `computeDiff()` (node/edge status), `computeCycles()` (reads cycle membership **solely** from the backend `graph.cycles`; derives per-side status + `edgeCycleStatus`), `computeMeta()`. |
| `utils.js` | Shared formatting/escaping/DOM helpers (`fmtNum`, `fmtFull`, `fmtDate`, `escHtml`, …). |

### Graph layout & render

| File | Purpose |
|------|---------|
| `layout.js` | `buildDOT()` — emits the DOT for the map. Overview groups nodes by `grouperForDig` at `window.dig` (one box per group, deduped inter-group flow edges); each group box is labelled `fullPath (memberCount)` and tagged `cycle-status-*` aggregated from its members. Box fill: pink at the crate tier, white otherwise; metric **circles are always filled** — red at the crate tier, blue (`N_FILL`) otherwise (never empty white). At `dig > 0` with crate grouping the folder-group boxes are wrapped in a labelled **crate cluster** (`subgraph cluster_crate_N`, faint-pink) so folders read as inside their crate; the file tier and flat/coarse views stay flat. The drilled (focus) view filters to the focused group (`grouperForDig(level, drillDig)` === `drillGroup`) and renders a **hybrid** at reveal depth `D` (derived from `window.focusDig`): a node whose folder level under the focus is ≤ `D` becomes an individual **file** node (label = file **name** only) inside its **full-path** dir sub-cluster (`nodeFullDir`, faint-filled, hoverable/clickable to drill); a deeper node collapses into a **folder box** keyed at the frontier `groupKeyAtDig(level, n, drillDig + D + 1)`. So depth 0 shows the focus's direct files in their dir cluster alongside its immediate subfolders as boxes; `⊞` opens one more level (edges remapped file→box via `renderId`). Either way it adds **callers** (green) / **dependencies** (orange) neighbour clusters whose `edge-in`/`edge-out` edges are `constraint=false`. The neighbour boxes, their connector edges, **and the cluster itself** each carry a `status-*` class (per (group,file) presence folded from the union diff: `added` = current-only, `removed` = baseline-only, `unchanged` = both; the cluster ORs over all its boxes) so they follow the Baseline/Current toggle just like internal nodes/edges — a caller/dependency that exists in only one snapshot hides on the other side, and an all-one-side cluster hides its background+label too (member boxes are siblings of the cluster `<g>`, not children, so the cluster class hides only the box/label). **Flow** edges are drawn solid and counted (neighbour discovery and the overview metric edges still guard on `edgeIsFlow`); **non-flow** `contains`/`reexports` edges are also emitted now — **dashed**, `constraint=false`, tagged `edge-nonflow` and **hidden by CSS until a leaf-node hover** reveals the connected ones — only an individual file or a collapsed folder/group box (frame gets `.leaf-hovered`), **not** a directory sub-cluster that already shows its files (a pair already flow-linked is skipped). They also appear in the popup. No `ratio=fill`/`size` (natural layout, packed spacing); edges carry `arrowsize=0.6` (smaller arrowheads, which otherwise read oversized once the viewBox scales up on sparse graphs). The **cycle filter** (`window.cycleOnly`) drops every node not in a dependency cycle (keeping the edges between cycle nodes and the callers/dependencies clusters). Metric (SLOC/HK) sizing helpers live here too. |
| `map-render.js` | `drawSVG()` (big-graph confirm guard, drilled views only) and `renderSVGNow()` (DOT→SVG via `window.gv`, then wires pan/zoom, the status bar, edge-highlight and tooltips). |

### Map interactions

| File | Purpose |
|------|---------|
| `map-interactions.js` | All behaviour on the main SVG map: node selection + the platform open-source modifier (`isOpenSrcClick`, ⌘/Ctrl), the shortcut legend (`kbdHintsHtml`), **drill** nav (`drillIntoGroup(key, level, dig)`/`drillOutOfGroup`) driving the always-visible **breadcrumb** (`renderBreadcrumb`): a **tier-dropdown anchor** (its label opens a crates ⇄ files menu → `switchTier`, which maps the focus across dimensions; shown only when the level has crates), a **root element** (`all`/`root`, drills out to the overview, replacing the old static "← all"), clickable path chips that each drill to themselves, and a trailing **reveal-depth lens chip** `⊟ depth N ⊞`. Per-chip hover counts: files under each chip; the crate/file total under the root. **Reveal depth** (`setDig` ±1 → `window.dig` in the overview, `window.focusDig` while focused): `lensInfo` computes the displayed depth (the offset from the landing, `0` by default) plus the ⊟/⊞ enabled state and hover previews (`focusRenderCount` in focus — files + collapsed boxes — / `groupCountAtDig` in the overview); `focusMinFz`/`focusMaxDepth`/`underDepthOf` derive the focus bounds and `overviewBaseDig` the overview landing. The lens replaces the former overview `.dig-lod` and last-crumb `−/+` controls. Clicking a **box body** (a folder box, a directory sub-cluster, or a crate cluster) drills the focus into it (`focusFolderTarget` → key + dig). The status bar (`computeGroupStats` → `statusLineFor`/`statusLineForGroup`: group/neighbour lines carry the full path, **folders** count and files/sloc/hk/cycle, the `_root` collapse sentinel shown as `/`; hovering a caller/dependency neighbour box shows the same crate-style stats), `setupEdgeHighlight(svgFrame, level)` (must run **before** `setupTooltips`, which removes SVG `<title>`s; the green/orange in/out connectors are hidden by default and revealed on cluster/node hover; `cluster_crate_*` overview clusters highlight all edges of the groups inside them, and **clicking a crate cluster drills into the whole crate** — `drillIntoGroup(crate, 'crate')` — while clicking a folder box inside it drills into just that folder), `setupTooltips`. **Hover is debounced + de-flickered** (`HOVER_DELAY`): `wireNodeHover` delays the glow/raise so quick passes don't flash and clears every other `node-hl` first (never two glows at once); `raisePaint` lifts the hovered node to the end of its SVG parent (paint order — SVG has no z-index); a single shared `ehSchedule` timer drives **all** edge-highlight changes (nodes and clusters) so crossing boundaries never flashes the arrows. |
| `panzoom.js` | `setupPanZoom()` — viewBox drag-to-pan, +/−/fit/fullscreen buttons, the SLOC/HK metric-size row, and the drill-back button (the reveal-depth control now lives in the breadcrumb's lens chip, not a standalone panzoom button). In **fullscreen** the page `<header>` (and body-attached overlays) move under the frame and the header stays **persistently visible** in a top `.fs-bar` (no slide-in); the floating top controls are offset below it. The default framing is the **capped fit** (`fitVB`): never zoom IN past `MAX_FIT_ZOOM` (1.3× absolute = frame px per SVG unit), so small graphs aren't magnified — the viewBox is enlarged (centred) to land the on-screen scale at 1.3; `renderView`'s preserve step overrides this when the user has panned/zoomed. The fit button (`zoomOut`) animates to the same capped framing. |

### Node modal / popup

| File | Purpose |
|------|---------|
| `modal.js` | `getModal()`/`closeModal()` overlay shell; delegated copy/select handlers; Esc/Space keys; mirrors the map's ⌘/Shift gestures inside the popup diagram; `setModalDiagram` re-attaches the shortcut legend. |
| `node-popup.js` | `buildDiagramSVG()` — the per-node neighbourhood SVG and `markPopupSelected()`. Neighbours (deduped by far node, every edge kind) are laid out as **vertically-stacked blocks, 5 cards per row, each block a fixed 5-wide** (the main node spans the same width). Top→bottom: `external` callers, one `<group> in: <value>` block per OTHER group, same-group `<group> in:` (fan in), the **main node**, same-group `<group> out:` (fan out), one `<group> out: <value>` block per other group, `external` dependencies. Block/tooltip labels use the **grouping key** (`ui.grouping.key`, e.g. `crate`/`module`) — never hardcoded. **Arrows connect only fan-in → node → fan-out**; every other block carries none. Per-group blocks are sorted by card count so the biggest sits nearest the node. Each block has a thin solid outline (grey external / blue fan / green in / orange out) and a label whose group value is bold. Card tint: cross-group green (callers) / yellow (deps), same-group neutral blue, external grey, cycle red; a neighbour linked only through non-flow edges (`contains`/`reexports`) gets a **dashed** outline. Card hover tooltip = file name (title) + `<group>:` and `path:` rows (path with the `{token}` root marker stripped, leading slash kept → `/foo/bar`). |
| `modal-content.js` | `buildModalContent()` — the modal's left field-table HTML (verbatim values via `fmtFull`, a git-host **Source** row, schema-driven metric rows/tooltips). |
| `source-links.js` | `gitWebBase`/`gitSourceUrl`/`nodeSourceUrl`/`connSourceLine` (git blob URLs at the analysed commit, optional `#L<line>`) and `absPath` (token→on-disk path). Pure, no DOM. |

### Shared

| File | Purpose |
|------|---------|
| `tooltip.js` | The shared `#tt` engine: `renderTooltip` (distribution table — `avg`, `min`, `max` + p1/p10/p50/p90/p99 percentiles), `renderDescTooltip` (title + formula + filled-calc + description with `<br>`/`` `code` `` markup), `renderNodeTooltip`/`renderGroupTooltip`, and `setupTooltip` — one delegated hover controller with a 300 ms delay; derives metric tooltips lazily on hover. Used by the table, map, popup and summary. |

### Tables & summary

| File | Purpose |
|------|---------|
| `node-table.js` | Sortable **Details** table (collapsed by default, re-titled per active side, search + **kind-filter checkboxes** when expanded (hidden while collapsed, like the search box) — files / folders / `<groups>`, with files + groups on by default (folders off), selection checkboxes). Besides file rows it lists synthetic **folder** and **group (crate)** aggregate rows (`buildAggregates` — each numeric column **summed** over the member files, a distinct `kind`, `_cat`-tagged); each aggregate carries a selection checkbox and a `cycle` count (member files in a dependency cycle, blank at 0). Clicking an aggregate **drills** into it on the map (`focusFolderTarget`/`drillIntoGroup`, like clicking its SVG box) while a file row opens the modal. Hovering/selecting an aggregate row lights up its on-map element and vice-versa via a shared key (`group:<crate>` / `folder:<dir>`, `section._gAggMap`) — best-effort, only when that element is currently rendered (crate boxes in the overview). The average/count footer (with percentile tooltips) is shown **only when the displayed rows are a single kind**. Hosts the **Prompt Generator** button. `attachModalCheckbox`/`setupNodeTable`. |
| `summary.js` | Review/diff **summary** table. Rows come from a **builder registry** (id → `<tr>` HTML, `''` to skip), and the display order is a **`LAYOUT` tree** of titled sections — each `{ title, rows: [...ids] }` — so reordering rows or sections is a pure data edit. Structural rows (`nodes`, `groups` = distinct grouping-key values, `edges`, `cycles`) are plain **counts**; `metric:<key>` rows show one **per-file statistic** chosen by a radio rendered as an **in-table divider row** (`{ radio: true }` in `LAYOUT`, placed between the count rows and the metric sections it drives; handler delegated on the tbody — `setupSummaryStatControl` — `avg`/`min`/`p50`/`p90`/`max`/`sum`, default `avg`; `sum` aggregates over files, the rest read `nodePercentiles`); changing it re-renders the table and round-trips through the URL (`stat=`, omitted for the default `avg`). Each section emits a `summary-subhead` divider row; a metric the snapshot lacks renders nothing, and a section left with no rows **drops its header**. Any metric builder not placed in `LAYOUT` lands in a trailing **Other** section so a new metric never silently vanishes. The table is shown in a **full-screen popup** opened by the **Statistics** button in the page header (`setupSummaryPopup` — `#summary-overlay`, closed by ✕/Esc/backdrop; its open state round-trips through the URL `panel=stats` so a refresh reopens it); each render also stashes a structured `window._summaryModel` (sections → `{label, baseline, current, delta}`) that the popup **footer** turns into downloads (`.json` / `.md` links) and **copy-to-clipboard** buttons (copy as json / markdown) — all client-side, no network (`summaryJSONText`/`summaryMarkdownText` feed both `downloadFile` and `copySummaryText`). The hover tooltip shows `avg`, `min`, `max` plus the p1/p10/p50/p90/p99 distribution. `direction`-driven Δ colouring (`effectiveDir(key, stat)`: neutral = uncoloured, and the **`sum`** stat is always uncoloured — its delta tracks the change in file count, not per-file quality; every other stat is count-normalised so the metric's own direction applies); a delta whose **rounded** magnitude is 0 renders as a plain uncoloured `0` (never `+0`/`−0`), and the Δ header column is labelled `Δ delta`. |

### Export

| File | Purpose |
|------|---------|
| `export-popup.js` | `openExportPopup()` — the Prompt Generator: selected-vs-recommended source, per-metric two-tier threshold colouring, `snapshot.presets`-driven preset buttons, Markdown prompt composition rendered via snarkdown, full state mirrored to the URL. |

### App shell

| File | Purpose |
|------|---------|
| `nav.js` | URL/history state: `getNavParams`, `navViewState`/`navViewUrl` (carry `level`/`side`/`group`/`mode`/`zoom`), `navPushView`/`navReplaceView`/`navPush`/`navSetSide`, and `openModalForNode` (the single node-modal entry point). |
| `view-state.js` | Which side is shown and how the map/tables reflect it: `activeSnap`/`viewMode`/`activeGraph`/`unionGraph`, `applySideVisibility`/`applySideSizing` (CSS-flip the shared union layout, no relayout), `setViewSide`, `recomputeAll`, `renderView`, and `applyViewState` (restores `group`/`mode`/`zoom` from a state object). |
| `snap-controls.js` | Header chrome: side-toggle wiring + `t` hotkey, the fly-out header, the warning count, `updateHeader`, the snapshot details/actions popup, and file-upload (snapshot swap) controls. The global map hotkeys (`t`, the Shift/Ctrl modifier classes in `map-interactions.js`) bail while the Prompt Generator popup is open (`window.isPromptPopupOpen` in `export-popup.js`) so keys — notably ⌘/Ctrl+C to copy — reach the popup instead of toggling map state. |
| `app.js` | The thin `DOMContentLoaded` bootstrap: read embedded snapshots, compute diff/cycles/meta, restore side/zoom/drill/node from the URL, load graphviz, render, and the `popstate` handler. |
| `ui.js` | Intentionally empty (kept because assets are inlined by name). |

### Shell template & styles

| File | Purpose |
|------|---------|
| `index.html` | The shell: one `<header>` row (brand — a link to the GitHub repo, title, two snapshot controls + a toggle, then a **Statistics** button), the single Files `.view` with `.frame-wrap` (svg frame, the navigation **breadcrumb** top-left — tier-dropdown anchor, path chips, trailing reveal-depth **lens chip** — zoom/size controls incl. a **cycle** filter toggle, kbd legend), and the full-screen **diff-summary popup** (`#summary-overlay`). |
| `base.css` · `map.css` · `modal.css` · `tables.css` · `export.css` · `snap.css` · `map-svg.css` | The former `index.css` split by concern; concatenated in `lib.rs` **in source order** into one inlined `<style>` (preserving the cascade, no extra requests → keeps the offline guarantee). `map-svg.css` holds the graphviz node/edge state rules: visibility toggles, **cycle red stroke** (side-gated), selection, hover, status bar and edge highlight. |

## Navigation: tier, focus & reveal depth

Navigation over the single Files graph is surfaced in one always-visible top-left
**breadcrumb** (`renderBreadcrumb`):

```
[ crates ▾ ] › all › auth › domain        ⟨ ⊟ depth 0 ⊞ ⟩
   tier-dropdown  root   path chips           reveal-depth lens
```

**Tier (`window.tier`, resolved by `viewTier`)** — the grouping **dimension**:
`'crate'` (group by the crate attribute, the default when the level declares a
grouping key) or `'file'` (ignore crates, group purely by directory). The leftmost
chip is the **tier dropdown** — its label opens a small menu (crates ⇄ files); it
is only shown when the level has crates. Picking a dimension (or re-picking the
current one) navigates to that tier's overview via `switchTier`.

**Root element** — the chip after the dropdown: `all` (crate tier) / `root` (file
tier). It drills out to the whole-tree overview (`drillOutOfGroup`); at the
overview it is the current (non-clickable) position. Its hover count is the total
crate / local-file count.

**Focus (`window.drillGroup` + `window.drillDig`)** — clicking a box drills into
just that group; `drillGroup` is the group key, `drillDig` the dig it sits at
(`digOfKeyForTier`). The breadcrumb path chips are the key split on `/`; each chip
drills to itself (`drillIntoGroup(key, level, chipDig)`). In the focused view,
directory sub-cluster labels are the **full** workspace path (`nodeFullDir`);
caller/dependency neighbour boxes drop the crate prefix when every neighbour shares
the drilled crate, else keep the full key.

**Reveal depth** — the trailing **lens chip** `⟨ ⊟ depth N ⊞ ⟩`, a single dial
stepped by `setDig` (±1). `N` is the offset from the **landing** (`0` at every
landing — the user only moves it with the buttons); `⊞` reveals one level deeper,
`⊟` collapses. The lens drives:
  - **overview** → `window.dig` (the LOD), with `depth = dig - overviewBaseDig`.
    The landing `overviewBaseDig` is the crate tier (dig 0) or, on the file tier,
    one level below the directory root (top folders) rather than the finest
    per-folder grouping. `⊟` below the landing folds crates/folders into coarser
    boxes; `⊞` reveals finer groups.
  - **focus** → `window.focusDig` (≤ 0), with `depth = focusDig - minFz`
    (`minFz = focusMinFz`). The focused view is **hybrid**: at reveal depth `D`,
    a node whose folder level under the focus is ≤ `D` renders as an individual
    **file** inside its directory sub-cluster; a deeper node collapses into a
    **folder box** at the frontier (focus + `D` + 1 levels, `groupKeyAtDig`). So
    depth `0` shows the focus's direct files (in their dir cluster) **plus** its
    immediate subfolders as collapsed boxes; `⊞` opens one more level. Clicking a
    folder box drills into it; clicking a file opens its modal.

  The lens is hidden when there is nothing to reveal/collapse. Hover counts under
  the buttons preview the rendered element count one step away (`focusRenderCount`
  — files + collapsed boxes — in focus; `groupCountAtDig` in the overview).

**Tier switching (`switchTier`, crate ⇄ files)** maps the focus across dimensions
around the **crate-root directory** boundary (a crate ≡ its source directory):
  - *crates → files* (`crateKeyToFileKey`): expand the crate chip into its real
    path segments, keep the folder tail.
  - *files → crates* (`fileKeyToCrateKey`): collapse the deepest path prefix that
    equals a crate root into the crate chip; a path inside **no** crate falls back
    to the nearest representable ancestor (down to the overview).
  - the focus lands at reveal depth 0 (most collapsed). The needed **crate →
    root-directory** map comes from `crateRoots` in `grouping.js`.

The **tier ladder** (`grouperForDig` / `groupKeyAtDig` in `grouping.js`) computes
group keys from file-id paths + the `crate` attribute — no extra backend data. The
crate tier descends/ascends via `crateRoots`/`crateDirs`/`maxCrateDepth`; the file
tier uses an **absolute** directory ladder anchored on `maxFileDepth` (so a fixed
directory key has one well-defined dig, making file-tier drilling unambiguous).
`window.tier` and `depth` (the lens offset from the landing — focus collapse or
overview LOD) round-trip through the URL (`tier=`, `depth=`, both omitted at the
default) and restore on load / `popstate` via `applyViewState`, which clamps the
restored `focusDig` into the focus's valid range. Per-node expand-in-place
overrides and showing individual files inline in the **overview** are **not yet
implemented** (the overview always renders group boxes).

**Node labels**: a collapsed box shows its full path + member-node count `(N)`
(what opens on drill-in); a file box shows just the file **name** — no counts. Box
mode only — metric (SLOC/HK) circles show the metric value and are always filled
(red at the crate tier, blue otherwise).

**Layout density**: the map is laid out at natural size with packed spacing
(`nodesep`/`ranksep` tiny, `height=0`/`width=0` boxes) and **no `ratio=fill` /
`size`** — the SVG viewBox scales uniformly to the frame, so nodes stay large and
inter-node gaps small instead of being stretched. Caller/dependency (`edge-in` /
`edge-out`) edges are `constraint=false` so they draw without dragging the layout
vertically. Strokes (node borders, edges) and arrowheads scale with the SVG fit
like everything else; edges set `arrowsize=0.6` so the arrowheads stay legible
rather than oversized on sparse, scaled-up graphs.

Nested **crate clusters** (a crate's folder groups wrapped in a labelled box once
the reveal depth opens that crate) are implemented (`cluster_crate_N`). Not yet
implemented: the diagonal in/out cluster placement.

## Affected status

Unchanged nodes/edges adjacent to changed (added/removed) nodes or edges are
promoted to `affected` status. Computed in `diff.js` `computeDiff()`
(browser-side), not in Rust.

## Cycle detection

`computeCycles()` in `diff.js` reads cycle membership **solely from the backend
`graph.cycles` array** in each embedded snapshot (the backend is the single source
of truth for SCC detection and `CycleKind` classification — no browser-side Tarjan
fallback). It derives the **per-side** status (`baseline-only`/`current-only`/
`both`/`none`) for nodes (`nodeCycleStatus`) and edges (`edgeCycleStatus`).

Cycle members are drawn with a **red stroke**: `layout.js` emits the real
`cycle-status-*` class on drilled file nodes/edges (read from `window.CYCLES`),
and on **group nodes** the status is aggregated from members (`aggCycleStatus`) so
the overview shows which crates/folders contain a cycle (with an `in cycle: N`
count in the status bar). `map-svg.css` colours any non-`none` `cycle-status-*`
node/edge red (normal weight — the colour alone marks it), side-gated by the
`.svg-frame.side-*` marker. An **edge** is red only when both endpoints share a
cycle (`edgeCycleStatus`), so a link from a cycle node to a non-cycle node stays
neutral. The per-node popup (`node-popup.js`) marks cycle nodes red the same way.
The summary reports cycle counts per side. The **cycle filter** (`window.cycleOnly`,
the `cycle` toggle by the size controls) reduces the map to only the cycle nodes
and the edges between them (callers/dependencies clusters kept).

## Offline guarantee

No CDN references in any asset; `graphviz.umd.js` embeds the WASM binary as a
base91-encoded string and instantiates it from an `ArrayBuffer` — works from
`file://` with no network access. The stylesheet is inlined as a single `<style>`
(concatenated CSS files), not linked, so there are no extra requests.

---

**Related docs**: [PRD.md](PRD.md) (viewer requirements) ·
main [DESIGN](../DESIGN.md) · [`code-ranker-cli/DESIGN.md`](../code-ranker-cli/DESIGN.md)
(the `report` command and `render_html_viewer`)

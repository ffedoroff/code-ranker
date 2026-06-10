# Technical Design — Code Ranker Viewer (`code-ranker-viewer`)

The technical design of the offline HTML viewer: the `code-ranker-viewer` crate
and its static assets (embedded into the `code-ranker` binary), the data-driven
rendering layer, the browser-side diff and cycle computation, the relative-dig
navigation, and the offline guarantee. This is a component slice of the technical
design — for the architecture overview, principles, domain model, the
plugin/extraction crates and the plugin system see the main
[DESIGN](../DESIGN.md); for the CLI crate that drives rendering (`run_report` →
`render_html_viewer`) see [`code-ranker-cli/DESIGN.md`](../code-ranker-cli/DESIGN.md).

<!-- toc -->

- [HTML assets (`crates/code-ranker-viewer/src/assets/`)](#html-assets-cratescode-ranker-viewersrcassets)
- [Asset layers](#asset-layers)
- [Relative dig (level-of-detail)](#relative-dig-level-of-detail)
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
`app.js`, `node-table.js`) — see
[`REFACTOR-split-plan.md`](REFACTOR-split-plan.md).

### Vendor

| File | Purpose |
|------|---------|
| `graphviz.umd.js` | Graphviz compiled to WASM (`@hpcc-js/wasm`, ~802 KB, self-contained); renders DOT→SVG in-browser. |
| `snarkdown.umd.js` | Tiny (~2 KB) Markdown→HTML renderer (`window.snarkdown`), vendored for offline use; renders the generated prompt preview. |

### Data (pure, no DOM)

| File | Purpose |
|------|---------|
| `schema.js` | The single data-access layer over the snapshot dictionaries (readers for `node_attributes` / `edge_kinds` / `node_kinds` / `cycle_kinds` / `attribute_groups` / `ui` / `presets`, plus `evalCalc`/`calcDisplay`). |
| `grouping.js` | The **grouping ladder** for relative dig: `grouperForDig(level, dig)` / `groupKeyAtDig` (dig 0 = crate tier; +N folders under the crate; −N progressive deepest-first collapse via `crateRoots`/`crateDirs`/`maxCrateDepth`), plus `groupLabel` (box label — the **full** folder path: crate dir + absorbed source dir + folders when digging in, the collapsed crate-dir path when digging out), `groupCountAtDig` (group-box count at a dig level — powers the dig-control +/- previews), `nodeFullDir` (full workspace-relative dir of a node, e.g. `/crates/foo/src` — drilled sub-cluster labels), `crateRelDir` (crate-relative dir for neighbour labels), `aggCycleStatus`, `clampDig`. Derives every tier from file-id paths + the crate attribute — no extra backend data. |
| `diff.js` | Browser-side diff: `computeDiff()` (node/edge status), `computeCycles()` (reads cycle membership **solely** from the backend `graph.cycles`; derives per-side status + `edgeCycleStatus`), `computeMeta()`. |
| `utils.js` | Shared formatting/escaping/DOM helpers (`fmtNum`, `fmtFull`, `fmtDate`, `escHtml`, …). |

### Graph layout & render

| File | Purpose |
|------|---------|
| `layout.js` | `buildDOT()` — emits the DOT for the map. Overview groups by `grouperForDig(level, window.dig)` (one node per group, deduped inter-group flow edges); each group box is labelled `fullPath (memberCount)` and tagged `cycle-status-*` aggregated from its members. Box fill: pink at the crate tier, white otherwise; metric **circles are always filled** — red at the crate tier, blue (`N_FILL`) otherwise (never empty white). At **dig in** (`dig > 0`) with crate grouping the folder-group boxes are wrapped in a labelled **crate cluster** (`subgraph cluster_crate_N`, faint-pink) so folders read as inside their crate; dig 0 / dig out stay flat. The drilled (focus) view filters to the focused group and, at `window.focusDig === 0`, renders per-file nodes (label = file **name** only) in **full-path** dir sub-clusters (`nodeFullDir`, e.g. `/crates/foo/src`; faint-filled so the whole folder area is hoverable/clickable to drill in); at `focusDig < 0` it instead collapses those files into **folder boxes** (grouped by `groupKeyAtDig` at a depth derived from `focusDig`, edges remapped file→box via `renderId`). Either way it adds **callers** (green) / **dependencies** (orange) neighbour clusters whose `edge-in`/`edge-out` edges are `constraint=false`. The neighbour boxes, their connector edges, **and the cluster itself** each carry a `status-*` class (per (group,file) presence folded from the union diff: `added` = current-only, `removed` = baseline-only, `unchanged` = both; the cluster ORs over all its boxes) so they follow the Baseline/Current toggle just like internal nodes/edges — a caller/dependency that exists in only one snapshot hides on the other side, and an all-one-side cluster hides its background+label too (member boxes are siblings of the cluster `<g>`, not children, so the cluster class hides only the box/label). Only **flow** edges are drawn/counted on the map (internal edges, neighbour discovery and the overview all guard on `edgeIsFlow`); non-flow `contains`/`reexports` links are shown only in the popup. No `ratio=fill`/`size` (natural layout, packed spacing); edges carry `arrowsize=0.6` (smaller arrowheads, which otherwise read oversized once the viewBox scales up on sparse graphs). The **cycle filter** (`window.cycleOnly`) drops every node not in a dependency cycle (keeping the edges between cycle nodes and the callers/dependencies clusters). Metric (SLOC/HK) sizing helpers live here too. |
| `map-render.js` | `drawSVG()` (big-graph confirm guard, drilled views only) and `renderSVGNow()` (DOT→SVG via `window.gv`, then wires pan/zoom, the status bar, edge-highlight and tooltips). |

### Map interactions

| File | Purpose |
|------|---------|
| `map-interactions.js` | All behaviour on the main SVG map: node selection + the platform open-source modifier (`isOpenSrcClick`, ⌘/Ctrl), the shortcut legend (`kbdHintsHtml`), **drill** nav (`drillIntoGroup(group, level, dig)`/`drillOutOfGroup`) with a **hierarchical breadcrumb** (`renderBreadcrumb`): a clickable trail `all <groups> › crate › folder …` — each segment drills to itself, the root returns to the overview (replaces the old static "← all"). **Relative-dig** (`setDig`/`updateDigLabel`): in the **overview** the `.dig-lod` control shows `/<group>/folder±N` (the grouping key, never hardcoded) with `groupCountAtDig` previews under the −/current/+ slots; `+` disables at `DIG_MAX` and — only once `dig ≥ 0` — when digging deeper no longer splits; dug out (`dig < 0`) keeps `+` enabled. **In focus** the `.dig-lod` is hidden and the collapse control lives in the breadcrumb's last crumb (`− <crumb> +`): `window.focusDig` (0 = individual files; negative collapses the focused group's files into folder boxes, deepest first) — `−` collapses, `+` expands back to files; each crumb and each +/- shows a hover count (items there / after the step). Clicking a **folder box** (folder mode) or a **directory sub-cluster** (files mode) drills into that folder (`focusFolderTarget` → group key + dig). The status bar (`computeGroupStats` → `statusLineFor`/`statusLineForGroup`: group/neighbour lines carry the full path, **folders** count and files/sloc/hk/cycle, the `_root` collapse sentinel shown as `/`; hovering a caller/dependency neighbour box shows the same crate-style stats), `setupEdgeHighlight(svgFrame, level)` (must run **before** `setupTooltips`, which removes SVG `<title>`s; the green/orange in/out connectors are hidden by default and revealed on cluster/node hover; `cluster_crate_*` overview clusters highlight all edges of the groups inside them, and **clicking a crate cluster drills into the whole crate** — `drillIntoGroup(crate, level, 0)`, crate-tier grouper — while clicking a folder box inside it drills into just that folder), `setupTooltips`. **Hover is debounced + de-flickered** (`HOVER_DELAY`): `wireNodeHover` delays the glow/raise so quick passes don't flash and clears every other `node-hl` first (never two glows at once); `raisePaint` lifts the hovered node to the end of its SVG parent (paint order — SVG has no z-index); a single shared `ehSchedule` timer drives **all** edge-highlight changes (nodes and clusters) so crossing boundaries never flashes the arrows. |
| `panzoom.js` | `setupPanZoom()` — viewBox drag-to-pan, +/−/fit/fullscreen buttons, the SLOC/HK metric-size row, the drill-back button, and the **relative-dig** (−/+) buttons that call `setDig`. The default framing is the **capped fit** (`fitVB`): never zoom IN past `MAX_FIT_ZOOM` (1.3× absolute = frame px per SVG unit), so small graphs aren't magnified — the viewBox is enlarged (centred) to land the on-screen scale at 1.3; `renderView`'s preserve step overrides this when the user has panned/zoomed. The fit button (`zoomOut`) animates to the same capped framing. |

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
| `tooltip.js` | The shared `#tt` engine: `renderTooltip` (percentile table), `renderDescTooltip` (title + formula + filled-calc + description with `<br>`/`` `code` `` markup), `renderNodeTooltip`/`renderGroupTooltip`, and `setupTooltip` — one delegated hover controller with a 300 ms delay; derives metric tooltips lazily on hover. Used by the table, map, popup and summary. |

### Tables & summary

| File | Purpose |
|------|---------|
| `node-table.js` | Sortable per-file **Details** table (collapsed by default, re-titled per active side, search when expanded, selection checkboxes, average/count footer with percentile tooltips). Hosts the **Prompt Generator** button. `attachModalCheckbox`/`setupNodeTable`. |
| `summary.js` | Review/diff **summary** table: structural aggregate rows (Nodes/Edges/Source-lines/Nodes-in-cycles) then per-metric medians from `ui.summary_metrics`, with `direction`-driven Δ colouring (neutral = uncoloured). |

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
| `index.html` | The shell: one `<header>` row (brand, title, two snapshot controls + a toggle), the single Files `.view` with `.frame-wrap` (svg frame, drill breadcrumb, **dig** control (`.dig-lod`) top-left, zoom/size controls incl. a **cycle** filter toggle, kbd legend), and the collapsible summary. |
| `base.css` · `map.css` · `modal.css` · `tables.css` · `export.css` · `snap.css` · `map-svg.css` | The former `index.css` split by concern; concatenated in `lib.rs` **in source order** into one inlined `<style>` (preserving the cascade, no extra requests → keeps the offline guarantee). `map-svg.css` holds the graphviz node/edge state rules: visibility toggles, **cycle red stroke** (side-gated), selection, hover, status bar and edge highlight. |

## Relative dig (level-of-detail)

Two orthogonal navigation axes over the single Files graph (the control is the
−/+ **dig** buttons top-left of the map, calling `setDig`):

- **`window.dig`** — a relative LOD on the **overview**:
  - `0` — every crate is its own node (the default; reproduces the legacy crate
    grouping byte-for-byte). Crate group boxes are pink; any non-crate grouping
    is a neutral white.
  - `+N` — **dig in**: descend N directory levels inside crates. Folder groups are
    labelled with their **full** path from the workspace root — crate dir +
    absorbed source dir + folders, e.g. `/crates/code-ranker-viewer/src`,
    `/crates/code-ranker-viewer/src/services` (`groupLabel`). The folder-group
    boxes of one crate are wrapped in a labelled **crate cluster**
    (`subgraph cluster_crate_N`) so they read as inside their crate.
  - `-N` — **dig out**: progressively collapse the **deepest** crates into their
    parent folder, one depth level per step (`crateDirs` + `maxCrateDepth`), until
    a single root group remains (its `_root` sentinel key is shown as `/`). The
    "−" button disables at that point.
- **focus (`window.drillGroup`)** — **clicking a group** drills into just that
  group's files; clicking a leaf file opens the popup. `window.drillDig` records
  the dig at drill time so the focused view filters by the matching grouper. In
  the focused view, directory sub-cluster labels are the **full** workspace path
  (`nodeFullDir` → `/crates/foo/src`); caller/dependency neighbour boxes drop the
  crate prefix when every neighbour shares the drilled crate (`/services`), else
  keep the full key. The focus is itself navigable:
  - **`window.focusDig`** — a sub level-of-detail *inside* the focus: `0` = the
    group's individual files (default); `−k` collapses them into **folder boxes**
    (deepest folders first). The `.dig-lod` is hidden here; the `−`/`+` control
    lives in the breadcrumb's last crumb instead.
  - **clicking a folder** — a folder box (collapsed) or a directory sub-cluster
    (files mode) drills into that folder (`focusFolderTarget`), pushing it onto
    the breadcrumb. A crate cluster in the overview drills into the whole crate.
  - the **breadcrumb** (`renderBreadcrumb`) is the clickable trail `all <groups> ›
    crate › folder …`; each segment drills to itself, the root to the overview.

The tier ladder (`grouperForDig` / `groupKeyAtDig`) lives in `grouping.js`, derived
entirely from file-id paths plus the `crate` grouping attribute — no extra backend
data. Both axes are carried in the URL (`dig=` param) and restored on load /
`popstate` via `applyViewState`.

**Node labels**: a group box shows its full path + member-node count `(N)` (what
opens on drill-in); a file box (drilled view) shows just the file **name** — no
counts. Box mode only — metric (SLOC/HK) circles show the metric value and are
always filled (red at the crate tier, blue otherwise).

**Layout density**: the map is laid out at natural size with packed spacing
(`nodesep`/`ranksep` tiny, `height=0`/`width=0` boxes) and **no `ratio=fill` /
`size`** — the SVG viewBox scales uniformly to the frame, so nodes stay large and
inter-node gaps small instead of being stretched. Caller/dependency (`edge-in` /
`edge-out`) edges are `constraint=false` so they draw without dragging the layout
vertically. Strokes (node borders, edges) and arrowheads scale with the SVG fit
like everything else; edges set `arrowsize=0.6` so the arrowheads stay legible
rather than oversized on sparse, scaled-up graphs.

Nested **crate clusters** at `dig +1` (crate boxes wrapping their folder groups)
are implemented (`cluster_crate_N`). Not yet implemented: the diagonal in/out
cluster placement — see `REFACTOR-split-plan.md`.

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
[REFACTOR-split-plan.md](REFACTOR-split-plan.md) (the asset split + zoom plan) ·
main [DESIGN](../DESIGN.md) · [`code-ranker-cli/DESIGN.md`](../code-ranker-cli/DESIGN.md)
(the `report` command and `render_html_viewer`)

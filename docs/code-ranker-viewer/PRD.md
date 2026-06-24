# PRD — Code Ranker Viewer (`code-ranker-viewer`)

The offline HTML viewer: the self-contained report `code-ranker report`
generates — single-snapshot visualization, client-side node sorting, the
Prompt Generator, and the browser-side baseline↔current diff. This is a
component slice of the product PRD — for the product overview, actors,
plugin/extraction layer, graph model and JSON schema, see the main
[PRD](../PRD.md); for the CLI command surface that produces these reports see
[`code-ranker-cli/PRD.md`](../code-ranker-cli/PRD.md).

<!-- toc -->

- [1. Visualization Reports — Step 2](#1-visualization-reports--step-2)
  - [HTML Report Generation](#html-report-generation)
  - [Node Sorting by Weight](#node-sorting-by-weight)
  - [Map navigation — semantic zoom & cycle visibility](#map-navigation--semantic-zoom--cycle-visibility)
  - [AI Prompt Generator (P2)](#ai-prompt-generator-p2)
  - [Principles-Based Prompt Generation (P3)](#principles-based-prompt-generation-p3)
- [2. Baseline Comparison — diff viewer (Step 4)](#2-baseline-comparison--diff-viewer-step-4)
  - [Graph Diff Engine](#graph-diff-engine)
  - [Diff HTML Report](#diff-html-report)

<!-- /toc -->

## 1. Visualization Reports — Step 2

### HTML Report Generation

- [x] `p1` - **ID**: `cpt-code-ranker-fr-html-report`

The `code-ranker report` subcommand MUST analyze the workspace and, when the
`html` artifact is selected (the default set is both `json` and `html`),
generate a single self-contained offline HTML file alongside the snapshot
`.json`. The HTML MUST include:

- Interactive file-graph visualization, with `external` library nodes
  shown in a distinct amber colour (dashed edges)
- A coupling metrics table showing node weight (fan-in + fan-out) for
  each file
- All JavaScript and CSS inlined (no CDN or external resources)

**Rationale**: A self-contained HTML file requires no server, no
internet, and no installed dependencies to view — maximizing
accessibility for developers and reviewers on any machine.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`

### Node Sorting by Weight

- [x] `p1` - **ID**: `cpt-code-ranker-fr-node-sorting`

The HTML report MUST allow the user to sort files by coupling weight
(fan-in + fan-out edge count). The report MUST display the top-N
heaviest files prominently. Sorting MUST be performed client-side within
the HTML (no server required).

**Rationale**: The heaviest nodes are the most likely candidates for
refactoring. Surfacing them first reduces the time to actionable insight.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`

### Map navigation — semantic zoom & cycle visibility

- [ ] `p1` - **ID**: `cpt-code-ranker-fr-map-zoom-cycles`

The map MUST support **semantic level-of-detail navigation** independent of pixel
zoom, surfaced in a single always-visible top-left breadcrumb:
`[ crates ▾ ] › all › auth › domain   ⟨ ⊟ depth N ⊞ ⟩`.

1. **Tier dropdown** — the leftmost chip selects the dimension being sliced —
   **crates** (Rust modules) or **files/folders**; its label opens the menu (shown
   only when the project has crates). Switching the tier MUST attempt to **map the
   current focus across dimensions** (a crate ↔ its source directory) and fall back
   to the nearest representable ancestor when no mapping exists.

2. **Root element + path** — a **root** chip (`all` for crates, `root` for files)
   returns to the whole-tree overview; each subsequent path chip drills to itself.
   Clicking a box on the map drills the focus into it.

3. **Reveal depth** — a **lens chip** at the end of the breadcrumb (`⊟ depth N ⊞`)
   controlling how deep the current focus is expanded; the user moves it with the
   buttons (`⊞` reveals one level deeper, `⊟` collapses). The overview opens at
   depth 0 (crates / top folders) and `⊞` reveals progressively finer folder
   groups; one step **past the deepest folder** the overview reaches a **files
   level** — the deepest single folder level is kept as clusters but each is drawn
   with its **individual files inside** (no further nesting), after which `⊞`
   stops. **Drilling into a crate/folder opens at the
   node-budget depth** — the deepest reveal whose visible node count stays under a
   fixed budget (20) — so the focus comes up usefully expanded rather than fully
   collapsed. In a focused group the view is a hybrid: files at or above the
   revealed level show individually inside their directory cluster, while deeper
   subfolders show as collapsed boxes. The lens is absent on a leaf file.

Breadcrumb counts (revealed on hover) report the files under each chip and the
crate/file total under the root. The tier, focus, and reveal depth MUST round-trip
through the URL (`tier=`, `group=`, `depth=`, each omitted at its default).

Deferred (P2): expanding individual collapsed boxes **in place** (a per-node
override on top of the global depth). Revealing individual files inline in the
**overview** is now implemented — the files level draws each deepest folder as a
cluster of its real file nodes.

Cycle membership MUST be **visible at every level**: file nodes and edges in a
dependency cycle are drawn red, and a collapsed group (crate/folder) is marked
when it contains cycle members. **Node-filter** toggles (next to the size
controls) are built from the snapshot's `ui.filter` — the built-in
`cycle` plus any a project adds via `[report] filter` — and reduce the map to only
the nodes where the active filter metric has signal; for `cycle` that is the nodes
in a cycle and the edges between them (callers/dependencies clusters kept). Cycle
data is sourced solely from the backend (`graph.cycles`); the per-metric
`info` / `warning` colour thresholds are derived from the `[rules.thresholds.file]`
gate and carried in the snapshot's `node_attributes` (see the CLI **Severity tiers**
reference).

**Rationale**: A flat per-file map does not scale to large workspaces. Semantic
zoom lets a developer start at the crate level and drill toward the files that
matter, while cycles — the highest-priority structural smell — stay visible at
the level they are reasoning about.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`

### AI Prompt Generator (P2)

- [x] `p2` - **ID**: `cpt-code-ranker-fr-ai-prompts`

The HTML report SHOULD include a UI control that generates a prompt for
an LLM, pre-populated with the top-N heaviest nodes and their coupling
context, asking for refactoring recommendations. The prompt format MUST
be copyable as plain text for direct paste into any LLM interface.

The **same recommendation engine is exposed on the CLI** as two `report`
output formats, so the guidance is reachable without opening the HTML
(driven from the snapshot's gate-derived `node_attributes[*].thresholds`
`info` / `warning` tiers — advisory, never a gate):

- `--output.prompt[.path]` — the LLM prompt for **one** principle, the same
  Markdown the HTML Prompt Generator produces (intent, summary, principle-doc
  link, a task checklist, the ranked offending modules, and the principle's
  connection lists). Defaults to a per-principle file
  `.code-ranker/{ts}-{git-hash-3}-{principle}.md` (or `stdout`).
- `--output.scorecard[.path]` — a console **triage** overview (a per-principle
  table of `warning` / `info` counts + the worst module, then the worst modules
  overall, then a hint to the prompt for the worst principle). Defaults to
  `stdout`.

The `scorecard` is narrowed by `--focus <NAME>` (one ranking metric or principle: `hk`, `cycle`,
`sloc`, `cognitive`, …; without it the table spans all principles) and `--severity
<info|warning|auto>` (the tier; repeatable; `auto` = warning-if-any-else-info), and
capped by `--top <N>`. The `prompt` is **auto-targeted at the single worst module**
and **requires `--top 1`** — there is no CLI principle selector. These flags apply
only with a `prompt` / `scorecard` format; an explicit `--index` is rejected with a
hint to use `--top`.

The CLI side of this engine is documented in
[`code-ranker-cli/DESIGN.md`](../code-ranker-cli/DESIGN.md#code-ranker-cli-recommendation-engine)
(`cpt-code-ranker-component-recommend`).

**Rationale**: Connecting structural data to an LLM's reasoning closes
the loop between measurement and advice without coupling the offline
tool to a specific LLM provider. The CLI surface lets an agent or a CI
step pull the same prompt/triage non-interactively.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`

### Principles-Based Prompt Generation (P3)

- [x] `p3` - **ID**: `cpt-code-ranker-fr-principles-prompts`

The HTML report SHOULD support a principles-audit prompt mode that maps
the top coupling findings to the canonical principle corpus under
`languages/<language>/` (full corpora: `rust/`, `python/`, `typescript/`;
other languages fall back to the language-neutral `base/` corpus)
and instructs the LLM to audit the codebase against each principle.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`

## 2. Baseline Comparison — diff viewer (Step 4)

These are the `report --baseline` (human-facing HTML diff) requirements of
Step 4. The machine gate (`check --baseline`) is specified in
[`code-ranker-cli/PRD.md`](../code-ranker-cli/PRD.md) (`cpt-code-ranker-fr-compare`,
`cpt-code-ranker-fr-diff-text-report`).

### Graph Diff Engine

- [x] `p1` - **ID**: `cpt-code-ranker-fr-graph-diff`

> **Computed browser-side from two embedded snapshots.** `report --baseline`
> embeds both the baseline (`cs-baseline`) and current (`cs-current`) snapshots
> inline; the data-driven viewer (`diff.js` `computeDiff` / `computeCycles`)
> derives node/edge added / removed / affected status and per-side cycle status
> at load. There is **no** server-side structured count summary in the JSON (the
> old `compare_snapshots` engine is gone); the relative gate
> (`cpt-code-ranker-fr-compare`) is rule-based, not count-based.

With `--baseline <snapshot>`, `code-ranker report` computes a structured
diff between the baseline snapshot and the current `[input]`: nodes and
edges added, removed, or affected. The diff includes an overall
verdict: `improved`, `degraded`, or `neutral`. The interactive
diff HTML uses Graphviz WASM (bundled in the binary) for client-side
DOT→SVG layout. The viewer shows the `files` level by default; when the snapshot
carries extra levels (e.g. `functions`, emitted by `[levels] functions = true`) a
**level switcher** (Files / Functions tabs above the map) selects which level's
map + node table to show — the switcher is hidden for a single-level report, so
those look exactly as before. On a fresh load
the map opens a **default view**: the files tier, drilled through any single-folder
chain from the root to the first branching folder, at the node-budget reveal depth
(so it comes up usefully expanded rather than at a bare overview). The underlying
**group view** — one node per group (e.g. per-crate, from `ui.grouping.key`), with
deduped inter-group edges — is what the crate tier / overview shows. **Clicking a
group node drills into it**: the map re-renders showing only that group's files in
directory sub-clusters, plus — drawn as an overlay after layout — a **Fan-in**
(callers, green) section at the **top** and a **Fan-out** (dependencies, orange)
section at the **bottom**, each listing the neighbour **crates** as `crate (N)`
(N = that crate's files coupled to the focus). Each section is **collapsed by
default** to a single `Fan-in N` bar with a `+`; expanding (`+`) lays the crates out
full-width, wrapping down, with a `−`. Collapsing/expanding never moves the graph,
pan or zoom (room for the expanded grid is reserved up front). A crate's **arrows to
its coupled files** show on hover — hover a crate → its arrows, a file → the arrows
reaching it, a section's background → all of them; clicking a crate drills into its folder. **Clicking a box** drills the focus into
it (a crate, a folder, or — at the crate boundary — a whole crate). An
already-**expanded** folder/crate cluster is navigated only by its **name label** —
its background is inert (no point re-drilling into what is already open). A
single always-visible **breadcrumb** (top-left of the diagram) carries the
navigation: a **tier dropdown** anchor (its label opens the crates ⇄ files menu), a
**root** chip (`all`/`root`, returning to the overview), the clickable path chips
(e.g. `[crates ▾] › all › auth › domain`), and a trailing **lens chip**
(`⊟ depth N ⊞`) controlling how deep the focus is revealed — drilling in opens at
the node-budget depth (deepest reveal under ~20 nodes) and the focused view shows
files (in dir clusters) down to the revealed level plus deeper subfolders as
collapsed boxes. Per-chip counts (files under the
chip; crate/file total under the root) appear on hover. The tier, focus, and reveal
depth are reflected in URL parameters (`tier=`, `group=`, `depth=`) so browser
Back / Forward / Refresh work correctly; mode changes update the URL via
`replaceState`.
The map is laid out **once** from the **union** of both snapshots; the
`[data-side]` Baseline/Current buttons are a pure CSS visibility flip. This
extends to the **Fan-in / Fan-out** overlay sections of a drilled group:
their crate chips and arrows each hide on the side where that caller/dependency
does not exist, exactly like internal nodes/edges.
**Current is shown by default.** The display mode is controlled by **size-mode
buttons built from the snapshot's `ui.size`** — `■` (box/label mode) plus
one button per offered metric: the built-in `SLOC` / `HK`, and any a project adds
via `[report] size` — reflected in the `mode=` URL parameter. In a metric mode
**every node is a sized circle** — never a box; a built-in metric uses its
calibrated scale, a custom one scales against the median of the rendered
population. In group view
circles are sized by the aggregate value across the group's files
(`max(baselineAgg, currentAgg)`); in the drilled file view each revealed file
circle is resized to the active side's value around its fixed centre, and a
**collapsed folder** is itself a circle sized by the aggregate metric of the files
it hides (so the metric stays readable in both the crate and file tiers). The
active side is reflected in `side=baseline|current`, the node-table title, and
header badges. Header slots, review mode, snapshot swap, cycle annotation
(side-aware red stroke from backend `cycles`), internal blue nodes, and amber
external nodes behave as before. The node
table column order is: checkbox, Name, Kind, Cycle, Status, LOC, HK,
Fan-in, Fan-out, H.vol, H.bugs, H.effort, H.time, H.len, H.vocab,
Cyclomatic, Cognitive, MI, MI SEI, Logical, Comments, Blank. A checkbox column
(leftmost) enables persistent multi-node selection (shared across
Baseline/Current by node id — a file present in both snapshots stays selected
when toggling; the selected-row count reflects the active side): clicking a checkbox
highlights the row (yellow) and the corresponding SVG node (yellow fill
- amber stroke); shift-click selects a range; the header checkbox
selects or deselects all visible rows (indeterminate when partial).
Besides files the table also lists **folder** and **group (crate)** aggregate
rows (each numeric column summed over its files, a `Cycle` count, blank at 0);
kind-filter checkboxes next to the search (files / folders / crates, files +
crates on by default) toggle which appear, clicking an aggregate drills into it
on the map, and hovering/selecting a row lights up its map element and vice
versa. The totals footer shows only when the displayed rows are a single kind.
Selection also works directly on the map: **holding Shift** turns the main
diagram into a selection surface (the cursor changes over the SVG), and
Shift-clicking an SVG node toggles its selection — exactly like ticking its
table checkbox, kept in sync — instead of opening the modal. Holding the
**"open source" modifier** — **Alt/Option (⌥) on every platform** (chosen over
⌘, which clashed with copy/paste on macOS, and over Ctrl, which is right-click on
macOS) — likewise changes the cursor and turns a node click into "open source":
it opens the file on the project's git host (from `git.origin`) in a new tab
instead of the modal (project files only). While either modifier is held — or the
cursor hovers the right edge — the map's right-side controls (zoom and node-size)
and a bottom-left shortcut legend are revealed; the legend spells out the active
keys (⌥ Option on macOS, Alt elsewhere — the same gesture either way).
The modal popup opened by clicking a row or an SVG node is fullscreen
(locks body scroll). Its **header is a breadcrumb** of the file's location in the
same style as the map (tier dropdown → `all`/`root` → crate/folder chips → the
file): clicking a path chip drills the map there and closes the popup, while the
tier dropdown switches crates⇄files representation **without** closing — the same
file stays open and only its breadcrumb re-renders. Closing the popup (✕/Esc/
backdrop) after navigating between files lands the map in the **folder of the file
shown at close** when it differs from the folder the popup opened on; staying within
the same folder (or on the same file) leaves the map where it was. The popup
includes a synced selection checkbox, fields in
order id (⎘ copy) → path (⎘ copy, filename bold) → source (a link to the
file on the project's git host, built from `git.origin`; project files
only) → kind → visibility → items/methods → cycle info → status → metric
sections in a single column. Hover highlight (blue drop-shadow) takes CSS
priority over selection highlight. **Space** toggles the selection checkbox
while the popup is open. The popup's neighbourhood diagram stacks the
connections in **labelled blocks, 5 cards per row**: from the top — external
callers, one block per **other crate** that calls in (`crate in: <crate>`),
same-crate callers (`fan in`), the **central node**, same-crate dependencies
(`fan out`), one block per other crate it depends on (`crate out: <crate>`), and
external dependencies; per-crate blocks are ordered with the biggest nearest the
node. The fan-in/out **arrow points to the nearest internal block** — same-crate
when present, otherwise the closest `crate in/out` block — so it appears even when
the coupling is purely cross-crate; only the external blocks have no arrow. The
arrow **matches the colour of the block it points to** (blue same-crate, green
callers, orange dependencies) and is shown **dashed and unlabelled** when fan-in/out
is 0 (the file connects only through non-flow `contains`/`reexports` edges).
Cards are tinted
like the map's clusters — green for callers, yellow for dependencies — while
same-crate cards stay neutral and 3rd-party (external) cards are grey. It mirrors
the map's gestures — Shift-click toggles a node's selection, ⌘/Ctrl-click opens
its source — and shows the same yellow highlight for nodes already selected; the
external cards and arrows are inert (not selectable, no source, no
⌘-navigation). Hovering a neighbour card shows its file name, `crate:` and
`path:` (repo-relative, `/foo/bar`).

**Rationale**: The diff is the quantitative answer to "did my
refactoring reduce coupling?" Without it, the user must compare two
static visualizations manually.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`,
`cpt-code-ranker-actor-pr-reviewer`

### Diff HTML Report

- [x] `p1` - **ID**: `cpt-code-ranker-fr-diff-html-report`

`code-ranker report --baseline` generates a single self-contained
offline HTML report (named `…-diff.html`) displaying:

- Added / removed / affected files and edges, color-coded by per-node diff
  state (added, removed, affected, unchanged)
- Cycle detection: files/edges in dependency cycles annotated with
  `baseline-only` / `current-only` / `both` / `none` status and red-stroke
  highlighting
- `external` library nodes shown in a distinct amber colour with dashed
  edges to distinguish them from internal file edges
- Diff summary: a full-screen overlay (**toggled** by the header **stat**
  button — it stays highlighted while open and closes on a second click;
  the page header stays visible) of structural counts
  (files/folders/groups/edges/nodes-in-cycles) and per-file metric statistics
  (avg/min/p50/p90/max/sum, picked by a radio and persisted in the URL `stat=`),
  baseline vs current with Δ, downloadable as **JSON** or **Markdown**
- All JavaScript and CSS bundled locally (no CDN or external resources)

**Rationale**: Self-contained HTML is viewable without tooling and
suitable for attaching to PRs or sharing with stakeholders.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-tech-lead`,
`cpt-code-ranker-actor-pr-reviewer`

---

**Related docs**: [DESIGN.md](DESIGN.md) (viewer technical design / HTML assets) ·
main [PRD](../PRD.md) · [`code-ranker-cli/PRD.md`](../code-ranker-cli/PRD.md)

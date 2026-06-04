# PRD — Code Split Viewer (`code-split-viewer`)

The offline HTML viewer: the self-contained report `code-split report`
generates — single-snapshot visualization, client-side node sorting, the
Prompt Generator, and the browser-side baseline↔current diff. This is a
component slice of the product PRD — for the product overview, actors,
plugin/extraction layer, graph model and JSON schema, see the main
[PRD](../PRD.md); for the CLI command surface that produces these reports see
[`code-split-cli/PRD.md`](../code-split-cli/PRD.md).

<!-- toc -->

- [1. Visualization Reports — Step 2](#1-visualization-reports--step-2)
  - [HTML Report Generation](#html-report-generation)
  - [Node Sorting by Weight](#node-sorting-by-weight)
  - [AI Prompt Generator (P2)](#ai-prompt-generator-p2)
  - [Principles-Based Prompt Generation (P3)](#principles-based-prompt-generation-p3)
- [2. Baseline Comparison — diff viewer (Step 4)](#2-baseline-comparison--diff-viewer-step-4)
  - [Graph Diff Engine](#graph-diff-engine)
  - [Diff HTML Report](#diff-html-report)

<!-- /toc -->

## 1. Visualization Reports — Step 2

### HTML Report Generation

- [x] `p1` - **ID**: `cpt-code-split-fr-html-report`

The `code-split report` subcommand MUST analyze the workspace and, when the
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

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

### Node Sorting by Weight

- [x] `p1` - **ID**: `cpt-code-split-fr-node-sorting`

The HTML report MUST allow the user to sort files by coupling weight
(fan-in + fan-out edge count). The report MUST display the top-N
heaviest files prominently. Sorting MUST be performed client-side within
the HTML (no server required).

**Rationale**: The heaviest nodes are the most likely candidates for
refactoring. Surfacing them first reduces the time to actionable insight.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

### AI Prompt Generator (P2)

- [x] `p2` - **ID**: `cpt-code-split-fr-ai-prompts`

The HTML report SHOULD include a UI control that generates a prompt for
an LLM, pre-populated with the top-N heaviest nodes and their coupling
context, asking for refactoring recommendations. The prompt format MUST
be copyable as plain text for direct paste into any LLM interface.

The **same recommendation engine is exposed on the CLI** as two `report`
output formats, so the guidance is reachable without opening the HTML
(driven from the snapshot's calibrated `node_attributes[*].thresholds`
`info` / `warning` tiers — advisory, never a gate):

- `--output.prompt[.path]` — the LLM prompt for **one** principle, the same
  Markdown the HTML Prompt Generator produces (intent, summary, principle-doc
  link, a task checklist, the ranked offending modules, and the principle's
  connection lists). Defaults to a per-principle file
  `.code-split/{ts}-{git-hash-3}-{preset}.md` (or `stdout`).
- `--output.scorecard[.path]` — a console **triage** overview (a per-principle
  table of `warning` / `info` counts + the worst module, then the worst modules
  overall, then a hint to the prompt for the worst principle). Defaults to
  `stdout`.

Both share three flags: `--preset <ID>` (a principle from the snapshot's
`presets`; **optional** — when omitted the principle with the most violations
is chosen), `--severity <info|warning|auto>` (the tier; repeatable for the
scorecard, single for the prompt; `auto` = warning-if-any-else-info), and
`--top <N>` (how many modules; `--top 1` = the single worst). These flags apply
only with a `prompt` / `scorecard` format; an explicit `--index` is rejected
with a hint to use `--top`.

The CLI side of this engine is documented in
[`code-split-cli/DESIGN.md`](../code-split-cli/DESIGN.md#code-split-cli-recommendation-engine)
(`cpt-code-split-component-recommend`).

**Rationale**: Connecting structural data to an LLM's reasoning closes
the loop between measurement and advice without coupling the offline
tool to a specific LLM provider. The CLI surface lets an agent or a CI
step pull the same prompt/triage non-interactively.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

### Principles-Based Prompt Generation (P3)

- [x] `p3` - **ID**: `cpt-code-split-fr-principles-prompts`

The HTML report SHOULD support a principles-audit prompt mode that maps
the top coupling findings to the canonical principle corpus under
`principles/<language>/` (currently `rust/`, `python/`, `typescript/`)
and instructs the LLM to audit the codebase against each principle.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

## 2. Baseline Comparison — diff viewer (Step 4)

These are the `report --baseline` (human-facing HTML diff) requirements of
Step 4. The machine gate (`check --baseline`) is specified in
[`code-split-cli/PRD.md`](../code-split-cli/PRD.md) (`cpt-code-split-fr-compare`,
`cpt-code-split-fr-diff-text-report`).

### Graph Diff Engine

- [x] `p1` - **ID**: `cpt-code-split-fr-graph-diff`

> **Computed browser-side from two embedded snapshots.** `report --baseline`
> embeds both the baseline (`cs-baseline`) and current (`cs-current`) snapshots
> inline; the data-driven viewer (`diff.js` `computeDiff` / `computeCycles`)
> derives node/edge added / removed / affected status and per-side cycle status
> at load. There is **no** server-side structured count summary in the JSON (the
> old `compare_snapshots` engine is gone); the relative gate
> (`cpt-code-split-fr-compare`) is rule-based, not count-based.

With `--baseline <snapshot>`, `code-split report` computes a structured
diff between the baseline snapshot and the current `[input]`: nodes and
edges added, removed, or affected. The diff includes an overall
verdict: `improved`, `degraded`, or `neutral`. The interactive
diff HTML uses Graphviz WASM (bundled in the binary) for client-side
DOT→SVG layout with directory cluster grouping; there is a single Files
view (no level switcher). The map is laid out **once** from the **union**
of both snapshots (Graphviz computes a single set of node positions); the
`[data-side]` Baseline/Current buttons are then a pure CSS visibility flip —
current-only (added) elements are hidden on the Baseline side, baseline-only
(removed) elements on the Current side — so every file present on both sides
keeps its exact position and never moves when toggling. **Current is shown by
default.** In the metric node-size modes (SLOC/HK) each circle is resized
to the active side's value around its fixed centre (a file that grew or
shrank changes size, not position). The active side is reflected
throughout: the `side=baseline|current` URL parameter, the node-table title
(`Details` / `Details Baseline` / `Details Current`), and a `Baseline` /
`Current` badge on the node-popup and Prompt-Generator headers. The two header
slots are the **current** (right) — the primary snapshot the report is
about, always present and **not removable** — and an optional **baseline**
(left, editable, removable). With no baseline it is
single-snapshot **review** mode: the baseline slot is an empty, editable
placeholder (`↑ Set baseline`) and the Baseline/Current buttons are hidden;
loading a baseline turns the report into a diff. Each header slot's hover
tooltip is labelled `Baseline` / `Current` and notes which side is currently
shown; that slot is also highlighted in the header. Two buttons swap in a
different snapshot from disk (each accepts a `.json` snapshot or an `.html`
report): **↑ Replace current** changes the evaluated snapshot, **↑ Set
baseline** loads a reference to diff against. Cycle membership comes from each
snapshot's backend-computed `cycles` (Kosaraju SCC over flow edges; the viewer
does not re-detect cycles); the viewer derives the per-side status and annotates
nodes/edges for red-stroke highlighting (solid red, no dasharray). The highlight
is **side-aware** —
a `baseline-only` cycle is red only on the Baseline side, `current-only` only
on Current, `both` on either, so a cycle removed in the current snapshot
stops being red once you switch to Current. Internal `file` nodes render
blue; `external` library nodes render amber with dashed edges. The node
table column order is: checkbox, Name, Kind, Cycle, Status, LOC, HK,
Fan-in, Fan-out, H.vol, H.bugs, H.effort, H.time, H.len, H.vocab,
Cyclomatic, Cognitive, MI, MI SEI, Logical, Comments, Blank. A checkbox column
(leftmost) enables persistent multi-node selection (shared across
Baseline/Current by node id — a file present in both snapshots stays selected
when toggling; the selected-row count reflects the active side): clicking a checkbox
highlights the row (yellow) and the corresponding SVG node (yellow fill
- amber stroke); shift-click selects a range; the header checkbox
selects or deselects all visible rows (indeterminate when partial).
Selection also works directly on the map: **holding Shift** turns the main
diagram into a selection surface (the cursor changes over the SVG), and
Shift-clicking an SVG node toggles its selection — exactly like ticking its
table checkbox, kept in sync — instead of opening the modal. Holding the
**"open source" modifier** — **⌘ on macOS, Ctrl elsewhere** (Ctrl is left
alone on macOS, where it maps to right-click) — likewise changes the cursor
and turns a node click into "open source": it opens the file on the project's
git host (from `git.origin`) in a new tab instead of the modal (project files
only). While either modifier is held — or the cursor hovers the right edge — the map's
right-side controls (zoom and node-size) and a bottom-left shortcut legend are
revealed; the legend spells out the active keys for the platform (⌘ on macOS,
Ctrl elsewhere).
The modal popup opened by clicking a row or an SVG node is fullscreen
(locks body scroll); it includes a synced selection checkbox, fields in
order id (⎘ copy) → path (⎘ copy, filename bold) → source (a link to the
file on the project's git host, built from `git.origin`; project files
only) → kind → visibility → items/methods → cycle info → status → metric
sections in a single column. Hover highlight (blue drop-shadow) takes CSS
priority over selection highlight. **Space** toggles the selection checkbox
while the popup is open. The popup's neighbourhood diagram mirrors the map's
gestures — Shift-click toggles a node's selection, ⌘/Ctrl-click opens its
source — and shows the same yellow highlight for nodes already selected; its
3rd-party (external) cards and arrows are drawn grey and are inert (not
selectable, no source, no ⌘-navigation).

**Rationale**: The diff is the quantitative answer to "did my
refactoring reduce coupling?" Without it, the user must compare two
static visualizations manually.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`,
`cpt-code-split-actor-pr-reviewer`

### Diff HTML Report

- [x] `p1` - **ID**: `cpt-code-split-fr-diff-html-report`

`code-split report --baseline` generates a single self-contained
offline HTML report (named `…-diff.html`) displaying:

- Added / removed / affected files and edges, color-coded by per-node diff
  state (added, removed, affected, unchanged)
- Cycle detection: files/edges in dependency cycles annotated with
  `baseline-only` / `current-only` / `both` / `none` status and red-stroke
  highlighting
- `external` library nodes shown in a distinct amber colour with dashed
  edges to distinguish them from internal file edges
- Diff summary table: node/edge counts and cycle counts (SCCs, nodes in
  cycles), baseline vs current with Δ
- All JavaScript and CSS bundled locally (no CDN or external resources)

**Rationale**: Self-contained HTML is viewable without tooling and
suitable for attaching to PRs or sharing with stakeholders.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`,
`cpt-code-split-actor-pr-reviewer`

---

**Related docs**: [DESIGN.md](DESIGN.md) (viewer technical design / HTML assets) ·
main [PRD](../PRD.md) · [`code-split-cli/PRD.md`](../code-split-cli/PRD.md)

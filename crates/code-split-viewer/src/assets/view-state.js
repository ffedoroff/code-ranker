// view-state.js — which snapshot side is shown and how the map/tables reflect
// it: side accessors (activeSnap/viewMode/activeGraph/unionGraph), per-side
// visibility + metric sizing, side toggle, recomputeAll, renderView, and
// applyViewState (hoisted out of the former app.js DOMContentLoaded closure).

// Which snapshot the diagrams / tables / modal show: 'baseline' or 'current'.
// In review mode (no baseline) it is always 'baseline'.
// `current` is the primary snapshot (the report's current state, normally always
// present); `baseline` is the optional baseline. activeSnap returns whichever the
// shown side has, falling back to the other so a single-snapshot report works.
function activeSnap() {
  return window.viewSide === 'baseline'
    ? (window.BASELINE ?? window.CURRENT)
    : (window.CURRENT ?? window.BASELINE);
}

// The three view modes: 'review' (no baseline → single "view"), or, in a
// diff, 'baseline' / 'current' depending on which side the user is looking at. This
// is the single source of truth for the labels/headers/URL across the viewer.
function viewMode() {
  if (!window.BASELINE || !window.CURRENT) return 'review';   // only one snapshot loaded
  return window.viewSide === 'current' ? 'current' : 'baseline';
}
// Label suffix for the active side: ' Baseline' / ' Current' in a diff, '' in review.
function viewModeSuffix() {
  const m = viewMode();
  return m === 'current' ? ' Current' : m === 'baseline' ? ' Baseline' : '';
}
function activeGraph(level) {
  return activeSnap()?.graphs?.[level] || { nodes: [], edges: [] };
}

// The graph drawn on the main map. External (3rd-party library) nodes and the
// edges into them are dropped here — they would clutter the file map. They are
// still kept in the snapshot and shown in the per-node neighbourhood modal.
function activeLocalGraph(level) {
  const g = activeGraph(level);
  const nodes = g.nodes.filter(n => !isExternalNode(n, level));
  const ids = new Set(nodes.map(n => n.id));
  const edges = g.edges.filter(e => ids.has(e.source) && ids.has(e.target));
  return { nodes, edges };
}

// The graph the main map is *laid out* from: the union of baseline+current (the diff
// graph), which is already external-free and carries a per-element `status`
// (added / removed / unchanged / affected). Laying out the union ONCE — then
// merely hiding the other side's added/removed elements via CSS — keeps every
// node that exists on both sides pinned in place: toggling Baseline/Current no longer
// reflows the graph, only the genuinely added/removed parts appear or disappear.
// (In review mode the diff is baseline-vs-baseline, so everything is `unchanged`.)
function unionGraph(level) {
  return window.DIFF?.[level] || { nodes: [], edges: [] };
}

// Flip which side's exclusive elements are visible on a frame, without relayout.
// `baseline` hides current-only (added) elements; `current` hides baseline-only (removed)
// ones; review keeps the lot. Drives the `.hide-{nodes,edges}-{added,removed}`
// CSS rules already defined for the frame.
function applySideVisibility(frame) {
  if (!frame) return;
  frame.classList.remove('hide-nodes-added', 'hide-edges-added',
                         'hide-nodes-removed', 'hide-edges-removed',
                         'side-baseline', 'side-current');
  const m = viewMode();
  if (m === 'baseline')     frame.classList.add('hide-nodes-added', 'hide-edges-added');
  else if (m === 'current') frame.classList.add('hide-nodes-removed', 'hide-edges-removed');
  // The `side-*` marker gates cycle highlighting (a `baseline-only` cycle is red
  // only on Baseline, `current-only` only on Current, `both` on either) and follows the
  // side actually shown — in review the single snapshot's cycles are all `both`.
  frame.classList.add(window.viewSide === 'current' ? 'side-current' : 'side-baseline');
}

// In the metric size modes (loc/hk), resize each circle to the *active side's*
// value while keeping the union-layout centre — so toggling Baseline/Current changes
// sizes (a file that grew/shrank) without ever moving a node. Default mode draws
// fixed boxes, identical on both sides, so it needs no per-side resize.
function applySideSizing(frame, level) {
  if (!frame) return;
  const sizeMode  = window.nodeSizeMode  || null;
  // Per-side circle resize only applies to metric modes in the drilled file view.
  if (!sizeMode || (window.drillGroup || null) === null) return;
  const mode = sizeMode;
  const byId = new Map((activeSnap()?.graphs?.[level]?.nodes || []).map(n => [n.id, n]));
  frame.querySelectorAll('g.node').forEach(g => {
    const n   = byId.get(g.dataset.nodeId);
    const ell = g.querySelector('ellipse');
    if (!n || !ell) return;   // node absent on this side → leave it (it's hidden)
    const cx = parseFloat(ell.getAttribute('cx'));
    const cy = parseFloat(ell.getAttribute('cy'));
    const d  = metricNodeDiam(n, mode);
    const r  = (d * 36).toFixed(2);     // graphviz: radius(pt) = diameter(in) × 72 / 2
    ell.setAttribute('rx', r);
    ell.setAttribute('ry', r);
    const txt = g.querySelector('text');
    if (txt) {
      const fs = metricFontSize(d);
      const v  = metricNodeVal(n, mode);
      txt.setAttribute('font-size', fs.toFixed(2));
      txt.setAttribute('x', cx);
      txt.setAttribute('y', (cy + fs * 0.3).toFixed(2));   // baseline ≈ centre + 0.3·fs
      txt.textContent = v > 0 ? fmtMetricShort(v) : '';
    }
  });
}

// Toggle Baseline/Current. The map is a single shared (union) layout, so switching
// sides is just a visibility flip — no relayout, no pan/zoom reset, and nodes
// present on both sides never move. Only the tables (active side) and the
// warning count are refreshed.
function setViewSide(side) {
  if (side === window.viewSide
      || (side === 'current'  && !window.CURRENT)
      || (side === 'baseline' && !window.BASELINE)) return;
  window.viewSide = side;
  window.navSetSide?.();
  // Clear stale hover highlights: refreshing the table below replaces the hovered
  // row without firing its `mouseleave`, which would otherwise leave its map node
  // lit — and a second hover would then light a second node.
  document.querySelectorAll('g.node.node-hl').forEach(n => n.classList.remove('node-hl'));
  document.querySelectorAll('.row-hl').forEach(r => r.classList.remove('row-hl'));
  // Active-side feedback is the highlighted control (snap-active); the old
  // Baseline/Current nav buttons are gone.
  document.querySelectorAll('.view').forEach(sec => {
    const frame = sec.querySelector('.svg-frame');
    applySideVisibility(frame);
    applySideSizing(frame, sec.dataset.view);
    sec._refreshNodeTable?.();
  });
  updateWarnCount();
  updateActiveSnapGroup();
  // If a node modal is open, re-render it for the new side so its diagram and
  // fields follow the toggle (otherwise only the map/tables/URL would update).
  const m = window._modalNode;
  if (m && document.getElementById('node-modal-overlay')?.style.display === 'flex')
    window.openModalForNode?.(m.id, m.level);
}

// The header "toggle" button and the `t` hotkey switch baseline⇄current. A no-op
// in review (a single snapshot — nothing to toggle).
function toggleViewSide() {
  if (!window.BASELINE || !window.CURRENT) return;
  setViewSide(window.viewSide === 'current' ? 'baseline' : 'current');
}

// Recompute everything after the user swaps a snapshot via the upload controls.
function recomputeAll() {
  const EMPTY = { graphs: {} };
  const baseline = window.BASELINE;
  const current  = window.CURRENT;
  // Either side may be absent (review). Fall back the missing side to the present
  // one so the diff is against itself (everything "unchanged"), never against empty.
  window.DIFF   = computeDiff(baseline ?? current ?? EMPTY, current ?? baseline ?? EMPTY);
  window.CYCLES = computeCycles(baseline ?? current ?? EMPTY, current ?? baseline ?? EMPTY);
  window.META   = computeMeta(baseline, current);

  buildSummary();
  updateFilesTab();

  // Reset drill + zoom state and rendered flags for all views. The node set
  // changed, so drop the memoised crate-root grouping cache.
  window.drillGroup = null;
  window.dig       = 0;
  window.drillDig  = 0;
  window.clearGroupingCache?.();
  document.querySelectorAll('.drill-breadcrumb').forEach(bc => { bc.style.display = 'none'; });
  document.querySelectorAll('.dig-lod').forEach(el => el.style.removeProperty('display'));
  document.querySelectorAll('.svg-frame').forEach(f => { delete f.dataset.bigConfirmed; });
  document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });

  updateHeader();
  window.updateDigLabel?.();

  // Re-render active view
  const active = document.querySelector('.view.active');
  if (active && window.gv) renderView(active);
}

function renderView(section, opts = {}) {
  const level   = section.dataset.view;
  const frame   = section.querySelector('.svg-frame');
  const loading = section.querySelector('.loading-indicator');

  // Plain count of distinct warning types next to the Prompt-Generator (AI) button.
  updateWarnCount();

  // Preserve pan/zoom across a re-render (a size-mode switch — Baseline/Current no
  // longer relayouts). Size modes have different coordinate extents, so we carry
  // the view as *relative* zoom + fractional centre (vs each layout's fit-all
  // viewBox) rather than absolute coords — otherwise the framing drifts.
  let viewSpec = null, wasZoomed = false;
  if (opts.preserve) {
    const cur = frame.querySelector('svg')?.getAttribute('viewBox');
    const nat = frame.dataset.naturalVB;
    if (cur && nat && cur !== nat) {
      const [cx, cy, cw, ch] = cur.split(/[ ,]+/).map(Number);
      const [ox, oy, ow, oh] = nat.split(/[ ,]+/).map(Number);
      if (ow > 0 && oh > 0) {
        viewSpec = {
          zw: cw / ow, zh: ch / oh,
          fx: (cx + cw / 2 - ox) / ow,
          fy: (cy + ch / 2 - oy) / oh,
        };
        wasZoomed = frame.classList.contains('zoomed');
      }
    }
  }

  if (loading) { loading.textContent = 'Computing layout…'; loading.classList.add('on'); }
  setTimeout(() => {
    // Lay out the union graph once; the active side is shown via CSS visibility.
    const g = unionGraph(level);
    drawSVG(frame, g.nodes, g.edges, level);
    applySideVisibility(frame);
    applySideSizing(frame, level);
    window._ntSelected?.[level]?.forEach(id => section._gNodeMap?.get(id)?.classList.add('node-selected'));
    if (viewSpec) {
      const svg = frame.querySelector('svg');
      const [ox, oy, ow, oh] = (frame.dataset.naturalVB || '0 0 0 0').split(/[ ,]+/).map(Number);
      if (svg && ow > 0 && oh > 0) {
        const nw = viewSpec.zw * ow, nh = viewSpec.zh * oh;
        const cx = ox + viewSpec.fx * ow, cy = oy + viewSpec.fy * oh;
        svg.setAttribute('viewBox', `${cx - nw / 2} ${cy - nh / 2} ${nw} ${nh}`);
        if (wasZoomed) frame.classList.add('zoomed');
      }
    }
    section.dataset.rendered = 'true';
    section._refreshNodeTable?.();
    // drawSVG just refreshed window._FOCUS — sync the dig control to it (its focus
    // bounds depend on the rendered group's folder depth).
    window.updateDigLabel?.(level);
    if (loading) loading.classList.remove('on');
  }, 30);
}

// Helper: apply group+mode from a state object, update UI and re-render if needed.
function applyViewState(st, { rerender = false } = {}) {
  const grp  = st.group || null;
  const mode = st.mode  || null;
  const dig = st.dig != null ? (Number(st.dig) | 0) : 0;
  let changed = false;
  if ((window.dig || 0)  !== dig) { window.dig          = dig; changed = true; }
  if (window.drillGroup   !== grp)  { window.drillGroup   = grp;  window.drillDig = grp ? dig : 0; changed = true; }
  if (window.nodeSizeMode !== mode) { window.nodeSizeMode = mode; changed = true; }
  // Sync breadcrumb (focus) and the relative-zoom control (overview only).
  const lvl = st.level ?? currentLevel();
  window.renderBreadcrumb?.(lvl);
  // Overview: the standalone dig control. Focus: it is hidden — the +/- collapse
  // control lives inside the breadcrumb instead (renderBreadcrumb).
  document.querySelectorAll('.dig-lod').forEach(el => { el.style.display = grp ? 'none' : ''; });
  window.updateDigLabel?.(lvl);
  // Sync metric buttons
  document.querySelectorAll('.size-row[data-row="metric"] .size-mode-btn').forEach(b => {
    const bMode = b.dataset.size === 'dot' ? null : b.dataset.size;
    b.classList.toggle('active', bMode === mode);
  });
  if ((changed || rerender) && window.gv) {
    document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
    const active = document.querySelector('.view.active');
    if (active) renderView(active, { preserve: false });
  }
}

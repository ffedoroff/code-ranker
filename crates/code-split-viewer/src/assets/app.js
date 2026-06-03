// Which snapshot the diagrams / tables / modal show: 'before' or 'after'.
// In review mode (no after snapshot) it is always 'before'.
// `after` is the primary snapshot (the report's current state, normally always
// present); `before` is the optional baseline. activeSnap returns whichever the
// shown side has, falling back to the other so a single-snapshot report works.
function activeSnap() {
  return window.viewSide === 'before'
    ? (window.BEFORE ?? window.AFTER)
    : (window.AFTER ?? window.BEFORE);
}

// The three view modes: 'review' (no after snapshot → single "view"), or, in a
// diff, 'before' / 'after' depending on which side the user is looking at. This
// is the single source of truth for the labels/headers/URL across the viewer.
function viewMode() {
  if (!window.BEFORE || !window.AFTER) return 'review';   // only one snapshot loaded
  return window.viewSide === 'after' ? 'after' : 'before';
}
// Label suffix for the active side: ' Baseline' / ' Current' in a diff, '' in review.
function viewModeSuffix() {
  const m = viewMode();
  return m === 'after' ? ' Current' : m === 'before' ? ' Baseline' : '';
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

// The graph the main map is *laid out* from: the union of before+after (the diff
// graph), which is already external-free and carries a per-element `status`
// (added / removed / unchanged / affected). Laying out the union ONCE — then
// merely hiding the other side's added/removed elements via CSS — keeps every
// node that exists on both sides pinned in place: toggling Before/After no longer
// reflows the graph, only the genuinely added/removed parts appear or disappear.
// (In review mode the diff is before-vs-before, so everything is `unchanged`.)
function unionGraph(level) {
  return window.DIFF?.[level] || { nodes: [], edges: [] };
}

// Flip which side's exclusive elements are visible on a frame, without relayout.
// `before` hides after-only (added) elements; `after` hides before-only (removed)
// ones; review keeps the lot. Drives the `.hide-{nodes,edges}-{added,removed}`
// CSS rules already defined for the frame.
function applySideVisibility(frame) {
  if (!frame) return;
  frame.classList.remove('hide-nodes-added', 'hide-edges-added',
                         'hide-nodes-removed', 'hide-edges-removed',
                         'side-before', 'side-after');
  const m = viewMode();
  if (m === 'before')     frame.classList.add('hide-nodes-added', 'hide-edges-added');
  else if (m === 'after') frame.classList.add('hide-nodes-removed', 'hide-edges-removed');
  // The `side-*` marker gates cycle highlighting (a `before-only` cycle is red
  // only on Before, `after-only` only on After, `both` on either) and follows the
  // side actually shown — in review the single snapshot's cycles are all `both`.
  frame.classList.add(window.viewSide === 'after' ? 'side-after' : 'side-before');
}

// In the metric size modes (loc/hk), resize each circle to the *active side's*
// value while keeping the union-layout centre — so toggling Before/After changes
// sizes (a file that grew/shrank) without ever moving a node. Default mode draws
// fixed boxes, identical on both sides, so it needs no per-side resize.
function applySideSizing(frame, level) {
  if (!frame) return;
  const mode = window.nodeSizeMode || 'default';
  if (mode === 'default') return;
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

// Toggle Before/After. The map is a single shared (union) layout, so switching
// sides is just a visibility flip — no relayout, no pan/zoom reset, and nodes
// present on both sides never move. Only the tables (active side) and the
// warning count are refreshed.
function setViewSide(side) {
  if (side === window.viewSide
      || (side === 'after'  && !window.AFTER)
      || (side === 'before' && !window.BEFORE)) return;
  window.viewSide = side;
  window.navSetSide?.();
  document.querySelectorAll('[data-side]').forEach(b => b.classList.toggle('active', b.dataset.side === side));
  document.querySelectorAll('.view').forEach(sec => {
    const frame = sec.querySelector('.svg-frame');
    applySideVisibility(frame);
    applySideSizing(frame, sec.dataset.view);
    sec._refreshNodeTable?.();
  });
  updateWarnCount();
  updateActiveSnapGroup();
}

// Refresh the distinct-warning-type count next to the Prompt-Generator button
// for the active level (it tracks the active side).
function updateWarnCount() {
  const warnEl = document.getElementById('nav-warn-count');
  if (!warnEl) return;
  const n = window.warningTypeCount?.(currentLevel()) ?? 0;
  warnEl.textContent = n ? String(n) : '';
}

function updateHeader() {
  const meta      = window.META;
  const hasBefore = meta.before !== null;   // an optional baseline is loaded
  const hasAfter  = meta.after  !== null;   // the primary snapshot (normally always present)
  const isReview  = !hasBefore;             // no baseline → single-snapshot "review"

  document.body.classList.toggle('mode-review', isReview);
  document.getElementById('title').textContent = `${meta.target} — ${isReview ? 'review' : 'diff'}`;

  // BEFORE = optional baseline: left, editable, removable; empty in review.
  document.getElementById('meta-before-date').textContent   = hasBefore && meta.before.date ? fmtDate(meta.before.date) + ' ' : '';
  document.getElementById('meta-before-name').textContent   = hasBefore ? meta.before.name : '';
  document.getElementById('meta-before-commit').textContent = hasBefore && meta.before.commit ? ` ${meta.before.commit}` : '';
  document.getElementById('meta-before-info').style.display = hasBefore && (meta.before.name || meta.before.commit) ? '' : 'none';
  document.getElementById('btn-upload-before').textContent   = hasBefore ? '↑ Replace baseline' : '↑ Set baseline';
  document.getElementById('btn-remove-before').style.display = hasBefore ? '' : 'none';

  // AFTER = the primary snapshot the report is about: always shown, not removable.
  document.getElementById('meta-after-date').textContent    = hasAfter && meta.after.date ? fmtDate(meta.after.date) + ' ' : '';
  document.getElementById('meta-after-name').textContent    = hasAfter ? meta.after.name : '';
  document.getElementById('meta-after-commit').textContent  = hasAfter && meta.after.commit ? ` ${meta.after.commit}` : '';
  document.getElementById('meta-after-info').style.display  = hasAfter && (meta.after.name || meta.after.commit) ? '' : 'none';
  document.getElementById('meta-arrow').style.display       = '';   // (before slot) → after, always

  // Before/After toggle only when both sides exist; review shows the single
  // (after) snapshot.
  if (isReview) window.viewSide = 'after';
  document.querySelectorAll('[data-side]').forEach(b => {
    b.style.display = hasBefore ? '' : 'none';
    b.classList.toggle('active', b.dataset.side === window.viewSide);
  });
  updateActiveSnapGroup();
}

// Files is the only graph level — nothing to toggle. Kept as a no-op so callers
// (app bootstrap, snapshot swap) need no changes.
function updateFilesTab() {}

// Recompute everything after the user swaps a snapshot via the upload controls.
function recomputeAll() {
  const EMPTY = { graphs: {} };
  const before = window.BEFORE;
  const after  = window.AFTER;
  // Either side may be absent (review). Fall back the missing side to the present
  // one so the diff is against itself (everything "unchanged"), never against empty.
  window.DIFF   = computeDiff(before ?? after ?? EMPTY, after ?? before ?? EMPTY);
  window.CYCLES = computeCycles(before ?? after ?? EMPTY, after ?? before ?? EMPTY);
  window.META   = computeMeta(before, after);

  buildSummary();
  updateFilesTab();

  // Reset rendered state for all views; the active one re-renders below.
  document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });

  updateHeader();

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

  // Preserve pan/zoom across a re-render (a size-mode switch — Before/After no
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
    if (loading) loading.classList.remove('on');
  }, 30);
}

function buildSnapPopupHTML(snap, refSnap, sideLabel) {
  if (!snap) return '';
  const sections = [];

  // Which snapshot this is (Before / After) and whether it is the one currently
  // shown on the map / tables.
  if (sideLabel) {
    const shown = sideLabel.toLowerCase() === window.viewSide;
    sections.push(`<div class="sp-side ${shown ? 'sp-side-shown' : ''}">${sideLabel}${shown ? ' · currently shown' : ''}</div>`);
  }

  // General
  const genRows = [];
  if (snap.generated_at) {
    let dateStr = fmtDate(snap.generated_at);
    if (refSnap?.generated_at) {
      const diffMs = new Date(snap.generated_at) - new Date(refSnap.generated_at);
      dateStr += ` <span class="sp-gen-diff">(diff ${fmtDuration(Math.abs(diffMs))})</span>`;
    }
    genRows.push(`<div class="sp-row"><span class="sp-lbl">Generated</span><span>${dateStr}</span></div>`);
  }
  if (snap.command)
    genRows.push(`<div class="sp-cmd-block"><div class="sp-cmd-bar"><span class="sp-lbl">Command</span><button class="sp-copy-btn" data-copy="${escAttr(snap.command)}" title="Copy">⧉</button></div><textarea class="sp-cmd-input" readonly rows="3">${escHtml(snap.command)}</textarea></div>`);
  if (genRows.length)
    sections.push(`<div class="sp-section-label">General</div>${genRows.join('')}`);

  // Git
  if (snap.git) {
    const { branch, commit, dirty_files } = snap.git;
    const gitRows = [];
    if (branch)              gitRows.push(`<div class="sp-row"><span class="sp-lbl">Branch</span><span>${escHtml(branch)}</span></div>`);
    if (commit)              gitRows.push(`<div class="sp-row"><span class="sp-lbl">Commit hash</span><span>${escHtml(commit)}</span></div>`);
    if (dirty_files != null) gitRows.push(`<div class="sp-row"><span class="sp-lbl">Dirty files</span><span>${dirty_files > 0 ? dirty_files : '0 (clean)'}</span></div>`);
    if (gitRows.length)
      sections.push(`<div class="sp-section-label">Git</div>${gitRows.join('')}`);
  }

  // Duration
  if (snap.timings?.length) {
    const trows = snap.timings.map(t =>
      `<div class="sp-row"><span class="sp-lbl">${escHtml(t.stage)}</span><span>${fmtMs(t.ms)}</span></div>`
    ).join('');
    sections.push(`<div class="sp-section-label">Duration</div>${trows}`);
  }

  return sections.join('');
}

function setupSnapPopup() {
  let popup = null, hideTimer = null;

  function getPopup() {
    if (!popup) {
      popup = document.createElement('div');
      popup.id = 'snap-popup';
      document.body.appendChild(popup);
      popup.addEventListener('mouseenter', () => { clearTimeout(hideTimer); hideTimer = null; });
      popup.addEventListener('mouseleave', scheduleHide);
    }
    return popup;
  }

  function scheduleHide() {
    hideTimer = setTimeout(() => { if (popup) popup.style.display = 'none'; }, 150);
  }

  function show(snap, anchor, refSnap, sideLabel) {
    if (!snap) return;
    const p = getPopup();
    p.innerHTML = buildSnapPopupHTML(snap, refSnap, sideLabel);
    p.querySelectorAll('.sp-copy-btn').forEach(btn => {
      btn.addEventListener('click', e => {
        e.stopPropagation();
        navigator.clipboard?.writeText(btn.dataset.copy).then(() => {
          btn.textContent = '✓';
          setTimeout(() => { btn.textContent = '⧉'; }, 1500);
        });
      });
    });
    p.style.display = 'block';
    requestAnimationFrame(() => {
      const r  = anchor.getBoundingClientRect();
      let left = r.left;
      let top  = r.bottom + 6;
      if (left + p.offsetWidth  > window.innerWidth  - 8) left = window.innerWidth  - p.offsetWidth  - 8;
      if (top  + p.offsetHeight > window.innerHeight - 8) top  = r.top - p.offsetHeight - 6;
      p.style.left = left + 'px';
      p.style.top  = top  + 'px';
    });
  }

  document.querySelectorAll('.snap-group').forEach((grp, i) => {
    grp.addEventListener('mouseenter', () => {
      clearTimeout(hideTimer);
      const snap = i === 0 ? window.BEFORE : window.AFTER;
      const ref  = i === 1 ? window.BEFORE : null;
      show(snap, grp, ref, i === 0 ? 'Baseline' : 'Current');
    });
    grp.addEventListener('mouseleave', scheduleHide);
  });
}

// Mark the header snapshot group (the file uploader at the top) for the side
// currently shown, so the active input is visually distinguished. Hidden in
// review mode (a single snapshot — nothing to distinguish).
function updateActiveSnapGroup() {
  const active = window.AFTER ? window.viewSide : null;
  document.querySelectorAll('.snap-group[data-snap]').forEach(grp => {
    grp.classList.toggle('snap-active', grp.dataset.snap === active);
  });
}

// Read a snapshot embedded inline in this page (a `<script type="application/json">` tag).
function readEmbeddedSnapshot(id) {
  const el = document.getElementById(id);
  if (!el) return null;
  const t = el.textContent.trim();
  return t && t !== 'null' ? JSON.parse(t) : null;
}

// Parse a snapshot from uploaded file text — either a raw JSON snapshot, or a code-split
// HTML report with the snapshot embedded (prefer `cs-current`, else `cs-baseline`).
function extractSnapshotFromText(text) {
  const s = text.trim();
  if (s.startsWith('{')) return JSON.parse(s);
  const doc = new DOMParser().parseFromString(text, 'text/html');
  const read = id => {
    const t = doc.getElementById(id)?.textContent?.trim();
    return t && t !== 'null' ? JSON.parse(t) : null;
  };
  return read('cs-current') || read('cs-baseline');
}

// Manual snapshot swap: load a different before/after snapshot (.json or .html) into the viewer.
function setupFileControls() {
  const inputBefore = document.getElementById('input-before');
  const inputAfter  = document.getElementById('input-after');

  document.getElementById('btn-upload-before').addEventListener('click', () => inputBefore.click());
  document.getElementById('btn-upload-after').addEventListener('click',  () => inputAfter.click());

  // Removing the baseline (before) drops back to single-snapshot review; the
  // after (primary) snapshot has no remove control.
  document.getElementById('btn-remove-before').addEventListener('click', () => {
    window.BEFORE = null;
    recomputeAll();
  });

  inputBefore.addEventListener('change', () => {
    const file = inputBefore.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = e => {
      try { window.BEFORE = extractSnapshotFromText(e.target.result); } catch { alert('Invalid snapshot file'); return; }
      recomputeAll();
    };
    reader.readAsText(file);
    inputBefore.value = '';
  });

  inputAfter.addEventListener('change', () => {
    const file = inputAfter.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = e => {
      try { window.AFTER = extractSnapshotFromText(e.target.result); } catch { alert('Invalid snapshot file'); return; }
      recomputeAll();
    };
    reader.readAsText(file);
    inputAfter.value = '';
  });
}

function setupGlobalControls() {
  document.querySelectorAll('[data-side]').forEach(btn => {
    btn.addEventListener('click', () => setViewSide(btn.dataset.side));
  });

  document.querySelectorAll('.report-switch a[data-view]').forEach(a => {
    a.addEventListener('click', e => {
      e.preventDefault();
      switchToLevel(a.dataset.view);
      window.navPush(a.dataset.view, null);
    });
  });
}

document.addEventListener('DOMContentLoaded', async () => {
  window.nodeSizeMode = 'default';

  // Read the snapshots embedded inline in the page (cs-baseline / cs-current script tags).
  window.BEFORE = readEmbeddedSnapshot('cs-baseline');
  window.AFTER  = readEmbeddedSnapshot('cs-current');

  const EMPTY = { graphs: {} };
  window.DIFF   = computeDiff(window.BEFORE ?? window.AFTER ?? EMPTY, window.AFTER ?? window.BEFORE ?? EMPTY);
  window.CYCLES = computeCycles(window.BEFORE ?? window.AFTER ?? EMPTY, window.AFTER ?? window.BEFORE ?? EMPTY);
  window.META   = computeMeta(window.BEFORE, window.AFTER);

  // Restore the active side from the URL (`side=before/after`); default to the
  // after (primary) snapshot. `before` is only honoured when a baseline exists.
  const urlSide = getNavParams().side;
  window.viewSide = (urlSide === 'before' && window.BEFORE) ? 'before'
                  : window.AFTER ? 'after'
                  : 'before';
  // If the Prompt Generator was open (state in the URL), restore its selected
  // nodes before the tables render so those rows come up already selected.
  const epState = (typeof epReadUrl === 'function') ? epReadUrl() : null;
  if (epState?.sel?.length) {
    if (!window._ntSelected) window._ntSelected = {};
    window._ntSelected[epState.level] = new Set(epState.sel);
  }
  document.querySelectorAll('.view').forEach(sec => setupNodeTable(sec, sec.dataset.view));
  setupGlobalControls();
  setupSnapPopup();
  setupFileControls();
  setupTooltip();
  buildSummary();
  updateFilesTab();
  updateHeader();

  document.getElementById('summary-header')?.addEventListener('click', () => {
    document.querySelector('.summary').classList.toggle('collapsed');
  });

  document.getElementById('nav-prompt-btn')?.addEventListener('click', () => {
    openExportPopup(currentLevel());
  });

  const active = document.querySelector('.view.active');
  const loading = active?.querySelector('.loading-indicator');
  if (loading) { loading.textContent = 'Loading Graphviz…'; loading.classList.add('on'); }

  window.gv = await window['@hpcc-js/wasm/graphviz'].Graphviz.load();

  renderView(active);

  // Restore state from URL, then set initial history entry
  const { level: urlLevel, node: urlNode } = getNavParams();
  if (urlLevel && urlLevel !== currentLevel()) switchToLevel(urlLevel);
  if (urlNode) openModalForNode(urlNode, urlLevel ?? currentLevel());
  // Replace initial history state so popstate can restore it
  history.replaceState({ level: currentLevel(), node: urlNode ?? null }, '', location.href);

  // Re-open the Prompt Generator if the URL says it was open.
  if (epState) {
    if (epState.level && epState.level !== currentLevel()) switchToLevel(epState.level);
    openExportPopup(epState.level, epState);
  }

  window.addEventListener('popstate', e => {
    const st = e.state || getNavParams();
    const lvl  = st.level;
    const nid  = st.node;
    const side = st.side;
    if (window.AFTER && (side === 'before' || side === 'after')) setViewSide(side);
    if (lvl && lvl !== currentLevel()) switchToLevel(lvl);
    if (nid) {
      openModalForNode(nid, lvl ?? currentLevel());
    } else {
      closeModalSilent();
    }
  });
});

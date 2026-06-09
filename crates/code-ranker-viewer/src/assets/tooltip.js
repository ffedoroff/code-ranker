// tooltip.js — the shared `#tt` tooltip engine: percentile + description
// renderers and the single delegated hover/click controller (setupTooltip).
// Used by the node table, the map, the popup and the summary. Split out of
// node-table.js (it is a cross-cutting concern, not part of the table).

function renderTooltip(label, data) {
  const d = typeof data === 'string' ? JSON.parse(data) : data;
  return `<div class="tt-title">${label}<span class="tt-count">${d.count} nodes</span></div>
<table class="tt-tbl">
<thead><tr><th>pct</th><th>value</th></tr></thead>
<tbody>
<tr><td>p1</td><td>${fmtNum(d.p1)}</td></tr>
<tr><td>p10</td><td>${fmtNum(d.p10)}</td></tr>
<tr><td>p50</td><td>${fmtNum(d.p50)}</td></tr>
<tr><td>p90</td><td>${fmtNum(d.p90)}</td></tr>
<tr><td>p99</td><td>${fmtNum(d.p99)}</td></tr>
</tbody></table>`;
}

function renderDescTooltip(label, desc, formula, calc) {
  const f = formula ? `<div class="tt-formula">${escHtml(formula)}</div>` : '';
  // `calc` is the same formula filled with this node's real values.
  const c = calc ? `<div class="tt-formula tt-calc">${escHtml(calc)}</div>` : '';
  // Descriptions are authored text (plugin specs). Everything is escaped first;
  // then two bits of light markup are re-enabled: `<br>` for line breaks, and
  // `` `code` `` spans (rendered highlighted and kept on one line — no wrap).
  const descHtml = escHtml(desc)
    .replace(/&lt;br\s*\/?&gt;/gi, '<br>')
    .replace(/`([^`]+)`/g, '<code class="tt-code">$1</code>');
  return `<div class="tt-title">${escHtml(label)}</div>${f}${c}<div class="tt-desc">${descHtml}</div>`;
}

// Hover tooltip for a node on the main map: title + the basic fields a developer
// wants at a glance — path, the grouping field (e.g. `crate`) when present, then
// `hk` and `sloc`. Reuses the shared `#tt` element (and its 300 ms delay).
function renderNodeTooltip(node, level) {
  const rows = [];
  const path = (node.path || node.id || '').replace(/^\{[^}]+\}\//, '');
  if (path) rows.push(['path', path]);
  const gk = levelUi(level).grouping?.key;
  if (gk) {
    const gv = nodeAttr(node, gk);
    if (gv != null && gv !== '') rows.push([(attrLabel(level, gk) || gk).toLowerCase(), String(gv)]);
  }
  for (const k of ['hk', 'sloc']) {
    const v = nodeAttr(node, k);
    if (v != null) rows.push([k, String(v)]);
  }
  const body = rows.map(([k, v]) => `<b>${escHtml(k)}:</b> ${escHtml(v)}`).join('<br>');
  return `<div class="tt-title">${escHtml(node.name || node.id)}</div>` +
         (body ? `<div class="tt-desc">${body}</div>` : '');
}

function renderGroupTooltip(stats) {
  const rows = [
    ['files', String(stats.files)],
    ['sloc',  stats.sloc > 0 ? fmtMetricShort(stats.sloc) : null],
    ['hk',    stats.hk   > 0 ? fmtMetricShort(stats.hk)   : null],
  ].filter(([, v]) => v !== null);
  const body = rows.map(([k, v]) => `<b>${k}:</b> ${escHtml(v)}`).join('<br>');
  return `<div class="tt-title">${escHtml(stats.name)}</div>` +
         (body ? `<div class="tt-desc">${body}</div>` : '');
}

function setupTooltip() {
  const tt = document.getElementById('tt');
  let current = null;
  let showTimer = null;
  let lastX = 0, lastY = 0;
  const SHOW_DELAY = 300;  // ms before a tooltip appears — avoids flicker on passing hovers

  // Tooltip title = the metric's full name from schema; falls back to the
  // element's own text.
  const titleOf = (el, lv) => {
    if (el.dataset.tipTitle) return el.dataset.tipTitle;
    const col = el.dataset.col;
    if (col && lv) return attrName(lv, col) || col;
    return el.textContent.trim();
  };

  const position = () => {
    const pad = 14;
    const tw = tt.offsetWidth, th = tt.offsetHeight;
    let x = lastX + pad, y = lastY + pad;
    if (x + tw > window.innerWidth  - 8) x = lastX - tw - pad;
    if (y + th > window.innerHeight - 8) y = lastY - th - pad;
    tt.style.left = x + 'px';
    tt.style.top  = y + 'px';
  };

  // Resolve the tooltip content for the hovered element, or null if none applies.
  const contentFor = e => {
    // A node on the main map (SVG `g.node` tagged with its id in setupTooltips):
    // show its basic fields. Native graphviz `<title>` is removed there, so this
    // is the only tooltip on the map.
    // SVG map nodes use the status bar instead of the #tt tooltip.
    if (e.target.closest('g.node[data-group-id]')) return null;
    if (e.target.closest('g.node[data-node-id]'))  return null;
    const cellTt  = e.target.closest('[data-tt]');
    const cellTip = e.target.closest('[data-tip]');
    const cellNum = e.target.closest('td[data-col]');
    if (cellTt && cellTt.dataset.tt) {
      // Prefer an explicit / column-derived full name (table footer cells);
      // fall back to the row's first cell (summary table, metric-per-row).
      const label = cellTt.dataset.tipTitle
        || cellTt.closest('tr')?.querySelector('td:first-child')?.textContent || '';
      return { el: cellTt, html: renderTooltip(label, cellTt.dataset.tt) };
    }
    if (cellTip && cellTip.dataset.tip) {
      return { el: cellTip, html: renderDescTooltip(titleOf(cellTip, null), cellTip.dataset.tip, cellTip.dataset.tipFormula, cellTip.dataset.tipCalc) };
    }
    if (cellNum) {
      // Value cells carry no precomputed tooltip — derive it on hover only, so we
      // never build a tooltip string for a cell the user never points at.
      const id  = cellNum.dataset.col;
      const lv  = cellNum.closest('[data-view]')?.dataset.view || 'files';
      const desc = attrDesc(lv, id);
      if (!desc) return null;  // no description — skip synthetic / unknown columns
      const formula = attrFormula(lv, id);
      const nid  = cellNum.closest('tr[data-node-id]')?.dataset.nodeId;
      const node = nid ? activeGraph(lv).nodes.find(n => n.id === nid) : null;
      const calc = node ? calcDisplay(lv, id, node) : '';
      return { el: cellNum, html: renderDescTooltip(titleOf(cellNum, lv), desc, formula, calc) };
    }
    return null;
  };

  const cancelShow = () => { if (showTimer) { clearTimeout(showTimer); showTimer = null; } };

  document.addEventListener('mouseover', e => {
    const r = contentFor(e);
    if (!r || r.el === current) return;     // nothing to show, or already anchored to it
    cancelShow();
    current = r.el;                          // anchor immediately (pending or shown)
    showTimer = setTimeout(() => {
      showTimer = null;
      tt.innerHTML = r.html;
      tt.removeAttribute('hidden');
      position();
    }, SHOW_DELAY);
  });

  // Hide (or cancel a still-pending show) the moment the pointer leaves the
  // anchored element — whatever kind it is. We check against `current` itself,
  // not a hardcoded selector list: an SVG map node (`g.node`) is shown by
  // `contentFor` but was absent from the old list, so leaving it never hid the
  // tooltip and `mousemove` kept dragging it across the map.
  document.addEventListener('mouseout', e => {
    if (!current) return;
    // Only react to leaving the anchored element (its inner SVG/HTML children
    // count as inside it).
    if (e.target !== current && !current.contains(e.target)) return;
    // A move into one of its own children is not a leave.
    if (e.relatedTarget && current.contains(e.relatedTarget)) return;
    hide();
  });

  document.addEventListener('mousemove', e => {
    lastX = e.clientX; lastY = e.clientY;
    if (!tt.hasAttribute('hidden')) position();
  });

  // Force-hide the tooltip. Needed because navigating between popup nodes (or
  // closing the modal) replaces the hovered element without firing `mouseout`,
  // which would otherwise leave a stale tooltip floating over the new content.
  const hide = () => { cancelShow(); tt.setAttribute('hidden', ''); current = null; };
  window.hideMetricTooltip = hide;
  // Any click (opening a node from the map/table, navigating between popup
  // cards, closing the modal) clears the tooltip.
  document.addEventListener('click', hide, true);
}

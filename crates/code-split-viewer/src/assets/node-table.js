function attachModalCheckbox(node, level, section) {
  const selectedIds = window._ntSelected?.[level];
  const tbody = section?.querySelector('.node-table-body .node-table tbody');

  const modalHdr = document.getElementById('node-modal-hdr');
  let modalCb = document.getElementById('node-modal-cb');
  if (!modalCb) {
    const wrap = document.createElement('label');
    wrap.id = 'node-modal-cb-wrap';
    wrap.className = 'nm-cb-wrap';
    wrap.title = 'Select node';
    modalCb = document.createElement('input');
    modalCb.type = 'checkbox';
    modalCb.id = 'node-modal-cb';
    modalCb.className = 'nt-cb';
    wrap.appendChild(modalCb);
    modalHdr.insertBefore(wrap, modalHdr.firstChild);
  }
  modalCb.checked = selectedIds?.has(node.id) ?? false;
  modalCb.onchange = () => {
    const sel = modalCb.checked;
    if (sel) selectedIds?.add(node.id); else selectedIds?.delete(node.id);
    const tableRow = tbody?.querySelector(`tr[data-node-id="${CSS.escape(node.id)}"]`);
    if (tableRow) {
      tableRow.classList.toggle('row-selected', sel);
      const tableCb = tableRow.querySelector('.nt-cb');
      if (tableCb) tableCb.checked = sel;
    }
    section?._gNodeMap?.get(node.id)?.classList.toggle('node-selected', sel);
    // Keep every popup-diagram card for this node in sync (a cycle node shows as
    // both fan-in and fan-out, plus the central card).
    window.markPopupSelected?.(node.id, sel);
    section?._updateAllCb?.();
  };
}

function setupNodeTable(section, level) {
  // Build columns from the snapshot's ui.columns — fully data-driven.
  // "name" is always prepended as the first column.
  const ui = levelUi(level);

  // Build column descriptors at setup time; re-read on each render in case the
  // active side switches (baseline/current).
  function buildCols() {
    const uiCols = levelUi(level).columns || [];
    const cols = [{ id: 'name', label: 'Name', isNum: false }];
    for (const key of uiCols) {
      const type = attrType(level, key);
      cols.push({
        id: key,
        label: attrShort(level, key) || key,
        isNum: type === 'int' || type === 'float',
      });
    }
    return cols;
  }

  let cols = buildCols();

  function nodeVal(n, id) {
    if (id === 'name') return n.id.replace(/^\{[^}]+\}\//, '') || n.id;
    if (id === 'kind')  return n.kind  ?? '';
    if (id === 'cycle') return n.cycle ?? null;
    return nodeAttr(n, id);
  }

  function fmtVal(v, id) {
    if (v === null || v === undefined) return '';
    if (typeof v === 'number') return fmtNum(v);
    const s = String(v);
    return id === 'name' ? s : s.replace(/_/g, ' ');
  }

  // Percentile distribution (p1/p10/p50/p90/p99) over the positive values of a
  // set of nodes — same shape as the summary's `nodePercentiles`, for the footer
  // tooltip.
  function pctOf(nodes, getVal) {
    const vals = nodes.map(getVal)
      .filter(v => typeof v === 'number' && isFinite(v) && v > 0)
      .sort((a, b) => a - b);
    if (!vals.length) return null;
    const q = p => {
      const i = p / 100 * (vals.length - 1), lo = Math.floor(i), hi = Math.ceil(i);
      return vals[lo] + (vals[hi] - vals[lo]) * (i - lo);
    };
    return { count: vals.length, p1: q(1), p10: q(10), p50: q(50), p90: q(90), p99: q(99) };
  }

  // ── DOM skeleton ──────────────────────────────────────────────────────────
  const wrap = document.createElement('div');
  // Collapsed by default (like the summary) — expand on header click.
  wrap.className = 'node-table-wrap collapsed';

  const hdr = document.createElement('div');
  hdr.className = 'node-table-header';
  const hdrTitle = document.createElement('span');
  hdrTitle.textContent = 'Details';
  const hdrBadge = document.createElement('span');
  hdrBadge.className = 'node-table-badge';
  const searchInput = document.createElement('input');
  searchInput.type = 'text';
  searchInput.placeholder = 'Search…';
  searchInput.className = 'nt-search-input';
  searchInput.addEventListener('click', e => e.stopPropagation());
  // Prompt Generator — moved here from the page header; sits right of the node count.
  const promptBtn = document.createElement('button');
  promptBtn.id = 'nav-prompt-btn';
  promptBtn.title = 'Generate an AI refactoring prompt';
  promptBtn.innerHTML = 'Prompt Generator <span class="nav-ai-letters">AI</span>' +
                        '<span class="nav-warn-count" id="nav-warn-count"></span>';
  promptBtn.addEventListener('click', e => { e.stopPropagation(); openExportPopup(level); });
  hdr.append(hdrTitle, hdrBadge, promptBtn, searchInput);

  const body = document.createElement('div');
  body.className = 'node-table-body';

  const container = document.createElement('div');
  container.className = 'node-table-container';
  const table = document.createElement('table');
  table.className = 'node-table';
  const thead = document.createElement('thead');
  const tbody = document.createElement('tbody');
  table.append(thead, tbody);
  container.appendChild(table);
  body.appendChild(container);
  wrap.append(hdr, body);

  const hintRow = section.querySelector('.hint-row');
  if (hintRow) hintRow.after(wrap);
  else section.appendChild(wrap);

  hdr.addEventListener('click', () => wrap.classList.toggle('collapsed'));

  // ── Sort / select state ───────────────────────────────────────────────────
  // Default sort from ui.default_sort, falling back to "name".
  let sortId  = levelUi(level).default_sort || 'name';
  let sortDir = 1;
  let searchQuery = '';
  if (!window._ntSelected) window._ntSelected = {};
  if (!window._ntSelected[level]) window._ntSelected[level] = new Set();
  const selectedIds = window._ntSelected[level];

  let allCb = null;
  let lastCheckedId = null;

  function updateAllCb() {
    const rows = [...tbody.querySelectorAll('tr[data-node-id]')];
    const n = rows.filter(r => r.classList.contains('row-selected')).length;
    if (allCb) {
      allCb.indeterminate = n > 0 && n < rows.length;
      allCb.checked = rows.length > 0 && n === rows.length;
    }
  }

  searchInput.addEventListener('input', () => { searchQuery = searchInput.value.trim().toLowerCase(); renderRows(); });

  function buildHeaders() {
    // Refresh cols in case active side (baseline/current) switched.
    cols = buildCols();
    thead.innerHTML = '';
    const tr = document.createElement('tr');
    const selTh = document.createElement('th');
    selTh.className = 'nt-sel-th';
    selTh.addEventListener('click', e => e.stopPropagation());
    allCb = document.createElement('input');
    allCb.type = 'checkbox';
    allCb.className = 'nt-cb';
    allCb.title = 'Select / deselect all visible';
    allCb.addEventListener('click', e => e.stopPropagation());
    allCb.addEventListener('change', () => {
      const sel = allCb.checked;
      allCb.indeterminate = false;
      tbody.querySelectorAll('tr[data-node-id]').forEach(row => {
        const nid = row.dataset.nodeId;
        row.classList.toggle('row-selected', sel);
        const rowCb = row.querySelector('.nt-cb');
        if (rowCb) rowCb.checked = sel;
        if (sel) selectedIds.add(nid); else selectedIds.delete(nid);
        section._gNodeMap?.get(nid)?.classList.toggle('node-selected', sel);
      });
    });
    selTh.appendChild(allCb);
    updateAllCb();
    tr.appendChild(selTh);
    cols.forEach(({ id, label, isNum }) => {
      const th = document.createElement('th');
      th.textContent = label;
      th.dataset.col = id;
      // Tooltips from schema — skip synthetic "name" column.
      if (id !== 'name') {
        const desc = attrDesc(level, id);
        const formula = attrFormula(level, id);
        const name = attrName(level, id);
        if (desc)    th.dataset.tip        = desc;
        if (formula) th.dataset.tipFormula = formula;
        if (name)    th.dataset.tipTitle   = name;
      }
      if (isNum) th.classList.add('num');
      if (id === sortId) th.classList.add(sortDir === 1 ? 'sort-asc' : 'sort-desc');
      th.addEventListener('click', e => {
        e.stopPropagation();
        if (sortId === id) sortDir = -sortDir; else { sortId = id; sortDir = 1; }
        buildHeaders();
        renderRows();
      });
      tr.appendChild(th);
    });
    thead.appendChild(tr);
  }

  // ── Visibility filter ─────────────────────────────────────────────────────
  function getVisible() {
    // Show every non-external node of the active snapshot. Baseline/Current picks
    // which snapshot; there is no per-status chip filtering anymore.
    return activeGraph(level).nodes.filter(n => !isExternalNode(n, level));
  }

  // ── Render ────────────────────────────────────────────────────────────────
  function renderRows() {
    // Reflect the active side in the title: Details / Details Baseline / Details Current.
    hdrTitle.textContent = 'Details' + (typeof viewModeSuffix === 'function' ? viewModeSuffix() : '');
    // Refresh cols in case active side changed.
    cols = buildCols();
    const visible = getVisible();
    const filtered = searchQuery
      ? visible.filter(n => nodeVal(n, 'name').toLowerCase().includes(searchQuery))
      : visible;
    hdrBadge.textContent = `${filtered.length}`;

    const sorted = [...filtered].sort((a, b) => {
      const av = nodeVal(a, sortId), bv = nodeVal(b, sortId);
      if (av === null && bv === null) return 0;
      if (av === null) return 1;
      if (bv === null) return -1;
      if (typeof av === 'number' && typeof bv === 'number') return (av - bv) * sortDir;
      return String(av).localeCompare(String(bv)) * sortDir;
    });

    tbody.innerHTML = '';
    if (sorted.length === 0) {
      const tr = document.createElement('tr');
      const td = document.createElement('td');
      td.colSpan = cols.length;
      td.className = 'nt-empty';
      td.textContent = searchQuery ? 'No matches' : 'No nodes visible';
      tr.appendChild(td);
      tbody.appendChild(tr);
      return;
    }
    sorted.forEach(n => {
      const tr = document.createElement('tr');
      tr.className = `nrow-${n.status}`;
      tr.style.cursor = 'pointer';
      tr.dataset.nodeId = n.id;
      if (selectedIds.has(n.id)) tr.classList.add('row-selected');
      tr.addEventListener('mouseenter', () => {
        tr.classList.add('row-hl');
        section._gNodeMap?.get(n.id)?.classList.add('node-hl');
      });
      tr.addEventListener('mouseleave', () => {
        tr.classList.remove('row-hl');
        section._gNodeMap?.get(n.id)?.classList.remove('node-hl');
      });
      tr.addEventListener('click', () => {
        // Route through openModalForNode so the modal show / header flyout /
        // open-node tracking all live in one place.
        if (window.openModalForNode?.(n.id, level)) window.navPush(level, n.id);
      });

      const selTd = document.createElement('td');
      selTd.className = 'nt-sel-td';
      const cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.className = 'nt-cb';
      cb.checked = selectedIds.has(n.id);
      selTd.appendChild(cb);
      selTd.addEventListener('click', e => {
        e.stopPropagation();
        const rows = [...tbody.querySelectorAll('tr[data-node-id]')];
        const currentIdx = rows.indexOf(tr);
        const anchorIdx  = lastCheckedId ? rows.findIndex(r => r.dataset.nodeId === lastCheckedId) : -1;

        if (e.shiftKey && anchorIdx !== -1 && currentIdx !== -1) {
          const targetState = !selectedIds.has(n.id);
          const lo = Math.min(anchorIdx, currentIdx);
          const hi = Math.max(anchorIdx, currentIdx);
          rows.slice(lo, hi + 1).forEach(row => {
            const nid = row.dataset.nodeId;
            row.classList.toggle('row-selected', targetState);
            const rowCb = row.querySelector('.nt-cb');
            if (rowCb) rowCb.checked = targetState;
            if (targetState) selectedIds.add(nid); else selectedIds.delete(nid);
            section._gNodeMap?.get(nid)?.classList.toggle('node-selected', targetState);
          });
        } else {
          const isSelected = tr.classList.toggle('row-selected');
          cb.checked = isSelected;
          section._gNodeMap?.get(n.id)?.classList.toggle('node-selected', isSelected);
          if (isSelected) selectedIds.add(n.id); else selectedIds.delete(n.id);
          lastCheckedId = n.id;
        }
        updateAllCb();
      });
      tr.appendChild(selTd);

      cols.forEach(({ id, isNum }) => {
        const td = document.createElement('td');
        const v  = nodeVal(n, id);
        td.dataset.col = id;
        if (isNum) td.classList.add('num');
        td.textContent = fmtVal(v, id);
        if (id === 'status') td.className += ` cell-s-${n.status}`;
        // Tooltip (description + formula + this node's computation) is derived
        // lazily on hover in `setupTooltip` — never precomputed for every cell.
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    });

    // ── Summary footer: average for numeric columns, count for text columns ──
    const foot = document.createElement('tr');
    foot.className = 'nt-foot';
    foot.appendChild(document.createElement('td')).className = 'nt-sel-td';
    cols.forEach(({ id, isNum }) => {
      const td = document.createElement('td');
      td.dataset.col = id;
      if (isNum) {
        td.classList.add('num');
        const nums = sorted.map(n => nodeVal(n, id)).filter(v => typeof v === 'number' && isFinite(v));
        const avg = nums.length ? nums.reduce((a, b) => a + b, 0) / nums.length : null;
        td.textContent = avg != null ? fmtVal(avg, id) : '';
        // Percentile distribution tooltip (like the summary section).
        const pct = pctOf(sorted, n => nodeVal(n, id));
        if (pct) {
          td.dataset.tt = JSON.stringify(pct);
          td.dataset.tipTitle = attrName(level, id) || id;
        }
      } else if (id === 'name') {
        td.textContent = `${sorted.length}`;            // total rows — the "sum" of text entries
      } else {
        const cnt = sorted.reduce((a, n) => { const v = nodeVal(n, id); return a + (v != null && v !== '' ? 1 : 0); }, 0);
        td.textContent = cnt ? String(cnt) : '';         // count of non-empty text values
      }
      foot.appendChild(td);
    });
    tbody.appendChild(foot);
    updateAllCb();
  }

  buildHeaders();
  renderRows();
  section._refreshNodeTable = renderRows;
  section._updateAllCb = updateAllCb;
}

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

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
  // active side switches (before/after).
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
  const copySelBtn = document.createElement('button');
  copySelBtn.className = 'nt-copy-sel-btn';
  copySelBtn.textContent = '⎘ Copy 0 selected';
  copySelBtn.title = 'Export selected nodes';
  copySelBtn.disabled = true;   // enabled once at least one row is selected
  copySelBtn.addEventListener('click', e => { e.stopPropagation(); openExportPopup(level); });
  hdr.append(hdrTitle, hdrBadge, searchInput, copySelBtn);

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
    copySelBtn.disabled = n === 0;   // visible but disabled when nothing is selected
    copySelBtn.textContent = `⎘ Copy ${n} selected`;
  }

  searchInput.addEventListener('input', () => { searchQuery = searchInput.value.trim().toLowerCase(); renderRows(); });

  function buildHeaders() {
    // Refresh cols in case active side (before/after) switched.
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
    // Show every non-external node of the active snapshot. Before/After picks
    // which snapshot; there is no per-status chip filtering anymore.
    return activeGraph(level).nodes.filter(n => !isExternalNode(n, level));
  }

  // ── Render ────────────────────────────────────────────────────────────────
  function renderRows() {
    // Reflect the active side in the title: Details / Details Before / Details After.
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
        const overlay = getModal();
        const mc = buildModalContent(n, level);
        document.getElementById('node-modal-hdr-title').innerHTML = mc.hdr;
        document.getElementById('node-modal-body').innerHTML = mc.body;
        window.setModalDiagram(mc.diagram);
        attachModalCheckbox(n, level, section);
        overlay.style.display = 'flex'; document.body.style.overflow = 'hidden';
        window.navPush(level, n.id);
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
  return `<div class="tt-title">${escHtml(label)}</div>${f}${c}<div class="tt-desc">${escHtml(desc)}</div>`;
}

function setupTooltip() {
  const tt = document.getElementById('tt');
  let current = null;

  // Tooltip title = the metric's full name from schema; falls back to the
  // element's own text.
  const titleOf = (el, lv) => {
    if (el.dataset.tipTitle) return el.dataset.tipTitle;
    const col = el.dataset.col;
    if (col && lv) return attrName(lv, col) || col;
    return el.textContent.trim();
  };

  document.addEventListener('mouseover', e => {
    const cellTt  = e.target.closest('[data-tt]');
    const cellTip = e.target.closest('[data-tip]');
    const cellNum = e.target.closest('td[data-col]');
    if (cellTt && cellTt.dataset.tt) {
      // Prefer an explicit / column-derived full name (table footer cells);
      // fall back to the row's first cell (summary table, metric-per-row).
      const label = cellTt.dataset.tipTitle
        || cellTt.closest('tr')?.querySelector('td:first-child')?.textContent || '';
      tt.innerHTML = renderTooltip(label, cellTt.dataset.tt);
      tt.removeAttribute('hidden');
      current = cellTt;
    } else if (cellTip && cellTip.dataset.tip) {
      tt.innerHTML = renderDescTooltip(titleOf(cellTip, null), cellTip.dataset.tip, cellTip.dataset.tipFormula, cellTip.dataset.tipCalc);
      tt.removeAttribute('hidden');
      current = cellTip;
    } else if (cellNum) {
      // Value cells carry no precomputed tooltip — derive it on hover only, so we
      // never build a tooltip string for a cell the user never points at. Every
      // metric column gets the description + formula (where one exists); the
      // per-node computation is added for metrics whose inputs are stored.
      const id  = cellNum.dataset.col;
      // Determine the active level from the closest section (sections use data-view).
      const lv  = cellNum.closest('[data-view]')?.dataset.view || 'files';
      const desc = attrDesc(lv, id);
      if (!desc) return;  // no description — skip synthetic / unknown columns
      const formula = attrFormula(lv, id);
      const nid  = cellNum.closest('tr[data-node-id]')?.dataset.nodeId;
      const node = nid ? activeGraph(lv).nodes.find(n => n.id === nid) : null;
      const calc = node ? calcDisplay(lv, id, node) : '';
      tt.innerHTML = renderDescTooltip(titleOf(cellNum, lv), desc, formula, calc);
      tt.removeAttribute('hidden');
      current = cellNum;
    }
  });

  document.addEventListener('mouseout', e => {
    const cell = e.target.closest('[data-tt]') || e.target.closest('[data-tip]') || e.target.closest('td[data-col]');
    if (cell && cell === current) {
      tt.setAttribute('hidden', '');
      current = null;
    }
  });

  document.addEventListener('mousemove', e => {
    if (tt.hasAttribute('hidden')) return;
    const pad = 14;
    const tw = tt.offsetWidth, th = tt.offsetHeight;
    let x = e.clientX + pad, y = e.clientY + pad;
    if (x + tw > window.innerWidth  - 8) x = e.clientX - tw - pad;
    if (y + th > window.innerHeight - 8) y = e.clientY - th - pad;
    tt.style.left = x + 'px';
    tt.style.top  = y + 'px';
  });

  // Force-hide the tooltip. Needed because navigating between popup nodes (or
  // closing the modal) replaces the hovered element without firing `mouseout`,
  // which would otherwise leave a stale tooltip floating over the new content.
  const hide = () => { tt.setAttribute('hidden', ''); current = null; };
  window.hideMetricTooltip = hide;
  // Any click (opening a node from the map/table, navigating between popup
  // cards, closing the modal) clears the tooltip.
  document.addEventListener('click', hide, true);
}

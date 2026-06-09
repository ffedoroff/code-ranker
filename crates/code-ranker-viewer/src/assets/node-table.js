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

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
    section?._updateAllCb?.();
  };
}

function setupNodeTable(section, level) {
  const BASE = [
    {id:'name',label:'Name'},{id:'kind',label:'Kind'},
    {id:'loc',label:'LOC'},{id:'hk',label:'HK'},
    {id:'fan_in',label:'Fan-in'},{id:'fan_out',label:'Fan-out'},
    {id:'h_vol',label:'H.vol'},{id:'h_bugs',label:'H.bugs'},{id:'h_effort',label:'H.effort'},
    {id:'h_time',label:'H.time(s)'},{id:'h_len',label:'H.len'},{id:'h_vocab',label:'H.vocab'},
    {id:'cyclomatic',label:'Cyclomatic'},{id:'cognitive',label:'Cognitive'},
    {id:'mi',label:'MI'},{id:'mi_sei',label:'MI SEI'},
    {id:'loc_logical',label:'Logical'},{id:'loc_comments',label:'Comments'},{id:'loc_blank',label:'Blank'},
  ];
  const CYCLE_COL = {id:'cycle',label:'Cycle'};
  const COLS = {
    modules:   [BASE[0], BASE[1], CYCLE_COL, ...BASE.slice(2)],
    files:     [BASE[0], CYCLE_COL, ...BASE.slice(2)],
    functions: BASE,
  };
  const cols   = COLS[level] || COLS.modules;
  const numSet = new Set(['loc','cyclomatic','cognitive','mi','mi_sei','fan_in','fan_out','hk',
                          'h_vol','h_bugs','h_effort','h_time','h_len','h_vocab',
                          'loc_logical','loc_comments','loc_blank']);

  function nodeVal(n, id) {
    const cx = n.complexity;
    switch (id) {
      case 'name':        return (n.path || '').replace(/^\{[^}]+\}\//, '') || n.id;
      case 'kind':        return n.kind || '';
      case 'status':      return n.status || '';
      case 'loc':         return cx?.loc?.source ?? n.loc ?? null;
      case 'cyclomatic':  return cx?.cyclomatic ?? null;
      case 'cognitive':   return cx?.cognitive  ?? null;
      case 'mi':          return cx?.maintainability?.mi     ?? null;
      case 'mi_sei':      return cx?.maintainability?.mi_sei ?? null;
      case 'fan_in':      return cx?.coupling?.fan_in  ?? null;
      case 'fan_out':     return cx?.coupling?.fan_out ?? null;
      case 'hk':          return cx?.coupling?.hk      ?? null;
      case 'h_vol':       return cx?.halstead?.volume     ?? null;
      case 'h_bugs':      return cx?.halstead?.bugs       ?? null;
      case 'h_effort':    return cx?.halstead?.effort     ?? null;
      case 'h_time':      return cx?.halstead?.time       ?? null;
      case 'h_len':       return cx?.halstead?.length     ?? null;
      case 'h_vocab':     return cx?.halstead?.vocabulary ?? null;
      case 'loc_logical': return cx?.loc?.logical  ?? null;
      case 'loc_comments':return cx?.loc?.comments ?? null;
      case 'loc_blank':   return cx?.loc?.blank    ?? null;
      case 'cycle':       return n.cycle_kind ?? null;
      default:            return null;
    }
  }

  function fmtVal(v, id) {
    if (v === null || v === undefined) return '';
    if (typeof v === 'number') return fmtNum(v);
    const s = String(v);
    return id === 'name' ? s : s.replace(/_/g, ' ');
  }

  // ── DOM skeleton ──────────────────────────────────────────────────────────
  const wrap = document.createElement('div');
  wrap.className = 'node-table-wrap';

  const hdr = document.createElement('div');
  hdr.className = 'node-table-header';
  const hdrTitle = document.createElement('span');
  hdrTitle.textContent = 'Nodes';
  const hdrBadge = document.createElement('span');
  hdrBadge.className = 'node-table-badge';
  const searchInput = document.createElement('input');
  searchInput.type = 'text';
  searchInput.placeholder = 'Search…';
  searchInput.className = 'nt-search-input';
  searchInput.addEventListener('click', e => e.stopPropagation());
  const copySelBtn = document.createElement('button');
  copySelBtn.className = 'nt-copy-sel-btn';
  copySelBtn.textContent = '⎘ Copy';
  copySelBtn.title = 'Export selected nodes';
  copySelBtn.style.display = 'none';
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
  let sortId  = 'name';
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
    copySelBtn.style.display = n > 0 ? '' : 'none';
  }

  searchInput.addEventListener('input', () => { searchQuery = searchInput.value.trim().toLowerCase(); renderRows(); });

  function buildHeaders() {
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
    cols.forEach(({id, label}) => {
      const th = document.createElement('th');
      th.textContent = label;
      th.dataset.col = id;
      if (COL_TIPS[id]) th.dataset.tip = COL_TIPS[id];
      if (COL_FORMULAS[id]) th.dataset.tipFormula = COL_FORMULAS[id];
      if (numSet.has(id)) th.classList.add('num');
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
    // Show every (non-external) node of the active snapshot. Before/After picks
    // which snapshot; there is no per-status chip filtering anymore.
    return activeGraph(level).nodes.filter(n => !n.external);
  }

  // ── Render ────────────────────────────────────────────────────────────────
  function renderRows() {
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
        document.getElementById('node-modal-diagram').innerHTML = mc.diagram;
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

      cols.forEach(({id}) => {
        const td = document.createElement('td');
        const v  = nodeVal(n, id);
        td.dataset.col = id;
        if (numSet.has(id)) td.classList.add('num');
        td.textContent = fmtVal(v, id);
        if (id === 'status') td.className += ` cell-s-${n.status}`;
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    });
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

function renderDescTooltip(label, desc, formula) {
  const f = formula ? `<div class="tt-formula">${escHtml(formula)}</div>` : '';
  return `<div class="tt-title">${escHtml(label)}</div>${f}<div class="tt-desc">${escHtml(desc)}</div>`;
}

function setupTooltip() {
  const tt = document.getElementById('tt');
  let current = null;

  document.addEventListener('mouseover', e => {
    const cellTt  = e.target.closest('[data-tt]');
    const cellTip = e.target.closest('[data-tip]');
    if (cellTt && cellTt.dataset.tt) {
      const row   = cellTt.closest('tr');
      const label = row?.querySelector('td:first-child')?.textContent ?? '';
      tt.innerHTML = renderTooltip(label, cellTt.dataset.tt);
      tt.removeAttribute('hidden');
      current = cellTt;
    } else if (cellTip && cellTip.dataset.tip) {
      tt.innerHTML = renderDescTooltip(cellTip.textContent.trim(), cellTip.dataset.tip, cellTip.dataset.tipFormula);
      tt.removeAttribute('hidden');
      current = cellTip;
    }
  });

  document.addEventListener('mouseout', e => {
    const cell = e.target.closest('[data-tt]') || e.target.closest('[data-tip]');
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
}

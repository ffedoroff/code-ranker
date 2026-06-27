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
    if (id === 'name') return n._cat ? (n.name ?? n.id) : (n.id.replace(/^\{[^}]+\}\//, '') || n.id);
    if (id === 'kind')  return n.kind  ?? '';
    if (id === 'cycle') return n.cycle ?? null;
    return nodeAttr(n, id);
  }

  // Folder / group (crate) aggregate rows: one synthetic node per directory and
  // per grouping-key value, with each numeric column SUMMED over its member files.
  // `_cat` marks the row category ('folder' / 'group'); files are left untagged.
  // `kind` carries a distinct label so the Kind column and the filters tell them
  // apart; `_sample`/`_group` drive the drill-on-click.
  const groupKey = () => levelUi(level).grouping?.key || null;
  function buildAggregates(files) {
    const numCols = cols.filter(c => c.isNum).map(c => c.id);
    // CYCLES is keyed [lang][level]; resolve active language before indexing.
    const _cycLang = (typeof currentLang === 'function' ? currentLang() : null)
                  || Object.keys(window.CYCLES || {})[0];
    const cyc = (_cycLang ? window.CYCLES?.[_cycLang] : null)?.[level]?.nodeCycleStatus;   // id → cycle status (cycle members only)
    const mk = (id, label, kind, cat, members, extra) => {
      const n = { id, name: label, kind, _cat: cat, _count: members.length, ...extra };
      for (const key of numCols) {
        let sum = 0, any = false;
        for (const f of members) {
          const v = nodeAttr(f, key);
          if (typeof v === 'number' && isFinite(v)) { sum += v; any = true; }
        }
        n[key] = any ? sum : null;
      }
      // Cycle column = how many member files sit in a dependency cycle (empty at 0).
      let inCycle = 0;
      if (cyc) for (const f of members) { const s = cyc.get(f.id); if (s && s !== 'none') inCycle++; }
      n.cycle = inCycle > 0 ? inCycle : null;
      return n;
    };
    const byFolder = new Map();
    for (const f of files) { const d = nodeFullDir(f); (byFolder.get(d) || byFolder.set(d, []).get(d)).push(f); }
    const folders = [...byFolder].map(([dir, ms]) => mk(`folder${dir}`, dir, 'folder', 'folder', ms, { _sample: ms[0] }));

    let groups = [];
    const gk = groupKey();
    if (gk) {
      const byGroup = new Map();
      for (const f of files) {
        const g = nodeAttr(f, gk);
        if (g == null || g === '') continue;
        const key = String(g);
        (byGroup.get(key) || byGroup.set(key, []).get(key)).push(f);
      }
      groups = [...byGroup].map(([g, ms]) => mk(`group${g}`, g, gk, 'group', ms, { _group: g }));
    }
    return { folders, groups };
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
  // Kind filters (files / folders / <groups>) — shown next to search, all on by
  // default. The groups box is labelled by the grouping key (e.g. "crates"); it is
  // omitted when the level has no grouping.
  const kindFilter = { file: true, folder: false, group: true };
  const filterWrap = document.createElement('span');
  filterWrap.className = 'nt-kind-filters';
  filterWrap.addEventListener('click', e => e.stopPropagation());
  const gk0 = levelUi(level).grouping?.key;
  const filterDefs = [['file', 'files'], ['folder', 'folders']];
  if (gk0) filterDefs.push(['group', `${gk0}s`]);
  for (const [cat, label] of filterDefs) {
    const lab = document.createElement('label');
    lab.className = 'nt-kind-opt';
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = kindFilter[cat];
    cb.dataset.cat = cat;
    cb.addEventListener('change', () => { kindFilter[cat] = cb.checked; renderRows(); });
    lab.append(cb, document.createTextNode(label));
    filterWrap.appendChild(lab);
  }
  // Prompt Generator — moved here from the page header; sits right of the node count.
  const promptBtn = document.createElement('button');
  promptBtn.id = 'nav-prompt-btn';
  promptBtn.title = 'Generate an AI refactoring prompt';
  promptBtn.innerHTML = 'Prompt Generator <span class="nav-ai-letters">AI</span>' +
                        '<span class="nav-warn-count" id="nav-warn-count"></span>';
  promptBtn.addEventListener('click', e => { e.stopPropagation(); openExportPopup(level); });
  hdr.append(hdrTitle, hdrBadge, promptBtn, searchInput, filterWrap);

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
    // Files = every non-external node of the active snapshot (Baseline/Current
    // picks which). Folders / groups are synthetic aggregate rows. The kind-filter
    // checkboxes decide which categories are listed.
    const files = activeGraph(level).nodes.filter(n => !isExternalNode(n, level));
    const out = [];
    const needAgg = kindFilter.folder || kindFilter.group;
    const agg = needAgg ? buildAggregates(files) : { folders: [], groups: [] };
    if (kindFilter.file)   out.push(...files);
    if (kindFilter.folder) out.push(...agg.folders);
    if (kindFilter.group)  out.push(...agg.groups);
    return out;
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
      // An empty / omitted value (e.g. an omitted metric, or a non-cycle row)
      // ranks as the minimum, so it sorts *with* the direction: first when
      // ascending, last when descending — never pinned to one end.
      const aEmpty = av === null || av === undefined || av === '';
      const bEmpty = bv === null || bv === undefined || bv === '';
      if (aEmpty && bEmpty) return 0;
      if (aEmpty) return -1 * sortDir;
      if (bEmpty) return 1 * sortDir;
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
      const agg = !!n._cat;   // folder / group aggregate row (vs a real file)
      // Shared highlight key with the SVG (group box → `group:<crate>`, folder →
      // `folder:<dir>`); lets a row and its map element light up together.
      const aggKey = agg ? (n._cat === 'group' ? 'group:' + n._group : 'folder:' + n.name) : null;
      const tr = document.createElement('tr');
      tr.style.cursor = 'pointer';
      if (agg) {
        tr.className = `nt-agg nt-agg-${n._cat}`;
        tr.dataset.aggKey = aggKey;
        if (selectedIds.has(n.id)) tr.classList.add('row-selected');
        tr.addEventListener('mouseenter', () => {
          tr.classList.add('row-hl');
          section._gAggMap?.get(aggKey)?.classList.add('node-hl');
        });
        tr.addEventListener('mouseleave', () => {
          tr.classList.remove('row-hl');
          section._gAggMap?.get(aggKey)?.classList.remove('node-hl');
        });
        // Click drills into the folder/group on the map (like clicking its SVG box).
        tr.addEventListener('click', () => {
          if (n._cat === 'group') window.drillIntoGroup?.(n._group, level, 0);
          else if (n._sample && window.focusFolderTarget) {
            const t = window.focusFolderTarget(level, n._sample);
            window.drillIntoGroup?.(t.key, level, t.dig);
          }
        });
      } else {
        tr.className = `nrow-${n.status}`;
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
      }

      const selTd = document.createElement('td');
      selTd.className = 'nt-sel-td';
      if (agg) {   // folder / group aggregates: a plain selectable checkbox
        const cb = document.createElement('input');
        cb.type = 'checkbox';
        cb.className = 'nt-cb';
        cb.checked = selectedIds.has(n.id);
        selTd.appendChild(cb);
        selTd.addEventListener('click', e => {
          e.stopPropagation();
          const isSel = tr.classList.toggle('row-selected');
          cb.checked = isSel;
          if (isSel) selectedIds.add(n.id); else selectedIds.delete(n.id);
          section._gAggMap?.get(aggKey)?.classList.toggle('node-selected', isSel);
        });
      } else {
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
      }
      tr.appendChild(selTd);

      cols.forEach(({ id, isNum }) => {
        const td = document.createElement('td');
        const v  = nodeVal(n, id);
        td.dataset.col = id;
        if (isNum) td.classList.add('num');
        td.textContent = fmtVal(v, id);
        if (id === 'status' && n.status) td.className += ` cell-s-${n.status}`;
        // Tooltip (description + formula + this node's computation) is derived
        // lazily on hover in `setupTooltip` — never precomputed for every cell.
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    });

    // ── Summary footer: average for numeric columns, count for text columns.
    // Shown only when the displayed rows are a SINGLE kind (files-only / one
    // aggregate category) — averaging across files + folders + groups together
    // would be meaningless. ──
    const cats = new Set(sorted.map(n => n._cat || 'file'));
    if (cats.size !== 1) { updateAllCb(); return; }
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

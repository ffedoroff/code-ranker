function buildDiagramSVG(node, level) {
  // Nodes that are selected on the main map get the same yellow highlight here.
  const selectedIds = window._ntSelected?.[level];
  const diff      = window.DIFF?.[level];
  // Use the ACTIVE side's raw snapshot (externals included, unlike DIFF). Tying
  // this to the shown side keeps the popup in-status: viewing the baseline shows
  // only baseline neighbours (no added/current-only nodes), and viewing current
  // shows only current neighbours (no removed/baseline-only nodes).
  const rawGraph  = activeGraph(level);
  const allEdges  = rawGraph.edges;
  // nodeMap: DIFF nodes (have status/cycle data) + raw external nodes as fallback
  const nodeMap   = new Map([
    ...(diff?.nodes || []).map(n => [n.id, n]),
    ...rawGraph.nodes.filter(n => isExternalNode(n, level)).map(n => [n.id, n]),
  ]);
  // Set of external node ids, built from the raw graph, for fast lookup in
  // connection-direction logic (NOT from edge flags).
  const extIds    = externalIdSet(rawGraph, level);

  const esc      = s => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
  const trunc    = (s, n) => s.length > n ? s.slice(0, n - 1) + '…' : s;
  const nameOf   = n => trunc(n.name || n.id.split('::').pop() || n.id, 18);

  // Card-metric keys driven by ui.card_metrics (e.g. ["hk","sloc"]).
  const ui          = levelUi(level);
  const cardMetrics = ui.card_metrics || [];
  const primaryKey   = cardMetrics[0] ?? null;
  const secondaryKey = cardMetrics[1] ?? null;

  // Abbreviated number for the card (e.g. 189,000 → 189K, 1,500,000 → 1.5M).
  // Respects `abbreviate:true` in the spec; otherwise uses plain fmtNum.
  const fmtCard = (key, v) => {
    if (v == null) return null;
    if (attrAbbrev(level, key)) {
      v = typeof v === 'number' ? v : Number(v);
      if (!isFinite(v)) return null;
      v = Math.round(v);
      if (v >= 1e6) return (v / 1e6).toFixed(v >= 1e7 ? 0 : 1).replace(/\.0$/, '') + 'M';
      if (v >= 1e3) return (v / 1e3).toFixed(v >= 1e4 ? 0 : 1).replace(/\.0$/, '') + 'K';
      return String(v);
    }
    return fmtNum(v);
  };

  // Column visual config
  const COL_STROKE = '#8ba6c0';
  const COL_DASH   = '6,4';
  const kindColor  = k => k === 'external' ? '#9aa0a6' : COL_STROKE;
  const kindDash   = _k => COL_DASH;

  // Is the far endpoint of this edge (the node at `idKey`) external? Look at the
  // far node via the extIds set — NOT any edge property.
  const isExtEndpoint = (e, idKey) => extIds.has(e[idKey]);

  // Collect connections for one direction, deduped by the far node. The popup is
  // the detailed view, so it shows EVERY edge kind (uses / reexports / contains)
  // — unlike the main map, which draws only flow edges. Each card's kind row
  // then labels which kinds connect it.
  const collectConns = (edgeArr, idKey) => {
    const byNode = new Map();
    for (const e of edgeArr) {
      const id = e[idKey];
      let rec = byNode.get(id);
      if (!rec) {
        rec = { node: nodeMap.get(id) || { id, name: id.split('::').pop() },
                kinds: new Set(), ext: false };
        byNode.set(id, rec);
      }
      rec.kinds.add(e.kind || 'uses');
      if (isExtEndpoint(e, idKey)) rec.ext = true;
    }
    const internal = [], external = [];
    for (const rec of byNode.values())
      (rec.ext ? external : internal).push(rec);
    return { internal, external };
  };

  const inConns  = collectConns(allEdges.filter(e => e.target === node.id), 'source');
  const outConns = collectConns(allEdges.filter(e => e.source === node.id), 'target');

  // Layout constants
  const SNW         = 148, SNH = 62;
  const MNH         = 110, MNH2 = MNH + 54;
  const CELL        = SNW + 12;          // one card-slot width
  const COL_PAD_X   = 12;               // horizontal padding inside column box
  const COL_GAP     = 12;              // gap between adjacent columns
  const ROW_H       = SNH + 10;
  const PAD_TOP     = 20;              // inside column: space above first row (below label)
  const PAD_BOT     = 14;
  const ARR_GAP     = 36;
  const SIDE_PAD    = 20;
  const MAX_TIER_COLS = 5;             // total columns across a tier's groups; overflow scrolls
  const MARG        = 20;
  const MNW_MIN     = 3 * CELL - 12 + 2 * COL_PAD_X;  // ≈ 492 minimum main-node width

  // Split a column budget across groups proportionally to their card counts,
  // each group getting 1..count columns and the sum capped at min(budget, total).
  const allocCols = (counts, budget) => {
    const total = counts.reduce((a, b) => a + b, 0);
    const cap   = Math.min(budget, total);
    const alloc = counts.map(n => Math.max(1, Math.min(n, Math.round(cap * n / total))));
    let sum = alloc.reduce((a, b) => a + b, 0);
    // Trim overshoot from the group with the most columns (keeping ≥1).
    while (sum > cap) {
      let idx = -1, best = 1;
      alloc.forEach((c, i) => { if (c > best) { best = c; idx = i; } });
      if (idx < 0) break;
      alloc[idx]--; sum--;
    }
    // Spend any slack on the group with the most cards that still has room.
    while (sum < cap) {
      let idx = -1, best = -1;
      alloc.forEach((c, i) => { if (c < counts[i] && counts[i] > best) { best = counts[i]; idx = i; } });
      if (idx < 0) break;
      alloc[idx]++; sum++;
    }
    return alloc;
  };

  // Build column descriptors for one direction: one internal-connections column
  // plus (when present) a separate grey `external` column on the same tier. The
  // two groups SHARE a ≤ MAX_TIER_COLS column budget (split by card count); rows
  // beyond what fits are not truncated — the diagram scrolls.
  const buildCols = ({ internal, external }) => {
    const raw = [];
    if (internal.length) raw.push({ kind: 'connections', all: internal, items: internal, count: internal.length, ext: false });
    if (external.length) raw.push({ kind: 'external',    all: external, items: external, count: external.length, ext: true  });
    if (raw.length === 0) return raw;

    const widths = allocCols(raw.map(c => c.count), MAX_TIER_COLS);
    raw.forEach((c, i) => {
      c.cardW = widths[i];
      c.px_w  = c.cardW * CELL - 12 + 2 * COL_PAD_X;
      const rows = [];
      for (let j = 0; j < c.items.length; j += c.cardW)
        rows.push(c.items.slice(j, j + c.cardW));
      c.rows = rows;
      c.h    = PAD_TOP + rows.length * ROW_H - (ROW_H - SNH) + PAD_BOT;
    });
    return raw;
  };

  const inCols  = buildCols(inConns);
  const outCols = buildCols(outConns);

  // Total pixel width of a column set
  const colsW = cols => cols.length === 0 ? 0
    : cols.reduce((s, c) => s + c.px_w, 0) + (cols.length - 1) * COL_GAP;

  // SVG width driven by columns; main node width computed after column positions are known
  const VW = Math.max(800, 2 * SIDE_PAD + colsW(inCols), 2 * SIDE_PAD + colsW(outCols));

  const maxInH  = inCols.length  > 0 ? Math.max(...inCols.map(c => c.h))  : 0;
  const maxOutH = outCols.length > 0 ? Math.max(...outCols.map(c => c.h)) : 0;

  const inAreaBottom = inCols.length  > 0 ? MARG + maxInH : 0;
  const MNY          = inCols.length  > 0 ? inAreaBottom + ARR_GAP : MARG;
  const outAreaTop   = outCols.length > 0 ? MNY + MNH2 + ARR_GAP : 0;
  const VH           = outCols.length > 0 ? outAreaTop + maxOutH + MARG : MNY + MNH2 + MARG;

  // Assign X positions to columns (group is centred in VW)
  const assignX = cols => {
    let x = (VW - colsW(cols)) / 2;
    for (const c of cols) { c.x = x; x += c.px_w + COL_GAP; }
  };

  if (inCols.length  > 0) assignX(inCols);
  if (outCols.length > 0) assignX(outCols);

  // Main node width: at least MNW_MIN, but wide enough to cover all arrow X positions
  const allCols   = [...inCols, ...outCols];
  const arrowXs   = allCols.map(c => c.x + c.px_w / 2);
  const tiersW = Math.max(colsW(inCols), colsW(outCols));
  const MNW = allCols.length > 0
    ? Math.max(MNW_MIN, tiersW, 2 * Math.max(...arrowXs.map(x => Math.abs(x - VW / 2))) + 2 * COL_PAD_X)
    : MNW_MIN;
  const MNX  = (VW - MNW) / 2;
  const MNCX = MNX + MNW / 2;

  // Assign Y: in-cols bottom-anchored, out-cols top-anchored
  for (const c of inCols)  c.y = inAreaBottom - c.h;
  for (const c of outCols) c.y = outAreaTop;

  // X of a card at position pos in a row of rowLen cards inside column col
  const nodeXInCol = (col, pos, rowLen) => {
    const span = rowLen * SNW + (rowLen - 1) * 12;
    return col.x + (col.px_w - span) / 2 + pos * CELL;
  };

  // Cycle highlight state
  const cycleNodes = window.CYCLES?.[level]?.nodeCycleStatus;
  const isCycleNode = id => {
    const cs = cycleNodes?.get(id);
    if (cs == null || cs === 'none') return false;
    if (cs === 'both') return true;
    return (typeof viewMode === 'function' && viewMode() === 'current')
      ? cs === 'current-only'
      : cs === 'baseline-only';   // baseline, or review (single snapshot)
  };

  // Fit to the panel WIDTH (never upscale past natural size); height follows the
  // viewBox aspect, so a tall stack overflows and the container scrolls. The
  // `data-node-cy` fraction (main-node vertical centre ÷ VH) lets the modal
  // scroll the central node to the middle of the viewport on open.
  const nodeCyFrac = (MNY + MNH2 / 2) / VH;
  let s = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${VW} ${VH}" data-node-cy="${nodeCyFrac.toFixed(5)}" style="display:block;width:100%;max-width:${VW}px;height:auto;margin:auto">`;
  s += `<defs>` +
    `<marker id="ah" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><path d="M0,0 L0,6 L8,3z" fill="#4d6f9c"/></marker>` +
    `<marker id="ah-ext" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><path d="M0,0 L0,6 L8,3z" fill="#9aa0a6"/></marker>` +
    `<clipPath id="mn-clip"><rect x="${MNX+10}" y="${MNY}" width="${MNW-20}" height="${MNH2}"/></clipPath>` +
    `</defs>`;

  // Side node card. `item` = { node, kinds:Set, ext }.
  // External nodes: grey card with the full id only (no metrics).
  // Internal files: title (centred) + a `pr` badge for private modules, a
  // primary (left, abbreviated) / secondary (right) metric row, and a bottom
  // row of connection-kind slots split into thirds.
  let _snIdx = 0;
  // Escape a string for use inside a double-quoted SVG/HTML attribute.
  const escA = s => esc(s).replace(/"/g, '&quot;');

  // Build the edge-kind slot row for a side card. Shows every edge kind that
  // connects this neighbour (uses / reexport / contains) as a labelled,
  // hover-described slot; the edge_kinds dictionary drives the labels/tooltips.
  const buildKindRow = (item, x, y) => {
    const kindKeys = [...(item.kinds || [])];
    if (kindKeys.length === 0) return '';
    const thirdW = SNW / 3;
    // Up to 3 slots (uses / reexports / contains all fit).
    const shown = kindKeys.slice(0, 3);
    return shown.map((k, i) => {
      const label = edgeKindLabel(level, k);
      const desc  = edgeKindDesc(level, k);
      // Non-flow kinds (reexports / contains) carry no metric, so they would be
      // invisible on the map and easy to miss — show their label always. Flow
      // kinds (uses) stay in the hover detail next to the metric.
      const cls = edgeIsFlow(level, k) ? 'sn-detail sn-hint' : 'sn-hint';
      return `<text class="${cls}" data-tip="${escA(desc)}" x="${x + thirdW * (i + 0.5)}" y="${y+SNH-7}" text-anchor="middle" font-size="8" fill="#5c7a96">${esc(label)}</text>`;
    }).join('');
  };

  const sideNode = (item, x, y) => {
    const n       = item.node;
    const inMap   = nodeMap.has(n.id);
    const cycle   = isCycleNode(n.id);
    const ext     = item.ext || isExternalNode(n, level);
    const cursor  = inMap ? 'pointer' : 'default';
    const clipId  = `sn-clip-${_snIdx++}`;
    const fill    = ext ? '#ececec' : '#f0f4f8';
    const stroke  = cycle ? '#c00' : ext ? '#9aa0a6' : (inMap ? '#8ba6c0' : '#bbb');
    const strokeW = cycle ? '2' : '1';
    const mono    = `font-family="ui-monospace,'SF Mono',monospace"`;
    const clipDef = `<defs><clipPath id="${clipId}"><rect x="${x+4}" y="${y}" width="${SNW-8}" height="${SNH}"/></clipPath></defs>`;
    const cls     = [ext ? 'diag-ext' : (selectedIds?.has(n.id) ? 'diag-selected' : ''),
                     cycle ? 'diag-cycle' : ''].filter(Boolean).join(' ');
    const open    = `<g data-diag-node="${esc(n.id)}"${cls ? ` class="${cls}"` : ''} style="cursor:${cursor}">` +
      `<rect x="${x}" y="${y}" width="${SNW}" height="${SNH}" rx="6" fill="${fill}" stroke="${stroke}" stroke-width="${strokeW}"/>`;
    const pathTip = ext ? (n.path || n.id)
                        : ((n.path || '').replace(/^\{[^}]+\}\//, '') || n.id);

    if (ext) {
      const extName = n.name || n.id;
      return clipDef + open +
        `<g clip-path="url(#${clipId})"><text ${mono} fill="#2c3e50">` +
        `<tspan class="sn-hint" data-tip="${escA(pathTip)}" x="${x+SNW/2}" y="${y+SNH/2+4}" text-anchor="middle" font-size="11" font-weight="600">${esc(extName)}</tspan>` +
        `</text></g></g>`;
    }

    // Primary card metric (left, abbreviated when spec.abbreviate=true)
    const primVal = primaryKey != null ? nodeAttr(n, primaryKey) : null;
    const primSimple = primVal != null ? (fmtCard(primaryKey, primVal) ?? '') : '';
    const primDetail = primVal != null ? (fmtCard(primaryKey, primVal) ?? '0') : '0';
    const primShort  = primaryKey != null ? attrShort(level, primaryKey) : '';

    // Secondary card metric (right, plain)
    const secVal = secondaryKey != null ? nodeAttr(n, secondaryKey) : null;
    const secStr = secVal != null ? String(secVal) : '—';
    const secShort = secondaryKey != null ? attrShort(level, secondaryKey) : '';

    const priv  = typeof n.visibility === 'string' && n.visibility !== 'public';
    const ty = y + 36;  // metric row baseline

    let detailPrim = '';
    if (primaryKey != null) {
      const tipTitle   = escA(attrName(level, primaryKey));
      const tipDesc    = escA(attrDesc(level, primaryKey));
      const tipFormula = attrFormula(level, primaryKey) ? ` data-tip-formula="${escA(attrFormula(level, primaryKey))}"` : '';
      const tipCalc    = calcDisplay(level, primaryKey, n) ? ` data-tip-calc="${escA(calcDisplay(level, primaryKey, n))}"` : '';
      detailPrim = `<text class="sn-detail sn-hint" data-tip-title="${tipTitle}" data-tip="${tipDesc}"${tipFormula}${tipCalc} x="${x+8}" y="${ty}" font-size="10" fill="#5c7a96">${esc(primDetail)}:${esc(primShort.toLowerCase())}</text>`;
    }

    let detailSec = '';
    if (secondaryKey != null) {
      const tipTitle = escA(attrName(level, secondaryKey));
      const tipDesc  = escA(attrDesc(level, secondaryKey));
      detailSec = `<text class="sn-detail sn-hint" data-tip-title="${tipTitle}" data-tip="${tipDesc}" x="${x+SNW-8}" y="${ty}" text-anchor="end" font-size="10" fill="#5c7a96">${esc(secShort.toLowerCase())}:${esc(secStr)}</text>`;
    }

    const kindRow = buildKindRow(item, x, y);

    const prBadge = priv
      ? `<g class="sn-detail sn-hint" data-tip="${escA('This module has non-public visibility.')}">` +
        `<rect x="${x+SNW-26}" y="${y+4}" width="22" height="13" rx="3" fill="#e0d2b8" stroke="#b3801f" stroke-width="0.5"/>` +
        `<text ${mono} x="${x+SNW-15}" y="${y+14}" text-anchor="middle" font-size="9" fill="#7a5b18">pr</text></g>`
      : '';

    return clipDef + open +
      `<g clip-path="url(#${clipId})" ${mono} fill="#2c3e50">` +
      `<text class="sn-hint" data-tip="${escA(pathTip)}" x="${x+SNW/2}" y="${y+16}" text-anchor="middle" font-size="11" font-weight="600">${esc(nameOf(n))}</text>` +
      (primSimple  ? `<text class="sn-simple" x="${x+8}" y="${ty}" font-size="10" fill="#5c7a96">${esc(primSimple)}</text>` : '') +
      (secVal != null ? `<text class="sn-simple" x="${x+SNW-8}" y="${ty}" text-anchor="end" font-size="10" fill="#5c7a96">${esc(secStr)}</text>` : '') +
      detailPrim +
      detailSec +
      kindRow +
      `</g>` + prBadge + `</g>`;
  };

  // Render one column (dashed box + optional header + node cards).
  const renderCol = col => {
    const color = kindColor(col.kind);
    const dash  = kindDash(col.kind);
    let r = '';
    r += `<rect x="${col.x}" y="${col.y}" width="${col.px_w}" height="${col.h}" rx="8" fill="none" stroke="${color}" stroke-width="1.5" stroke-dasharray="${dash}"/>`;
    if (col.ext) {
      const label = `external  ${col.all.length}`;
      r += `<text x="${col.x+10}" y="${col.y+13}" font-family="system-ui,sans-serif" font-size="10" fill="${color}" font-weight="600">${label}</text>`;
    }
    col.rows.forEach((row, ri) =>
      row.forEach((item, pi) =>
        r += sideNode(item, nodeXInCol(col, pi, row.length), col.y + PAD_TOP + ri * ROW_H)
      )
    );
    return r;
  };

  // Fan-in columns (above main node, bottom-anchored) — one arrow per column
  if (inCols.length > 0) {
    inCols.forEach(c => {
      s += renderCol(c);
      const cx  = Math.round(c.x + c.px_w / 2);
      const my  = Math.round((c.y + c.h + MNY) / 2);
      const stroke = c.ext ? '#9aa0a6' : '#4d6f9c';
      const marker = c.ext ? 'ah-ext' : 'ah';
      s += `<line x1="${cx}" y1="${c.y + c.h}" x2="${cx}" y2="${MNY}" stroke="${stroke}" stroke-width="1.5" marker-end="url(#${marker})"/>`;
      // Fan-in is the flow-edge metric; the column may also show non-flow
      // neighbours (reexports / contains), so label with the metric, not the
      // card count, and only when there is flow coupling to report.
      if (!c.ext && node.fan_in != null && node.fan_in > 0)
        s += `<text x="${cx+5}" y="${my+4}" font-family="system-ui,sans-serif" font-size="10" fill="#5c7a96">Fan-in: ${node.fan_in}</text>`;
    });
  }

  // Main node
  const mono = `font-family="ui-monospace,'SF Mono','Fira Code',monospace"`;
  // Monospace char width ≈ 0.6 × font-size; the key/value rows render at 14px.
  const mnValTrunc = (label, v) => trunc(v, Math.max(4, Math.floor((MNW - 20 - label.length * 8.4) / 8.4)));
  const mnCycle = isCycleNode(node.id);
  const mnExt   = isExternalNode(node, level);
  const mnFill   = mnExt ? '#ececec' : '#dbe9f4';
  const mnStroke = mnCycle ? '#c00' : mnExt ? '#9aa0a6' : '#4d6f9c';
  // For project files the id IS the relativized path (a `path` attr is dropped
  // when it equals the id), so fall back to the id; then strip the leading root
  // token to get the repo-relative path.
  const nodePath = (node.path || node.id || '').replace(/^\{[^}]+\}\//, '');
  const copyVal = mnExt ? node.id : nodePath;
  // Absolute on-disk path (token expanded) for the path tooltip.
  const absFull = absPath(mnExt ? (node.path || node.id) : node.id);
  const mnCls = [mnExt ? 'diag-ext' : (selectedIds?.has(node.id) ? 'diag-selected' : ''),
                 mnCycle ? 'diag-cycle' : ''].filter(Boolean).join(' ');
  // Copying is per-label (each `.mn-copy` text copies its own value on click),
  // not whole-card — so a stray click on the card never copies. `copyVal` is kept
  // only as the initial "copied" preview text.
  s += `<g class="mn-card${mnCls ? ' ' + mnCls : ''}" data-node-id="${esc(node.id)}">`;
  s += `<rect x="${MNX}" y="${MNY}" width="${MNW}" height="${MNH2}" rx="10" fill="${mnFill}" stroke="${mnStroke}" stroke-width="${mnCycle ? '3' : '2'}"/>`;
  s += `<g class="mn-card-body" clip-path="url(#mn-clip)">`;

  if (mnExt) {
    // External node main card: title + whatever attributes the node has, labelled
    // generically via attrLabel (no hardcoded key names or tool-specific copy).
    const extName = node.name || node.id;
    let ey = MNY + 58;
    s += `<text class="mn-copy" data-copy="${escA(extName)}" ${mono} x="${MNX+MNW/2}" y="${MNY+28}" text-anchor="middle" font-size="16" font-weight="700" fill="#1a2f45">${esc(trunc(extName, 36))}</text>`;
    // Always show kind.
    const kindDesc = nodeKindSpec(level, node.kind).label || node.kind || 'external';
    s += `<text class="sn-hint" data-tip-title="${escA(attrLabel(level, 'external'))}" data-tip="${escA(attrDesc(level, 'external'))}" ${mono} x="${MNX+14}" y="${ey}" font-size="14" fill="#2c3e50"><tspan font-weight="700">kind: </tspan>${esc(node.kind || 'external')}</text>`;
    if (node.version != null) {
      ey += 22;
      const vDesc = attrDesc(level, 'version');
      const vTip  = vDesc ? ` class="sn-hint" data-tip-title="${escA(attrLabel(level, 'version'))}" data-tip="${escA(vDesc)}"` : '';
      s += `<text${vTip} ${mono} x="${MNX+14}" y="${ey}" font-size="14" fill="#2c3e50"><tspan font-weight="700">version: </tspan>${esc(node.version)}</text>`;
    }
    if (node.path) {
      ey += 22;
      // Card keeps the compact `{registry}`/`{cargo}` token form; the tooltip
      // shows the expanded on-disk location.
      s += `<text class="sn-hint mn-copy" data-copy="${escA(node.path)}" data-tip-title="${escA(attrLabel(level, 'path') || 'Path')}" data-tip="${escA(absFull || node.path)}" ${mono} x="${MNX+14}" y="${ey}" font-size="14" fill="#2c3e50"><tspan font-weight="700">path: </tspan>${esc(mnValTrunc('path: ', node.path))}</text>`;
    }
  } else {
    s += `<text class="mn-copy" data-copy="${escA(node.name||node.id)}" ${mono} x="${MNX+MNW/2}" y="${MNY+28}" text-anchor="middle" font-size="16" font-weight="700" fill="#1a2f45">${esc(trunc(node.name||node.id, 36))}</text>`;
    // Visibility shown in the card only when NOT public.
    const visStr = typeof node.visibility === 'string' && node.visibility !== 'public'
      ? node.visibility : null;
    let my = MNY + 58;
    if (visStr) {
      s += `<text class="mn-copy" data-copy="${escA(visStr)}" ${mono} x="${MNX+14}" y="${my}" font-size="14" fill="#2c3e50"><tspan font-weight="700">visibility: </tspan>${esc(visStr)}</text>`;
      my += 22;
    }
    // Tooltip shows the absolute on-disk path (the displayed value is the
    // project-relative, truncated path).
    s += `<text class="sn-hint mn-copy" data-copy="${escA(nodePath)}" data-tip-title="${escA(attrLabel(level, 'path') || 'Path')}" data-tip="${escA(absFull || nodePath)}" ${mono} x="${MNX+14}" y="${my}" font-size="14" fill="#2c3e50"><tspan font-weight="700">path: </tspan>${esc(mnValTrunc('path: ', nodePath))}</text>`;
    my += 22;

    // Grouping field (e.g. `crate`): show it as its own row unless it is already
    // displayed (path / visibility) or surfaced as a card metric.
    const groupKey = ui.grouping?.key;
    const shownKeys = new Set(['path', 'visibility', primaryKey, secondaryKey].filter(k => k != null));
    if (groupKey && !shownKeys.has(groupKey)) {
      const gVal = nodeAttr(node, groupKey);
      if (gVal != null && gVal !== '') {
        const gLabel = (attrLabel(level, groupKey) || groupKey).toLowerCase();
        const gDesc  = attrDesc(level, groupKey);
        const gTip   = gDesc
          ? ` class="sn-hint mn-copy" data-tip-title="${escA(attrName(level, groupKey) || attrLabel(level, groupKey) || groupKey)}" data-tip="${escA(gDesc)}"`
          : ` class="mn-copy"`;
        s += `<text${gTip} data-copy="${escA(String(gVal))}" ${mono} x="${MNX+14}" y="${my}" font-size="14" fill="#2c3e50"><tspan font-weight="700">${esc(gLabel)}: </tspan>${esc(mnValTrunc(gLabel + ': ', String(gVal)))}</text>`;
        my += 22;
      }
    }

    // Primary card metric row
    if (primaryKey != null) {
      const primRaw = nodeAttr(node, primaryKey);
      const primFmt = primRaw != null ? (fmtCard(primaryKey, primRaw) ?? '0') : '0';
      const primName = attrShort(level, primaryKey).toLowerCase();
      const tipTitle   = escA(attrName(level, primaryKey));
      const tipDesc    = escA(attrDesc(level, primaryKey));
      const tipFormula = attrFormula(level, primaryKey) ? ` data-tip-formula="${escA(attrFormula(level, primaryKey))}"` : '';
      const tipCalc    = calcDisplay(level, primaryKey, node) ? ` data-tip-calc="${escA(calcDisplay(level, primaryKey, node))}"` : '';
      s += `<text class="sn-hint mn-copy" data-copy="${escA(primFmt)}" data-tip-title="${tipTitle}" data-tip="${tipDesc}"${tipFormula}${tipCalc} ${mono} x="${MNX+14}" y="${my}" font-size="14" fill="#2c3e50"><tspan font-weight="700">${esc(primName)}: </tspan>${esc(primFmt)}</text>`;
      my += 22;
    }

    // Secondary card metric row
    if (secondaryKey != null) {
      const secRaw = nodeAttr(node, secondaryKey);
      const secFmt = secRaw != null ? String(secRaw) : '—';
      const secName = attrShort(level, secondaryKey).toLowerCase();
      const tipTitle = escA(attrName(level, secondaryKey));
      const tipDesc  = escA(attrDesc(level, secondaryKey));
      s += `<text class="sn-hint mn-copy" data-copy="${escA(secFmt)}" data-tip-title="${tipTitle}" data-tip="${tipDesc}" ${mono} x="${MNX+14}" y="${my}" font-size="14" fill="#2c3e50"><tspan font-weight="700">${esc(secName)}: </tspan>${esc(secFmt)}</text>`;
    }
  }
  s += `</g>`;
  // Shown for ~1s after a copy (the body is hidden meanwhile, see index.css):
  s += `<text class="mn-copied-msg mn-copied-val" ${mono} x="${MNX+MNW/2}" y="${MNY+MNH2/2-8}" text-anchor="middle" font-size="11" fill="#5c7a96">${esc(mnValTrunc('', copyVal))}</text>`;
  s += `<text class="mn-copied-msg" ${mono} x="${MNX+MNW/2}" y="${MNY+MNH2/2+18}" text-anchor="middle" font-size="20" font-weight="700" fill="#4d6f9c">copied</text>`;
  s += `</g>`;

  // Fan-out columns (below main node, top-anchored) — one arrow per column
  if (outCols.length > 0) {
    outCols.forEach(c => {
      const cx  = Math.round(c.x + c.px_w / 2);
      const my  = Math.round((MNY + MNH2 + c.y) / 2);
      const stroke = c.ext ? '#9aa0a6' : '#4d6f9c';
      const marker = c.ext ? 'ah-ext' : 'ah';
      s += `<line x1="${cx}" y1="${MNY+MNH2}" x2="${cx}" y2="${c.y}" stroke="${stroke}" stroke-width="1.5" marker-end="url(#${marker})"/>`;
      // Fan-out is the flow-edge metric; the column may also show non-flow
      // neighbours (reexports / contains), so label with the metric, not the
      // card count, and only when there is flow coupling to report.
      if (!c.ext && node.fan_out != null && node.fan_out > 0)
        s += `<text x="${cx+5}" y="${my+4}" font-family="system-ui,sans-serif" font-size="10" fill="#5c7a96">Fan-out: ${node.fan_out}</text>`;
      s += renderCol(c);
    });
  }

  s += '</svg>';
  return s;
}

// Convert a git remote `origin` URL into its web base (https://host/group/proj),
// handling scp-style SSH (git@host:group/proj.git), ssh:// and https remotes.
function gitWebBase(origin) {
  if (!origin) return null;
  const s = String(origin).trim();
  if (/^https?:\/\//i.test(s)) {
    return s.replace(/^(https?:\/\/)[^@/]+@/i, '$1')  // drop embedded credentials
            .replace(/\.git\/?$/i, '')
            .replace(/\/$/, '');
  }
  // scp-like (`git@host:group/proj.git`) or `ssh://git@host/group/proj.git`.
  const m = s.match(/^(?:ssh:\/\/)?(?:[^@]+@)?([^:/]+)[:/](.+?)(?:\.git)?\/?$/);
  return m ? `https://${m[1]}/${m[2]}` : null;
}

// Build a blob link to a project file at the analysed commit. `relPath` is the
// repo-relative path (the displayed path, with the `{root}/` token stripped).
// The node id IS the relativized path. An optional `line` adds a `#L<n>` anchor
// (GitHub and GitLab both use that form).
function gitSourceUrl(git, relPath, line) {
  const base = gitWebBase(git?.origin);
  if (!base || !relPath) return null;
  const ref  = git.commit || git.branch || 'HEAD';
  const enc  = relPath.split('/').map(encodeURIComponent).join('/');
  const blob = /(^|\/)github\.com\//i.test(base) ? 'blob' : '-/blob';   // GitLab uses /-/blob/
  const anchor = (line != null && Number.isFinite(+line)) ? `#L${line}` : '';
  return `${base}/${blob}/${ref}/${enc}${anchor}`;
}

// Git-host source URL for a node: only project files (external nodes live
// elsewhere). The node id IS its relativized path; strip the leading `{...}/`
// root token to get the repo-relative path. Returns null for external nodes.
// An optional `line` adds a `#L<n>` anchor to the blob URL.
function nodeSourceUrl(node, level, line) {
  if (!node) return null;
  if (level != null && isExternalNode(node, level)) return null;
  // Fallback for callers that don't pass level: check node.external flag.
  if (node.external === true) return null;
  // Use node.id as the path (strip the root token).
  const rel = (node.id || '').replace(/^\{[^}]+\}\//, '');
  if (!rel) return null;
  return gitSourceUrl(activeSnap()?.git, rel, line);
}
// Expose on window so modal.js can use it from click handlers.
window.nodeSourceUrl = nodeSourceUrl;

// Line to anchor when opening a fan-in neighbour's source from the popup. Only
// edges where the neighbour is the *source* and the central node is the target
// are considered — for those the edge's `line` (the `use` site) lives in the
// neighbour's own file. Pick the first flow edge (e.g. `uses`) that carries a
// line, else the edge with the largest line. Returns null when there is no such
// edge (e.g. a pure fan-out card, where the line would belong to the central
// file instead) so the caller opens the URL without an anchor.
function connSourceLine(neighbourId, centralId, level) {
  const edges = (activeGraph(level).edges || [])
    .filter(e => e.source === neighbourId && e.target === centralId && e.line != null);
  if (!edges.length) return null;
  const flow = edges.find(e => edgeIsFlow(level, e.kind));
  if (flow) return flow.line;
  return edges.reduce((m, e) => (e.line > m.line ? e : m)).line;
}
window.connSourceLine = connSourceLine;

// Reconstruct the absolute on-disk path from a relativized id/path: replace the
// leading `{token}/` with the snapshot's real root — `{target}` → the analyzed
// project dir, a named root (`{registry}` …) → `roots[token]`. Returns the input
// unchanged when there is no token or the root is unknown. Used for the path
// tooltip in the node popup.
function absPath(idOrPath) {
  const snap = activeSnap();
  const m = /^\{([^}]+)\}\/(.*)$/.exec(idOrPath || '');
  if (!snap || !m) return idOrPath || '';
  const base = m[1] === 'target' ? (snap.target ?? snap.roots?.target) : snap.roots?.[m[1]];
  return base ? `${base}/${m[2]}` : (idOrPath || '');
}

function buildModalContent(node, level) {
  const cycles  = window.CYCLES?.[level];
  const cs      = cycles?.nodeCycleStatus?.get(node.id);
  const mnExt   = isExternalNode(node, level);
  // Displayed path: external keeps its compact `{registry}`/`{cargo}` token
  // form; for project files the id IS the relativized path (the `path` attr is
  // dropped when equal to the id), so fall back to the id, then drop the leading
  // root token to get the repo-relative path.
  const path    = mnExt ? (node.path || node.id || '')
                        : (node.path || node.id || '').replace(/^\{[^}]+\}\//, '');
  // Absolute on-disk path (token expanded) for the Path-row tooltip.
  const absFull = absPath(mnExt ? (node.path || node.id) : node.id);
  const vis     = typeof node.visibility === 'string' ? node.visibility : null;

  // sections: array of { label: string|null, rows: string[] }
  const sections = [];
  let cur = { label: null, rows: [] };

  const tipAttr = s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;');

  // Build a field row. `key` is the attribute key; `v` is the formatted value
  // string; optional `calc` is the live derivation line.
  const row = (key, v, opts) => {
    if (v == null || v === '') return;
    const label   = attrLabel(level, key) || (key.charAt(0).toUpperCase() + key.slice(1));
    const title   = attrName(level, key)  || label;
    const desc    = attrDesc(level, key);
    const formula = attrFormula(level, key);
    const calc    = opts?.calc || '';
    const attr = desc
      ? ` data-tip="${tipAttr(desc)}" data-tip-title="${tipAttr(title)}"` +
        (formula ? ` data-tip-formula="${tipAttr(formula)}"` : '') +
        (calc    ? ` data-tip-calc="${tipAttr(calc)}"` : '')
      : '';
    cur.rows.push(`<tr${attr}><td class="nm-key">${label}</td><td class="nm-val">${v}</td></tr>`);
  };

  // A plain row with no schema lookup (for id, path, source — structural fields).
  const rawRow = (label, valHtml, tipTitle, tipDesc) => {
    const attr = tipDesc
      ? ` data-tip="${tipAttr(tipDesc)}" data-tip-title="${tipAttr(tipTitle || label)}"`
      : '';
    cur.rows.push(`<tr${attr}><td class="nm-key">${label}</td><td class="nm-val">${valHtml}</td></tr>`);
  };

  const sect = label => { sections.push(cur); cur = { label, rows: [] }; };

  const esc = s => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');

  // Format a numeric value: integers without decimals, floats with up to `d`
  // significant decimal places and thousands separators.
  const fmt = (v, d) => {
    if (v == null) return null;
    const s = d > 0
      ? parseFloat(v.toFixed(d)).toFixed(d).replace(/\.0+$/, '').replace(/(\.\d*[1-9])0+$/, '$1')
      : String(Math.round(v));
    const [int, dec] = s.includes('.') ? s.split('.') : [s, ''];
    const fi = int.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
    return dec ? `${fi}.${dec}` : fi;
  };
  const n3 = v => v == null ? null : String(Math.round(v)).replace(/\B(?=(\d{3})+(?!\d))/g, ',');

  // ── Structural fields ─────────────────────────────────────────────────────

  if (path) {
    const si   = path.lastIndexOf('/');
    const dir  = si >= 0 ? esc(path.slice(0, si + 1)) : '';
    const file = esc(si >= 0 ? path.slice(si + 1) : path);
    rawRow('Path',
      `${dir}<strong>${file}</strong>`,
      attrName(level, 'path') || 'Path',
      absFull || attrDesc(level, 'path') || 'Location of this node.'
    );
    // Source link for project files (not for external nodes).
    if (!mnExt) {
      const url = nodeSourceUrl(node, level);
      if (url) {
        const host = url.replace(/^https?:\/\//i, '').split('/')[0];
        cur.rows.push(
          `<tr><td class="nm-key">Source</td><td class="nm-val">` +
          `<a class="nm-src" href="${esc(url)}" target="_blank" rel="noopener noreferrer">${esc(host)} ↗</a>` +
          `</td></tr>`
        );
      }
    }
  }

  // id for external nodes
  if (mnExt) row('id', node.id);
  row('kind', node.kind || null);
  row('version', node.version ?? null);
  if (mnExt) row('external', 'true');
  // visibility: only when present and not "public"
  if (vis && vis !== 'public') row('visibility', vis);
  if (node.items != null) row('items', n3(node.items));
  // node.cycle is the cycle kind (mutual/chain/…); cs is the diff-side status
  // (both/baseline-only/current-only) computed at runtime from window.CYCLES.
  if (node.cycle != null) row('cycle', node.cycle);
  if (cs && cs !== 'none') rawRow('Cycle status', cs, 'Cycle status', 'Whether this cycle exists on the baseline side, current side, or both.');
  if (!document.body.classList.contains('mode-review')) row('status', node.status);

  // ── Numeric metric sections, driven by numericAttrKeys + attribute_groups ─

  // Group keys by their `group` field (preserving declaration order).
  const numKeys = numericAttrKeys(level);
  const groups  = attributeGroups(level);  // { id: { label, description } }

  // Collect keys that have a non-null value on this node, grouped.
  const grouped = {};   // groupId → [key, ...]
  const ungrouped = []; // keys with no group
  for (const k of numKeys) {
    const v = nodeAttr(node, k);
    if (v == null) continue;
    const g = attrGroup(level, k);
    if (g) {
      if (!grouped[g]) grouped[g] = [];
      grouped[g].push(k);
    } else {
      ungrouped.push(k);
    }
  }

  // Emit ungrouped numeric keys first (no section header).
  if (ungrouped.length > 0) {
    sect(null);
    for (const k of ungrouped) {
      const v  = nodeAttr(node, k);
      const ty = attrType(level, k);
      const formatted = ty === 'float' ? fmt(v, 2) : n3(v);
      row(k, formatted, { calc: calcDisplay(level, k, node) });
    }
  }

  // Emit each group in the order they appear in attribute_groups.
  const groupOrder = Object.keys(groups);
  // Emit groups that appear in attribute_groups first, then any remaining.
  const allGroupIds = [
    ...groupOrder.filter(g => grouped[g]),
    ...Object.keys(grouped).filter(g => !groupOrder.includes(g)),
  ];

  for (const gId of allGroupIds) {
    const keys = grouped[gId];
    if (!keys || keys.length === 0) continue;
    const gLabel = groups[gId]?.label || gId;
    sect(gLabel);
    for (const k of keys) {
      const v  = nodeAttr(node, k);
      const ty = attrType(level, k);
      const formatted = ty === 'float' ? fmt(v, 2) : n3(v);
      row(k, formatted, { calc: calcDisplay(level, k, node) });
    }
  }

  sections.push(cur);

  const renderSect = s =>
    `${s.label ? `<div class="nm-sect-label">${s.label}</div>` : ''}` +
    `<table class="nm-table">${s.rows.join('')}</table>`;

  const body = sections.filter(s => s.rows.length > 0).map(renderSect).join('');

  const sideSuffix = (typeof viewModeSuffix === 'function') ? viewModeSuffix().trim() : '';
  return {
    hdr:      `<span class="nm-title">${node.name}</span><span class="nm-badge">${node.kind}</span>` +
              (sideSuffix ? `<span class="nm-side">${sideSuffix}</span>` : ''),
    body,
    diagram:  buildDiagramSVG(node, level),
  };
}

// Reflect a node's selection on EVERY popup-diagram card for it. A node in a
// dependency cycle appears twice — once as fan-in (top) and once as fan-out
// (bottom) — plus possibly as the central card, so all instances must update.
function markPopupSelected(nodeId, sel) {
  const id = CSS.escape(nodeId);
  document.querySelectorAll(
    `#node-modal-diagram [data-diag-node="${id}"], #node-modal-diagram .mn-card[data-node-id="${id}"]`
  ).forEach(el => el.classList.toggle('diag-selected', sel));
}
window.markPopupSelected = markPopupSelected;

// Toggle a node's selection from the map, mirroring the table-row checkbox:
// keep the shared selectedIds Set, the SVG highlight, the table row + checkbox,
// the popup-diagram cards, and the "N selected" footer all in sync.
function toggleNodeSelected(node, level, section) {
  if (!window._ntSelected) window._ntSelected = {};
  if (!window._ntSelected[level]) window._ntSelected[level] = new Set();
  const selectedIds = window._ntSelected[level];

  const sel = !selectedIds.has(node.id);
  if (sel) selectedIds.add(node.id); else selectedIds.delete(node.id);

  section?._gNodeMap?.get(node.id)?.classList.toggle('node-selected', sel);

  const row = section?.querySelector(
    `.node-table-body .node-table tbody tr[data-node-id="${CSS.escape(node.id)}"]`);
  if (row) {
    row.classList.toggle('row-selected', sel);
    const cb = row.querySelector('.nt-cb');
    if (cb) cb.checked = sel;
  }
  markPopupSelected(node.id, sel);
  section?._updateAllCb?.();
}

// The "open source" modifier is platform-specific: ⌘ (Meta) on macOS — where
// Ctrl is deliberately left alone (it maps to right-click) — and Ctrl elsewhere.
const IS_MAC = /Mac|iP(hone|ad|od)/.test(
  (typeof navigator !== 'undefined' && (navigator.platform || navigator.userAgent)) || ''
);
const OPEN_SRC_KEY = IS_MAC ? 'Meta' : 'Control';
const isOpenSrcClick = e => (IS_MAC ? e.metaKey : e.ctrlKey);
// Exposed on window so modal.js (the popup diagram) can mirror the gesture —
// `const` declarations are not auto-attached to the global object.
window.isOpenSrcClick = isOpenSrcClick;

// Shortcut-legend markup with the platform's actual keys; reused by the main map
// (`#kbd-hints`) and the popup (`#node-modal-hints`, filled in modal.js).
function kbdHintsHtml() {
  const srcKey = IS_MAC ? '⌘' : 'Ctrl';
  return `<span class="kbd-hint"><kbd>⇧ Shift</kbd> + click — select node</span>` +
         `<span class="kbd-hint"><kbd>${srcKey}</kbd> + click — view source</span>` +
         `<span class="kbd-hint kbd-hint-toggle"><kbd>t</kbd> — toggle baseline/current</span>`;
}
window.kbdHintsHtml = kbdHintsHtml;

// Map modifier modes, each changing the cursor (see the CSS) and rerouting node
// clicks (see the click handler in setupTooltips):
//   • Shift (`.shift-select`)      — toggle a node's selection instead of the modal;
//   • ⌘ (mac) / Ctrl (`.ctrl-link`) — open the node's source on the git host.
(function initMapModifiers() {
  const setShift = on => document.body.classList.toggle('shift-select', on);
  const setSrc   = on => document.body.classList.toggle('ctrl-link', on);

  // Fill the bottom-left shortcut legend with the platform's actual keys.
  const hints = document.getElementById('kbd-hints');
  if (hints) hints.innerHTML = kbdHintsHtml();
  window.addEventListener('keydown', e => {
    if (e.key === 'Shift') setShift(true);
    if (e.key === OPEN_SRC_KEY) setSrc(true);
  });
  window.addEventListener('keyup', e => {
    if (e.key === 'Shift') setShift(false);
    if (e.key === OPEN_SRC_KEY) setSrc(false);
  });
  window.addEventListener('blur', () => { setShift(false); setSrc(false); });
})();

function setupTooltips(svgFrame, level) {
  svgFrame.querySelectorAll('g.edge title, g.cluster title').forEach(t => t.remove());

  // The SVG is the union (baseline+current) layout, so map EVERY union node — not just
  // the active side's — or baseline-only (removed) / current-only (added) nodes would
  // lack click handlers and a `_gNodeMap` entry on the side where they're visible.
  const nodeMap  = new Map(unionGraph(level).nodes.map(n => [n.id, n]));
  const section  = svgFrame.closest('.view');
  const gNodeMap = new Map();

  svgFrame.querySelectorAll('g.node').forEach(g => {
    const titleEl = g.querySelector('title');
    const nodeId  = titleEl?.textContent?.trim();
    titleEl?.remove();

    const node = nodeMap.get(nodeId);
    if (!node) return;

    g.dataset.nodeId = nodeId;
    gNodeMap.set(nodeId, g);

    g.style.cursor = 'pointer';
    g.addEventListener('click', e => {
      e.stopPropagation();
      // ⌘ (mac) / Ctrl = "open source": jump to the file on the git host.
      if (isOpenSrcClick(e)) {
        const url = nodeSourceUrl(node, level);
        if (url) window.open(url, '_blank', 'noopener');
        return;
      }
      // Shift = "select mode": toggle this node's selection.
      if (e.shiftKey) { toggleNodeSelected(node, level, section); return; }
      // Route through openModalForNode so the modal show / header flyout / open-node
      // tracking all live in one place.
      if (window.openModalForNode?.(node.id, level)) window.navPush?.(level, node.id);
    });

    g.addEventListener('mouseenter', () => {
      g.classList.add('node-hl');
      const row = section?.querySelector(`tr[data-node-id="${nodeId.replace(/\\/g,'\\\\').replace(/"/g,'\\"')}"]`);
      row?.classList.add('row-hl');
    });
    g.addEventListener('mouseleave', () => {
      g.classList.remove('node-hl');
      section?.querySelector(`tr[data-node-id="${nodeId.replace(/\\/g,'\\\\').replace(/"/g,'\\"')}"]`)
              ?.classList.remove('row-hl');
    });
  });

  if (section) section._gNodeMap = gNodeMap;
}

// Above this many nodes, laying out the graph with graphviz is slow, so we ask
// for explicit confirmation before rendering (once per frame).
const SVG_NODE_LIMIT = 500;

function drawSVG(svgFrame, nodes, edges, level) {
  if (nodes.length > SVG_NODE_LIMIT && svgFrame.dataset.bigConfirmed !== '1') {
    svgFrame.innerHTML =
      `<div class="too-many">` +
        `<div class="too-many-title">too many nodes: ${nodes.length}</div>` +
        `<div class="too-many-sub">Rendering the full diagram may be slow. Render it anyway?</div>` +
        `<button class="too-many-btn" type="button">Render diagram</button>` +
      `</div>`;
    svgFrame.querySelector('.too-many-btn').addEventListener('click', () => {
      svgFrame.dataset.bigConfirmed = '1';
      // Show the same "Computing layout…" indicator as a mode switch, and defer
      // the (slow, blocking) graphviz layout one tick so the indicator actually
      // paints before the main thread is busy.
      const loading = svgFrame.closest('[data-view]')?.querySelector('.loading-indicator');
      if (loading) { loading.textContent = 'Computing layout…'; loading.classList.add('on'); }
      setTimeout(() => {
        renderSVGNow(svgFrame, nodes, edges, level);
        if (loading) loading.classList.remove('on');
      }, 30);
    });
    return;
  }
  renderSVGNow(svgFrame, nodes, edges, level);
}

function renderSVGNow(svgFrame, nodes, edges, level) {
  const dot = buildDOT(nodes, edges, level);
  const svgStr = window.gv.dot(dot);
  svgFrame.innerHTML = svgStr;
  const svg = svgFrame.querySelector('svg');
  if (svg) {
    svg.setAttribute('width', '100%');
    svg.setAttribute('height', '100%');
    svg.style.display = 'block';
    setupPanZoom(svgFrame, svg);
    setupTooltips(svgFrame, level);
  }
}

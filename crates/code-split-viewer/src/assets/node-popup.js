// node-popup.js — the per-node neighbourhood SVG diagram shown inside the modal
// (buildDiagramSVG) and the helper that mirrors a node selection across every
// card for that node (markPopupSelected). Split out of the former diagram.js.

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

  // Cross-crate detection: a neighbour whose grouping value (e.g. `crate`) differs
  // from the main node's. Such callers/dependencies get the same green/yellow tint
  // as the map's callers/dependencies clusters.
  const _groupKey  = ui.grouping?.key;
  const _mainCrate = _groupKey != null ? nodeAttr(node, _groupKey) : null;
  const isCrossCrate = n => _groupKey != null && _mainCrate != null
    && nodeAttr(n, _groupKey) != null && nodeAttr(n, _groupKey) !== _mainCrate;

  // Abbreviated number for the card (e.g. 189,000 → 189K, 1,500,000 → 1.5M).
  // Respects `abbreviate:true` in the spec; otherwise uses plain fmtNum.
  const fmtCard = (key, v) => {
    if (v == null) return null;
    if (attrAbbrev(level, key)) {
      v = typeof v === 'number' ? v : Number(v);
      if (!isFinite(v)) return null;
      v = Math.round(v);
      // Whole-number magnitudes only — the K/M suffix is already approximate, so
      // no decimal digit (1500000 → 2M, 189000 → 189K).
      if (v >= 1e6) return Math.round(v / 1e6) + 'M';
      if (v >= 1e3) return Math.round(v / 1e3) + 'K';
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

  const sideNode = (item, x, y, dir) => {
    const n       = item.node;
    const inMap   = nodeMap.has(n.id);
    const cycle   = isCycleNode(n.id);
    const ext     = item.ext || isExternalNode(n, level);
    const clipId  = `sn-clip-${_snIdx++}`;
    // Cross-crate callers get the green / dependencies the yellow tint of the
    // map's callers/dependencies clusters; same-crate neighbours stay neutral.
    const xc      = !ext && isCrossCrate(n);
    const fill    = ext                   ? '#ececec'
                  : xc && dir === 'in'    ? '#edf7ed'
                  : xc && dir === 'out'   ? '#fdf3e3'
                  :                         '#f0f4f8';
    const stroke  = cycle ? '#c00' : ext ? '#9aa0a6' : (inMap ? '#8ba6c0' : '#bbb');
    const strokeW = cycle ? '2' : '1';
    // Dashed outline when the neighbour is NOT counted in fan_in/fan_out — i.e. it
    // links only through non-flow edges (contains / reexports), not a `uses` flow.
    const isFlow  = [...(item.kinds || [])].some(k => edgeIsFlow(level, k));
    const dash    = isFlow ? '' : ' stroke-dasharray="5,3"';
    const mono    = `font-family="ui-monospace,'SF Mono',monospace"`;
    const clipDef = `<defs><clipPath id="${clipId}"><rect x="${x+4}" y="${y}" width="${SNW-8}" height="${SNH}"/></clipPath></defs>`;
    const cls     = [ext ? 'diag-ext' : (selectedIds?.has(n.id) ? 'diag-selected' : ''),
                     cycle ? 'diag-cycle' : '',
                     inMap ? '' : 'sn-static'].filter(Boolean).join(' ');   // cursor via CSS
    const open    = `<g data-diag-node="${esc(n.id)}"${cls ? ` class="${cls}"` : ''}>` +
      `<rect x="${x}" y="${y}" width="${SNW}" height="${SNH}" rx="6" fill="${fill}" stroke="${stroke}" stroke-width="${strokeW}"${dash}/>`;
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

    // Hover tooltip: file name (title) + crate and the full repo-relative path
    // (`/foo/bar` — the `{token}` root marker stripped, leading slash kept).
    const crateVal = _groupKey != null ? nodeAttr(n, _groupKey) : null;
    const relPath  = String(n.path || n.id || '').replace(/^\{[^}]+\}/, '');
    const tipBody  = [
      crateVal != null && crateVal !== '' ? `crate: ${crateVal}` : '',
      relPath ? `path: ${relPath}` : '',
    ].filter(Boolean).join('<br>');

    return clipDef + open +
      `<g clip-path="url(#${clipId})" ${mono} fill="#2c3e50">` +
      `<text class="sn-hint" data-tip-title="${escA(n.name || n.id)}" data-tip="${escA(tipBody)}" x="${x+SNW/2}" y="${y+16}" text-anchor="middle" font-size="11" font-weight="600">${esc(nameOf(n))}</text>` +
      (primSimple  ? `<text class="sn-simple" x="${x+8}" y="${ty}" font-size="10" fill="#5c7a96">${esc(primSimple)}</text>` : '') +
      (secVal != null ? `<text class="sn-simple" x="${x+SNW-8}" y="${ty}" text-anchor="end" font-size="10" fill="#5c7a96">${esc(secStr)}</text>` : '') +
      detailPrim +
      detailSec +
      kindRow +
      `</g>` + prBadge + `</g>`;
  };

  // Render one column (dashed box + optional header + node cards).
  const renderCol = (col, dir) => {
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
        r += sideNode(item, nodeXInCol(col, pi, row.length), col.y + PAD_TOP + ri * ROW_H, dir)
      )
    );
    return r;
  };

  // Fan-in columns (above main node, bottom-anchored) — one arrow per column
  if (inCols.length > 0) {
    inCols.forEach(c => {
      s += renderCol(c, 'in');
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
      // Central card is roomy → verbatim value, no abbreviation (side cards abbreviate).
      const primFmt = primRaw != null ? (fmtFull(primRaw) ?? '0') : '0';
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
      const secFmt = secRaw != null ? (fmtFull(secRaw) ?? '—') : '—';
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
      s += renderCol(c, 'out');
    });
  }

  s += '</svg>';
  return s;
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


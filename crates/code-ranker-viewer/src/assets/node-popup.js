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

  const esc      = escHtml;
  const trunc    = (s, n) => s.length > n ? s.slice(0, n - 1) + '…' : s;
  const nameOf   = n => trunc(n.name || n.id.split('::').pop() || n.id, 18);

  // Card-metric keys driven by ui.card (e.g. ["hk","sloc"]).
  const ui          = levelUi(level);
  const cardMetrics = ui.card || [];
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

  // ── Layout: card blocks stacked vertically, 5 cards per row ──────────────────
  //   external          (external callers)
  //   crate in: a / b…  (cross-crate callers, one block per crate)  ── arrow ──┐
  //   fan in            (same-crate callers)                        ── arrow ──┤
  //   [ main node ]                                                            │
  //   fan out           (same-crate dependencies)                  ── arrow ──┤
  //   crate out: c…     (cross-crate dependencies, one block per crate) arrow ─┘
  //   external          (external dependencies)
  // Every block is a FIXED 5 columns wide (the main node spans the same width);
  // height grows with the row count. The fan-in/out arrow connects the node to
  // the NEAREST internal block (same-crate `fan` or, when absent, the closest
  // cross-crate `crate in/out`) — matching what fan_in/fan_out count. Only the
  // external blocks (third-party, tracked as fan_*_external) carry no arrow.
  const SNW = 148, SNH = 62;
  const MNH2 = 110 + 54;
  const COLS      = 5;            // cards per row (fixed)
  const CARD_GAP  = 12;          // gap between cards in a row
  const ROW_GAP   = 12;          // gap between rows in a block
  const ROW_H     = SNH + ROW_GAP;
  const LBL_H     = 16;          // block label strip above the cards
  const BLOCK_GAP = 16;          // block ↔ block (no arrow)
  const ARR_GAP   = 40;          // fan block ↔ node (arrow runs here)
  const MARG      = 20;
  const HPAD      = 6;           // dashed-box horizontal padding around the cards
  const BOX_VPAD  = 6;           // dashed-box bottom padding below the last row

  const blockW = COLS * SNW + (COLS - 1) * CARD_GAP;   // fixed 5-wide
  const VW     = blockW + 2 * MARG;
  const blockX = MARG;                                  // block left edge (centred → MARG)
  const MNW    = blockW;
  const MNX    = MARG;
  const MNCX   = MNX + MNW / 2;

  // Card X for a 0-based column position; the row list and pixel height of a block.
  const cardX  = pos => blockX + pos * (SNW + CARD_GAP);
  const rowsOf = items => { const r = []; for (let i = 0; i < items.length; i += COLS) r.push(items.slice(i, i + COLS)); return r; };
  const blockH = items => { const rows = Math.ceil(items.length / COLS); return rows ? LBL_H + rows * SNH + (rows - 1) * ROW_GAP : 0; };

  // Split the internal connections of one direction into the main node's own
  // crate (the `fan` block) and one block per OTHER crate, sorted by crate name.
  // Dashed-box stroke per block type (cards inside keep their own tint).
  const BOX_EXT = '#9aa0a6', BOX_FAN = '#8ba6c0', BOX_IN = '#88bb88', BOX_OUT = '#ccaa77';

  const sameCrate  = recs => recs.filter(r => !isCrossCrate(r.node));
  // Cross-crate connections grouped per crate, sorted by card count DESCENDING
  // (biggest crate first) — used to place bigger crates nearer the node.
  const crossByCrate = recs => {
    const m = new Map();
    for (const r of recs) {
      if (!isCrossCrate(r.node)) continue;
      const c = String(nodeAttr(r.node, _groupKey));
      (m.get(c) || m.set(c, []).get(c)).push(r);
    }
    return [...m.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]));
  };

  // Ordered block descriptors. `fan` marks the same-group block; `ext` marks the
  // third-party block (NOT counted in fan_in/fan_out — those live in
  // fan_*_external). Bigger crates sit CLOSER to the node: above → ascending
  // toward fan-in (biggest just above it); below → descending from fan-out.
  // A block's label is a plain prefix + a bolder crate-name suffix (nullable).
  const mk = (items, dir, label, crate, color, fan, ext) => ({ items, dir, label, crate, color, fan: !!fan, ext: !!ext, h: blockH(items), y: 0 });
  const crateIn  = crossByCrate(inConns.internal);    // desc by count
  const crateOut = crossByCrate(outConns.internal);   // desc by count
  // The main node's own group value — labels the same-group fan blocks too.
  const ownCrate = _mainCrate != null && _mainCrate !== '' ? String(_mainCrate) : null;
  // Group label is the grouping key (e.g. "crate", "module", "package") — never hardcoded.
  const gLabel = _groupKey || 'group';

  const above = [];
  if (inConns.external.length) above.push(mk(inConns.external, 'in', 'external', null, BOX_EXT, false, true));
  for (const [c, items] of [...crateIn].reverse()) above.push(mk(items, 'in', `${gLabel} in: `, c, BOX_IN));
  const fanInRecs = sameCrate(inConns.internal);
  if (fanInRecs.length) above.push(mk(fanInRecs, 'in', ownCrate ? `${gLabel} in: ` : 'fan in', ownCrate, BOX_FAN, true));

  const below = [];
  const fanOutRecs = sameCrate(outConns.internal);
  if (fanOutRecs.length) below.push(mk(fanOutRecs, 'out', ownCrate ? `${gLabel} out: ` : 'fan out', ownCrate, BOX_FAN, true));
  for (const [c, items] of crateOut) below.push(mk(items, 'out', `${gLabel} out: `, c, BOX_OUT));
  if (outConns.external.length) below.push(mk(outConns.external, 'out', 'external', null, BOX_EXT, false, true));

  // The in/out arrow + "Fan-in/out: N" label anchor to the blocks fan_in/fan_out
  // actually count: every INTERNAL block (same-group `fan` + cross-group
  // `crate in/out`), never the external one. The arrow targets the block nearest
  // the node — the last internal block above, the first internal block below — so
  // it shows even when there is no same-crate block (e.g. only cross-crate deps).
  const fanInTarget  = [...above].reverse().find(b => !b.ext) || null;
  const fanOutTarget = below.find(b => !b.ext) || null;

  // Stack from the top down, then the node, then the blocks below it.
  let cursor = MARG;
  above.forEach((b, i) => {
    b.y = cursor;
    cursor += b.h;
    cursor += (i === above.length - 1 && !b.ext) ? ARR_GAP : BLOCK_GAP;
  });
  const MNY = cursor;
  cursor = MNY + MNH2;
  below.forEach((b, i) => {
    cursor += (i === 0 && !b.ext) ? ARR_GAP : BLOCK_GAP;
    b.y = cursor;
    cursor += b.h;
  });
  const VH = cursor + MARG;

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
  // One arrowhead marker per block colour, so the fan-in/out line + head matches
  // the colour of the block it points to (blue same-crate, green `crate in`,
  // orange `crate out`).
  const arrowMarkerId = c => 'ah-' + c.replace('#', '');
  const arrowMarkers = [BOX_FAN, BOX_IN, BOX_OUT, BOX_EXT].map(c =>
    `<marker id="${arrowMarkerId(c)}" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><path d="M0,0 L0,6 L8,3z" fill="${c}"/></marker>`).join('');
  s += `<defs>` +
    arrowMarkers +
    `<clipPath id="mn-clip"><rect x="${MNX+10}" y="${MNY}" width="${MNW-20}" height="${MNH2}"/></clipPath>` +
    `</defs>`;

  // Side node card. `item` = { node, kinds:Set, ext }.
  // External nodes: grey card with the full id only (no metrics).
  // Internal files: title (centred) + a `pr` badge for private modules, a
  // primary (left, abbreviated) / secondary (right) metric row, and a bottom
  // row of connection-kind slots split into thirds.
  let _snIdx = 0;
  // Escape a string for use inside a double-quoted SVG/HTML attribute.
  const escA = escAttr;

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
      crateVal != null && crateVal !== '' ? `${_groupKey}: ${crateVal}` : '',
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

  // Render a block: a full-width dashed outline + a coloured label strip + the
  // cards in a 5-wide grid.
  const renderBlock = b => {
    if (!b.items.length) return '';
    let r = `<rect x="${blockX - HPAD}" y="${b.y}" width="${blockW + 2*HPAD}" height="${b.h + BOX_VPAD}" rx="8" fill="none" stroke="${b.color}" stroke-width="1"/>`;
    r += `<text x="${blockX}" y="${b.y + 11}" font-family="system-ui,sans-serif" font-size="11" fill="${b.color}" font-weight="600">${esc(b.label)}${b.crate != null ? `<tspan font-weight="800">${esc(b.crate)}</tspan>` : ''}</text>`;
    rowsOf(b.items).forEach((row, ri) =>
      row.forEach((item, pi) => {
        r += sideNode(item, cardX(pi), b.y + LBL_H + ri * ROW_H, b.dir);
      })
    );
    return r;
  };

  // Blocks above the node (external, per-crate, then same-crate fan-in).
  for (const b of above) s += renderBlock(b);
  // Fan-in arrow — nearest internal caller block (same-crate fan-in, else the
  // closest cross-crate `crate in` block) → node. Dashed (and unlabelled) when
  // fan_in is 0: the block exists but links only through non-flow edges
  // (contains / reexports), so every card in it is dashed too.
  if (fanInTarget) {
    const flowIn = node.fan_in != null && node.fan_in > 0;
    const dashIn = flowIn ? '' : ' stroke-dasharray="5,3"';
    s += `<line x1="${MNCX}" y1="${fanInTarget.y + fanInTarget.h + BOX_VPAD}" x2="${MNCX}" y2="${MNY}" stroke="${fanInTarget.color}" stroke-width="1.5"${dashIn} marker-end="url(#${arrowMarkerId(fanInTarget.color)})"/>`;
    if (flowIn)
      s += `<text x="${MNCX+8}" y="${Math.round((fanInTarget.y + fanInTarget.h + MNY) / 2) + 4}" font-family="system-ui,sans-serif" font-size="10" fill="#5c7a96">Fan-in: ${node.fan_in}</text>`;
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

  // Fan-out arrow — node → nearest internal dependency block (same-crate fan-out,
  // else the closest cross-crate `crate out` block). Dashed (and unlabelled) when
  // fan_out is 0: the block exists but links only through non-flow edges
  // (contains / reexports), so every card in it is dashed too.
  if (fanOutTarget) {
    const flowOut = node.fan_out != null && node.fan_out > 0;
    const dashOut = flowOut ? '' : ' stroke-dasharray="5,3"';
    s += `<line x1="${MNCX}" y1="${MNY+MNH2}" x2="${MNCX}" y2="${fanOutTarget.y}" stroke="${fanOutTarget.color}" stroke-width="1.5"${dashOut} marker-end="url(#${arrowMarkerId(fanOutTarget.color)})"/>`;
    if (flowOut)
      s += `<text x="${MNCX+8}" y="${Math.round((MNY + MNH2 + fanOutTarget.y) / 2) + 4}" font-family="system-ui,sans-serif" font-size="10" fill="#5c7a96">Fan-out: ${node.fan_out}</text>`;
  }
  // Blocks below the node (same-crate fan-out, per-crate, then external).
  for (const b of below) s += renderBlock(b);

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


// Tooltip hints for each metric key
const NM_HINTS = {
  cyclomatic:  'McCabe cyclomatic complexity — number of linearly independent paths (branches + 1). Good: 1–5; review at >10.',
  cognitive:   'Cognitive complexity — how hard the code is to read. Penalises nesting more than cyclomatic.',
  exits:       'Number of return/exit points in the function.',
  args:        'Number of function arguments.',
  functions:   'Number of nested function definitions.',
  closures:    'Number of closure / lambda definitions.',
  items:       'Rust only. Number of top-level definitions in the module: fn, struct, enum, trait, impl, type, const, static, mod, macro, union.',
  methods:     'Rust only. Number of methods defined in the impl block(s) of this type.',
  fan_in:      'Incoming dependency edges — how many other nodes use this node. High fan_in = risky to change.',
  fan_out:     'Outgoing dependency edges — how many nodes this node depends on. High fan_out = broad responsibilities.',
  hk:          'Henry-Kafura complexity: loc × (fan_in × fan_out)². Combines size with coupling.',
  mi:          'Maintainability Index (Oman 1992). Higher is better. >85 easy; 65–85 moderate; <65 difficult.',
  mi_sei:      'MI (SEI variant) — adds a bonus for comment density. Equals MI when there are no comments.',
  source:      'Source lines — lines with at least one non-whitespace, non-comment character.',
  logical:     'Logical lines — counts statements, not physical lines.',
  comments:    'Comment-only lines (inline comments on code lines are not counted).',
  blank:       'Empty or whitespace-only lines.',
  length:      'Halstead program length N = N1 + N2 (total token occurrences).',
  vocabulary:  'Halstead vocabulary n = n1 + n2 (distinct operators + operands).',
  volume:      'Halstead volume V = N × log₂(n). Information content in bits.',
  effort:      'Halstead mental effort E = (n1/2n2) × N × log₂(n). Correlates with dev time.',
  'time (s)':  'Estimated programming time in seconds: T = E / 18.',
  bugs:        'Estimated latent bugs B = V / 3000. Use as relative ranking, not absolute count.',
};

// Full display labels for abbreviated metric keys (tooltips still key off the
// original short key via NM_HINTS).
const NM_LABELS = {
  hk:         'Henry-Kafura',
  fan_in:     'Fan-in',
  fan_out:    'Fan-out',
  mi:         'Maintainability Index',
  mi_sei:     'Maintainability Index (SEI)',
};

function buildDiagramSVG(node, level) {
  const diff      = window.DIFF?.[level];
  // Use raw snapshot edges so external crate nodes (filtered from DIFF) are still shown
  const rawGraph  = (window.AFTER ?? window.BEFORE)?.graphs?.[level] || { nodes: [], edges: [] };
  const allEdges  = rawGraph.edges;
  // nodeMap: DIFF nodes (have status/cycle data) + raw external nodes as fallback
  const nodeMap   = new Map([
    ...(diff?.nodes || []).map(n => [n.id, n]),
    ...rawGraph.nodes.filter(n => n.external).map(n => [n.id, n]),
  ]);
  const esc      = s => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
  const trunc    = (s, n) => s.length > n ? s.slice(0, n - 1) + '…' : s;
  const nameOf   = n => trunc(n.name || n.id.split('::').pop() || n.id, 18);
  // Private nodes get a " [pr]" suffix after the name.
  const visTag   = n => (typeof n.visibility === 'string' && n.visibility === 'private') ? ' [pr]' : '';
  const fmtNum   = v => v != null ? String(Math.round(v)).replace(/\B(?=(\d{3})+(?!\d))/g, ',') : '—';

  // Per-kind visual config — KIND_ORDER controls left-to-right column order
  const KIND_ORDER = ['uses', 'calls', 'reexports', 'contains'];
  const COL_STROKE = '#8ba6c0';
  const COL_DASH   = '6,4';
  // External (3rd-party) columns are amber, matching the library node styling.
  const kindColor  = k => k === 'external' ? '#b3801f' : COL_STROKE;
  const kindDash   = _k => COL_DASH;

  // Dedup edges by node id within a single kind
  const uniqEdges = (edgeArr, idKey) => {
    const seen = new Set();
    const result = [];
    for (const e of edgeArr) {
      const id = e[idKey];
      if (!seen.has(id)) {
        seen.add(id);
        result.push({ node: nodeMap.get(id) || { id, name: id.split('::').pop() }, edge: e });
      }
    }
    return result;
  };

  // Is the far endpoint of this edge (the side that is NOT the selected node) an
  // external 3rd-party library? Such edges are pulled into a separate column
  // drawn farther from the main node than the internal dependency columns.
  const isExtEndpoint = (e, idKey) => {
    if (e.external === true) return true;
    const far = nodeMap.get(e[idKey]);
    return far ? (far.external === true || far.kind === 'external') : false;
  };

  // Group internal edges by kind; collect external-library edges separately so
  // they can be rendered as a farther tier.
  const groupByKind = (edgeArr, idKey) => {
    const groups = {};
    const extEdges = [];
    for (const e of edgeArr) {
      if (isExtEndpoint(e, idKey)) { extEdges.push(e); continue; }
      const k = e.kind || 'unknown';
      (groups[k] = groups[k] || []).push(e);
    }
    const result = {};
    for (const [k, edges] of Object.entries(groups))
      result[k] = uniqEdges(edges, idKey);
    return { groups: result, ext: uniqEdges(extEdges, idKey) };
  };

  // `Contains` (mod-declaration) edges are structural — kept in the snapshot
  // but not shown as dependency columns here.
  const flow      = e => e.kind !== 'contains';
  const inSplit   = groupByKind(allEdges.filter(e => e.to   === node.id && flow(e)), 'from');
  const outSplit  = groupByKind(allEdges.filter(e => e.from === node.id && flow(e)), 'to');

  // Layout constants
  const SNW         = 148, SNH = 58;
  const MNH         = 110, MNH2 = MNH + 54;
  const CELL        = SNW + 12;          // one card-slot width
  const COL_PAD_X   = 12;               // horizontal padding inside column box
  const COL_GAP     = 12;              // gap between adjacent columns
  const ROW_H       = SNH + 10;
  const PAD_TOP     = 20;              // inside column: space above first row (below label)
  const PAD_BOT     = 14;
  const ARR_GAP     = 36;
  const SIDE_PAD    = 20;
  const MAX_ITEMS   = 24;
  const MAX_COL_CARDS = 4;             // max cards per row in one column
  const MARG        = 20;
  const MNW_MIN     = 3 * CELL - 12 + 2 * COL_PAD_X;  // ≈ 492 minimum main-node width

  // Build column descriptors for one direction (returns array of col objects).
  // The external-library edges become one trailing `external` column tagged
  // `ext: true` so the Y layout can push it to a farther tier.
  const buildCols = ({ groups, ext }) => {
    const keys = [
      ...KIND_ORDER.filter(k => groups[k]),
      ...Object.keys(groups).filter(k => !KIND_ORDER.includes(k)),
    ];
    const specs = keys.map(kind => ({ kind, all: groups[kind], ext: false }));
    if (ext && ext.length) specs.push({ kind: 'external', all: ext, ext: true });
    const raw = specs.map(({ kind, all, ext }) => {
      const items = all.slice(0, MAX_ITEMS);
      const extra = all.length - items.length;
      return { kind, all, items, extra, count: items.length, ext };
    }).filter(c => c.count > 0);

    if (raw.length === 0) return raw;

    const total = raw.reduce((s, c) => s + c.count, 0);
    for (const c of raw) {
      c.cardW = Math.max(1, Math.min(c.count, Math.floor(c.count / total * MAX_COL_CARDS)));
      c.px_w  = c.cardW * CELL - 12 + 2 * COL_PAD_X;
      const rows = [];
      for (let i = 0; i < c.items.length; i += c.cardW)
        rows.push(c.items.slice(i, i + c.cardW));
      c.rows = rows;
      c.h    = PAD_TOP + rows.length * ROW_H - (ROW_H - SNH) + PAD_BOT;
    }
    return raw;
  };

  const inCols  = buildCols(inSplit);
  const outCols = buildCols(outSplit);

  // Total pixel width of a column set
  const colsW = cols => cols.length === 0 ? 0
    : cols.reduce((s, c) => s + c.px_w, 0) + (cols.length - 1) * COL_GAP;

  // SVG width driven by columns; main node width computed after column positions are known
  const VW = Math.max(800, 2 * SIDE_PAD + colsW(inCols), 2 * SIDE_PAD + colsW(outCols));

  const maxInH  = inCols.length  > 0 ? Math.max(...inCols.map(c => c.h))  : 0;
  const maxOutH = outCols.length > 0 ? Math.max(...outCols.map(c => c.h)) : 0;

  // Y layout: in-cols are bottom-anchored, out-cols are top-anchored. The
  // external-library column is a separate column but stays on the same tier as
  // the internal columns (same anchor) — not pushed farther from the node.
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
  const MNW = allCols.length > 0
    ? Math.max(MNW_MIN, 2 * Math.max(...arrowXs.map(x => Math.abs(x - VW / 2))) + 2 * COL_PAD_X)
    : MNW_MIN;
  const MNX  = (VW - MNW) / 2;
  const MNCX = MNX + MNW / 2;

  // Assign Y: in-cols bottom-anchored, out-cols top-anchored (external column
  // included on the same tier).
  for (const c of inCols)  c.y = inAreaBottom - c.h;
  for (const c of outCols) c.y = outAreaTop;

  // X of a card at position pos in a row of rowLen cards inside column col
  const nodeXInCol = (col, pos, rowLen) => {
    const span = rowLen * SNW + (rowLen - 1) * 12;
    return col.x + (col.px_w - span) / 2 + pos * CELL;
  };

  // Cycle highlight state — a node in a dependency cycle is always marked red
  // (no toggle), matching the main map.
  const cycleNodes = window.CYCLES?.[level]?.nodeCycleStatus;
  const isCycleNode = id => {
    const cs = cycleNodes?.get(id);
    return cs != null && cs !== 'none';
  };

  let s = `<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" viewBox="0 0 ${VW} ${VH}" preserveAspectRatio="xMidYMid meet">`;
  s += `<defs>` +
    `<marker id="ah" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><path d="M0,0 L0,6 L8,3z" fill="#4d6f9c"/></marker>` +
    `<marker id="ah-ext" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><path d="M0,0 L0,6 L8,3z" fill="#b3801f"/></marker>` +
    `<clipPath id="mn-clip"><rect x="${MNX+10}" y="${MNY}" width="${MNW-20}" height="${MNH2}"/></clipPath>` +
    `</defs>`;

  // Side node card
  let _snIdx = 0;
  const sideNode = ({node: n}, x, y) => {
    const inMap   = nodeMap.has(n.id);
    const cycle   = isCycleNode(n.id);
    const ext     = n.external === true || n.kind === 'external';
    const hk      = fmtNum(n.complexity?.coupling?.hk);
    const loc     = n.complexity?.loc?.source != null ? String(n.complexity.loc.source) : '—';
    const cursor  = inMap ? 'pointer' : 'default';
    const clipId  = `sn-clip-${_snIdx++}`;
    // External (3rd-party library) nodes are amber, matching the main map legend.
    const fill    = ext ? '#f6e2c0' : '#f0f4f8';
    const stroke  = cycle ? '#c00' : ext ? '#b3801f' : (inMap ? '#8ba6c0' : '#bbb');
    const strokeW = cycle ? '2' : '1';
    // External library cards show their full id (`ext:serde_json`) centred, with
    // no loc/hk (libraries carry no metrics). Internal files show name + loc/hk.
    const inner = ext
      ? `<tspan x="${x+SNW/2}" y="${y+SNH/2+4}" text-anchor="middle" font-size="11" font-weight="600">${esc(n.id)}</tspan>`
      : `<tspan x="${x+SNW/2}" y="${y+17}" text-anchor="middle" font-size="11" font-weight="600">${esc(nameOf(n))}${visTag(n)}</tspan>` +
        `<tspan x="${x+8}" dy="16" font-size="10" fill="#5c7a96">loc: ${esc(loc)}</tspan>` +
        `<tspan x="${x+8}" dy="14" font-size="10" fill="#5c7a96">hk: ${esc(hk)}</tspan>`;
    // Hover tooltip: the full path (or the id for external libraries).
    const sideTip = ext ? n.id : ((n.path || '').replace(/^\{[^}]+\}\//, '') || n.id);
    return `<defs><clipPath id="${clipId}"><rect x="${x+4}" y="${y}" width="${SNW-8}" height="${SNH}"/></clipPath></defs>` +
      `<g data-diag-node="${esc(n.id)}" style="cursor:${cursor}"><title>${esc(sideTip)}</title>` +
      `<rect x="${x}" y="${y}" width="${SNW}" height="${SNH}" rx="6" fill="${fill}" stroke="${stroke}" stroke-width="${strokeW}"/>` +
      `<g clip-path="url(#${clipId})"><text font-family="ui-monospace,'SF Mono',monospace" fill="#2c3e50">` +
      inner +
      `</text></g></g>`;
  };

  // Render one column (dashed box + kind label + node cards)
  const renderCol = col => {
    const color = kindColor(col.kind);
    const dash  = kindDash(col.kind);
    const label = `${col.kind}  ${col.all.length}${col.extra > 0 ? ` (+${col.extra})` : ''}`;
    let r = '';
    r += `<rect x="${col.x}" y="${col.y}" width="${col.px_w}" height="${col.h}" rx="8" fill="none" stroke="${color}" stroke-width="1.5" stroke-dasharray="${dash}"/>`;
    r += `<text x="${col.x+10}" y="${col.y+13}" font-family="system-ui,sans-serif" font-size="10" fill="${color}" font-weight="600">${label}</text>`;
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
      // External edges use an amber arrow (matching the library styling) and
      // carry no fan_in/fan_out label — they are tracked as `fan_out_external`,
      // not part of fan_in/fan_out.
      const stroke = c.ext ? '#b3801f' : '#4d6f9c';
      const marker = c.ext ? 'ah-ext' : 'ah';
      s += `<line x1="${cx}" y1="${c.y + c.h}" x2="${cx}" y2="${MNY}" stroke="${stroke}" stroke-width="1.5" marker-end="url(#${marker})"/>`;
      if (!c.ext && c.kind !== 'contains' && c.all.length > 0)
        s += `<text x="${cx+5}" y="${my+4}" font-family="system-ui,sans-serif" font-size="10" fill="#5c7a96">fan_in: ${c.all.length}</text>`;
    });
  }

  // Main node
  const comp = node.complexity;
  const hk   = fmtNum(comp?.coupling?.hk);
  const loc  = comp?.loc?.source != null ? String(comp.loc.source) : '—';
  const mnValTrunc = (label, v) => trunc(v, Math.max(4, Math.floor((MNW - 20 - label.length * 7.2) / 7.2)));
  const mono = `font-family="ui-monospace,'SF Mono','Fira Code',monospace"`;
  const mnCycle = isCycleNode(node.id);
  // An external (3rd-party library) main node is amber, like its side cards,
  // and carries no path/hk/loc — just its id.
  const mnExt    = node.external === true || node.kind === 'external';
  const mnFill   = mnExt ? '#f6e2c0' : '#dbe9f4';
  const mnStroke = mnCycle ? '#c00' : mnExt ? '#b3801f' : '#4d6f9c';
  const nodePath = (node.path || '').replace(/^\{[^}]+\}\//, '');
  // The whole main card is click-to-copy: clicking it copies the path (the id
  // for an external library). A `copy` cursor signals this; handled by the
  // `.mn-card` listener in modal.js.
  const copyVal = mnExt ? node.id : nodePath;
  s += `<g class="mn-card" data-copy="${esc(copyVal)}"><title>click to copy ${mnExt ? 'id' : 'path'}</title>`;
  s += `<rect x="${MNX}" y="${MNY}" width="${MNW}" height="${MNH2}" rx="10" fill="${mnFill}" stroke="${mnStroke}" stroke-width="${mnCycle ? '3' : '2'}"/>`;
  s += `<g class="mn-card-body" clip-path="url(#mn-clip)">`;
  if (mnExt) {
    s += `<text ${mono} x="${MNX+MNW/2}" y="${MNY+MNH2/2+6}" text-anchor="middle" font-size="16" font-weight="700" fill="#1a2f45">${esc(trunc(node.id, 40))}</text>`;
  } else {
    s += `<text ${mono} x="${MNX+MNW/2}" y="${MNY+28}" text-anchor="middle" font-size="16" font-weight="700" fill="#1a2f45">${esc(trunc(node.name||node.id, 36))}${visTag(node)}</text>`;
    s += `<text ${mono} x="${MNX+14}" y="${MNY+58}" font-size="11" fill="#2c3e50"><tspan font-weight="700">path: </tspan>${esc(mnValTrunc('path: ', nodePath))}</text>`;
    s += `<text ${mono} x="${MNX+14}" y="${MNY+80}" font-size="11" fill="#2c3e50"><tspan font-weight="700">hk: </tspan>${esc(hk)}</text>`;
    s += `<text ${mono} x="${MNX+14}" y="${MNY+102}" font-size="11" fill="#2c3e50"><tspan font-weight="700">loc: </tspan>${esc(loc)}</text>`;
  }
  s += `</g>`;
  // Shown for ~1s after a copy (the body is hidden meanwhile, see index.css).
  s += `<text class="mn-copied-msg" ${mono} x="${MNX+MNW/2}" y="${MNY+MNH2/2+7}" text-anchor="middle" font-size="22" font-weight="700" fill="#4d6f9c">copied</text>`;
  s += `</g>`;

  // Fan-out columns (below main node, top-anchored) — one arrow per column
  if (outCols.length > 0) {
    outCols.forEach(c => {
      const cx  = Math.round(c.x + c.px_w / 2);
      const my  = Math.round((MNY + MNH2 + c.y) / 2);
      // External edges use an amber arrow and carry no fan_out label
      // (tracked as `fan_out_external`, separate from `fan_out`).
      const stroke = c.ext ? '#b3801f' : '#4d6f9c';
      const marker = c.ext ? 'ah-ext' : 'ah';
      s += `<line x1="${cx}" y1="${MNY+MNH2}" x2="${cx}" y2="${c.y}" stroke="${stroke}" stroke-width="1.5" marker-end="url(#${marker})"/>`;
      if (!c.ext && c.kind !== 'contains' && c.all.length > 0)
        s += `<text x="${cx+5}" y="${my+4}" font-family="system-ui,sans-serif" font-size="10" fill="#5c7a96">fan_out: ${c.all.length}</text>`;
      s += renderCol(c);
    });
  }

  s += '</svg>';
  return s;
}

function buildModalContent(node, level) {
  const cycles  = window.CYCLES?.[level];
  const cs      = cycles?.nodeCycleStatus?.get(node.id);
  const path    = (node.path || '').replace(/^\{[^}]+\}\//, '');
  const lineStr = node.line != null ? `:${node.line}` : '';
  const vis     = typeof node.visibility === 'string'
    ? node.visibility
    : node.visibility?.restricted ? `restricted(${node.visibility.restricted})` : null;

  // sections: array of { label: string|null, rows: string[] }
  const sections = [];
  let cur = { label: null, rows: [] };

  const row = (k, v) => {
    if (v == null || v === '') return;
    const hint = NM_HINTS[k];
    const attr = hint ? ` data-nm-hint="${hint.replace(/"/g, '&quot;')}"` : '';
    // Consistent capitalization: full label if known, else capitalize the key.
    const label = NM_LABELS[k] || (k.charAt(0).toUpperCase() + k.slice(1));
    cur.rows.push(`<tr><td class="nm-key${hint ? ' nm-has-hint' : ''}"${attr}>${label}</td><td class="nm-val">${v}</td></tr>`);
  };
  const sect = label => { sections.push(cur); cur = { label, rows: [] }; };

  const n3 = v => {
    if (v == null) return null;
    return String(Math.round(v)).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  };
  const fmt = (v, d) => {
    if (v == null) return null;
    const s = d > 0
      ? parseFloat(v.toFixed(d)).toFixed(d).replace(/\.0+$/, '').replace(/(\.\d*[1-9])0+$/, '$1')
      : String(Math.round(v));
    const [int, dec] = s.includes('.') ? s.split('.') : [s, ''];
    const fi = int.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
    return dec ? `${fi}.${dec}` : fi;
  };

  const esc = s => s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
  if (path) {
    const si   = path.lastIndexOf('/');
    const dir  = si >= 0 ? esc(path.slice(0, si + 1)) : '';
    const file = esc(si >= 0 ? path.slice(si + 1) : path);
    cur.rows.push(
      `<tr><td class="nm-key">Path</td><td class="nm-val">` +
      `${dir}<strong>${file}</strong>${lineStr ? esc(lineStr) : ''}` +
      `</td></tr>`
    );
  }
  row('kind',       node.kind || null);
  row('visibility', vis);
  if (node.item_count   != null) row('items',   n3(node.item_count));
  if (node.method_count != null) row('methods', n3(node.method_count));
  row('cycle',      cs && cs !== 'none' ? cs : null);
  row('cycle type', node.cycle_kind ?? null);
  if (!document.body.classList.contains('mode-review')) row('status', node.status);

  const cx = node.complexity;
  if (cx) {
    const cpl = cx.coupling;
    if (cpl) {
      sect('Coupling');
      row('fan_in',  n3(cpl.fan_in));
      row('fan_out', n3(cpl.fan_out));
      if (cpl.hk != null) row('hk', fmt(cpl.hk, 0));
    }

    const loc = cx.loc;
    if (loc) {
      sect('Lines of Code');
      row('source',   n3(loc.source));
      row('logical',  n3(loc.logical));
      row('comments', n3(loc.comments));
      row('blank',    n3(loc.blank));
    }

    sect('Complexity');
    row('cyclomatic', n3(cx.cyclomatic));
    row('cognitive',  n3(cx.cognitive));
    row('exits',      cx.exits > 0 ? n3(cx.exits) : null);
    row('args',       cx.args  > 0 ? n3(cx.args)  : null);
    row('functions',  cx.functions > 0 ? n3(cx.functions) : null);
    row('closures',   cx.closures  > 0 ? n3(cx.closures)  : null);
    const mi = cx.maintainability;
    if (mi) {
      row('mi',     fmt(mi.mi,     1));
      row('mi_sei', fmt(mi.mi_sei, 1));
    }

    const hs = cx.halstead;
    if (hs) {
      sect('Halstead');
      row('length',     n3(hs.length));
      row('vocabulary', n3(hs.vocabulary));
      row('volume',     fmt(hs.volume,  1));
      row('effort',     fmt(hs.effort,  0));
      row('time (s)',   fmt(hs.time,    1));
      row('bugs',       fmt(hs.bugs,    4));
    }
  }
  sections.push(cur);

  const renderSect = s =>
    `${s.label ? `<div class="nm-sect-label">${s.label}</div>` : ''}` +
    `<table class="nm-table">${s.rows.join('')}</table>`;

  const body = sections.filter(s => s.rows.length > 0).map(renderSect).join('');

  return {
    hdr:      `<span class="nm-title">${node.name}</span><span class="nm-badge">${node.kind}</span>`,
    body,
    diagram:  buildDiagramSVG(node, level),
  };
}

function setupTooltips(svgFrame, level) {
  svgFrame.querySelectorAll('g.edge title, g.cluster title').forEach(t => t.remove());

  const nodeMap  = new Map(activeGraph(level).nodes.map(n => [n.id, n]));
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
      const overlay = getModal();
      const mc = buildModalContent(node, level);
      document.getElementById('node-modal-hdr-title').innerHTML = mc.hdr;
      document.getElementById('node-modal-body').innerHTML = mc.body;
      document.getElementById('node-modal-diagram').innerHTML = mc.diagram;
      attachModalCheckbox(node, level, section);
      overlay.style.display = 'flex'; document.body.style.overflow = 'hidden';
      window.navPush?.(level, node.id);
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

function drawSVG(svgFrame, nodes, edges, level) {
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

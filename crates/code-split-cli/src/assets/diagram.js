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
  hk:          'Henry-Kafura complexity: sloc × (fan_in × fan_out)². Combines size with coupling.',
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
  const fmtNum   = v => v != null ? String(Math.round(v)).replace(/\B(?=(\d{3})+(?!\d))/g, ',') : '—';

  // Column visual config
  const COL_STROKE = '#8ba6c0';
  const COL_DASH   = '6,4';
  // The external (3rd-party) column is amber, matching the library node styling.
  const kindColor  = k => k === 'external' ? '#b3801f' : COL_STROKE;
  const kindDash   = _k => COL_DASH;
  // Abbreviated number for the card (e.g. 189,000 → 189K, 1,500,000 → 1.5M).
  const fmtK = v => {
    if (v == null) return '—';
    v = Math.round(v);
    if (v >= 1e6) return (v / 1e6).toFixed(v >= 1e7 ? 0 : 1).replace(/\.0$/, '') + 'M';
    if (v >= 1e3) return (v / 1e3).toFixed(v >= 1e4 ? 0 : 1).replace(/\.0$/, '') + 'K';
    return String(v);
  };

  // Is the far endpoint of this edge (the node at `idKey`, i.e. the side that is
  // NOT the selected node) an external 3rd-party library? This must look at the
  // far NODE, not the edge's `external` flag: that flag marks the edge's `to`
  // endpoint as a library, so on an external node's fan-in (idKey = 'from') it
  // would wrongly tag the internal source file as external.
  const isExtEndpoint = (e, idKey) => {
    const far = nodeMap.get(e[idKey]);
    return far ? (far.external === true || far.kind === 'external') : false;
  };

  // Collect connections for one direction, deduped by the far node. Each record
  // carries the SET of edge kinds (uses/reexports/contains) linking it to the
  // main node, so a single card can show its connection type(s). A node is shown
  // only when it has an information-flow edge (uses/reexports); a `contains`-only
  // structural link (a `mod foo;` never imported) stays hidden, as on the main map.
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
    for (const rec of byNode.values()) {
      const hasFlow = rec.kinds.has('uses') || rec.kinds.has('reexports');
      if (!hasFlow) continue;                  // contains-only → not shown
      (rec.ext ? external : internal).push(rec);
    }
    return { internal, external };
  };

  const inConns  = collectConns(allEdges.filter(e => e.to   === node.id), 'from');
  const outConns = collectConns(allEdges.filter(e => e.from === node.id), 'to');

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
  const MAX_ITEMS   = 24;
  const MAX_COL_CARDS = 4;             // max cards per row in one column
  const MAX_COL_ROWS  = 6;             // wrap into more columns past this many rows
  const MARG        = 20;
  const MNW_MIN     = 3 * CELL - 12 + 2 * COL_PAD_X;  // ≈ 492 minimum main-node width

  // Build column descriptors for one direction: one internal-connections column
  // plus (when present) a separate amber `external` column on the same tier.
  const buildCols = ({ internal, external }) => {
    const specs = [];
    if (internal.length) specs.push({ kind: 'connections', all: internal, ext: false });
    if (external.length) specs.push({ kind: 'external',    all: external, ext: true  });
    const raw = specs.map(({ kind, all, ext }) => {
      const items = all.slice(0, MAX_ITEMS);
      const extra = all.length - items.length;
      return { kind, all, items, extra, count: items.length, ext };
    }).filter(c => c.count > 0);

    if (raw.length === 0) return raw;

    for (const c of raw) {
      // Wrap a tall column into more sub-columns once it would exceed
      // `MAX_COL_ROWS` rows (e.g. 10 items → 2 columns of 5), capped at
      // `MAX_COL_CARDS` wide.
      c.cardW = Math.min(MAX_COL_CARDS, Math.max(1, Math.ceil(c.count / MAX_COL_ROWS)));
      c.px_w  = c.cardW * CELL - 12 + 2 * COL_PAD_X;
      const rows = [];
      for (let i = 0; i < c.items.length; i += c.cardW)
        rows.push(c.items.slice(i, i + c.cardW));
      c.rows = rows;
      c.h    = PAD_TOP + rows.length * ROW_H - (ROW_H - SNH) + PAD_BOT;
    }
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

  // Side node card. `item` = { node, kinds:Set, ext }.
  // External libraries: amber card with the full `ext:` id only (no metrics).
  // Internal files: title (centred) + a `pr` badge for private modules, an
  // hk (left, abbreviated) / loc (right) row, and a bottom row of three
  // connection-kind slots (uses · reexport · contains) split exactly into
  // thirds — present kinds are labelled, absent ones leave their third empty.
  let _snIdx = 0;
  // [edge kind, display label, hover hint].
  const KIND_SLOTS = [
    ['uses',      'uses',     'Import / use dependency — this file uses items from the other.'],
    ['reexports', 'reexport', 'Re-export (pub use) — this file re-exposes the other file\'s items as part of its own API.'],
    ['contains',  'contains', 'Module declaration (mod foo;) — structural ownership, kept in the JSON but excluded from fan-in / HK / cycles.'],
  ];
  // Escape a string for use inside a double-quoted SVG/HTML attribute.
  const escA = s => esc(s).replace(/"/g, '&quot;');
  const sideNode = (item, x, y) => {
    const n       = item.node;
    const inMap   = nodeMap.has(n.id);
    const cycle   = isCycleNode(n.id);
    const ext     = item.ext || n.external === true || n.kind === 'external';
    const cursor  = inMap ? 'pointer' : 'default';
    const clipId  = `sn-clip-${_snIdx++}`;
    const fill    = ext ? '#f6e2c0' : '#f0f4f8';
    const stroke  = cycle ? '#c00' : ext ? '#b3801f' : (inMap ? '#8ba6c0' : '#bbb');
    const strokeW = cycle ? '2' : '1';
    const mono    = `font-family="ui-monospace,'SF Mono',monospace"`;
    const clipDef = `<defs><clipPath id="${clipId}"><rect x="${x+4}" y="${y}" width="${SNW-8}" height="${SNH}"/></clipPath></defs>`;
    // No native `<title>` here: it would pop a second (browser) tooltip that
    // conflicts with the styled `#tt` tooltip shown on the labels.
    const open    = `<g data-diag-node="${esc(n.id)}" style="cursor:${cursor}">` +
      `<rect x="${x}" y="${y}" width="${SNW}" height="${SNH}" rx="6" fill="${fill}" stroke="${stroke}" stroke-width="${strokeW}"/>`;
    // Path shown as a styled tooltip when hovering the name.
    const pathTip = ext ? (n.path || n.id)
                        : ((n.path || '').replace(/^\{[^}]+\}\//, '') || n.id);

    if (ext) {
      // Library name without the `ext:` id prefix; hover it for the path tooltip.
      const extName = n.name || n.id.replace(/^ext:/, '');
      return clipDef + open +
        `<g clip-path="url(#${clipId})"><text ${mono} fill="#2c3e50">` +
        `<tspan class="sn-hint" data-tip="${escA(pathTip)}" x="${x+SNW/2}" y="${y+SNH/2+4}" text-anchor="middle" font-size="11" font-weight="600">${esc(extName)}</tspan>` +
        `</text></g></g>`;
    }

    // hk: when absent, show nothing in the simple view (no `—`); the detail view
    // still spells it out as `0:hk`.
    const hkRaw = n.complexity?.coupling?.hk;
    const hkSimple = hkRaw != null ? fmtK(hkRaw) : '';
    const hkDetail = hkRaw != null ? fmtK(hkRaw) : '0';
    const loc   = n.complexity?.loc?.source != null ? String(n.complexity.loc.source) : '—';
    const priv  = (typeof n.visibility === 'string' && n.visibility === 'private');
    const thirdW = SNW / 3;
    // Each present connection-kind is its own hoverable label (tooltip + highlight).
    let kindRow = '';
    KIND_SLOTS.forEach(([k, disp, hint], i) => {
      if (!item.kinds?.has(k)) return;
      kindRow += `<text class="sn-detail sn-hint" data-tip="${escA(hint)}" x="${x + thirdW * (i + 0.5)}" y="${y+SNH-7}" text-anchor="middle" font-size="8" fill="#5c7a96">${disp}</text>`;
    });
    // `pr` chip (private module): hover-only (like the detail row), with the same
    // styled `#tt` tooltip. `sn-detail` hides it until the card is hovered.
    const prBadge = priv
      ? `<g class="sn-detail sn-hint" data-tip="${escA('Private module — its declared visibility is private.')}">` +
        `<rect x="${x+SNW-26}" y="${y+4}" width="22" height="13" rx="3" fill="#e0d2b8" stroke="#b3801f" stroke-width="0.5"/>` +
        `<text ${mono} x="${x+SNW-15}" y="${y+14}" text-anchor="middle" font-size="9" fill="#7a5b18">pr</text></g>`
      : '';
    const ty = y + 36;  // hk / loc row baseline
    // Two states toggled by CSS on `g[data-diag-node]:hover` (see index.css):
    // `.sn-simple` shows bare hk / loc, `.sn-detail` shows labelled values plus
    // the connection-kind row. Title and `pr` badge are always visible.
    return clipDef + open +
      `<g clip-path="url(#${clipId})" ${mono} fill="#2c3e50">` +
      `<text class="sn-hint" data-tip="${escA(pathTip)}" x="${x+SNW/2}" y="${y+16}" text-anchor="middle" font-size="11" font-weight="600">${esc(nameOf(n))}</text>` +
      `<text class="sn-simple" x="${x+8}" y="${ty}" font-size="10" fill="#5c7a96">${esc(hkSimple)}</text>` +
      `<text class="sn-simple" x="${x+SNW-8}" y="${ty}" text-anchor="end" font-size="10" fill="#5c7a96">${esc(loc)}</text>` +
      // Whole `value:key` string is one tooltip target (key + value both hover).
      `<text class="sn-detail sn-hint" data-tip-title="${escA(COL_NAMES.hk)}" data-tip="${escA(COL_TIPS.hk)}" data-tip-formula="${escA(COL_FORMULAS.hk)}" data-tip-calc="${escA(hkCalc(n.complexity))}" x="${x+8}" y="${ty}" font-size="10" fill="#5c7a96">${esc(hkDetail)}:hk</text>` +
      `<text class="sn-detail sn-hint" data-tip-title="${escA(COL_NAMES.loc)}" data-tip="${escA(COL_TIPS.loc)}" x="${x+SNW-8}" y="${ty}" text-anchor="end" font-size="10" fill="#5c7a96">sloc:${esc(loc)}</text>` +
      kindRow +
      `</g>` + prBadge + `</g>`;
  };

  // Render one column (dashed box + optional header + node cards). The internal
  // `connections` column has no header — its kinds are shown per-card and its
  // count is on the arrow; only the `external` column is labelled.
  const renderCol = col => {
    const color = kindColor(col.kind);
    const dash  = kindDash(col.kind);
    let r = '';
    r += `<rect x="${col.x}" y="${col.y}" width="${col.px_w}" height="${col.h}" rx="8" fill="none" stroke="${color}" stroke-width="1.5" stroke-dasharray="${dash}"/>`;
    if (col.ext) {
      const label = `external  ${col.all.length}${col.extra > 0 ? ` (+${col.extra})` : ''}`;
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
      // External edges use an amber arrow (matching the library styling) and
      // carry no fan_in/fan_out label — they are tracked as `fan_out_external`,
      // not part of fan_in/fan_out.
      const stroke = c.ext ? '#b3801f' : '#4d6f9c';
      const marker = c.ext ? 'ah-ext' : 'ah';
      s += `<line x1="${cx}" y1="${c.y + c.h}" x2="${cx}" y2="${MNY}" stroke="${stroke}" stroke-width="1.5" marker-end="url(#${marker})"/>`;
      if (!c.ext && c.kind !== 'contains' && c.all.length > 0)
        s += `<text x="${cx+5}" y="${my+4}" font-family="system-ui,sans-serif" font-size="10" fill="#5c7a96">Fan-in: ${c.all.length}</text>`;
    });
  }

  // Main node
  const comp = node.complexity;
  // hk is 0 (not `—`) when absent — coupling is omitted from the JSON when zero.
  const hk   = fmtNum(comp?.coupling?.hk ?? 0);
  const loc  = comp?.loc?.source != null ? String(comp.loc.source) : '—';
  const mnValTrunc = (label, v) => trunc(v, Math.max(4, Math.floor((MNW - 20 - label.length * 7.2) / 7.2)));
  const mono = `font-family="ui-monospace,'SF Mono','Fira Code',monospace"`;
  const mnCycle = isCycleNode(node.id);
  // An external (3rd-party library) main node is amber, like its side cards,
  // and carries no hk/loc — just its id and (when known) its cargo-cache path
  // (e.g. `{registry}/tokio-1.49.0`, which encodes the resolved version).
  const mnExt    = node.external === true || node.kind === 'external';
  const mnFill   = mnExt ? '#f6e2c0' : '#dbe9f4';
  const mnStroke = mnCycle ? '#c00' : mnExt ? '#b3801f' : '#4d6f9c';
  const nodePath = (node.path || '').replace(/^\{[^}]+\}\//, '');
  // The whole main card is click-to-copy: clicking it copies the path (the id
  // for an external library). A `copy` cursor signals this; handled by the
  // `.mn-card` listener in modal.js.
  const copyVal = mnExt ? node.id : nodePath;
  // No native `<title>` (it would conflict with the styled `#tt` tooltips on the
  // card's labels); the click-to-copy affordance is the pointer cursor + feedback.
  s += `<g class="mn-card" data-copy="${esc(copyVal)}">`;
  s += `<rect x="${MNX}" y="${MNY}" width="${MNW}" height="${MNH2}" rx="10" fill="${mnFill}" stroke="${mnStroke}" stroke-width="${mnCycle ? '3' : '2'}"/>`;
  s += `<g class="mn-card-body" clip-path="url(#mn-clip)">`;
  if (mnExt) {
    // Same layout as an internal file card: a centred title, then left-aligned
    // `key: value` rows (kind / version / path) instead of centred lines.
    const extName = node.name || node.id.replace(/^ext:/, '');
    let ey = MNY + 58;
    s += `<text ${mono} x="${MNX+MNW/2}" y="${MNY+28}" text-anchor="middle" font-size="16" font-weight="700" fill="#1a2f45">${esc(trunc(extName, 36))}</text>`;
    s += `<text class="sn-hint" data-tip-title="Kind" data-tip="${escA('External 3rd-party library, recorded at depth 1 (its internals are not analyzed).')}" ${mono} x="${MNX+14}" y="${ey}" font-size="11" fill="#2c3e50"><tspan font-weight="700">kind: </tspan>${esc(node.kind || 'external')}</text>`;
    if (node.version) {
      ey += 22;
      s += `<text ${mono} x="${MNX+14}" y="${ey}" font-size="11" fill="#2c3e50"><tspan font-weight="700">version: </tspan>${esc(node.version)}</text>`;
    }
    if (node.path) {
      ey += 22;
      s += `<text class="sn-hint" data-tip-title="Path" data-tip="${escA('Crate location in the cargo cache; the directory name encodes the resolved version.')}" ${mono} x="${MNX+14}" y="${ey}" font-size="11" fill="#2c3e50"><tspan font-weight="700">path: </tspan>${esc(mnValTrunc('path: ', node.path))}</text>`;
    }
  } else {
    s += `<text ${mono} x="${MNX+MNW/2}" y="${MNY+28}" text-anchor="middle" font-size="16" font-weight="700" fill="#1a2f45">${esc(trunc(node.name||node.id, 36))}</text>`;
    // Visibility shown in the card only when NOT public (e.g. `private`); public
    // is the default and lives in the left key/value list.
    const visStr = typeof node.visibility === 'string'
      ? (node.visibility !== 'public' ? node.visibility : null)
      : node.visibility?.restricted ? `restricted(${node.visibility.restricted})` : null;
    let my = MNY + 58;
    if (visStr) {
      s += `<text ${mono} x="${MNX+14}" y="${my}" font-size="11" fill="#2c3e50"><tspan font-weight="700">visibility: </tspan>${esc(visStr)}</text>`;
      my += 22;
    }
    s += `<text class="sn-hint" data-tip-title="Path" data-tip="${escA('Path of this file, relative to the analyzed project root.')}" ${mono} x="${MNX+14}" y="${my}" font-size="11" fill="#2c3e50"><tspan font-weight="700">path: </tspan>${esc(mnValTrunc('path: ', nodePath))}</text>`;
    my += 22;
    s += `<text class="sn-hint" data-tip-title="${escA(COL_NAMES.hk)}" data-tip="${escA(COL_TIPS.hk)}" data-tip-formula="${escA(COL_FORMULAS.hk)}" data-tip-calc="${escA(hkCalc(node.complexity))}" ${mono} x="${MNX+14}" y="${my}" font-size="11" fill="#2c3e50"><tspan font-weight="700">hk: </tspan>${esc(hk)}</text>`;
    my += 22;
    s += `<text class="sn-hint" data-tip-title="${escA(COL_NAMES.loc)}" data-tip="${escA(COL_TIPS.loc)}" ${mono} x="${MNX+14}" y="${my}" font-size="11" fill="#2c3e50"><tspan font-weight="700">sloc: </tspan>${esc(loc)}</text>`;
  }
  s += `</g>`;
  // Shown for ~1s after a copy (the body is hidden meanwhile, see index.css):
  // the exact value that was copied, above a "copied" confirmation.
  s += `<text class="mn-copied-msg" ${mono} x="${MNX+MNW/2}" y="${MNY+MNH2/2-8}" text-anchor="middle" font-size="11" fill="#5c7a96">${esc(mnValTrunc('', copyVal))}</text>`;
  s += `<text class="mn-copied-msg" ${mono} x="${MNX+MNW/2}" y="${MNY+MNH2/2+18}" text-anchor="middle" font-size="20" font-weight="700" fill="#4d6f9c">copied</text>`;
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
        s += `<text x="${cx+5}" y="${my+4}" font-family="system-ui,sans-serif" font-size="10" fill="#5c7a96">Fan-out: ${c.all.length}</text>`;
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
function gitSourceUrl(git, relPath, line) {
  const base = gitWebBase(git?.origin);
  if (!base || !relPath) return null;
  const ref  = git.commit || git.branch || 'HEAD';
  const enc  = relPath.split('/').map(encodeURIComponent).join('/');
  const blob = /(^|\/)github\.com\//i.test(base) ? 'blob' : '-/blob';   // GitLab uses /-/blob/
  const anchor = line != null ? `#L${line}` : '';
  return `${base}/${blob}/${ref}/${enc}${anchor}`;
}

function buildModalContent(node, level) {
  const cycles  = window.CYCLES?.[level];
  const cs      = cycles?.nodeCycleStatus?.get(node.id);
  // For external libraries keep the full `{registry}`/`{cargo}` prefix (it shows
  // the source); for project files drop the leading root token.
  const path    = node.external ? (node.path || '')
                                 : (node.path || '').replace(/^\{[^}]+\}\//, '');
  const lineStr = node.line != null ? `:${node.line}` : '';
  const vis     = typeof node.visibility === 'string'
    ? node.visibility
    : node.visibility?.restricted ? `restricted(${node.visibility.restricted})` : null;

  // sections: array of { label: string|null, rows: string[] }
  const sections = [];
  let cur = { label: null, rows: [] };

  // Map a field-row key to the node-table column id, so a metric's tooltip
  // (description + formula) is byte-identical to the one shown in the table.
  const TIP_COL = {
    fan_in: 'fan_in', fan_out: 'fan_out', hk: 'hk',
    source: 'loc', logical: 'loc_logical', comments: 'loc_comments', blank: 'loc_blank',
    cyclomatic: 'cyclomatic', cognitive: 'cognitive', mi: 'mi', mi_sei: 'mi_sei',
    length: 'h_len', vocabulary: 'h_vocab', volume: 'h_vol', effort: 'h_effort',
    'time (s)': 'h_time', bugs: 'h_bugs',
  };
  const tipAttr = s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;');
  const row = (k, v, calc) => {
    if (v == null || v === '') return;
    // Prefer the shared table description + formula (`#tt` tooltip); fall back to
    // the plain `NM_HINTS` text for fields that are not table columns. `calc`, when
    // given, adds the formula filled with this node's real values.
    const colId = TIP_COL[k];
    const desc    = colId ? COL_TIPS[colId] : NM_HINTS[k];
    const formula = colId ? COL_FORMULAS[colId] : null;
    // Consistent capitalization: full label if known, else capitalize the key.
    const label = NM_LABELS[k] || (k.charAt(0).toUpperCase() + k.slice(1));
    const title = (colId && COL_NAMES[colId]) || label;
    // Tooltip on the whole <tr> so it fires on both the key and the value cell.
    const attr = desc
      ? ` data-tip="${tipAttr(desc)}" data-tip-title="${tipAttr(title)}"${formula ? ` data-tip-formula="${tipAttr(formula)}"` : ''}${calc ? ` data-tip-calc="${tipAttr(calc)}"` : ''}`
      : '';
    cur.rows.push(`<tr${attr}><td class="nm-key">${label}</td><td class="nm-val">${v}</td></tr>`);
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
    // "Source" link to the file on the project's git host, computed from the
    // snapshot's `git.origin`. Only project files (external libs live elsewhere).
    if (!node.external) {
      const url = gitSourceUrl(activeSnap()?.git, path, node.line);
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
  // For external libraries, surface every field we have (id, version, …).
  if (node.external) row('id', node.id);
  row('kind',       node.kind || null);
  row('version',    node.version ?? null);
  if (node.external) row('external', 'true');
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
      if (cpl.hk != null) row('hk', fmt(cpl.hk, 0), hkCalc(cx));
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
      row('volume',     fmt(hs.volume,  1), metricCalc('h_vol', cx));
      row('effort',     fmt(hs.effort,  0));
      row('time (s)',   fmt(hs.time,    1), metricCalc('h_time', cx));
      row('bugs',       fmt(hs.bugs,    4), metricCalc('h_bugs', cx));
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

// Toggle a node's selection from the map, mirroring the table-row checkbox:
// keep the shared selectedIds Set, the SVG highlight, the table row + checkbox,
// and the "N selected" footer all in sync.
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
  section?._updateAllCb?.();
}

// Holding Shift turns the main map into a node-selection surface: the cursor
// changes (see `.shift-select` in the CSS) and clicking a node toggles its
// selection instead of opening the modal.
(function initShiftSelect() {
  const set = on => document.body.classList.toggle('shift-select', on);
  window.addEventListener('keydown', e => { if (e.key === 'Shift') set(true); });
  window.addEventListener('keyup',   e => { if (e.key === 'Shift') set(false); });
  window.addEventListener('blur',    () => set(false));
})();

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
      // Shift = "select mode": toggle this node's selection (same as ticking its
      // table-row checkbox) instead of opening the modal.
      if (e.shiftKey) { toggleNodeSelected(node, level, section); return; }
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
      renderSVGNow(svgFrame, nodes, edges, level);
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

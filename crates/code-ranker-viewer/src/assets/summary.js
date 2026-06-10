function nodePercentiles(snap, level, getVal) {
  const nodes = (snap?.graphs?.[level]?.nodes || []).filter(n => !isExternalNode(n, level));
  const vals = nodes.map(n => getVal(n)).filter(v => typeof v === 'number' && isFinite(v) && v > 0);
  if (!vals.length) return null;
  vals.sort((a, b) => a - b);
  const pct = p => {
    const idx = p / 100 * (vals.length - 1);
    const lo = Math.floor(idx), hi = Math.ceil(idx);
    return vals[lo] + (vals[hi] - vals[lo]) * (idx - lo);
  };
  const avg = vals.reduce((s, v) => s + v, 0) / vals.length;
  return { count: vals.length, avg, min: vals[0], max: vals[vals.length - 1],
           p1: pct(1), p10: pct(10), p50: pct(50), p90: pct(90), p99: pct(99) };
}

function buildSummary() {
  const tbody = document.getElementById('summary-tbody');
  const thead = document.getElementById('summary-thead');
  if (!tbody) return;

  // Review = a single snapshot (no baseline). `current` is the primary; in review
  // the lone column reads whichever snapshot is present.
  const isReview = !window.BASELINE || !window.CURRENT;
  const baseline   = window.BASELINE ?? window.CURRENT;
  const current    = window.CURRENT  ?? window.BASELINE;

  const levels   = ['files'];
  const LLABELS  = { files: 'Files' };

  const titleEl = document.getElementById('summary-title');
  if (titleEl) titleEl.textContent = isReview ? 'Summary' : 'Diff summary';

  // Header
  if (thead) {
    if (isReview) {
      thead.innerHTML =
        `<tr><th>Metric</th>` +
        levels.map((l, i) =>
          `<th class="num level-header${i > 0 ? ' grp-start' : ''}">${LLABELS[l]}</th>`
        ).join('') + `</tr>`;
    } else {
      thead.innerHTML =
        `<tr><th rowspan="2" class="metric-header">Metric</th>` +
        levels.map((l, i) =>
          `<th colspan="3" class="level-header${i > 0 ? ' grp-start' : ''}">${LLABELS[l]}</th>`
        ).join('') + `</tr><tr>` +
        levels.map((_, i) =>
          `<th class="num${i > 0 ? ' grp-start' : ''}">Baseline</th><th class="num">Current</th><th class="num">Δ delta</th>`
        ).join('') + `</tr>`;
    }
  }

  // Helpers
  const countNodes = (snap, level) =>
    ((snap?.graphs || {})[level]?.nodes || []).filter(n => !isExternalNode(n, level)).length;

  // Edges between two internal nodes — the edges actually drawn on the map
  // (external endpoints dropped, matching countNodes / activeLocalGraph).
  const countEdges = (snap, level) => {
    const g = (snap?.graphs || {})[level];
    if (!g) return 0;
    const ids = new Set((g.nodes || []).filter(n => !isExternalNode(n, level)).map(n => n.id));
    return (g.edges || []).filter(e => ids.has(e.source) && ids.has(e.target)).length;
  };

  // Sum of a numeric node attribute across internal nodes (project total).
  const sumAttr = (snap, level, key) =>
    ((snap?.graphs || {})[level]?.nodes || [])
      .filter(n => !isExternalNode(n, level))
      .reduce((s, n) => {
        const v = nodeAttr(n, key);
        return s + (typeof v === 'number' && isFinite(v) ? v : 0);
      }, 0);

  const hasAttrKey = (level, key) => !!levelSpec(level).node_attributes?.[key];

  const fmtV = v => typeof v === 'number' && isFinite(v) ? fmtNum(v) : '';

  // `dir` is tri-state: true = lower_better, false = higher_better, null/undefined
  // = neutral (no colour). A non-boolean direction means the metric has no agreed
  // "good" way to move (raw sizes, structural counts), so the delta stays uncoloured.
  const fmtDelta = (d, dir) => {
    // Decide sign + colour from the ROUNDED display magnitude, not the raw delta:
    // a fractional delta (e.g. 0.04) that formats to "0" must read as a plain,
    // uncoloured 0 — never a coloured "+0" / "−0".
    const mag = fmtNum(Math.abs(d));
    if (mag === '0') return `<td class="num">0</td>`;
    const ds = d > 0 ? `+${mag}` : `−${mag}`;
    let cls = '';
    if (typeof dir === 'boolean') {
      const lb = dir;
      cls = (lb ? d < 0 : d > 0) ? ' delta-good' : (lb ? d > 0 : d < 0) ? ' delta-bad' : '';
    }
    return `<td class="num${cls}">${ds}</td>`;
  };

  const valueCells = (getB, getA, dir = null) =>
    levels.map((level, i) => {
      const gs = i > 0 ? ' grp-start' : '';
      const b = getB(level), a = getA(level);
      if (isReview) return `<td class="num${gs}">${fmtV(b)}</td>`;
      const d = typeof b === 'number' && typeof a === 'number' ? a - b : null;
      return `<td class="num${gs}">${fmtV(b)}</td><td class="num">${fmtV(a)}</td>` +
             (d !== null ? fmtDelta(d, dir) : '<td></td>');
    }).join('');

  const cycleCells = (getB, getA) =>
    levels.map((level, i) => {
      const gs = i > 0 ? ' grp-start' : '';
      const b = getB(level), a = getA(level);
      const cc = (v, extra) => v > 0
        ? `<td class="num${extra}"><span class="cycle-badge">${v}</span></td>`
        : `<td class="num${extra}">${v}</td>`;
      if (isReview) return cc(b, gs);
      return cc(b, gs) + cc(a, '') + fmtDelta(a - b, true);
    }).join('');

  const ttAttr = pct => pct ? ` data-tt="${escAttr(JSON.stringify(pct))}"` : '';

  // statCells: getNode reads a node → number (for percentile tooltip). `dir` is the
  // tri-state direction passed straight to fmtDelta (true/false/null).
  const statCells = (getNode, dir = null) =>
    levels.map((level, i) => {
      const gs = i > 0 ? ' grp-start' : '';
      const b = nodePercentiles(baseline, level, getNode);
      const a = nodePercentiles(current,  level, getNode);
      const bAvg = b ? b.avg : null;
      const aAvg = a ? a.avg : null;
      if (isReview) return `<td class="num${gs}"${ttAttr(b)}>${fmtV(bAvg)}</td>`;
      const d = typeof bAvg === 'number' && typeof aAvg === 'number' ? aAvg - bAvg : null;
      return `<td class="num${gs}"${ttAttr(b)}>${fmtV(bAvg)}</td>` +
             `<td class="num"${ttAttr(a)}>${fmtV(aAvg)}</td>` +
             (d !== null ? fmtDelta(d, dir) : '<td></td>');
    }).join('');

  const row = (label, cells, tip, formula) => {
    const tipAttr = tip ? ` data-tip="${escAttr(tip)}"` : '';
    const fAttr = formula ? ` data-tip-formula="${escAttr(formula)}"` : '';
    return `<tr><td class="metric-cell"${tipAttr}${fAttr}>${label}</td>${cells}</tr>`;
  };

  // ── Row builders: id → function returning the <tr> HTML ('' = skip this row in
  // this snapshot). Metadata (label/tip/formula/direction) all comes from
  // schema.js; metric values are the per-file AVERAGE via statCells. The display
  // order is the explicit ROW_ORDER list at the bottom — reorder THAT to move
  // rows around. ──
  const level0       = levels[0];
  // summary_metrics is the snapshot's curated, already-pruned metric order (Rust
  // assemble_level keeps only keys present on internal nodes — render verbatim).
  const summaryKeys  = levelUi(level0).summary_metrics || [];

  // A per-metric row: per-file AVERAGE via statCells (label/tip/formula/direction
  // from schema.js).
  const metricRow = key => {
    const dirRaw = attrDirection(level0, key);  // 'lower_better' | 'higher_better' | null
    const dir    = dirRaw === 'lower_better' ? true : dirRaw === 'higher_better' ? false : null;
    return row(attrName(level0, key), statCells(n => nodeAttr(n, key), dir),
               attrDesc(level0, key) || undefined, attrFormula(level0, key) || undefined);
  };
  // A project-wide TOTAL row: sum of a node attribute across internal files
  // (neutral — a raw total has no "good" direction). Skipped if the attribute is
  // absent from this snapshot.
  const totalRow = (key, label, tip) => () =>
    levels.some(l => hasAttrKey(l, key))
      ? row(label, valueCells(
          level => sumAttr(baseline, level, key),
          level => sumAttr(current, level, key)), tip)
      : '';
  const cyclesRow = () => {
    const anyCycles = levels.some(level => {
      const cy = window.CYCLES?.[level];
      return cy && (cy.cycleBaseline + cy.cycleBoth + cy.cycleCurrent) > 0;
    });
    if (!anyCycles) return '';
    // Tooltip: how many cycle groups of each kind were found, from the active
    // snapshot's backend-computed `cycles`. Kind labels come from schema.js.
    const level = 'files';
    const kc = {};
    for (const g of (current?.graphs?.[level]?.cycles || [])) kc[g.kind] = (kc[g.kind] || 0) + 1;
    const kparts = Object.entries(kc).filter(([, n]) => n > 0)
      .map(([k, n]) => `${cycleKindLabel(level, k)}: ${n}`);
    const cyclesTip = kparts.length
      ? `Nodes in at least one dependency cycle. Cycle groups by type — ${kparts.join(', ')}.`
      : 'Number of nodes that participate in at least one dependency cycle.';
    return row('Nodes in cycles', cycleCells(
      level => { const cy = window.CYCLES?.[level]; return cy ? cy.cycleBaseline + cy.cycleBoth : 0; },
      level => { const cy = window.CYCLES?.[level]; return cy ? cy.cycleCurrent  + cy.cycleBoth : 0; }
    ), cyclesTip);
  };

  const builders = {
    'nodes-sum':  () => row('Nodes', valueCells(           // count — no "good" direction
                          level => countNodes(baseline, level),
                          level => countNodes(current, level))),
    'edges-sum':  () => row('Edges', valueCells(           // count — no "good" direction
                          level => countEdges(baseline, level),
                          level => countEdges(current, level)),
                          'Total dependency edges between internal nodes (external-library edges excluded).'),
    // Project-wide raw / source line totals (sums of per-file loc / sloc).
    'loc-sum':    totalRow('loc',  'Lines (total)',        'Total raw lines across all files — the sum of every file’s loc.'),
    'sloc-sum':   totalRow('sloc', 'Source lines (total)', 'Total source lines across all files — the sum of every file’s sloc.'),
    'cycles-sum': cyclesRow,
  };
  // One builder per summary metric (except sloc — its total is the `sloc-sum` row),
  // keyed `metric:<key>`.
  for (const key of summaryKeys) if (key !== 'sloc') builders[`metric:${key}`] = () => metricRow(key);

  // ── ROW ORDER — EDIT THIS LIST to rearrange the summary rows. Every row is named
  // explicitly. Ids resolving to '' (absent in this snapshot — e.g. a metric this
  // language does not emit) are skipped; any builder id missing from the list is
  // appended at the end so a newly-added metric never silently vanishes. ──
  const ROW_ORDER = [
    'nodes-sum',
    'edges-sum',
    'loc-sum',
    'sloc-sum',
    'cycles-sum',
    'metric:fan_in',
    'metric:fan_out',
    'metric:hk',
    'metric:cyclomatic',
    'metric:cognitive',
    'metric:mi',
    'metric:mi_sei',
    'metric:volume',
    'metric:bugs',
    'metric:effort',
    'metric:time',
    'metric:length',
    'metric:vocabulary',
    'metric:lloc',
    'metric:cloc',
    'metric:blank',
    'metric:tloc',
  ];

  const listed = new Set(ROW_ORDER);
  const order  = [...ROW_ORDER, ...Object.keys(builders).filter(id => !listed.has(id))];
  const rows   = order.map(id => (builders[id] ? builders[id]() : '')).filter(Boolean);

  tbody.innerHTML = rows.join('');
}

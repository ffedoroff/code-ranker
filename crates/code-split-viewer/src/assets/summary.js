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
  return { count: vals.length, p1: pct(1), p10: pct(10), p50: pct(50), p90: pct(90), p99: pct(99) };
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
          `<th class="num${i > 0 ? ' grp-start' : ''}">Baseline</th><th class="num">Current</th><th class="num">Δ</th>`
        ).join('') + `</tr>`;
    }
  }

  // Helpers
  const countNodes = (snap, level) =>
    ((snap?.graphs || {})[level]?.nodes || []).filter(n => !isExternalNode(n, level)).length;

  const fmtV = v => typeof v === 'number' && isFinite(v) ? fmtNum(v) : '';

  const fmtDelta = (d, lb) => {
    const ds = d === 0 ? '0' : (d > 0 ? `+${fmtNum(d)}` : `−${fmtNum(-d)}`);
    const cls = (lb ? d < 0 : d > 0) ? ' delta-good' : (lb ? d > 0 : d < 0) ? ' delta-bad' : '';
    return `<td class="num${cls}">${ds}</td>`;
  };

  const valueCells = (getB, getA, lb = false) =>
    levels.map((level, i) => {
      const gs = i > 0 ? ' grp-start' : '';
      const b = getB(level), a = getA(level);
      if (isReview) return `<td class="num${gs}">${fmtV(b)}</td>`;
      const d = typeof b === 'number' && typeof a === 'number' ? a - b : null;
      return `<td class="num${gs}">${fmtV(b)}</td><td class="num">${fmtV(a)}</td>` +
             (d !== null ? fmtDelta(d, lb) : '<td></td>');
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

  // statCells: getNode reads a node → number (for percentile tooltip)
  const statCells = (getNode, lb = false) =>
    levels.map((level, i) => {
      const gs = i > 0 ? ' grp-start' : '';
      const b = nodePercentiles(baseline, level, getNode);
      const a = nodePercentiles(current,  level, getNode);
      const bAvg = b ? b.p50 : null;
      const aAvg = a ? a.p50 : null;
      if (isReview) return `<td class="num${gs}"${ttAttr(b)}>${fmtV(bAvg)}</td>`;
      const d = typeof bAvg === 'number' && typeof aAvg === 'number' ? aAvg - bAvg : null;
      return `<td class="num${gs}"${ttAttr(b)}>${fmtV(bAvg)}</td>` +
             `<td class="num"${ttAttr(a)}>${fmtV(aAvg)}</td>` +
             (d !== null ? fmtDelta(d, lb) : '<td></td>');
    }).join('');

  const row = (label, cells, tip, formula) => {
    const tipAttr = tip ? ` data-tip="${escAttr(tip)}"` : '';
    const fAttr = formula ? ` data-tip-formula="${escAttr(formula)}"` : '';
    return `<tr><td class="metric-cell"${tipAttr}${fAttr}>${label}</td>${cells}</tr>`;
  };

  const rows = [];

  // Node counts
  rows.push(row('Nodes', valueCells(
    level => countNodes(baseline, level),
    level => countNodes(current, level)
  )));

  // Cycles
  const anyCycles = levels.some(level => {
    const cy = window.CYCLES?.[level];
    return cy && (cy.cycleBaseline + cy.cycleBoth + cy.cycleCurrent) > 0;
  });
  if (anyCycles) {
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
    rows.push(row('Nodes in cycles', cycleCells(
      level => { const cy = window.CYCLES?.[level]; return cy ? cy.cycleBaseline + cy.cycleBoth : 0; },
      level => { const cy = window.CYCLES?.[level]; return cy ? cy.cycleCurrent  + cy.cycleBoth : 0; }
    ), cyclesTip));
  }

  // Metric rows — ordered list from the snapshot's ui.summary_metrics.
  // The Rust orchestrator already prunes this list to metrics present on
  // internal nodes (see assemble_level), so we render every key verbatim — no
  // data-presence re-check here. Each key's metadata (label, description,
  // formula, direction) comes from schema.js.
  for (const level of levels) {
    const summaryKeys = levelUi(level).summary_metrics || [];
    for (const key of summaryKeys) {
      const label   = attrName(level, key);
      const tip     = attrDesc(level, key);
      const formula = attrFormula(level, key);
      const lb      = attrDirection(level, key) === 'lower_better';
      rows.push(row(label, statCells(n => nodeAttr(n, key), lb), tip || undefined, formula || undefined));
    }
    // Only iterate once — all metric rows for all levels are handled above.
    // (levels array is ['files'] for now, but the structure is kept future-proof.)
    break;
  }

  tbody.innerHTML = rows.join('');
}

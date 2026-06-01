function nodePercentiles(snap, level, getVal) {
  const nodes = (snap?.graphs?.[level]?.nodes || []).filter(n => !n.external);
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

const METRIC_DESCS = {
  'Cyclomatic complexity': 'Number of linearly independent paths through the code. Higher values indicate complex branching logic.',
  'Cognitive complexity':  'Measures how difficult the code is to understand, accounting for nesting depth and non-structural control flow.',
  'Lines of code':         'Source lines (sloc) — lines with at least one non-whitespace, non-comment character. Blank and comment-only lines are not counted; a code line with a trailing comment still counts.',
  'Maintainability index': 'Maintainability Index (0–100, higher is more maintainable). Derived from Halstead volume, cyclomatic complexity, and LOC.',
  'Maintainability (SEI)': 'SEI variant of the Maintainability Index — adds a bonus for comment density.',
  'Halstead volume':       'Algorithm size in bits, from distinct operators and operands.',
  'Halstead bugs':         'Estimated delivered bugs — a rough predictor of defect density.',
  'Halstead effort':       'Mental effort to implement the algorithm.',
  'Halstead time, s':      'Estimated implementation time, in seconds.',
  'Halstead length':       'Program length — total operator + operand occurrences.',
  'Halstead vocabulary':   'Vocabulary — distinct operators + operands.',
  'Fan-in':                'Number of files that import (depend on) this one. High fan-in means broadly reused.',
  'Fan-out':               'Number of files this one imports. High fan-out means many dependencies. External-library edges are counted separately.',
  'Henry–Kafura (HK)':     'Henry–Kafura complexity — combines file size with incoming/outgoing coupling (internal file→file edges only).',
  'Logical LOC':           'Logical lines — counts statements, not physical lines.',
  'Comment lines':         'Comment-only lines (inline comments on code lines are not counted).',
  'Blank lines':           'Empty or whitespace-only lines.',
};

const COL_TIPS = {
  loc:          METRIC_DESCS['Lines of code'],
  cyclomatic:   METRIC_DESCS['Cyclomatic complexity'],
  cognitive:    METRIC_DESCS['Cognitive complexity'],
  mi:           METRIC_DESCS['Maintainability index'],
  mi_sei:       METRIC_DESCS['Maintainability (SEI)'],
  fan_in:       METRIC_DESCS['Fan-in'],
  fan_out:      METRIC_DESCS['Fan-out'],
  hk:           METRIC_DESCS['Henry–Kafura (HK)'],
  h_vol:        METRIC_DESCS['Halstead volume'],
  h_bugs:       METRIC_DESCS['Halstead bugs'],
  h_effort:     METRIC_DESCS['Halstead effort'],
  h_time:       METRIC_DESCS['Halstead time, s'],
  h_len:        METRIC_DESCS['Halstead length'],
  h_vocab:      METRIC_DESCS['Halstead vocabulary'],
  loc_logical:  METRIC_DESCS['Logical LOC'],
  loc_comments: METRIC_DESCS['Comment lines'],
  loc_blank:    METRIC_DESCS['Blank lines'],
};

// Formula shown on its own bold line in a metric's description tooltip
// (used by both the summary table and the node-table column headers).
const METRIC_FORMULAS = {
  'Henry–Kafura (HK)':     'LOC × (Fan-in × Fan-out)²',
  'Cyclomatic complexity': 'branches + 1',
  'Maintainability index': '171 − 5.2·ln(volume) − 0.23·cyclomatic − 16.2·ln(loc)',
  'Maintainability (SEI)': 'MI + 50·sin(√(2.4 × comment-ratio))',
  'Halstead length':       'N₁ + N₂',
  'Halstead vocabulary':   'η₁ + η₂',
  'Halstead volume':       'length × log₂(vocabulary)',
  'Halstead effort':       'volume × difficulty',
  'Halstead bugs':         'volume ÷ 3000',
  'Halstead time, s':      'effort ÷ 18',
};
// Same formulas keyed by node-table column id.
const COL_FORMULAS = {
  hk:         METRIC_FORMULAS['Henry–Kafura (HK)'],
  cyclomatic: METRIC_FORMULAS['Cyclomatic complexity'],
  mi:         METRIC_FORMULAS['Maintainability index'],
  mi_sei:     METRIC_FORMULAS['Maintainability (SEI)'],
  h_len:      METRIC_FORMULAS['Halstead length'],
  h_vocab:    METRIC_FORMULAS['Halstead vocabulary'],
  h_vol:      METRIC_FORMULAS['Halstead volume'],
  h_effort:   METRIC_FORMULAS['Halstead effort'],
  h_bugs:     METRIC_FORMULAS['Halstead bugs'],
  h_time:     METRIC_FORMULAS['Halstead time, s'],
};

function buildSummary() {
  const tbody = document.getElementById('summary-tbody');
  const thead = document.getElementById('summary-thead');
  if (!tbody) return;

  const isReview = window.AFTER === null;
  const before   = window.BEFORE;
  const after    = window.AFTER ?? window.BEFORE;

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
          `<th class="num${i > 0 ? ' grp-start' : ''}">Before</th><th class="num">After</th><th class="num">Δ</th>`
        ).join('') + `</tr>`;
    }
  }

  // Helpers
  const countNodes = (snap, level) =>
    ((snap?.graphs || {})[level]?.nodes || []).filter(n => !n.external).length;

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

  // stat: get reads stats obj → number; getNode reads a node → number (for percentile tooltip)
  const statCells = (get, getNode, lb = false) =>
    levels.map((level, i) => {
      const gs = i > 0 ? ' grp-start' : '';
      const b = nodePercentiles(before, level, getNode);
      const a = nodePercentiles(after,  level, getNode);
      const bAvg = b ? b.p50 : null;
      const aAvg = a ? a.p50 : null;
      if (isReview) return `<td class="num${gs}"${ttAttr(b)}>${fmtV(bAvg)}</td>`;
      const d = typeof bAvg === 'number' && typeof aAvg === 'number' ? aAvg - bAvg : null;
      return `<td class="num${gs}"${ttAttr(b)}>${fmtV(bAvg)}</td>` +
             `<td class="num"${ttAttr(a)}>${fmtV(aAvg)}</td>` +
             (d !== null ? fmtDelta(d, lb) : '<td></td>');
    }).join('');

  const row = (label, cells, tip) => {
    const tipAttr = tip ? ` data-tip="${escAttr(tip)}"` : '';
    const f = METRIC_FORMULAS[label];
    const fAttr = f ? ` data-tip-formula="${escAttr(f)}"` : '';
    return `<tr><td class="metric-cell"${tipAttr}${fAttr}>${label}</td>${cells}</tr>`;
  };

  const rows = [];

  // Node counts
  rows.push(row('Nodes', valueCells(
    level => countNodes(before, level),
    level => countNodes(after, level)
  )));

  // Cycles
  const anyCycles = levels.some(level => {
    const cy = window.CYCLES?.[level];
    return cy && (cy.cycleBefore + cy.cycleBoth + cy.cycleAfter) > 0;
  });
  if (anyCycles) {
    rows.push(row('Nodes in cycles', cycleCells(
      level => { const cy = window.CYCLES?.[level]; return cy ? cy.cycleBefore + cy.cycleBoth : 0; },
      level => { const cy = window.CYCLES?.[level]; return cy ? cy.cycleAfter  + cy.cycleBoth : 0; }
    ), 'Number of nodes that participate in at least one dependency cycle.'));
  }

  const STATS = [
    { get: st => st?.cyclomatic,              getNode: n => n.complexity?.cyclomatic,              label: 'Cyclomatic complexity', lb: true  },
    { get: st => st?.cognitive,               getNode: n => n.complexity?.cognitive,               label: 'Cognitive complexity',  lb: true  },
    { get: st => st?.loc?.source,             getNode: n => n.complexity?.loc?.source,             label: 'Lines of code',         lb: false },
    { get: st => st?.maintainability?.mi,     getNode: n => n.complexity?.maintainability?.mi,     label: 'Maintainability index',  lb: false },
    { get: st => st?.maintainability?.mi_sei, getNode: n => n.complexity?.maintainability?.mi_sei, label: 'Maintainability (SEI)',  lb: false },
    { get: st => st?.halstead?.volume,        getNode: n => n.complexity?.halstead?.volume,        label: 'Halstead volume',       lb: true  },
    { get: st => st?.halstead?.bugs,          getNode: n => n.complexity?.halstead?.bugs,          label: 'Halstead bugs',         lb: true  },
    { get: st => st?.halstead?.effort,        getNode: n => n.complexity?.halstead?.effort,        label: 'Halstead effort',       lb: true  },
    { get: st => st?.halstead?.time,          getNode: n => n.complexity?.halstead?.time,          label: 'Halstead time, s',      lb: true  },
    { get: st => st?.halstead?.length,        getNode: n => n.complexity?.halstead?.length,        label: 'Halstead length',       lb: true  },
    { get: st => st?.halstead?.vocabulary,    getNode: n => n.complexity?.halstead?.vocabulary,    label: 'Halstead vocabulary',   lb: true  },
    { get: st => st?.coupling?.fan_in,        getNode: n => n.complexity?.coupling?.fan_in,        label: 'Fan-in',                lb: false },
    { get: st => st?.coupling?.fan_out,       getNode: n => n.complexity?.coupling?.fan_out,       label: 'Fan-out',               lb: false },
    { get: st => st?.coupling?.hk,            getNode: n => n.complexity?.coupling?.hk,            label: 'Henry–Kafura (HK)',     lb: true  },
    { get: st => st?.loc?.logical,            getNode: n => n.complexity?.loc?.logical,            label: 'Logical LOC',           lb: false },
    { get: st => st?.loc?.comments,           getNode: n => n.complexity?.loc?.comments,           label: 'Comment lines',         lb: false },
    { get: st => st?.loc?.blank,              getNode: n => n.complexity?.loc?.blank,              label: 'Blank lines',           lb: false },
  ];

  for (const { get, getNode, label, lb } of STATS) {
    const hasData = levels.some(level =>
      nodePercentiles(before, level, getNode) !== null ||
      nodePercentiles(after,  level, getNode) !== null
    );
    if (!hasData) continue;
    rows.push(row(label, statCells(get, getNode, lb), METRIC_DESCS[label]));
  }

  tbody.innerHTML = rows.join('');
}

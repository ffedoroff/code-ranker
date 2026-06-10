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
  // Delta magnitude (unsigned). The value cells stay rounded (fmtNum), but the
  // delta keeps enough precision to never collapse a real change to "0": large
  // deltas use the normal rounded/abbreviated form, while a delta that *would*
  // round to 0 yet is genuinely non-zero (e.g. avg 4.83→4.82, Δ −0.0012) is shown
  // to 2 significant digits. Only an exactly-equal pair (within float noise) is 0.
  const fmtDeltaNum = d => {
    const ad = Math.abs(d);
    if (ad < 1e-9) return '0';
    const rounded = fmtNum(ad);
    return rounded !== '0' ? rounded : String(Number(ad.toPrecision(2)));
  };
  const fmtDelta = (d, dir) => {
    const mag = fmtDeltaNum(d);
    if (mag === '0') return `<td class="num">0</td>`;   // exactly equal — plain, uncoloured
    const ds = d > 0 ? `+${mag}` : `−${mag}`;
    let cls = '';
    if (typeof dir === 'boolean') {
      const lb = dir;
      cls = (lb ? d < 0 : d > 0) ? ' delta-good' : (lb ? d > 0 : d < 0) ? ' delta-bad' : '';
    }
    return `<td class="num${cls}">${ds}</td>`;
  };

  const ttAttr = pct => pct ? ` data-tt="${escAttr(JSON.stringify(pct))}"` : '';

  // Render the value cells for one row from its baseline/current numbers: in a diff
  // that is Baseline | Current | Δ, in review the single value. `ttB`/`ttA` carry
  // the per-side distribution for the hover tooltip (metric rows); `badge` wraps a
  // positive count in a cycle badge; `dir` drives the Δ colour.
  const valueCellsHTML = (b, a, { dir = null, ttB = null, ttA = null, badge = false } = {}) => {
    const cell = (v, tt) => {
      const inner = badge && typeof v === 'number' && v > 0
        ? `<span class="cycle-badge">${fmtV(v)}</span>` : fmtV(v);
      return `<td class="num"${tt ? ttAttr(tt) : ''}>${inner}</td>`;
    };
    if (isReview) return cell(b, ttB);
    const d = typeof b === 'number' && typeof a === 'number' ? a - b : null;
    return cell(b, ttB) + cell(a, ttA) + (d !== null ? fmtDelta(d, dir) : '<td></td>');
  };

  // A row as BOTH its rendered <tr> and a plain data record (the export reads the
  // records, so the table and the downloaded file never drift). `b`/`a` are the
  // baseline/current numbers (null when absent).
  const rowRecord = (label, b, a, opts = {}) => {
    const tipAttr = opts.tip ? ` data-tip="${escAttr(opts.tip)}"` : '';
    const fAttr   = opts.formula ? ` data-tip-formula="${escAttr(opts.formula)}"` : '';
    const html = `<tr><td class="metric-cell"${tipAttr}${fAttr}>${label}</td>${valueCellsHTML(b, a, opts)}</tr>`;
    const bn = typeof b === 'number' ? b : null;
    const an = isReview ? null : (typeof a === 'number' ? a : null);
    const delta = bn != null && an != null ? an - bn : null;
    return { html, data: { label, baseline: bn, current: an, delta } };
  };

  // ── Row builders: id → function returning the <tr> HTML ('' = skip this row in
  // this snapshot). Metadata (label/tip/formula/direction) all comes from schema.js.
  // metric:<key> rows show the stat picked by the header radio; structural rows are
  // plain counts. The display order is the `LAYOUT` section tree at the bottom —
  // edit THAT (sections and their `rows`) to move rows around. ──
  const level0       = levels[0];
  // summary_metrics is the snapshot's curated, already-pruned metric order (Rust
  // assemble_level keeps only keys present on internal nodes — render verbatim).
  const summaryKeys  = levelUi(level0).summary_metrics || [];

  // A per-metric row. The active summary stat (window._summaryStat — avg by
  // default, set by the header radio) picks what each cell shows: `sum` aggregates
  // the metric over internal files, every other stat (avg/min/p50/p90/max) reads
  // the per-file distribution. label/tip/formula/direction come from schema.js.
  const metricRow = key => {
    if (!hasAttrKey(level0, key)) return null;   // metric not present in this snapshot
    const dirRaw = attrDirection(level0, key);   // 'lower_better' | 'higher_better' | null
    const dir    = dirRaw === 'lower_better' ? true : dirRaw === 'higher_better' ? false : null;
    const stat   = window._summaryStat || 'avg';
    const opts   = { dir, tip: attrDesc(level0, key) || undefined, formula: attrFormula(level0, key) || undefined };
    // Label: the metric key (abbreviation) followed by its human name/explanation
    // from the spec — e.g. `loc - Lines`, `hk - Henry–Kafura (HK)`. Falls back to
    // the bare key when the spec carries no distinct name.
    const human  = attrName(level0, key);   // name || label || key (schema.js)
    const label  = human && human.toLowerCase() !== key.toLowerCase() ? `${key} - ${human}` : key;
    if (stat === 'sum')
      return rowRecord(label, sumAttr(baseline, level0, key), sumAttr(current, level0, key), opts);
    const distB = nodePercentiles(baseline, level0, n => nodeAttr(n, key));
    const distA = nodePercentiles(current,  level0, n => nodeAttr(n, key));
    return rowRecord(label, distB ? distB[stat] : null, distA ? distA[stat] : null,
                     { ...opts, ttB: distB, ttA: distA });
  };
  // Distinct grouping-key values (the groups drawn on the map — e.g. crates) per
  // side. Empty when the level has no grouping.
  const groupingKey = levelUi(level0).grouping?.key || null;
  const countGroups = (snap, level) => {
    if (!groupingKey) return 0;
    const s = new Set();
    for (const n of ((snap?.graphs || {})[level]?.nodes || []))
      if (!isExternalNode(n, level)) {
        const v = nodeAttr(n, groupingKey);
        if (v != null && v !== '') s.add(v);
      }
    return s.size;
  };
  const groupsLabel = groupingKey
    ? groupingKey.charAt(0).toUpperCase() + groupingKey.slice(1) + 's'   // crate → Crates
    : 'Groups';
  // Distinct directories holding the files (full dir path per node) per side.
  const countFolders = (snap, level) => {
    const s = new Set();
    for (const n of ((snap?.graphs || {})[level]?.nodes || []))
      if (!isExternalNode(n, level)) s.add(nodeFullDir(n));
    return s.size;
  };
  const cyclesRow = () => {
    const cy = window.CYCLES?.[level0];
    if (!cy || (cy.cycleBaseline + cy.cycleBoth + cy.cycleCurrent) === 0) return null;
    // Tooltip: how many cycle groups of each kind were found, from the active
    // snapshot's backend-computed `cycles`. Kind labels come from schema.js.
    const kc = {};
    for (const g of (current?.graphs?.[level0]?.cycles || [])) kc[g.kind] = (kc[g.kind] || 0) + 1;
    const kparts = Object.entries(kc).filter(([, n]) => n > 0)
      .map(([k, n]) => `${cycleKindLabel(level0, k)}: ${n}`);
    const tip = kparts.length
      ? `Nodes in at least one dependency cycle. Cycle groups by type — ${kparts.join(', ')}.`
      : 'Number of nodes that participate in at least one dependency cycle.';
    return rowRecord('Nodes in cycles', cy.cycleBaseline + cy.cycleBoth, cy.cycleCurrent + cy.cycleBoth,
                     { dir: true, badge: true, tip });
  };

  // ── Structural count rows. These are plain counts (no per-file distribution),
  // so the per-stat radio does not apply to them. ──
  const builders = {
    'nodes':   () => rowRecord(LLABELS[level0] || 'Nodes',   // "Files" at the files level
                       countNodes(baseline, level0), countNodes(current, level0)),
    'folders': () => rowRecord('Folders',
                       countFolders(baseline, level0), countFolders(current, level0),
                       { tip: 'Distinct directories that contain the files.' }),
    'groups':  () => groupingKey
                       ? rowRecord(groupsLabel,
                           countGroups(baseline, level0), countGroups(current, level0),
                           { tip: `Distinct ${groupingKey} values — the groups shown on the map.` })
                       : null,
    'edges':   () => rowRecord('Edges',
                       countEdges(baseline, level0), countEdges(current, level0),
                       { tip: 'Total dependency edges between internal nodes (external-library edges excluded).' }),
    'cycles':  cyclesRow,
  };

  // ── LAYOUT — the table as a tree of titled sections, each holding its row ids in
  // order. EDIT THIS to rearrange: move a section, reorder its `rows`, or retitle
  // it. Row ids: 'nodes'/'groups'/'edges'/'cycles' (structural counts) and
  // 'metric:<key>' (per-file stat rows, driven by the radio). `{ radio: true }` is
  // the in-table aggregation control — placed where the rows stop being plain
  // counts and start following the radio. A metric the snapshot lacks renders
  // nothing; a section left with no rows drops its header. ──
  const LAYOUT = [
    { title: 'sum always', rows: ['nodes', 'folders', 'groups', 'edges', 'cycles'] },
    { radio: true },
    { title: 'Coupling',   rows: ['metric:fan_in', 'metric:fan_out', 'metric:hk'] },
    { title: 'Lines',      rows: ['metric:loc', 'metric:sloc', 'metric:lloc', 'metric:cloc', 'metric:blank', 'metric:tloc'] },
    { title: 'Complexity', rows: ['metric:cyclomatic', 'metric:cognitive', 'metric:mi', 'metric:mi_sei'] },
    { title: 'Halstead',   rows: ['metric:volume', 'metric:bugs', 'metric:effort', 'metric:time', 'metric:length', 'metric:vocabulary'] },
  ];

  // One metric builder per key referenced (LAYOUT ∪ summary_metrics); metricRow
  // itself returns '' for keys absent from this snapshot.
  const laidOutRows = LAYOUT.flatMap(s => s.rows || []);
  const metricKeys  = new Set([
    ...summaryKeys,
    ...laidOutRows.filter(id => id.startsWith('metric:')).map(id => id.slice('metric:'.length)),
  ]);
  for (const key of metricKeys) builders[`metric:${key}`] = () => metricRow(key);

  // Sub-header divider spanning every column (metric label + per-side value cells).
  const headSpan  = 1 + levels.length * (isReview ? 1 : 3);
  const headerRow = title =>
    `<tr class="summary-subhead"><td colspan="${headSpan}">${escHtml(title)}</td></tr>`;
  // The aggregation radio rendered as a full-width divider row inside the table
  // (change is handled by a delegated listener on the tbody — see setupSummaryStatControl).
  const statRow = () => {
    const cur = window._summaryStat || 'avg';
    const opts = SUMMARY_STATS.map(s =>
      `<label class="summary-stat-opt"><input type="radio" name="summary-stat" value="${s}"` +
      `${s === cur ? ' checked' : ''}>${s}</label>`).join('');
    return `<tr class="summary-stat-row"><td colspan="${headSpan}"><span class="summary-stat">${opts}</span></td></tr>`;
  };

  // Any metric builder not placed in LAYOUT lands in a trailing "Other" section, so
  // a newly-added metric never silently vanishes.
  const placed    = new Set(laidOutRows);
  const leftovers = Object.keys(builders).filter(id => !placed.has(id));
  const sections  = leftovers.length ? [...LAYOUT, { title: 'Other', rows: leftovers }] : LAYOUT;

  // Render each section: build its rows, drop the empties, and emit the header only
  // when at least one row survives. Each section also feeds the export model.
  const out = [], model = [];
  for (const sec of sections) {
    if (sec.radio) { out.push(statRow()); continue; }   // in-table aggregation control
    const recs = sec.rows.map(id => (builders[id] ? builders[id]() : null)).filter(Boolean);
    if (!recs.length) continue;
    if (sec.title) out.push(headerRow(sec.title));
    out.push(...recs.map(r => r.html));
    model.push({ section: sec.title, rows: recs.map(r => r.data) });
  }
  tbody.innerHTML = out.join('');

  // Structured model for the JSON/MD export (mirrors exactly what is rendered).
  window._summaryModel = {
    target:   window.META?.target || 'snapshot',
    mode:     isReview ? 'review' : 'diff',
    stat:     window._summaryStat || 'avg',
    baseline: window.META?.baseline || null,
    current:  isReview ? null : (window.META?.current || null),
    sections: model,
  };
}

// The stats the header radio offers for the metric rows. `sum` aggregates over
// files; the rest read the per-file distribution. The structural count rows
// (Files/Folders/groups/Edges/cycles) ignore this and always show their count.
const SUMMARY_STATS = ['avg', 'min', 'p50', 'p90', 'max', 'sum'];

// Wire the in-table aggregation radio (once). The radio row is re-rendered by
// every buildSummary, so the handler is delegated on the persistent tbody. The
// choice re-renders the table and round-trips through the URL (`stat=` — nav.js).
function setupSummaryStatControl() {
  const tbody = document.getElementById('summary-tbody');
  if (!tbody || tbody._statWired) return;
  tbody._statWired = true;
  if (!window._summaryStat) window._summaryStat = 'avg';
  tbody.addEventListener('change', e => {
    if (e.target.name !== 'summary-stat') return;
    window._summaryStat = e.target.value;
    buildSummary();
    window.navReplaceView?.();
  });
}
window.setupSummaryStatControl = setupSummaryStatControl;
// Whether `s` is a valid aggregation id (guards URL-restored values).
window.isSummaryStat = s => SUMMARY_STATS.includes(s);
// Apply an aggregation chosen from outside (URL restore / popstate): updates the
// state, re-renders the table, and reflects the choice in the radio.
window.setSummaryStat = s => {
  if (!SUMMARY_STATS.includes(s) || s === (window._summaryStat || 'avg')) return;
  window._summaryStat = s;
  const radio = document.querySelector(`.summary-stat input[value="${s}"]`);
  if (radio) radio.checked = true;
  buildSummary();
};

// ── Export ────────────────────────────────────────────────────────────────────
// Trigger a client-side file download (everything stays offline — no network).
function downloadFile(name, text, mime) {
  const blob = new Blob([text], { type: mime });
  const url  = URL.createObjectURL(blob);
  const a    = document.createElement('a');
  a.href = url; a.download = name;
  document.body.appendChild(a); a.click(); a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// Base file name: `<target>-summary-<stat>` (e.g. `code-ranker-summary-avg`).
function summaryFileBase() {
  const m = window._summaryModel || {};
  const slug = String(m.target || 'summary').replace(/[^\w.-]+/g, '-').replace(/^-+|-+$/g, '') || 'summary';
  return `${slug}-summary-${m.stat || 'avg'}`;
}

// The export TEXT builders (shared by download + copy-to-clipboard).
function summaryJSONText() {
  const m = window._summaryModel;
  return m ? JSON.stringify(m, null, 2) : '';
}
// Markdown: a title + provenance line, then one table per section. In a diff each
// table is | Metric | Baseline | Current | Δ |; in review just | Metric | Value |.
function summaryMarkdownText() {
  const m = window._summaryModel;
  if (!m) return '';
  const review = m.mode === 'review';
  const fmt = v => (v == null ? '' : fmtNum(v));
  const dlt = v => {
    if (v == null) return '';
    const ad = Math.abs(v);
    if (ad < 1e-9) return '0';
    const r = fmtNum(ad);
    const mag = r !== '0' ? r : String(Number(ad.toPrecision(2)));
    return v > 0 ? `+${mag}` : `−${mag}`;
  };
  const side = s => s ? `${s.name}${s.commit ? ` (${s.commit})` : ''}` : '—';
  const lines = [`# ${m.target} — ${review ? 'summary' : 'diff summary'}`, ''];
  if (!review) lines.push(`Baseline: ${side(m.baseline)} · Current: ${side(m.current)}`, '');
  lines.push(`Stat: \`${m.stat}\``, '');
  for (const sec of m.sections) {
    if (sec.section) lines.push(`## ${sec.section}`, '');
    lines.push(review ? '| Metric | Value |' : '| Metric | Baseline | Current | Δ |',
               review ? '| --- | ---: |'   : '| --- | ---: | ---: | ---: |');
    for (const r of sec.rows)
      lines.push(review
        ? `| ${r.label} | ${fmt(r.baseline)} |`
        : `| ${r.label} | ${fmt(r.baseline)} | ${fmt(r.current)} | ${dlt(r.delta)} |`);
    lines.push('');
  }
  return lines.join('\n');
}

function exportSummaryJSON()     { const t = summaryJSONText();     if (t) downloadFile(`${summaryFileBase()}.json`, t, 'application/json'); }
function exportSummaryMarkdown() { const t = summaryMarkdownText(); if (t) downloadFile(`${summaryFileBase()}.md`,   t, 'text/markdown'); }

// Copy to the clipboard with a brief "copied ✓" confirmation on the button.
function copySummaryText(text, btn) {
  if (!text) return;
  navigator.clipboard?.writeText(text).then(() => {
    if (!btn) return;
    const prev = btn.textContent;
    btn.textContent = 'copied ✓';
    setTimeout(() => { btn.textContent = prev; }, 1200);
  });
}
window.exportSummaryJSON = exportSummaryJSON;
window.exportSummaryMarkdown = exportSummaryMarkdown;

// ── Popup open/close + export wiring (once) ─────────────────────────────────────
// `syncUrl` writes the open/closed state to the URL (`panel=stats`) so a refresh
// reopens the popup; it is passed false when restoring FROM the URL/history.
// (Event handlers pass the Event object, which is truthy → syncUrl on by default.)
function openSummaryPopup(syncUrl = true) {
  const ov = document.getElementById('summary-overlay');
  if (!ov) return;
  window._statsOpen = true;
  buildSummary();                 // refresh to the active side/stat before showing
  // Keep the page header visible: start the white fill just below it.
  const hdr = document.querySelector('header');
  ov.style.top = (hdr ? hdr.offsetHeight : 0) + 'px';
  ov.style.display = 'flex';
  document.body.style.overflow = 'hidden';
  if (syncUrl) window.navReplaceView?.();
}
function closeSummaryPopup(syncUrl = true) {
  const ov = document.getElementById('summary-overlay');
  window._statsOpen = false;
  if (ov) ov.style.display = 'none';
  document.body.style.overflow = '';
  if (syncUrl) window.navReplaceView?.();
}
window.openSummaryPopup = openSummaryPopup;
window.closeSummaryPopup = closeSummaryPopup;

function setupSummaryPopup() {
  const ov = document.getElementById('summary-overlay');
  if (!ov || ov._wired) return;
  ov._wired = true;
  document.getElementById('stats-btn')?.addEventListener('click', openSummaryPopup);
  document.getElementById('summary-close')?.addEventListener('click', closeSummaryPopup);
  // Footer: download links (.json / .md) + copy-to-clipboard buttons.
  document.getElementById('summary-dl-json')?.addEventListener('click', e => { e.preventDefault(); exportSummaryJSON(); });
  document.getElementById('summary-dl-md')?.addEventListener('click', e => { e.preventDefault(); exportSummaryMarkdown(); });
  document.getElementById('summary-copy-json')?.addEventListener('click', e => copySummaryText(summaryJSONText(), e.currentTarget));
  document.getElementById('summary-copy-md')?.addEventListener('click', e => copySummaryText(summaryMarkdownText(), e.currentTarget));
  ov.addEventListener('mousedown', e => { if (e.target === ov) closeSummaryPopup(); });
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape' && ov.style.display !== 'none') closeSummaryPopup();
  });
}
window.setupSummaryPopup = setupSummaryPopup;

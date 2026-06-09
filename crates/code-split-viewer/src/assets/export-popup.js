// Distinct warning *types* — how many metrics have at least one internal node
// over their `warning` threshold, plus `cycle` as one binary type (any node in
// a dependency cycle). Shown next to the Prompt-Generator (AI) button.
function warningTypeCount(level) {
  // Count over the active side, so the badge tracks Baseline/Current like the rest.
  const nodes = ((typeof activeGraph === 'function' ? activeGraph(level).nodes : window.DIFF?.[level]?.nodes) || [])
    .filter(n => !isExternalNode(n, level));
  const sortMetrics = levelUi(level).sort_metrics || [];
  let count = sortMetrics.filter(m => {
    if (m === 'cycle') return false; // handled separately below
    const th = attrThresholds(level, m);
    if (!th) return false;
    return nodes.some(n => (nodeAttr(n, m) ?? 0) > th.warning);
  }).length;
  const cy = window.CYCLES?.[level]?.nodeCycleStatus;
  if (cy && nodes.some(n => { const cs = cy.get(n.id); return cs != null && cs !== 'none'; })) count += 1;
  return count;
}
window.warningTypeCount = warningTypeCount;

// True while the Prompt-Generator popup is on screen. Global map hotkeys bail on
// this so keys (notably Ctrl/Cmd+C to copy the generated prompt) reach the popup
// instead of toggling map modifiers.
function isPromptPopupOpen() {
  const ov = document.getElementById('export-popup-overlay');
  return !!ov && ov.style.display !== 'none';
}
window.isPromptPopupOpen = isPromptPopupOpen;

// ── Prompt-Generator state in the URL ────────────────────────────────────────
// The popup persists its full state in the query string so a refresh restores it
// exactly (open state, preset, source, count, sort metric, connection toggles,
// and the selected node ids). `epsel` is repeated once per selected id.
const EP_KEYS = ['ep', 'eppreset', 'epsrc', 'epn', 'epsort', 'epconn', 'epsel'];

function epWriteUrlState(s) {
  const p = new URLSearchParams(location.search);
  EP_KEYS.forEach(k => p.delete(k));
  p.set('ep', s.level);
  if (s.preset) p.set('eppreset', s.preset);
  p.set('epsrc', s.src === 'selected' ? 'sel' : 'rec');
  if (s.n != null && s.n !== '') p.set('epn', String(s.n));
  if (s.sort) p.set('epsort', s.sort);
  if (s.conn && s.conn.length) p.set('epconn', s.conn.join(','));
  (s.sel || []).forEach(id => p.append('epsel', id));
  history.replaceState(history.state, '', '?' + p);
}

function epReadUrl() {
  const p = new URLSearchParams(location.search);
  if (!p.has('ep')) return null;
  return {
    level:  p.get('ep'),
    preset: p.get('eppreset') || null,
    src:    p.get('epsrc') || null,
    n:      p.get('epn'),
    sort:   p.get('epsort') || null,
    conn:   (p.get('epconn') || '').split(',').filter(Boolean),
    sel:    p.getAll('epsel'),
  };
}

function epClearUrl() {
  const p = new URLSearchParams(location.search);
  let changed = false;
  EP_KEYS.forEach(k => { if (p.has(k)) { changed = true; p.delete(k); } });
  if (changed) history.replaceState(history.state, '', p.toString() ? '?' + p : location.pathname);
}

function openExportPopup(level, restore) {
  const selectedIds = window._ntSelected?.[level];
  // Operate on the active side (Baseline/Current), so the generated prompt matches
  // the snapshot the user is looking at — same source the map and table use.
  // (Review mode → the single snapshot.) Edges are kept to local↔local pairs,
  // mirroring the diff's edge set: no external links, no cross-side noise.
  const activeG     = (typeof activeGraph === 'function') ? activeGraph(level) : (window.DIFF?.[level] || {});
  const allNodes    = activeG.nodes || [];
  const localIds    = new Set(allNodes.filter(n => !isExternalNode(n, level)).map(n => n.id));
  const allEdges    = (activeG.edges || []).filter(e => localIds.has(e.source) && localIds.has(e.target));
  const selNodes    = allNodes.filter(n => selectedIds?.has(n.id));

  const cleanPath = p => (p || '').replace(/^\{[^}]+\}\//, '');
  // Edge endpoints are node ids; render them as the node's path (id as a fallback)
  // so connection lists in the prompt use paths, not raw ids.
  const nodeById = new Map(allNodes.map(n => [n.id, n]));
  const pathOf = id => { const n = nodeById.get(id); return n ? (cleanPath(n.path) || n.id) : id; };

  // ── popup DOM (created once) ──────────────────────────────────────────
  let overlay = document.getElementById('export-popup-overlay');
  if (!overlay) {
    const presets     = snapshotPresets();
    const ui          = levelUi(level);
    const sortMetrics = ui.sort_metrics || ['hk'];

    const sortOptions = sortMetrics.map(m => {
      const label = m === 'cycle' ? 'in a cycle' : attrShort(level, m);
      return `<option value="${m}">${label}</option>`;
    }).join('');

    const presetBtns = presets.map(p =>
      `<button class="exp-preset-btn" data-preset="${p.id}">${p.label}<span class="exp-preset-count"></span></button>`
    ).join('');

    overlay = document.createElement('div');
    overlay.id = 'export-popup-overlay';
    overlay.innerHTML =
      '<div id="export-popup">' +
        '<div id="export-popup-hdr">' +
          '<h3 id="export-popup-title">Prompt Generator</h3>' +
          '<button id="export-popup-close">✕</button>' +
        '</div>' +
        '<div class="exp-modes">' +
          '<div class="exp-cb-group">' +
            '<span class="exp-conn-label">Connections:</span>' +
            '<label class="exp-mode-cb"><input type="checkbox" data-mode="conn-in"> in</label>' +
            '<label class="exp-mode-cb"><input type="checkbox" data-mode="conn-out"> out</label>' +
            '<label class="exp-mode-cb"><input type="checkbox" data-mode="conn-common"> common</label>' +
          '</div>' +
          '<div class="exp-source-group">' +
            '<label class="exp-src-radio"><input type="radio" name="exp-source" value="selected" checked> <span class="exp-sel-count">0</span> Selected</label>' +
            '<span class="exp-source-or">OR</span>' +
            '<label class="exp-src-radio"><input type="radio" name="exp-source" value="recommended"> <input type="number" class="exp-rec-count" min="1" max="9999" value="1"></label>' +
            `<select class="exp-sort-select" title="Recommend the top rows sorted by this metric">${sortOptions}</select>` +
          '</div>' +
        '</div>' +
        '<div class="exp-textarea-wrap">' +
          '<div id="export-preview" class="exp-md-preview"></div>' +
          '<textarea id="export-textarea" readonly></textarea>' +
          '<button class="exp-copy-btn">Copy markdown <span class="exp-copy-icon">⎘</span></button>' +
        '</div>' +
        '<div class="exp-presets">' +
          '<div class="exp-presets-label">Presets</div>' +
          `<div class="exp-preset-btns">${presetBtns}</div>` +
        '</div>' +
      '</div>';
    document.body.appendChild(overlay);

    const closeExport = () => { window.flyoutHeader?.unmount('prompt'); overlay.style.display = 'none'; document.body.style.overflow = ''; epClearUrl(); };
    document.getElementById('export-popup-close').addEventListener('click', closeExport);
    overlay.addEventListener('mousedown', e => { if (e.target === overlay) closeExport(); });
    document.addEventListener('keydown', e => { if (e.key === 'Escape' && overlay.style.display !== 'none') closeExport(); });
    overlay.querySelector('.exp-copy-btn').addEventListener('click', () => {
      const ta = document.getElementById('export-textarea');
      navigator.clipboard?.writeText(ta.value).then(() => {
        const btn = overlay.querySelector('.exp-copy-btn');
        const orig = btn.innerHTML;
        btn.innerHTML = 'Copied ✓';
        setTimeout(() => { btn.innerHTML = orig; }, 1400);
      });
    });
  }

  // Wrap a preset's title + prompt + doc_url into the full instruction the AI
  // receives: intent, the summary, the link to the full principle, and a
  // research/report protocol (download & read the principle, report violations
  // in the modules below, save the report to `.code-split/<timestamp>-<id>.md`).
  const composePrompt = id => {
    const preset = snapshotPresets().find(p => p.id === id);
    if (!preset) return '';
    const { title, prompt: summary, doc_url: url } = preset;
    const lines = [
      `# ${title}`,
      '',
      'I want to apply this to some modules in my system.',
      '',
      '## Summary',
      '',
      summary,
      '',
    ];
    if (url) {
      lines.push(
        `**Full principle:** [${url}](${url})`,
        '',
        'Download and read the full principle to understand it in detail. If you cannot download it, **stop the task immediately**.',
        '');
    }
    lines.push(
      '## Task',
      '',
      '- Prepare a precise, detailed estimate and a report of where the modules below violate it.',
      '- If you find more serious violations elsewhere during research, mention them in the report too.',
      '- Show a summary of the report in chat.',
      `- If any violation is found, suggest saving the report to a file as a plan for a detailed review, named \`.code-split/<YYYYMMDD-HHMMSS>-${id}.md\` (e.g. \`.code-split/20260601-191019-${id}.md\`).`,
      '',
      '**Focus the research and report primarily on the modules below.**');
    return lines.join('\n');
  };

  // Rebind handlers each open (closures capture fresh selNodes/edges)
  const ta = document.getElementById('export-textarea');
  let activePresetKey = null;

  const internalNodes = () => allNodes.filter(n => !isExternalNode(n, level) && n.status !== 'removed');

  // For a sort metric: ALL candidate nodes sorted worst-first (so the count can
  // keep adding rows), plus how many cross the `warning` / `info` thresholds.
  // `cycle` → only nodes in a cycle (sorted by hk).
  const recoFor = metric => {
    if (metric === 'cycle') {
      const cy = window.CYCLES?.[level];
      const inCycle = internalNodes().filter(n => cy?.nodeCycleStatus?.get(n.id) != null)
        .sort((a, b) => (nodeAttr(b, 'hk') ?? 0) - (nodeAttr(a, 'hk') ?? 0));
      return { metric: 'cycle', sorted: inCycle, warningCount: inCycle.length, infoCount: inCycle.length };
    }
    const th = attrThresholds(level, metric) || attrThresholds(level, 'hk') || { info: 0, warning: 0 };
    const sorted = internalNodes()
      .sort((a, b) =>
        (nodeAttr(b, metric) ?? 0) - (nodeAttr(a, metric) ?? 0) ||
        (nodeAttr(b, 'sloc')   ?? 0) - (nodeAttr(a, 'sloc')   ?? 0) ||
        (nodeAttr(b, 'items')  ?? 0) - (nodeAttr(a, 'items')  ?? 0)
      );
    const warningCount = sorted.filter(n => (nodeAttr(n, metric) ?? 0) > th.warning).length;
    const infoCount    = sorted.filter(n => (nodeAttr(n, metric) ?? 0) > th.info).length;
    return { metric, info: th.info, warning: th.warning, sorted, warningCount, infoCount };
  };

  const recCount = overlay.querySelector('.exp-rec-count');
  const sortSel  = overlay.querySelector('.exp-sort-select');
  const activeMetric = () => sortSel.value;

  // Mirror the current controls into the URL (called from buildContent, so every
  // state change is persisted). `sel` is the FULL selection set across both sides
  // (baseline-only + current-only + common), not just the active side's — otherwise
  // opening the popup on one side would drop the other side's selections on reload.
  const epWriteUrl = () => epWriteUrlState({
    level,
    preset: activePresetKey,
    src:    overlay.querySelector('input[name="exp-source"]:checked')?.value,
    n:      recCount.value,
    sort:   sortSel.value,
    conn:   [...overlay.querySelectorAll('.exp-mode-cb input')]
              .filter(c => c.checked && !c.disabled).map(c => c.dataset.mode),
    sel:    [...(window._ntSelected?.[level] || [])],
  });

  const getActiveNodes = () => {
    const src = overlay.querySelector('input[name="exp-source"]:checked')?.value;
    if (src === 'recommended') {
      const count = parseInt(recCount.value) || 0;
      return recoFor(activeMetric()).sorted.slice(0, count);
    }
    return selNodes;
  };

  // Emphasis by zone: warning gets a calm text-colour highlight; info is left
  // plain (no class) to keep the UI low-sensitivity.
  const colorCount = () => {
    const r = recoFor(activeMetric());
    const c = parseInt(recCount.value) || 0;
    recCount.classList.remove('exp-rec-warn');
    if (c > 0 && c <= r.warningCount) recCount.classList.add('exp-rec-warn');
  };

  // Selecting a preset points the sort dropdown at its metric and sets the count
  // to that preset's headline recommendation (warning count if any, else info).
  const updateRecoUI = id => {
    const preset  = id ? snapshotPresets().find(p => p.id === id) : null;
    const metric  = preset?.sort_metric || levelUi(level).default_sort || sortSel.options[0]?.value;
    if (metric) sortSel.value = metric;
    const r = recoFor(sortSel.value);
    recCount.value = String(r.warningCount > 0 ? r.warningCount : r.infoCount);
    colorCount();
  };

  // Per-preset badge: warning-level count as a calm text-colour pill (a label);
  // info-level count as a plain number (no pill, no emphasis); else nothing.
  const updatePresetBadges = () => {
    overlay.querySelectorAll('.exp-preset-btn').forEach(btn => {
      const badge = btn.querySelector('.exp-preset-count');
      if (!badge) return;
      const preset = snapshotPresets().find(p => p.id === btn.dataset.preset);
      const metric = preset?.sort_metric || levelUi(level).default_sort || sortSel.options[0]?.value;
      const r = recoFor(metric);
      if (r.warningCount > 0) {
        badge.textContent = String(r.warningCount);
        badge.className = 'exp-preset-count exp-preset-count--warn';
      } else if (r.infoCount > 0) {
        badge.textContent = String(r.infoCount);
        badge.className = 'exp-preset-count exp-preset-count--info';
      } else {
        badge.textContent = '';
        badge.className = 'exp-preset-count';
      }
    });
  };

  const buildContent = () => {
    const activeNodes = getActiveNodes();
    const activeSet   = new Set(activeNodes.map(n => n.id));
    const innerEdges  = allEdges.filter(e => activeSet.has(e.source) && activeSet.has(e.target));
    const outerEdges  = allEdges.filter(e => activeSet.has(e.source) !== activeSet.has(e.target));
    const inEdges     = outerEdges.filter(e => activeSet.has(e.target));
    const outEdges    = outerEdges.filter(e => activeSet.has(e.source));

    // A checkbox is enabled only when it would actually contribute something;
    // otherwise it is disabled and unchecked (it can't influence the output).
    const counts = { 'conn-common': innerEdges.length, 'conn-in': inEdges.length, 'conn-out': outEdges.length };
    const cbs = [...overlay.querySelectorAll('.exp-mode-cb input')];
    cbs.forEach(cb => {
      const empty = !(counts[cb.dataset.mode] > 0);
      cb.disabled = empty;
      if (empty) cb.checked = false;
      cb.closest('.exp-mode-cb')?.classList.toggle('exp-mode-cb--off', empty);
    });

    const on = id => { const c = cbs.find(c => c.dataset.mode === id); return !!(c && !c.disabled && c.checked); };
    const parts = [];
    if (activePresetKey) {
      const p = composePrompt(activePresetKey);
      if (p) parts.push(p);
    }
    // Node paths are always included (the modules the prompt is about). In
    // Recommended mode they are ordered by the sort metric, annotated with each
    // node's value, and preceded by a short explanation of that metric.
    if (activeNodes.length) {
      const src = overlay.querySelector('input[name="exp-source"]:checked')?.value;
      const path = n => (cleanPath(n.path) || n.id) + (n.line != null ? `:${n.line}` : '');
      if (src === 'recommended') {
        const m = activeMetric();
        if (m === 'cycle') {
          const lines = activeNodes.map(n => `- \`${path(n)}\``).join('\n');
          parts.push(['## Modules in a dependency cycle', lines].filter(Boolean).join('\n\n'));
        } else {
          const label   = attrShort(level, m);
          const desc    = attrDesc(level, m);
          const formula = attrFormula(level, m);
          const lines = activeNodes.map(n => {
            const v = nodeAttr(n, m);
            const vr = typeof v === 'number' ? Math.round(v) : v;
            return (vr != null && vr !== 0) ? `- \`${path(n)}\` (${label}: ${vr})` : `- \`${path(n)}\``;
          }).join('\n');
          const intro = [desc, formula ? `**Formula:** \`${formula}\`` : ''].filter(Boolean).join('\n\n');
          parts.push([`## Modules ordered by ${label}`, intro, lines].filter(Boolean).join('\n\n'));
        }
      } else {
        parts.push('## Modules\n\n' + activeNodes.map(n => `- \`${path(n)}\``).join('\n'));
      }
    }
    const edgeFmt = edges => edges.length ? edges.map(e => `- \`${pathOf(e.source)}\` → \`${pathOf(e.target)}\` (${e.kind})`).join('\n') : '_(none)_';
    if (on('conn-common')) parts.push('## Connections — common\n\n' + edgeFmt(innerEdges));
    if (on('conn-in'))     parts.push('## Connections — in\n\n'  + edgeFmt(inEdges));
    if (on('conn-out'))    parts.push('## Connections — out\n\n' + edgeFmt(outEdges));
    ta.value = parts.join('\n\n');
    const preview = document.getElementById('export-preview');
    if (preview && typeof window.snarkdown === 'function') {
      preview.innerHTML = window.snarkdown(ta.value);
    }
    epWriteUrl();
  };

  overlay.querySelectorAll('.exp-mode-cb input').forEach(cb => { cb.onchange = buildContent; });

  overlay.querySelectorAll('input[name="exp-source"]').forEach(r => { r.onchange = buildContent; });
  // Editing the recommended count implies the Recommended source.
  overlay.querySelector('.exp-rec-count').oninput = () => {
    const rec = overlay.querySelector('input[name="exp-source"][value="recommended"]');
    if (rec) rec.checked = true;
    colorCount();
    buildContent();
  };
  // Changing the sort metric re-ranks the recommended list (implies Recommended).
  sortSel.onchange = () => {
    const rec = overlay.querySelector('input[name="exp-source"][value="recommended"]');
    if (rec) rec.checked = true;
    colorCount();
    buildContent();
  };

  const applyPresetChecks = id => {
    const preset = id ? snapshotPresets().find(p => p.id === id) : null;
    // connections values in the snapshot: "in" / "out" / "common" → map to data-mode
    const connMap = { in: 'conn-in', out: 'conn-out', common: 'conn-common' };
    const active = (preset?.connections || []).map(c => connMap[c]).filter(Boolean);
    overlay.querySelectorAll('.exp-mode-cb input').forEach(cb => {
      cb.checked = active.includes(cb.dataset.mode);
    });
  };

  overlay.querySelectorAll('.exp-preset-btn').forEach(btn => {
    btn.onclick = () => {
      const key = btn.dataset.preset;
      if (activePresetKey === key) {
        activePresetKey = null;
        btn.classList.remove('exp-preset-btn--active');
        applyPresetChecks(null);
      } else {
        activePresetKey = key;
        overlay.querySelectorAll('.exp-preset-btn').forEach(b => b.classList.remove('exp-preset-btn--active'));
        btn.classList.add('exp-preset-btn--active');
        applyPresetChecks(key);
        // Switch to Recommended and size the count to this preset's recommendation.
        const rec = overlay.querySelector('input[name="exp-source"][value="recommended"]');
        if (rec) rec.checked = true;
      }
      updateRecoUI(activePresetKey);
      buildContent();
    };
  });

  // With nothing selected, the "Selected" radio + "OR" are hidden and the source
  // defaults to Recommended; otherwise the source defaults to Selected.
  const noSel = selNodes.length === 0;
  overlay.querySelector('input[name="exp-source"][value="selected"]')
    ?.closest('.exp-src-radio')?.style.setProperty('display', noSel ? 'none' : '');
  overlay.querySelector('.exp-source-or')?.style.setProperty('display', noSel ? 'none' : '');
  // With nothing selected there is only one source — hide its lone radio dot too,
  // leaving just the count + sort dropdown.
  overlay.querySelector('input[name="exp-source"][value="recommended"]')
    ?.style.setProperty('display', noSel ? 'none' : '');
  // Real selected-node count shown next to the "Selected" radio.
  const selCountEl = overlay.querySelector('.exp-sel-count');
  if (selCountEl) selCountEl.textContent = String(selNodes.length);

  if (restore) {
    // Restore from the URL: preset, source, count, sort metric, connection toggles.
    activePresetKey = restore.preset || null;
    overlay.querySelectorAll('.exp-preset-btn').forEach(b =>
      b.classList.toggle('exp-preset-btn--active', b.dataset.preset === activePresetKey));
    const srcVal = restore.src === 'sel' ? 'selected' : 'recommended';
    overlay.querySelectorAll('input[name="exp-source"]').forEach(r => { r.checked = r.value === srcVal; });
    if (restore.sort) sortSel.value = restore.sort;
    recCount.value = (restore.n != null && restore.n !== '') ? restore.n : '1';
    overlay.querySelectorAll('.exp-mode-cb input').forEach(c => { c.checked = restore.conn.includes(c.dataset.mode); });
  } else {
    // Fresh open: only paths, no active preset; seed the criterion from default.
    activePresetKey = null;
    overlay.querySelectorAll('.exp-preset-btn').forEach(b => b.classList.remove('exp-preset-btn--active'));
    overlay.querySelectorAll('.exp-mode-cb input').forEach(c => { c.checked = false; });
    overlay.querySelectorAll('input[name="exp-source"]').forEach(r => {
      r.checked = noSel ? r.value === 'recommended' : r.value === 'selected';
    });
    const defaultSort = levelUi(level).default_sort;
    if (defaultSort) sortSel.value = defaultSort;
    updateRecoUI(null);
    recCount.value = '1';   // default: recommend 1 row
  }
  colorCount();
  updatePresetBadges(); // count badges on each preset button
  buildContent();       // also mirrors state into the URL
  // Reflect the active side in the title: Prompt Generator / … Baseline / … Current.
  const titleEl = document.getElementById('export-popup-title');
  if (titleEl) titleEl.textContent = 'Prompt Generator' +
    (typeof viewModeSuffix === 'function' ? viewModeSuffix() : '');
  overlay.style.display = 'flex';
  document.body.style.overflow = 'hidden';
  window.flyoutHeader?.mount(overlay, 'prompt');
}

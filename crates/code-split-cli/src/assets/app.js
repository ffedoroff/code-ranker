let activePreset = 'diff';

function isGraphsIdentical() {
  if (!window.AFTER || !window.DIFF) return false;
  return ['modules', 'files', 'functions'].every(level => {
    const d = window.DIFF[level];
    return !d || (d.nodes.every(n => n.status === 'unchanged') &&
                  d.edges.every(e => e.status === 'unchanged'));
  });
}

function setActivePreset(name) {
  activePreset = name;
  document.querySelectorAll('[data-preset]').forEach(b => b.classList.toggle('active', b.dataset.preset === name));
  document.getElementById('custom-indicator').textContent = isGraphsIdentical() ? 'Identical' : '';
}

function showCustomState() {
  document.querySelectorAll('[data-preset]').forEach(b => b.classList.remove('active'));
  document.getElementById('custom-indicator').textContent = isGraphsIdentical() ? 'Identical' : 'Custom';
}

function updateHeader() {
  const meta     = window.META;
  const hasAfter = meta.after !== null;
  const isReview = !hasAfter;
  const wasReview = document.body.classList.contains('mode-review');

  document.body.classList.toggle('mode-review', isReview);
  document.getElementById('title').textContent = `${meta.target} — ${isReview ? 'review' : 'diff'}`;
  document.getElementById('meta-before-date').textContent   = meta.before.date ? fmtDate(meta.before.date) + ' ' : '';
  document.getElementById('meta-before-name').textContent   = meta.before.name;
  document.getElementById('meta-before-commit').textContent = meta.before.commit ? ` ${meta.before.commit}` : '';
  document.getElementById('meta-before-info').style.display = meta.before.name || meta.before.commit ? '' : 'none';
  document.getElementById('meta-arrow').style.display       = isReview ? 'none' : '';
  document.getElementById('meta-after-date').textContent    = hasAfter && meta.after.date ? fmtDate(meta.after.date) + ' ' : '';
  document.getElementById('meta-after-name').textContent    = hasAfter ? meta.after.name   : '';
  document.getElementById('meta-after-commit').textContent  = hasAfter && meta.after.commit ? ` ${meta.after.commit}` : '';
  document.getElementById('meta-after-info').style.display  = hasAfter && (meta.after.name || meta.after.commit) ? '' : 'none';
  document.getElementById('btn-remove-after').style.display = hasAfter ? '' : 'none';
  document.getElementById('btn-upload-after').textContent   = isReview ? '↑ compare…' : '↑ change';

  document.querySelectorAll('[data-preset="after"],[data-preset="diff"]').forEach(btn => {
    btn.disabled = !hasAfter;
    btn.classList.toggle('disabled', !hasAfter);
  });

  if (!isReview && wasReview) {
    setActivePreset('diff');
    document.querySelectorAll('.view').forEach(sec => sec._applyPreset?.('diff'));
  } else if (isReview && !wasReview) {
    setActivePreset('before');
    document.querySelectorAll('.view').forEach(sec => sec._applyPreset?.('before'));
  }
  updateReviewButtons(document.querySelector('.view.active'));
}

function updateFilesTab() {
  const hasFileNodes = snap =>
    ((snap?.graphs || {}).files?.nodes || []).some(n => n.kind === 'file');
  const show = hasFileNodes(window.BEFORE) || hasFileNodes(window.AFTER);

  const wrap = document.getElementById('nav-files-item');
  if (wrap) wrap.style.display = show ? '' : 'none';

  if (!show && document.querySelector('.view.active')?.dataset.view === 'files') {
    document.querySelector('.report-switch a[data-view="modules"]')?.click();
  }
}

// Recompute everything after the user swaps a snapshot via the upload controls.
function recomputeAll() {
  const EMPTY = { graphs: {} };
  const before = window.BEFORE;
  const after  = window.AFTER;
  window.DIFF   = computeDiff(before ?? EMPTY, after ?? before ?? EMPTY);
  window.CYCLES = computeCycles(before ?? EMPTY, after ?? before ?? EMPTY);
  window.META   = computeMeta(before, after);

  buildSummary();
  updateFilesTab();

  // Reset rendered state; refresh chip counts first, then updateHeader may override
  document.querySelectorAll('.view').forEach(sec => {
    sec.dataset.rendered = 'false';
    sec._refreshCounts?.();
  });

  updateHeader();
  setActivePreset(activePreset);

  // Re-render active view
  const active = document.querySelector('.view.active');
  if (active && window.gv) renderView(active);
}

function renderView(section) {
  if (section.dataset.rendered === 'true') return;
  const level   = section.dataset.view;
  const frame   = section.querySelector('.svg-frame');
  const loading = section.querySelector('.loading-indicator');

  if (loading) { loading.textContent = 'Computing layout…'; loading.classList.add('on'); }
  setTimeout(() => {
    drawSVG(frame, window.DIFF[level].nodes, window.DIFF[level].edges, level);
    window._ntSelected?.[level]?.forEach(id => section._gNodeMap?.get(id)?.classList.add('node-selected'));
    section.dataset.rendered = 'true';
    section._applyFrameClasses?.();
    if (loading) loading.classList.remove('on');
  }, 30);
}

function buildSnapPopupHTML(snap, refSnap) {
  if (!snap) return '';
  const sections = [];

  // General
  const genRows = [];
  if (snap.generated_at) {
    let dateStr = fmtDate(snap.generated_at);
    if (refSnap?.generated_at) {
      const diffMs = new Date(snap.generated_at) - new Date(refSnap.generated_at);
      dateStr += ` <span class="sp-gen-diff">(diff ${fmtDuration(Math.abs(diffMs))})</span>`;
    }
    genRows.push(`<div class="sp-row"><span class="sp-lbl">Generated</span><span>${dateStr}</span></div>`);
  }
  if (snap.command)
    genRows.push(`<div class="sp-cmd-block"><div class="sp-cmd-bar"><span class="sp-lbl">Command</span><button class="sp-copy-btn" data-copy="${escAttr(snap.command)}" title="Copy">⧉</button></div><textarea class="sp-cmd-input" readonly rows="3">${escHtml(snap.command)}</textarea></div>`);
  if (genRows.length)
    sections.push(`<div class="sp-section-label">General</div>${genRows.join('')}`);

  // Git
  if (snap.git) {
    const { branch, commit, dirty_files } = snap.git;
    const gitRows = [];
    if (branch)              gitRows.push(`<div class="sp-row"><span class="sp-lbl">Branch</span><span>${escHtml(branch)}</span></div>`);
    if (commit)              gitRows.push(`<div class="sp-row"><span class="sp-lbl">Commit hash</span><span>${escHtml(commit)}</span></div>`);
    if (dirty_files != null) gitRows.push(`<div class="sp-row"><span class="sp-lbl">Dirty files</span><span>${dirty_files > 0 ? dirty_files : '0 (clean)'}</span></div>`);
    if (gitRows.length)
      sections.push(`<div class="sp-section-label">Git</div>${gitRows.join('')}`);
  }

  // Duration
  if (snap.timings?.length) {
    const trows = snap.timings.map(t =>
      `<div class="sp-row"><span class="sp-lbl">${escHtml(t.stage)}</span><span>${fmtMs(t.ms)}</span></div>`
    ).join('');
    sections.push(`<div class="sp-section-label">Duration</div>${trows}`);
  }

  return sections.join('');
}

function setupSnapPopup() {
  let popup = null, hideTimer = null;

  function getPopup() {
    if (!popup) {
      popup = document.createElement('div');
      popup.id = 'snap-popup';
      document.body.appendChild(popup);
      popup.addEventListener('mouseenter', () => { clearTimeout(hideTimer); hideTimer = null; });
      popup.addEventListener('mouseleave', scheduleHide);
    }
    return popup;
  }

  function scheduleHide() {
    hideTimer = setTimeout(() => { if (popup) popup.style.display = 'none'; }, 150);
  }

  function show(snap, anchor, refSnap) {
    if (!snap) return;
    const p = getPopup();
    p.innerHTML = buildSnapPopupHTML(snap, refSnap);
    p.querySelectorAll('.sp-copy-btn').forEach(btn => {
      btn.addEventListener('click', e => {
        e.stopPropagation();
        navigator.clipboard?.writeText(btn.dataset.copy).then(() => {
          btn.textContent = '✓';
          setTimeout(() => { btn.textContent = '⧉'; }, 1500);
        });
      });
    });
    p.style.display = 'block';
    requestAnimationFrame(() => {
      const r  = anchor.getBoundingClientRect();
      let left = r.left;
      let top  = r.bottom + 6;
      if (left + p.offsetWidth  > window.innerWidth  - 8) left = window.innerWidth  - p.offsetWidth  - 8;
      if (top  + p.offsetHeight > window.innerHeight - 8) top  = r.top - p.offsetHeight - 6;
      p.style.left = left + 'px';
      p.style.top  = top  + 'px';
    });
  }

  document.querySelectorAll('.snap-group').forEach((grp, i) => {
    grp.addEventListener('mouseenter', () => {
      clearTimeout(hideTimer);
      const snap = i === 0 ? window.BEFORE : window.AFTER;
      const ref  = i === 1 ? window.BEFORE : null;
      show(snap, grp, ref);
    });
    grp.addEventListener('mouseleave', scheduleHide);
  });
}

// Read a snapshot embedded inline in this page (a `<script type="application/json">` tag).
function readEmbeddedSnapshot(id) {
  const el = document.getElementById(id);
  if (!el) return null;
  const t = el.textContent.trim();
  return t && t !== 'null' ? JSON.parse(t) : null;
}

// Parse a snapshot from uploaded file text — either a raw JSON snapshot, or a code-split
// HTML report with the snapshot embedded (prefer `cs-after`, else `cs-before`).
function extractSnapshotFromText(text) {
  const s = text.trim();
  if (s.startsWith('{')) return JSON.parse(s);
  const doc = new DOMParser().parseFromString(text, 'text/html');
  const read = id => {
    const t = doc.getElementById(id)?.textContent?.trim();
    return t && t !== 'null' ? JSON.parse(t) : null;
  };
  return read('cs-after') || read('cs-before');
}

// Manual snapshot swap: load a different before/after snapshot (.json or .html) into the viewer.
function setupFileControls() {
  const inputBefore = document.getElementById('input-before');
  const inputAfter  = document.getElementById('input-after');

  document.getElementById('btn-upload-before').addEventListener('click', () => inputBefore.click());
  document.getElementById('btn-upload-after').addEventListener('click',  () => inputAfter.click());

  document.getElementById('btn-remove-after').addEventListener('click', () => {
    window.AFTER = null;
    recomputeAll();
  });

  inputBefore.addEventListener('change', () => {
    const file = inputBefore.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = e => {
      try { window.BEFORE = extractSnapshotFromText(e.target.result); } catch { alert('Invalid snapshot file'); return; }
      recomputeAll();
    };
    reader.readAsText(file);
    inputBefore.value = '';
  });

  inputAfter.addEventListener('change', () => {
    const file = inputAfter.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = e => {
      try { window.AFTER = extractSnapshotFromText(e.target.result); } catch { alert('Invalid snapshot file'); return; }
      recomputeAll();
    };
    reader.readAsText(file);
    inputAfter.value = '';
  });
}

const REVIEW_CHIP = { nodes: 'nodes-unchanged', edges: 'edges-unchanged', cycles: 'cycle-before' };

function updateReviewButtons(section) {
  if (!document.body.classList.contains('mode-review')) return;
  const level = section?.dataset.view;
  const diff  = window.DIFF?.[level];
  const cy    = window.CYCLES?.[level];

  const nodeCount  = diff?.nodes.filter(n => !n.external).length ?? 0;
  const edgeCount  = diff?.edges.length ?? 0;
  const cycleCount = cy ? (cy.cycleBoth + cy.cycleBefore + cy.cycleAfter) : 0;

  const chipActive = id => section?.querySelector(`[data-chip="${id}"]`)?.classList.contains('active') ?? true;

  const nodesBtn  = document.querySelector('[data-review="nodes"]');
  const edgesBtn  = document.querySelector('[data-review="edges"]');
  const cyclesBtn = document.querySelector('[data-review="cycles"]');

  if (nodesBtn)  { nodesBtn.textContent  = `Nodes: ${nodeCount}`;   nodesBtn.classList.toggle('active',  chipActive('nodes-unchanged')); }
  if (edgesBtn)  { edgesBtn.textContent  = `Edges: ${edgeCount}`;   edgesBtn.classList.toggle('active',  chipActive('edges-unchanged')); }
  if (cyclesBtn) {
    cyclesBtn.textContent = `Cycles: ${cycleCount}`;
    cyclesBtn.classList.toggle('has-cycles', cycleCount > 0);
    cyclesBtn.classList.toggle('active', chipActive('cycle-before') && cycleCount > 0);
  }
}

function setupReviewControls() {
  Object.entries(REVIEW_CHIP).forEach(([type, chipId]) => {
    const btn = document.querySelector(`[data-review="${type}"]`);
    if (!btn) return;
    btn.addEventListener('click', () => {
      const sec  = document.querySelector('.view.active');
      const chip = sec?.querySelector(`[data-chip="${chipId}"]`);
      if (!chip || chip.classList.contains('disabled')) return;
      chip.classList.toggle('active');
      sec._applyFrameClasses?.();
      updateReviewButtons(sec);
    });
  });
}

function setupGlobalControls() {
  document.querySelectorAll('[data-preset]').forEach(btn => {
    btn.addEventListener('click', () => {
      if (btn.disabled || btn.classList.contains('disabled')) return;
      const preset = btn.dataset.preset;
      document.querySelector('.view.active')?._applyPreset?.(preset);
      setActivePreset(preset);
    });
  });


  document.querySelectorAll('.report-switch a[data-view]').forEach(a => {
    a.addEventListener('click', e => {
      e.preventDefault();
      switchToLevel(a.dataset.view);
      window.navPush(a.dataset.view, null);
    });
  });
}

document.addEventListener('DOMContentLoaded', async () => {
  window.nodeSizeMode = 'default';

  // Read the snapshots embedded inline in the page (cs-before / cs-after script tags).
  window.BEFORE = readEmbeddedSnapshot('cs-before');
  window.AFTER  = readEmbeddedSnapshot('cs-after');

  const EMPTY = { graphs: {} };
  window.DIFF   = computeDiff(window.BEFORE ?? EMPTY, window.AFTER ?? window.BEFORE ?? EMPTY);
  window.CYCLES = computeCycles(window.BEFORE ?? EMPTY, window.AFTER ?? window.BEFORE ?? EMPTY);
  window.META   = computeMeta(window.BEFORE, window.AFTER);

  document.querySelectorAll('.view').forEach(setupView);
  document.querySelectorAll('.view').forEach(sec => setupNodeTable(sec, sec.dataset.view));
  const initialPreset = window.AFTER === null ? 'before' : 'diff';
  document.querySelectorAll('.view').forEach(sec => sec._applyPreset?.(initialPreset));
  setupGlobalControls();
  setupReviewControls();
  setupSnapPopup();
  setupFileControls();
  setupTooltip();
  buildSummary();
  updateFilesTab();
  updateHeader();

  document.getElementById('summary-header')?.addEventListener('click', () => {
    document.querySelector('.summary').classList.toggle('collapsed');
  });

  document.getElementById('nav-prompt-btn')?.addEventListener('click', () => {
    openExportPopup(currentLevel());
  });

  const active = document.querySelector('.view.active');
  const loading = active?.querySelector('.loading-indicator');
  if (loading) { loading.textContent = 'Loading Graphviz…'; loading.classList.add('on'); }

  window.gv = await window['@hpcc-js/wasm/graphviz'].Graphviz.load();

  renderView(active);
  setActivePreset(initialPreset);

  // Restore state from URL, then set initial history entry
  const { level: urlLevel, node: urlNode } = getNavParams();
  if (urlLevel && urlLevel !== currentLevel()) switchToLevel(urlLevel);
  if (urlNode) openModalForNode(urlNode, urlLevel ?? currentLevel());
  // Replace initial history state so popstate can restore it
  history.replaceState({ level: currentLevel(), node: urlNode ?? null }, '', location.href);

  window.addEventListener('popstate', e => {
    const st = e.state || getNavParams();
    const lvl  = st.level;
    const nid  = st.node;
    if (lvl && lvl !== currentLevel()) switchToLevel(lvl);
    if (nid) {
      openModalForNode(nid, lvl ?? currentLevel());
    } else {
      closeModalSilent();
    }
  });
});

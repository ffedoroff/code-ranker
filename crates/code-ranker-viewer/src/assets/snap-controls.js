// snap-controls.js — the header chrome: side-toggle wiring, the fly-out header,
// the warning count, updateHeader, the snapshot details/actions popup, and the
// file-upload (snapshot swap) controls. Split out of app.js.

function setupModeToggle() {
  document.getElementById('meta-mode')?.addEventListener('click', toggleViewSide);
  document.addEventListener('keydown', e => {
    if (window.isPromptPopupOpen?.()) return;   // popup open → let keys reach it
    if ((e.key !== 't' && e.key !== 'T') || e.metaKey || e.ctrlKey || e.altKey) return;
    const t = e.target;
    if (t && (/^(input|textarea|select)$/i.test(t.tagName) || t.isContentEditable)) return;
    toggleViewSide();
  });
}

// Relocate the live <header> into a full-screen overlay (the node modal / the
// Prompt-Generator popup) as a slide-down bar, so its controls (toggle, snapshot
// controls, prompt button) stay reachable while the overlay covers the page;
// restored on close. One owner at a time. Fullscreen does its own header
// relocation (panzoom), so this defers while a fullscreen element is active.
window.flyoutHeader = (function () {
  let home = null, homeNext = null, bar = null, owner = null, ownerEl = null;
  return {
    mount(container, key) {
      if (owner || document.fullscreenElement || !container) return;
      const header = document.querySelector('header');
      if (!header) return;
      home = header.parentElement; homeNext = header.nextSibling;
      bar = document.createElement('div');
      // Always visible in these overlays — not a hover-reveal flyout.
      bar.className = 'fs-bar visible';
      bar.append(header);
      container.appendChild(bar);
      // Reserve exactly the bar's height as top space (it may wrap to >1 line, so
      // a fixed value would be wrong) — reading offsetHeight forces a sync layout.
      container.classList.add('fly-header-host');
      container.style.paddingTop = bar.offsetHeight + 'px';
      owner = key; ownerEl = container;
    },
    unmount(key) {
      if (owner !== key) return;
      const header = document.querySelector('header');
      if (home && header) home.insertBefore(header, homeNext);
      bar?.remove();
      ownerEl?.classList.remove('fly-header-host');
      if (ownerEl) ownerEl.style.paddingTop = '';
      bar = null; home = null; owner = null; ownerEl = null;
    }
  };
})();

// Refresh the distinct-warning-type count next to the Prompt-Generator button
// for the active level (it tracks the active side).
function updateWarnCount() {
  const warnEl = document.getElementById('nav-warn-count');
  if (!warnEl) return;
  const n = window.warningTypeCount?.(currentLevel()) ?? 0;
  warnEl.textContent = n ? String(n) : '';
}

function updateHeader() {
  const meta      = window.META;
  const hasBaseline = meta.baseline !== null;
  const hasCurrent  = meta.current  !== null;
  const isReview  = !hasBaseline || !hasCurrent;   // only one snapshot present → "review"

  document.body.classList.toggle('mode-review', isReview);
  // The mode word moved into the meta area, so the title is just the project.
  // Capped to an ellipsis in CSS; the full value stays reachable via `title`.
  const titleEl = document.getElementById('title');
  titleEl.textContent = meta.target;
  titleEl.title = meta.target;
  // Toggle button: shown only in a diff (click or `t` to switch baseline⇄current).
  // In review there is a single snapshot — nothing to toggle — so it is hidden.
  const modeEl = document.getElementById('meta-mode');
  modeEl.style.display = isReview ? 'none' : '';
  modeEl.textContent = 'toggle';
  modeEl.title = 'Click to toggle baseline ⇄ current (press t)';

  // Each snapshot control is shown only when its snapshot exists; the missing
  // side's "Set …" action lives in the surviving control's popup.
  document.querySelector('.snap-group[data-snap="baseline"]').style.display = hasBaseline ? '' : 'none';
  const baselineName = document.getElementById('meta-baseline-name');
  baselineName.textContent = hasBaseline ? meta.baseline.name : '';
  baselineName.title       = hasBaseline ? meta.baseline.name : '';
  document.getElementById('meta-baseline-commit').textContent = hasBaseline && meta.baseline.commit ? ` ${meta.baseline.commit}` : '';

  document.querySelector('.snap-group[data-snap="current"]').style.display = hasCurrent ? '' : 'none';
  const currentName = document.getElementById('meta-current-name');
  currentName.textContent = hasCurrent ? meta.current.name : '';
  currentName.title       = hasCurrent ? meta.current.name : '';
  document.getElementById('meta-current-commit').textContent  = hasCurrent && meta.current.commit ? ` ${meta.current.commit}` : '';

  // Keep the shown side on a snapshot that actually exists.
  if (!hasCurrent)       window.viewSide = 'baseline';
  else if (!hasBaseline) window.viewSide = 'current';
  updateActiveSnapGroup();

}

// Build the per-level views, the level switcher, and the language switcher from
// the loaded snapshot. The `files` view is the static template in index.html;
// every other level (e.g. `functions`) is cloned from it. The language switcher
// appears above the level switcher and is hidden when only one language exists.
// On language change the non-files level sections are torn down and rebuilt for
// the new language (id-namespaced to avoid collisions).
function updateFilesTab() {
  const snap = window.CURRENT ?? window.BASELINE;
  if (!snap) return;

  // Determine the active language; default to the first key.
  const langs = (typeof langKeys === 'function') ? langKeys(snap) : Object.keys(snap.languages || {});
  if (!langs.length) return;
  const activeLang = (typeof currentLang === 'function' && currentLang()) || langs[0];
  if (typeof setLang === 'function') setLang(activeLang);

  const langSnap = (snap.languages || {})[activeLang] || {};
  const levels = Object.keys(langSnap.graphs || {});
  if (!levels.length) levels.push('files');
  // Stable order: files first.
  levels.sort((a, b) => (a === 'files' ? -1 : b === 'files' ? 1 : 0));

  const main = document.querySelector('main');
  const template = document.querySelector('.view[data-view="files"]');
  if (!main || !template) return;

  // Tear down previously cloned non-files level sections (from a prior language
  // or a prior snapshot) so we rebuild them fresh for the active language.
  main.querySelectorAll('.view[data-view]:not([data-view="files"])').forEach(sec => sec.remove());

  // One section per extra level: clone the files template with ids namespaced by
  // both language and level to avoid collisions (lang-level suffix).
  for (const level of levels) {
    if (level === 'files') continue;
    const sec = template.cloneNode(true);
    sec.dataset.view = level;
    sec.classList.remove('active');
    delete sec.dataset.rendered;
    sec.querySelectorAll('.svg-frame').forEach(f => { f.innerHTML = ''; });
    // Namespace ids: `*-files` → `*-<lang>-<level>` (unique across language+level).
    const suffix = `${activeLang}-${level}`;
    sec.querySelectorAll('[id]').forEach(el => { el.id = el.id.replace(/files$/, suffix); });
    main.appendChild(sec);
  }

  // ── Language switcher ────────────────────────────────────────────────────
  const langSw = document.getElementById('lang-switch');
  if (langSw) {
    langSw.innerHTML = '';
    for (const lang of langs) {
      const a = document.createElement('a');
      a.href = '#';
      a.dataset.lang = lang;
      a.textContent = lang.charAt(0).toUpperCase() + lang.slice(1);
      if (lang === activeLang) a.classList.add('selected');
      a.addEventListener('click', e => {
        e.preventDefault();
        if (lang === (typeof currentLang === 'function' ? currentLang() : null)) return;
        if (typeof switchToLang === 'function') switchToLang(lang);
      });
      langSw.appendChild(a);
    }
    langSw.hidden = langs.length < 2;
  }

  // ── Level switcher ───────────────────────────────────────────────────────
  const sw = document.getElementById('report-switch');
  if (!sw) return;
  sw.innerHTML = '';
  const cur = currentLevel() ?? 'files';
  for (const level of levels) {
    const a = document.createElement('a');
    a.href = '#';
    a.dataset.view = level;
    a.textContent = level.charAt(0).toUpperCase() + level.slice(1);
    if (level === cur) a.classList.add('selected');
    a.addEventListener('click', e => {
      e.preventDefault();
      if (level === currentLevel()) return;
      switchToLevel(level);
      try { const st = navViewState(); history.pushState(st, '', navViewUrl(st)); } catch (_) {}
    });
    sw.appendChild(a);
  }
  sw.hidden = levels.length < 2;
}


function buildSnapPopupHTML(snap, refSnap, sideLabel) {
  if (!snap) return '';
  const sections = [];

  // Which snapshot this is (Baseline / Current) and whether it is the one currently
  // shown on the map / tables.
  if (sideLabel) {
    const shown = sideLabel.toLowerCase() === window.viewSide;
    sections.push(`<div class="sp-side ${shown ? 'sp-side-shown' : ''}">${sideLabel}${shown ? ' · currently shown' : ''}</div>`);
  }

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
  if (snap.git && Object.keys(snap.git).length) {
    const gitRows = Object.entries(snap.git).map(([k, v]) => {
      const cls = k === 'origin' ? ' class="sp-origin"' : '';
      return `<div class="sp-row"><span class="sp-lbl">${escHtml(k)}</span><span${cls}>${escHtml(String(v ?? ''))}</span></div>`;
    }).join('');
    sections.push(`<div class="sp-section-label">Git</div>${gitRows}`);
  }

  // Versions
  if (snap.versions && Object.keys(snap.versions).length) {
    const vrows = Object.entries(snap.versions).map(([k, v]) =>
      `<div class="sp-row"><span class="sp-lbl">${escHtml(k)}</span><span>${escHtml(v)}</span></div>`
    ).join('');
    sections.push(`<div class="sp-section-label">Versions</div>${vrows}`);
  }

  // Duration
  if (snap.timings?.length) {
    const trows = snap.timings.map(t =>
      `<div class="sp-row"><span class="sp-lbl">${escHtml(t.stage)}</span><span>${fmtMs(t.ms)}</span></div>`
    ).join('');
    sections.push(`<div class="sp-section-label">Duration</div>${trows}`);
  }

  // Actions — moved out of the header. Wired in `show()`. The baseline control
  // offers replace + remove; the current control offers replace, plus "Set
  // baseline" in review (where there is no separate baseline control).
  // Symmetric actions: each side can be replaced; it can be removed only while the
  // OTHER side exists (so at least one snapshot always remains); and the missing
  // side can be set from here.
  const acts = [];
  if (sideLabel === 'Baseline') {
    acts.push('<button class="sp-action" data-act="upload-baseline">↑ Replace baseline</button>');
    if (window.CURRENT)   acts.push('<button class="sp-action sp-action-x" data-act="remove-baseline">✕ Remove baseline</button>');
    if (!window.CURRENT)  acts.push('<button class="sp-action" data-act="upload-current">↑ Set current</button>');
  } else if (sideLabel === 'Current') {
    acts.push('<button class="sp-action" data-act="upload-current">↑ Replace current</button>');
    if (window.BASELINE)  acts.push('<button class="sp-action sp-action-x" data-act="remove-current">✕ Remove current</button>');
    if (!window.BASELINE) acts.push('<button class="sp-action" data-act="upload-baseline">↑ Set baseline</button>');
  }
  if (acts.length)
    sections.push(`<div class="sp-section-label">Actions</div><div class="sp-actions">${acts.join('')}</div>`);

  return sections.join('');
}

function setupSnapPopup() {
  let popup = null, openFor = null;   // openFor = the .snap-group whose popup is shown

  function getPopup() {
    if (!popup) {
      popup = document.createElement('div');
      popup.id = 'snap-popup';
      // Fullscreen shows only the frame; attach there so the header's popup is visible.
      (document.fullscreenElement || document.body).appendChild(popup);
    }
    return popup;
  }

  function hide() { if (popup) popup.style.display = 'none'; openFor = null; }

  function show(snap, anchor, refSnap, sideLabel) {
    if (!snap) return;
    const p = getPopup();
    p.innerHTML = buildSnapPopupHTML(snap, refSnap, sideLabel);
    p.querySelectorAll('.sp-copy-btn').forEach(btn => {
      btn.addEventListener('click', e => {
        e.stopPropagation();
        navigator.clipboard?.writeText(btn.dataset.copy).then(() => {
          btn.textContent = '✓';
          setTimeout(() => { btn.textContent = '⧉'; }, 1500);
        });
      });
    });
    // Upload / remove actions (moved here from the header).
    p.querySelectorAll('.sp-action').forEach(btn => {
      btn.addEventListener('click', e => {
        e.stopPropagation();
        const act = btn.dataset.act;
        if (act === 'upload-baseline')      document.getElementById('input-baseline').click();
        else if (act === 'upload-current')  document.getElementById('input-current').click();
        else if (act === 'remove-baseline') { window.BASELINE = null; recomputeAll(); }
        else if (act === 'remove-current')  { window.CURRENT = null;  recomputeAll(); }
        hide();
      });
    });
    p.style.display = 'block';
    openFor = anchor;
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
    // Click the control body → switch which side is shown on the map.
    grp.addEventListener('click', () => setViewSide(i === 0 ? 'baseline' : 'current'));
    // Click the pencil → open/close the details + actions popup (no hover trigger).
    // `stopPropagation` so it does not also switch the side or count as an outside click.
    grp.querySelector('.snap-edit')?.addEventListener('click', e => {
      e.stopPropagation();
      if (openFor === grp) { hide(); return; }
      const snap = i === 0 ? window.BASELINE : window.CURRENT;
      const ref  = i === 1 ? window.BASELINE : null;
      show(snap, grp, ref, i === 0 ? 'Baseline' : 'Current');
    });
  });

  // Dismiss on an outside click or Escape.
  document.addEventListener('click', e => {
    if (openFor && popup && !popup.contains(e.target) && !openFor.contains(e.target)) hide();
  });
  document.addEventListener('keydown', e => { if (e.key === 'Escape') hide(); });
}

// Mark the header snapshot group (the file uploader at the top) for the side
// currently shown, so the active input is visually distinguished. Hidden in
// review mode (a single snapshot — nothing to distinguish).
function updateActiveSnapGroup() {
  const active = window.CURRENT ? window.viewSide : null;
  document.querySelectorAll('.snap-group[data-snap]').forEach(grp => {
    grp.classList.toggle('snap-active', grp.dataset.snap === active);
  });
}

// Read a snapshot embedded inline in this page (a `<script type="application/json">` tag).
function readEmbeddedSnapshot(id) {
  const el = document.getElementById(id);
  if (!el) return null;
  const t = el.textContent.trim();
  return t && t !== 'null' ? JSON.parse(t) : null;
}

// Parse a snapshot from uploaded file text — either a raw JSON snapshot, or a code-ranker
// HTML report with the snapshot embedded (prefer `cs-current`, else `cs-baseline`).
// Rejects a snapshot whose format version this viewer can't read (single
// `window.SCHEMA_VERSION`, injected by the renderer from the build's SCHEMA_VERSION).
function extractSnapshotFromText(text) {
  const s = text.trim();
  let snap;
  if (s.startsWith('{')) {
    snap = JSON.parse(s);
  } else {
    const doc = new DOMParser().parseFromString(text, 'text/html');
    const read = id => {
      const t = doc.getElementById(id)?.textContent?.trim();
      return t && t !== 'null' ? JSON.parse(t) : null;
    };
    snap = read('cs-current') || read('cs-baseline');
  }
  const want = window.SCHEMA_VERSION;
  if (snap && want && snap.schema_version !== want) {
    throw new Error(
      `snapshot schema_version "${snap.schema_version}" ≠ this viewer's "${want}" — ` +
      `regenerate the report with a matching code-ranker version`);
  }
  return snap;
}

// Manual snapshot swap: load a different baseline/current snapshot (.json or .html) into the viewer.
function setupFileControls() {
  const inputBaseline = document.getElementById('input-baseline');
  const inputCurrent  = document.getElementById('input-current');

  // The upload / remove buttons now live in the snapshot popup (see `show`),
  // which clicks these hidden inputs. Here we only handle the chosen file.

  inputBaseline.addEventListener('change', () => {
    const file = inputBaseline.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = e => {
      try { window.BASELINE = extractSnapshotFromText(e.target.result); } catch (err) { alert(err.message || 'Invalid snapshot file'); return; }
      recomputeAll();
    };
    reader.readAsText(file);
    inputBaseline.value = '';
  });

  inputCurrent.addEventListener('change', () => {
    const file = inputCurrent.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = e => {
      try { window.CURRENT = extractSnapshotFromText(e.target.result); } catch (err) { alert(err.message || 'Invalid snapshot file'); return; }
      recomputeAll();
    };
    reader.readAsText(file);
    inputCurrent.value = '';
  });
}


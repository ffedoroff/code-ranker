function getNavParams() {
  const p = new URLSearchParams(location.search);
  return {
    lang:  p.get('lang'),
    level: p.get('level'),
    node:  p.get('node'),
    side:  p.get('side'),
    group: p.get('group'),
    mode:  p.get('mode'),
    depth: p.get('depth'),
    tier:  p.get('tier'),
    stat:  p.get('stat'),
    panel: p.get('panel'),
  };
}
// The active diff side carried in the URL — only in diff mode (a current snapshot
// exists); review mode has a single view and omits the param.
function navSide() {
  return window.CURRENT && window.viewSide ? window.viewSide : null;
}

// Summary aggregation stat carried in the URL — only when non-default (avg is the
// implicit default, omitted to keep the URL clean).
function navStat() {
  const s = window._summaryStat || 'avg';
  return s !== 'avg' ? s : null;
}

// Reveal depth carried in the URL: the lens offset from the context default —
// while focused, how far the focus is collapsed (−focusDig, 0 = files, the drill
// landing); at the overview, the grouping LOD (window.dig, 0 = crates). Always 0
// at the default so it is omitted from the URL.
function navDepth() {
  const lvl = currentLevel();
  if (window.drillGroup != null) return (window.focusDig || 0) - (window.focusMinFz?.(lvl) ?? 0);
  return (window.dig || 0) - (window.overviewBaseDig?.(lvl) ?? 0);
}

function navViewState() {
  const snap = window.CURRENT ?? window.BASELINE;
  const langs = (typeof langKeys === 'function' && snap) ? langKeys(snap) : [];
  // Omit `lang` when there is only one language — keeps URLs clean for existing
  // single-language reports.
  const langVal = langs.length > 1 ? ((typeof currentLang === 'function' ? currentLang() : null) || langs[0]) : null;
  return {
    lang:  langVal,
    level: currentLevel() ?? null,
    side:  navSide(),
    group: window.drillGroup  || null,
    mode:  window.nodeSizeMode || null,
    depth: navDepth(),
    tier:  window.tier || null,
    stat:  navStat(),
    panel: window._statsOpen ? 'stats' : null,
  };
}
function navViewUrl(st) {
  const p = new URLSearchParams();
  if (st.lang)  p.set('lang',  st.lang);
  if (st.level) p.set('level', st.level);
  if (st.side)  p.set('side',  st.side);
  if (st.group) p.set('group', st.group);
  if (st.mode)  p.set('mode',  st.mode);
  if (st.depth) p.set('depth', st.depth);
  if (st.tier)  p.set('tier',  st.tier);
  if (st.stat)  p.set('stat',  st.stat);
  if (st.panel) p.set('panel', st.panel);
  return p.toString() ? '?' + p : location.pathname;
}
// Drill navigation (in/out of a group) — adds a history entry so Back works.
window.navPushView = function() {
  const st = navViewState();
  history.pushState(st, '', navViewUrl(st));
};
// Mode/side change — updates URL in-place (no new history entry, but refresh restores).
window.navReplaceView = function() {
  const st = navViewState();
  history.replaceState(st, '', navViewUrl(st));
};

// Open a node modal: push level + group + mode + node to history.
window.navPush = function(level, nodeId) {
  const p = new URLSearchParams();
  if (level)  p.set('level', level);
  const side = navSide();
  if (side)   p.set('side',  side);
  const grp = window.drillGroup || null;
  if (grp)    p.set('group', grp);
  const mode = window.nodeSizeMode || null;
  if (mode)   p.set('mode',  mode);
  const depth = navDepth();
  if (depth)  p.set('depth', depth);
  const tier = window.tier || null;
  if (tier)   p.set('tier',  tier);
  const stat = navStat();
  if (stat)   p.set('stat',  stat);
  const panel = window._statsOpen ? 'stats' : null;
  if (panel)  p.set('panel', panel);
  if (nodeId) p.set('node',  nodeId);
  const url = p.toString() ? '?' + p : location.pathname;
  history.pushState({ level: level ?? null, node: nodeId ?? null, side, group: grp, mode, depth, tier, stat, panel }, '', url);
};
// Update only the `side` param in place (Baseline/Current toggle).
window.navSetSide = function() {
  const st = { ...(history.state || {}), ...navViewState() };
  history.replaceState(st, '', navViewUrl(st));
};
function currentLevel() {
  return document.querySelector('.view.active')?.dataset.view ?? null;
}
function switchToLevel(target) {
  document.querySelectorAll('.view').forEach(v => v.classList.toggle('active', v.dataset.view === target));
  document.querySelectorAll('.report-switch a').forEach(l => l.classList.toggle('selected', l.dataset.view === target));
  const sec = document.querySelector('.view.active');
  if (sec && sec.dataset.rendered !== 'true' && window.gv) renderView(sec);
}
// Switch the active language and rebuild the level sections + switchers for it.
// Persists `lang` in the URL; uses replaceState (no extra history entry).
function switchToLang(target) {
  if (typeof setLang === 'function') setLang(target);
  // Highlight the selected tab in the language switcher.
  document.querySelectorAll('#lang-switch a').forEach(a => a.classList.toggle('selected', a.dataset.lang === target));
  // Rebuild level sections / diff / cycles for the new language.
  if (typeof updateFilesTab === 'function') updateFilesTab();
  if (typeof recomputeAll  === 'function') recomputeAll();
  // Persist language in the URL.
  const st = navViewState();
  history.replaceState(st, '', navViewUrl(st));
}
function openModalForNode(nodeId, level) {
  // Is the node on the side currently shown? (vs. only in the union/DIFF)
  const onSide   = activeGraph(level).nodes.find(n => n.id === nodeId);
  // DIFF is keyed [lang][level]; resolve the active language.
  const _nLang   = (typeof currentLang === 'function' ? currentLang() : null) || Object.keys(window.DIFF || {})[0];
  const nodeData = onSide ?? window.DIFF?.[_nLang]?.[level]?.nodes?.find(n => n.id === nodeId);
  if (!nodeData) return false;
  // Remember which node the modal shows so a baseline⇄current toggle can re-render it.
  // On a FRESH open (the overlay is not already visible) also remember it as the
  // open anchor: closeModal compares it against the node shown at close time to
  // decide whether the user navigated to a different file (→ land the map in that
  // file's folder) or stayed on the same one (→ leave the map untouched).
  const _wasOpen = document.getElementById('node-modal-overlay')?.style.display === 'flex';
  window._modalNode = { id: nodeId, level };
  if (!_wasOpen) window._modalOpenId = nodeId;
  // Clear any tooltip anchored to the element we're about to replace.
  window.hideMetricTooltip?.();
  const section = document.querySelector(`.view[data-view="${level}"]`);
  const overlay = getModal();
  if (onSide) {
    const mc = buildModalContent(nodeData, level);
    document.getElementById('node-modal-hdr-title').innerHTML = mc.hdr;
    document.getElementById('node-modal-body').innerHTML = mc.body;
    window.setModalDiagram(mc.diagram);
    attachModalCheckbox(nodeData, level, section);
  } else {
    // The node does not exist on the side now shown (a removed node viewed as
    // current, or an added node viewed as baseline). Don't render its card or
    // its (stale, other-side) values — just say it isn't here.
    const side = viewModeSuffix().trim();   // 'Baseline' / 'Current' (diff mode only)
    document.getElementById('node-modal-hdr-title').innerHTML =
      `<span class="nm-title">${escHtml(nodeData.name || nodeId)}</span>`;
    document.getElementById('node-modal-body').innerHTML =
      `<div class="nm-absent">Not present in the ${escHtml(side.toLowerCase())} snapshot.</div>`;
    window.setModalDiagram('');
  }
  overlay.style.display = 'flex';
  document.body.style.overflow = 'hidden';
  window.flyoutHeader?.mount(overlay, 'modal');
  return true;
}

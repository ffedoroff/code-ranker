function getNavParams() {
  const p = new URLSearchParams(location.search);
  return {
    level: p.get('level'),
    node:  p.get('node'),
    side:  p.get('side'),
    group: p.get('group'),
    mode:  p.get('mode'),
    dig:   p.get('dig'),
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

function navViewState() {
  return {
    level: currentLevel() ?? null,
    side:  navSide(),
    group: window.drillGroup  || null,
    mode:  window.nodeSizeMode || null,
    dig:   window.dig || 0,
    stat:  navStat(),
    panel: window._statsOpen ? 'stats' : null,
  };
}
function navViewUrl(st) {
  const p = new URLSearchParams();
  if (st.level) p.set('level', st.level);
  if (st.side)  p.set('side',  st.side);
  if (st.group) p.set('group', st.group);
  if (st.mode)  p.set('mode',  st.mode);
  if (st.dig)   p.set('dig',   st.dig);
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
  const dig = window.dig || 0;
  if (dig)    p.set('dig',   dig);
  const stat = navStat();
  if (stat)   p.set('stat',  stat);
  const panel = window._statsOpen ? 'stats' : null;
  if (panel)  p.set('panel', panel);
  if (nodeId) p.set('node',  nodeId);
  const url = p.toString() ? '?' + p : location.pathname;
  history.pushState({ level: level ?? null, node: nodeId ?? null, side, group: grp, mode, dig, stat, panel }, '', url);
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
function openModalForNode(nodeId, level) {
  // Is the node on the side currently shown? (vs. only in the union/DIFF)
  const onSide   = activeGraph(level).nodes.find(n => n.id === nodeId);
  const nodeData = onSide ?? window.DIFF?.[level]?.nodes?.find(n => n.id === nodeId);
  if (!nodeData) return false;
  // Remember which node the modal shows so a baseline⇄current toggle can re-render it.
  window._modalNode = { id: nodeId, level };
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

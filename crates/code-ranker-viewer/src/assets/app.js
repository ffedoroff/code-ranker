document.addEventListener('DOMContentLoaded', async () => {
  window.nodeSizeMode = null;
  window.drillGroup   = null;
  window.dig         = 0;   // relative LOD on the overview (see grouping.js)
  window.drillDig    = 0;
  window.cycleOnly   = false;   // cycle filter (show only nodes in cycles)

  // Read the snapshots embedded inline in the page (cs-baseline / cs-current script tags).
  window.BASELINE = readEmbeddedSnapshot('cs-baseline');
  window.CURRENT  = readEmbeddedSnapshot('cs-current');

  const EMPTY = { graphs: {} };
  window.DIFF   = computeDiff(window.BASELINE ?? window.CURRENT ?? EMPTY, window.CURRENT ?? window.BASELINE ?? EMPTY);
  window.CYCLES = computeCycles(window.BASELINE ?? window.CURRENT ?? EMPTY, window.CURRENT ?? window.BASELINE ?? EMPTY);
  window.META   = computeMeta(window.BASELINE, window.CURRENT);

  // Restore the active side from the URL (`side=baseline/current`); default to the
  // current (primary) snapshot. `baseline` is only honoured when a baseline exists.
  const urlSide = getNavParams().side;
  window.viewSide = (urlSide === 'baseline' && window.BASELINE) ? 'baseline'
                  : window.CURRENT ? 'current'
                  : 'baseline';
  // Restore the summary aggregation from the URL (`stat=`) before the first render
  // so the table and the radio come up on the chosen stat.
  const urlStat = getNavParams().stat;
  if (urlStat && window.isSummaryStat?.(urlStat)) window._summaryStat = urlStat;
  // Was the Statistics popup open? (`panel=stats`) — reopen it after setup.
  window._statsOpen = getNavParams().panel === 'stats';
  // If the Prompt Generator was open (state in the URL), restore its selected
  // nodes before the tables render so those rows come up already selected.
  const epState = (typeof epReadUrl === 'function') ? epReadUrl() : null;
  if (epState?.sel?.length) {
    if (!window._ntSelected) window._ntSelected = {};
    window._ntSelected[epState.level] = new Set(epState.sel);
  }
  document.querySelectorAll('.view').forEach(sec => setupNodeTable(sec, sec.dataset.view));
  setupSnapPopup();
  setupModeToggle();
  setupFileControls();
  setupTooltip();
  buildSummary();
  setupSummaryStatControl();
  setupSummaryPopup();
  updateFilesTab();
  updateHeader();
  if (window._statsOpen) openSummaryPopup(false);   // restore the popup from the URL


  const active = document.querySelector('.view.active');
  const loading = active?.querySelector('.loading-indicator');
  if (loading) { loading.textContent = 'Loading Graphviz…'; loading.classList.add('on'); }

  window.gv = await window['@hpcc-js/wasm/graphviz'].Graphviz.load();

  renderView(active);


  // Restore state from URL, then set initial history entry
  const { level: urlLevel, node: urlNode, group: urlGroup, mode: urlMode, dig: urlDig, stat: urlStat2 } = getNavParams();
  if (urlLevel && urlLevel !== currentLevel()) switchToLevel(urlLevel);
  applyViewState({ level: urlLevel, group: urlGroup, mode: urlMode, dig: urlDig, stat: urlStat2 }, { rerender: !!(urlGroup || urlMode || urlDig) });
  if (urlNode) openModalForNode(urlNode, urlLevel ?? currentLevel());
  // Replace initial history state so popstate can restore it
  history.replaceState(
    { level: currentLevel(), node: urlNode ?? null, group: urlGroup ?? null, mode: urlMode ?? null, dig: window.dig || 0, side: window.viewSide, stat: window.navStat?.() ?? null, panel: window._statsOpen ? 'stats' : null },
    '', location.href
  );

  // Re-open the Prompt Generator if the URL says it was open.
  if (epState) {
    if (epState.level && epState.level !== currentLevel()) switchToLevel(epState.level);
    openExportPopup(epState.level, epState);
  }

  window.addEventListener('popstate', e => {
    const st   = e.state || getNavParams();
    const lvl  = st.level;
    const nid  = st.node;
    const side = st.side;
    if (window.CURRENT && (side === 'baseline' || side === 'current')) setViewSide(side);
    if (lvl && lvl !== currentLevel()) switchToLevel(lvl);
    applyViewState({ level: lvl ?? currentLevel(), group: st.group, mode: st.mode, dig: st.dig, stat: st.stat }, { rerender: true });
    if (nid) {
      openModalForNode(nid, lvl ?? currentLevel());
    } else {
      closeModalSilent();
    }
    // Restore the Statistics popup open/closed state without re-touching the URL.
    if (st.panel === 'stats') openSummaryPopup(false); else closeSummaryPopup(false);
  });
});

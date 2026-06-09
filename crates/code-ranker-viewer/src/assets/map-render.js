// map-render.js — the main-map render entry points: drawSVG (big-graph guard)
// and renderSVGNow (DOT -> SVG via graphviz, then wire pan/zoom + interactions).
// Split out of diagram.js.

// Above this many nodes, laying out the graph with graphviz is slow, so we ask
// for explicit confirmation before rendering (once per frame).
const SVG_NODE_LIMIT = 500;

function drawSVG(svgFrame, nodes, edges, level) {
  const drillGroup = window.drillGroup || null;

  // Group view (drillGroup=null) is always fast — just one node per group.
  // Only warn when drilled into a very large group.
  if (drillGroup !== null) {
    const gOf = grouperForDig(level, window.drillDig ?? 0);
    const drillCount = nodes.filter(n => gOf(n) === drillGroup).length;
    if (drillCount > SVG_NODE_LIMIT && svgFrame.dataset.bigConfirmed !== '1') {
      svgFrame.innerHTML =
        `<div class="too-many">` +
          `<div class="too-many-title">too many nodes: ${drillCount}</div>` +
          `<div class="too-many-sub">Rendering the full diagram may be slow. Render it anyway?</div>` +
          `<button class="too-many-btn" type="button">Render diagram</button>` +
        `</div>`;
      svgFrame.querySelector('.too-many-btn').addEventListener('click', () => {
        svgFrame.dataset.bigConfirmed = '1';
        const loading = svgFrame.closest('[data-view]')?.querySelector('.loading-indicator');
        if (loading) { loading.textContent = 'Computing layout…'; loading.classList.add('on'); }
        setTimeout(() => {
          renderSVGNow(svgFrame, nodes, edges, level);
          if (loading) loading.classList.remove('on');
        }, 30);
      });
      return;
    }
  }
  renderSVGNow(svgFrame, nodes, edges, level);
}

function renderSVGNow(svgFrame, nodes, edges, level) {
  const vpW = svgFrame.offsetWidth  || svgFrame.clientWidth  || 0;
  const vpH = svgFrame.offsetHeight || svgFrame.clientHeight || 0;
  const viewport = (vpW > 0 && vpH > 0) ? { w: vpW, h: vpH } : null;
  const dot = buildDOT(nodes, edges, level, viewport);
  const svgStr = window.gv.dot(dot);
  svgFrame.innerHTML = svgStr;
  const svg = svgFrame.querySelector('svg');
  if (svg) {
    svg.setAttribute('width', '100%');
    svg.setAttribute('height', '100%');
    svg.style.display = 'block';
    setupPanZoom(svgFrame, svg);
    // Status bar: one persistent element per frame-wrap, reused across re-renders.
    const fw = svgFrame.parentElement;
    let statusBar = fw.querySelector(':scope > .svg-status-bar');
    if (!statusBar) {
      statusBar = document.createElement('div');
      statusBar.className = 'svg-status-bar';
      fw.appendChild(statusBar);
    }
    statusBar.hidden = true;
    statusBar.textContent = '';
    svgFrame._statusBar = statusBar;
    setupEdgeHighlight(svgFrame, level);   // reads titles before setupTooltips removes them
    setupTooltips(svgFrame, level);
  }
}


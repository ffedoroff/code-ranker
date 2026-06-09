// map-interactions.js — all behaviour on the main SVG map: node selection, the
// platform open-source modifier, the shortcut legend, drill + relative-zoom
// navigation, the status bar, edge highlighting and tooltips/handlers. Split out
// of diagram.js. setupEdgeHighlight must run BEFORE setupTooltips (it reads SVG
// <title> elements that setupTooltips then removes).

// keep the shared selectedIds Set, the SVG highlight, the table row + checkbox,
// the popup-diagram cards, and the "N selected" footer all in sync.
function toggleNodeSelected(node, level, section) {
  if (!window._ntSelected) window._ntSelected = {};
  if (!window._ntSelected[level]) window._ntSelected[level] = new Set();
  const selectedIds = window._ntSelected[level];

  const sel = !selectedIds.has(node.id);
  if (sel) selectedIds.add(node.id); else selectedIds.delete(node.id);

  section?._gNodeMap?.get(node.id)?.classList.toggle('node-selected', sel);

  const row = section?.querySelector(
    `.node-table-body .node-table tbody tr[data-node-id="${CSS.escape(node.id)}"]`);
  if (row) {
    row.classList.toggle('row-selected', sel);
    const cb = row.querySelector('.nt-cb');
    if (cb) cb.checked = sel;
  }
  markPopupSelected(node.id, sel);
  section?._updateAllCb?.();
}

// The "open source" modifier is platform-specific: ⌘ (Meta) on macOS — where
// Ctrl is deliberately left alone (it maps to right-click) — and Ctrl elsewhere.
const IS_MAC = /Mac|iP(hone|ad|od)/.test(
  (typeof navigator !== 'undefined' && (navigator.platform || navigator.userAgent)) || ''
);
const OPEN_SRC_KEY = IS_MAC ? 'Meta' : 'Control';
const isOpenSrcClick = e => (IS_MAC ? e.metaKey : e.ctrlKey);
// Exposed on window so modal.js (the popup diagram) can mirror the gesture —
// `const` declarations are not auto-attached to the global object.
window.isOpenSrcClick = isOpenSrcClick;

// Shortcut-legend markup with the platform's actual keys; reused by the main map
// (`#kbd-hints`) and the popup (`#node-modal-hints`, filled in modal.js).
function kbdHintsHtml() {
  const srcKey = IS_MAC ? '⌘' : 'Ctrl';
  return `<span class="kbd-hint"><kbd>⇧ Shift</kbd> + click — select node</span>` +
         `<span class="kbd-hint"><kbd>${srcKey}</kbd> + click — view source</span>` +
         `<span class="kbd-hint kbd-hint-toggle"><kbd>t</kbd> — toggle baseline/current</span>`;
}
window.kbdHintsHtml = kbdHintsHtml;

// Map modifier modes, each changing the cursor (see the CSS) and rerouting node
// clicks (see the click handler in setupTooltips):
//   • Shift (`.shift-select`)      — toggle a node's selection instead of the modal;
//   • ⌘ (mac) / Ctrl (`.ctrl-link`) — open the node's source on the git host.
(function initMapModifiers() {
  const setShift = on => document.body.classList.toggle('shift-select', on);
  const setSrc   = on => document.body.classList.toggle('ctrl-link', on);

  // Fill the bottom-left shortcut legend with the platform's actual keys.
  const hints = document.getElementById('kbd-hints');
  if (hints) hints.innerHTML = kbdHintsHtml();
  window.addEventListener('keydown', e => {
    if (window.isPromptPopupOpen?.()) return;   // popup open → don't grab Ctrl/Shift
    if (e.key === 'Shift') setShift(true);
    if (e.key === OPEN_SRC_KEY) setSrc(true);
  });
  window.addEventListener('keyup', e => {
    if (e.key === 'Shift') setShift(false);
    if (e.key === OPEN_SRC_KEY) setSrc(false);
  });
  window.addEventListener('blur', () => { setShift(false); setSrc(false); });
})();

function drillIntoGroup(groupId, level) {
  window.drillGroup = groupId;
  // The drilled view filters by the grouper that produced this group key, so
  // remember the zoom that was active at drill time.
  window.drillDig  = window.dig || 0;
  const frameWrap = document.querySelector(`.view[data-view="${level}"] .frame-wrap`);
  const bc = frameWrap?.querySelector('.drill-breadcrumb');
  if (bc) {
    bc.style.display = '';
    const grpKey = levelUi(level).grouping?.key || 'group';
    bc.querySelector('.drill-group-name').textContent = `${grpKey}: ${groupId}`;
  }
  // The relative-zoom control applies to the overview only — hide it while focused.
  frameWrap?.querySelector('.dig-lod')?.style.setProperty('display', 'none');
  window.navPushView?.();
  document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
  const active = document.querySelector('.view.active');
  if (active && window.gv) renderView(active, { preserve: false });
}

function drillOutOfGroup(level) {
  window.drillGroup = null;
  const frameWrap = document.querySelector(`.view[data-view="${level}"] .frame-wrap`);
  const bc = frameWrap?.querySelector('.drill-breadcrumb');
  if (bc) bc.style.display = 'none';
  frameWrap?.querySelector('.dig-lod')?.style.removeProperty('display');
  updateDigLabel(level);
  window.navPushView?.();
  document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
  const active = document.querySelector('.view.active');
  if (active && window.gv) renderView(active, { preserve: false });
}

// Relative "dig" (level-of-detail) control for the overview. `delta` is +1 (dig
// IN — descend into folders) or -1 (dig OUT — collapse the deepest crates into
// folders). Dig acts on the whole overview; if currently focused into a group,
// stepping dig leaves focus so the new LOD is visible across the map. See
// grouping.js for the tier ladder.
function setDig(delta, level) {
  level = level || currentLevel();
  const z = clampDig((window.dig || 0) + delta);
  if (z === (window.dig || 0) && window.drillGroup === null) return;
  window.dig = z;
  if (window.drillGroup !== null) {
    window.drillGroup = null;
    document.querySelectorAll('.drill-breadcrumb').forEach(bc => { bc.style.display = 'none'; });
    document.querySelectorAll('.dig-lod').forEach(el => el.style.removeProperty('display'));
  }
  updateDigLabel(level);
  window.navReplaceView?.();
  document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
  const active = document.querySelector('.view.active');
  if (active && window.gv) renderView(active, { preserve: false });
}
window.setDig = setDig;

// Sync the dig-control label + button disabled-state for a level. dig 0 shows the
// grouping key (e.g. "crate"); otherwise shows the signed level as "crate folder
// ±N". Under each button sits the count of group boxes that pressing it would
// show. "Out" is disabled once the overview has collapsed to a single root group
// (dig reaches -maxCrateDepth); "in" at the static DIG_MAX.
function updateDigLabel(level) {
  level = level || currentLevel();
  const root = document.querySelector(`.view[data-view="${level}"] .dig-lod`);
  if (!root) return;
  const z   = window.dig || 0;
  const gk  = levelUi(level).grouping?.key || 'group';
  const val = root.querySelector('.dig-lod-val');
  if (val) val.textContent = z === 0 ? gk : `/crate/folder${z > 0 ? '+' : ''}${z}`;
  // Group-box counts: current level under the label, and what one step out / in
  // would render under the − / + buttons.
  const curN = window.groupCountAtDig?.(level, z);
  const outN = window.groupCountAtDig?.(level, z - 1);
  const inN  = window.groupCountAtDig?.(level, z + 1);
  const maxD = window.maxCrateDepth?.(level) ?? 0;
  // "Out" runs all the way to a single _root group. "In" also stops once digging
  // deeper no longer splits anything (next-level count == current) — every file
  // already sits at its deepest folder.
  root.querySelector('[data-lod="out"]')?.toggleAttribute('disabled', z <= -maxD || z <= DIG_MIN);
  root.querySelector('[data-lod="in"]') ?.toggleAttribute('disabled',
    z >= DIG_MAX || (inN != null && curN != null && inN === curN));
  const curC = root.querySelector('[data-count="cur"]');
  const outC = root.querySelector('[data-count="out"]');
  const inC  = root.querySelector('[data-count="in"]');
  if (curC) curC.textContent = curN != null ? String(curN) : '';
  if (outC) outC.textContent = outN != null ? String(outN) : '';
  if (inC)  inC.textContent  = inN  != null ? String(inN)  : '';
}
window.updateDigLabel = updateDigLabel;

// Format a single status-bar line for a file node.
function statusLineFor(node, level) {
  const parts = [];
  const name = node.name || node.id.split('/').pop() || node.id;
  parts.push(name);
  const path = (node.path || node.id || '').replace(/^\{[^}]+\}\//, '');
  if (path && path !== name) parts.push(path);
  const gk = levelUi(level)?.grouping?.key;
  if (gk) {
    const gv = nodeAttr(node, gk);
    if (gv != null && gv !== '') parts.push(`${gk}: ${gv}`);
  }
  const hkV = nodeAttr(node, 'hk') ?? node.hk;
  if (hkV != null) parts.push(`hk: ${fmtMetricShort(Number(hkV))}`);
  const slocV = nodeAttr(node, 'sloc') ?? nodeAttr(node, 'loc') ?? node.sloc ?? node.loc;
  if (slocV != null) parts.push(`sloc: ${fmtMetricShort(Number(slocV))}`);
  if (node.fan_in  != null) parts.push(`fan-in: ${node.fan_in}`);
  if (node.fan_out != null) parts.push(`fan-out: ${node.fan_out}`);
  return parts.join('  ·  ');
}

// Aggregate per-group stats (files/sloc/hk/cycle) keyed by a grouper closure —
// the figures the status bar shows for a crate/group box, and for the external
// caller/dependency neighbour boxes in the drilled view.
function computeGroupStats(level, grouper) {
  const cyc = window.CYCLES?.[level]?.nodeCycleStatus;
  const stats = new Map();
  for (const n of unionGraph(level).nodes) {
    const grp = grouper(n);
    let s = stats.get(grp);
    if (!s) { s = { name: grp, files: 0, folders: 0, sloc: 0, hk: 0, cycle: 0, _common: null, _dirs: new Set() }; stats.set(grp, s); }
    s.files++;
    s.sloc += Number(n.sloc ?? n.loc ?? 0);
    s.hk   += Number(n.hk ?? 0);
    const cs = cyc?.get(n.id);
    if (cs && cs !== 'none') s.cycle++;
    // Track the members' directories → the group's distinct-folder count and the
    // common directory (its full path).
    const dir = relPathOf(n.id).split('/').slice(0, -1);
    s._dirs.add(dir.join('/'));
    if (s._common === null) s._common = dir.slice();
    else { let i = 0; while (i < s._common.length && i < dir.length && s._common[i] === dir[i]) i++; s._common.length = i; }
  }
  for (const s of stats.values()) {
    s.path = s._common && s._common.length ? '/' + s._common.join('/') : '/';
    s.folders = s._dirs.size;
    delete s._common; delete s._dirs;
  }
  return stats;
}

// Format a single status-bar line for a group node.
function statusLineForGroup(stats) {
  const parts = [stats.name];
  // Full directory path of the group, unless it just repeats the name.
  const norm = s => String(s).replace(/^[←→]\s*/, '').replace(/^\//, '');
  if (stats.path && stats.path !== '/' && norm(stats.path) !== norm(stats.name)) parts.push(stats.path);
  if (stats.files)   parts.push(`files: ${stats.files}`);
  if (stats.folders) parts.push(`folders: ${stats.folders}`);
  if (stats.sloc > 0) parts.push(`sloc: ${fmtMetricShort(stats.sloc)}`);
  if (stats.hk   > 0) parts.push(`hk: ${fmtMetricShort(stats.hk)}`);
  if (stats.cycle > 0) parts.push(`in cycle: ${stats.cycle}`);
  return parts.join('  ·  ');
}

// Build edge-highlight behaviour: on node/cluster hover dim unrelated edges and
// show connected ones; if IN/OUT cluster edges exceed 10, hide them until the
// cluster zone is hovered. Must be called BEFORE setupTooltips (reads titles).
function setupEdgeHighlight(svgFrame, level) {
  const allEdgeEls = [...svgFrame.querySelectorAll('g.edge')];
  const allNodeEls = [...svgFrame.querySelectorAll('g.node')];
  if (allEdgeEls.length === 0) return;
  // Node lookup so a dir sub-cluster's edges can be matched by the same
  // crate-relative dir label ("/src/…") that layout.js prints.
  const nodeById = new Map((typeof unionGraph === 'function' ? unionGraph(level).nodes : []).map(n => [n.id, n]));

  const sb = svgFrame._statusBar;
  const showSB = text => { if (sb) { sb.textContent = text; sb.hidden = false; } };
  const hideSB = ()   => { if (sb) { sb.hidden = true; sb.textContent = ''; } };

  // Classify IN/OUT edges by the DOT class attribute written in layout.js.
  // Using CSS classes instead of \x01 prefix in edge titles because the HTML
  // parser strips U+0001 control chars when setting innerHTML.
  const inEdges  = allEdgeEls.filter(e => e.classList.contains('edge-in'));
  const outEdges = allEdgeEls.filter(e => e.classList.contains('edge-out'));

  // Build nodeId → Set<edgeEl> from edge titles ("src->tgt").
  const edgeMap = new Map();
  for (const edgeEl of allEdgeEls) {
    const title = edgeEl.querySelector('title')?.textContent?.trim() ?? '';
    const sep   = title.indexOf('->');
    if (sep < 0) continue;
    const src = title.slice(0, sep);
    const tgt = title.slice(sep + 2);
    for (const id of [src, tgt]) {
      if (!edgeMap.has(id)) edgeMap.set(id, new Set());
      edgeMap.get(id).add(edgeEl);
    }
  }

  // ── Shared helpers ───────────────────────────────────────────────────────────
  const applyHighlight = connected => {
    svgFrame.classList.add('node-hovered');
    for (const e of allEdgeEls) {
      e.classList.remove('edge-connected', 'edge-dim');
      if (connected.has(e)) e.classList.add('edge-connected');
      else                   e.classList.add('edge-dim');
    }
  };
  const clearHighlight = () => {
    svgFrame.classList.remove('node-hovered');
    for (const e of allEdgeEls) e.classList.remove('edge-connected', 'edge-dim');
  };

  // ── Cluster highlight: hover on cluster background highlights all its edges ──
  // Graphviz SVG uses generated ids (clust1, clust2, …) — the subgraph name is
  // only in the cluster's <title> child. Nodes are NOT inside cluster <g>s.
  // cluster_in  → inEdges (class="edge-in" set in layout.js DOT attributes)
  // cluster_out → outEdges (class="edge-out")
  // cluster_N   → directory sub-cluster; label = dir path; match edgeMap keys
  const clusterData = new Map();
  let clusterInEl = null, clusterOutEl = null;

  for (const clusterEl of svgFrame.querySelectorAll('g.cluster')) {
    const cTitle = clusterEl.querySelector('title')?.textContent?.trim() || '';
    const label  = clusterEl.querySelector('text')?.textContent?.trim()  || '';

    let edges, nc;
    if (cTitle === 'cluster_in') {
      clusterInEl = clusterEl;
      edges = new Set(inEdges);
      nc = inEdges.length;
    } else if (cTitle === 'cluster_out') {
      clusterOutEl = clusterEl;
      edges = new Set(outEdges);
      nc = outEdges.length;
    } else if (cTitle.startsWith('cluster_crate_')) {
      // Overview crate cluster (dig IN): match the group boxes whose key sits in
      // this crate (key === crate, or starts with `crate/`). edgeMap keys here
      // are group ids, not file ids.
      const matchIds = [...edgeMap.keys()].filter(k => k === label || k.startsWith(label + '/'));
      edges = new Set();
      for (const id of matchIds) {
        for (const e of (edgeMap.get(id) ?? new Set())) edges.add(e);
      }
      nc = matchIds.length;
    } else {
      // Directory sub-cluster: label is the full workspace-relative dir
      // ("/libs/modkit-odata-macros/src") — must match layout.js's dirOf.
      const matchIds = [...edgeMap.keys()].filter(k => {
        const node = nodeById.get(k);
        return node ? nodeFullDir(node) === label : false;
      });
      edges = new Set();
      for (const id of matchIds) {
        for (const e of (edgeMap.get(id) ?? new Set())) edges.add(e);
      }
      nc = matchIds.length;
    }

    const ec = edges.size;
    const statusText = [label,
      nc ? `${nc} node${nc !== 1 ? 's' : ''}` : '',
      ec ? `${ec} edge${ec !== 1 ? 's' : ''}` : '',
    ].filter(Boolean).join('  ·  ');
    clusterData.set(clusterEl, { edges, statusText });

    clusterEl.addEventListener('mouseenter', () => { applyHighlight(edges); showSB(statusText); });
    clusterEl.addEventListener('mouseleave', () => { clearHighlight(); hideSB(); });
  }

  // ── IN/OUT edges are always hidden by default; revealed on cluster/node hover ──
  // (Previously only hidden past a count threshold; now hidden regardless so the
  // green/orange connectors never clutter the map until you hover.)
  const hideInOut = true;
  const hideIn = hideInOut, hideOut = hideInOut;
  if (hideInOut) {
    inEdges.forEach(e  => e.classList.add('cluster-edge-hidden'));
    outEdges.forEach(e => e.classList.add('cluster-edge-hidden'));
  }

  // Use the cluster elements found by title above (ids are generated: clust1, …)
  if (hideIn && clusterInEl) {
    clusterInEl.addEventListener('mouseenter', () => svgFrame.classList.add('show-in-edges'));
    clusterInEl.addEventListener('mouseleave', () => svgFrame.classList.remove('show-in-edges'));
  }
  if (hideOut && clusterOutEl) {
    clusterOutEl.addEventListener('mouseenter', () => svgFrame.classList.add('show-out-edges'));
    clusterOutEl.addEventListener('mouseleave', () => svgFrame.classList.remove('show-out-edges'));
  }

  // ── Node hover ───────────────────────────────────────────────────────────────
  for (const nodeEl of allNodeEls) {
    const nodeId = nodeEl.querySelector('title')?.textContent?.trim();
    if (!nodeId) continue;

    nodeEl.addEventListener('mouseenter', () => {
      applyHighlight(edgeMap.get(nodeId) ?? new Set());
      // Status bar is updated by setupTooltips handlers (fire after these).
    });

    nodeEl.addEventListener('mouseleave', e => {
      // When moving back to a cluster background re-apply cluster highlight;
      // otherwise clear. setupTooltips mouseleave is registered after ours and
      // will skip hideStatus when relatedTarget is inside a cluster.
      const destCluster = e.relatedTarget?.closest?.('g.cluster');
      const cd = destCluster ? clusterData.get(destCluster) : null;
      if (cd) { applyHighlight(cd.edges); showSB(cd.statusText); }
      else    clearHighlight();
    });
  }
}

function setupTooltips(svgFrame, level) {
  svgFrame.querySelectorAll('g.edge title, g.cluster title').forEach(t => t.remove());

  const drillGroup = window.drillGroup || null;
  const section    = svgFrame.closest('.view');
  const gNodeMap   = new Map();

  const sb = svgFrame._statusBar;
  const showStatus = text => { if (sb) { sb.textContent = text; sb.hidden = false; } };
  const hideStatus = ()   => { if (sb) { sb.hidden = true; sb.textContent = ''; } };

  if (drillGroup !== null) {
    // ── Drilled file view: wire up individual file nodes ─────────────────────────
    // Map EVERY union node so baseline-only / current-only nodes get handlers too.
    const nodeMap = new Map(unionGraph(level).nodes.map(n => [n.id, n]));
    // External neighbour boxes are keyed by the drill-time grouper (same as
    // layout.js) — aggregate their stats so a hover shows crate-style details.
    const neighbourStats = computeGroupStats(level, grouperForDig(level, window.drillDig ?? 0));

    svgFrame.querySelectorAll('g.node').forEach(g => {
      const titleEl = g.querySelector('title');
      const nodeId  = titleEl?.textContent?.trim();
      titleEl?.remove();

      // External neighbor node (caller / dependency from another group)?
      const neighborPrefix = nodeId?.startsWith('IN\x01') ? 'IN\x01'
                           : nodeId?.startsWith('OUT\x01') ? 'OUT\x01' : null;
      if (neighborPrefix) {
        const neighborGroup = nodeId.slice(neighborPrefix.length);
        const arrow = neighborPrefix === 'IN\x01' ? '← ' : '→ ';
        g.addEventListener('click', e => {
          e.stopPropagation();
          drillIntoGroup(neighborGroup, level);
        });
        g.addEventListener('mouseenter', () => {
          g.classList.add('node-hl');
          const st = neighbourStats.get(neighborGroup);
          showStatus(st ? statusLineForGroup({ ...st, name: arrow + st.name })
                        : arrow + neighborGroup);
        });
        g.addEventListener('mouseleave', e => {
          g.classList.remove('node-hl');
          if (!e.relatedTarget?.closest?.('g.cluster')) hideStatus();
        });
        return;
      }

      const node = nodeMap.get(nodeId);
      if (!node) return;

      g.dataset.nodeId = nodeId;
      gNodeMap.set(nodeId, g);

      g.addEventListener('click', e => {
        e.stopPropagation();
        if (isOpenSrcClick(e)) {
          const url = nodeSourceUrl(node, level);
          if (url) window.open(url, '_blank', 'noopener');
          return;
        }
        if (e.shiftKey) { toggleNodeSelected(node, level, section); return; }
        if (window.openModalForNode?.(node.id, level)) window.navPush?.(level, node.id);
      });

      g.addEventListener('mouseenter', () => {
        g.classList.add('node-hl');
        section?.querySelector(`tr[data-node-id="${nodeId.replace(/\\/g,'\\\\').replace(/"/g,'\\"')}"]`)
                ?.classList.add('row-hl');
        showStatus(statusLineFor(node, level));
      });
      g.addEventListener('mouseleave', e => {
        g.classList.remove('node-hl');
        section?.querySelector(`tr[data-node-id="${nodeId.replace(/\\/g,'\\\\').replace(/"/g,'\\"')}"]`)
                ?.classList.remove('row-hl');
        if (!e.relatedTarget?.closest?.('g.cluster')) hideStatus();
      });
    });

  } else {
    // ── Group view: tag group nodes and wire up drill-in click ───────────────────
    const gOf = grouperForDig(level, window.dig || 0);
    const groupStats = computeGroupStats(level, gOf);

    svgFrame.querySelectorAll('g.node').forEach(g => {
      const titleEl = g.querySelector('title');
      const groupId = titleEl?.textContent?.trim();
      titleEl?.remove();
      if (!groupId) return;
      const stats = groupStats.get(groupId);
      if (!stats) return;

      g.dataset.groupId    = groupId;
      g.dataset.groupStats = JSON.stringify(stats);

      g.addEventListener('click', e => {
        e.stopPropagation();
        drillIntoGroup(groupId, level);
      });
      g.addEventListener('mouseenter', () => {
        g.classList.add('node-hl');
        showStatus(statusLineForGroup(stats));
      });
      g.addEventListener('mouseleave', e => {
        g.classList.remove('node-hl');
        if (!e.relatedTarget?.closest?.('g.cluster')) hideStatus();
      });
    });
  }

  if (section) section._gNodeMap = gNodeMap;
}

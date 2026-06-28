// Fallback palette (used only when the snapshot's node_kinds dictionary omits a
// colour). Real colours come from node_kinds[kind].fill / .stroke.
const N_FILL  = '#dbe9f4';
const N_COLOR = '#4d6f9c';
const E_COLOR = '#4d6f9c';
const EXT_FILL  = '#f6e2c0';
const EXT_COLOR = '#b3801f';

function dotId(id) {
  return '"' + id.replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"';
}

// ── Metric node sizing (loc/hk circle modes) — reads flat node attributes.
// Module-scope so the post-layout per-side resize (`applySideSizing`) reuses the
// exact same math. The size-mode key maps to an attribute: 'loc' → sloc (the
// source-line count, falling back to the structural loc), 'hk' → hk. ──
const METRIC_BASE_DIAM = 0.3, METRIC_BASE_LOC = 100, METRIC_BASE_HK = 1000;
function metricNodeVal(n, mode) {
  if (!n || !mode) return 0;
  // The size-mode key IS the attribute key (data-driven from `ui.size`).
  // `loc` is the one historical alias (→ sloc) kept for older shared links.
  if (mode === 'loc') return Number(n.sloc ?? n.loc ?? 0);
  return Number(n[mode] ?? 0);
}
// The value that maps to the minimum circle (`METRIC_BASE_DIAM`); larger values
// grow as sqrt. Built-in loc/hk keep their calibrated bases; any other metric
// uses the median of the rendered population's positive values (cached per render
// in `window._sizeBaseCache`), so a ratio ~1 and a count in the thousands both
// spread sensibly. Falls back to 1 when no population base was computed.
function sizeBaseFor(mode) {
  if (mode === 'loc') return METRIC_BASE_LOC;
  if (mode === 'hk')  return METRIC_BASE_HK;
  const c = window._sizeBaseCache;
  return c && c.mode === mode && c.base > 0 ? c.base : 1;
}
function metricSizeBase(nodes, mode) {
  if (mode === 'loc') return METRIC_BASE_LOC;
  if (mode === 'hk')  return METRIC_BASE_HK;
  const vals = nodes.map(n => metricNodeVal(n, mode)).filter(v => v > 0).sort((a, b) => a - b);
  return vals.length ? vals[Math.floor(vals.length / 2)] || 1 : 1;
}
function metricNodeDiam(n, mode) {
  const v = metricNodeVal(n, mode), base = sizeBaseFor(mode);
  return +(METRIC_BASE_DIAM * Math.sqrt(Math.max(v, base) / base)).toFixed(3);
}
// Diameter for an aggregate (sum over all files in a group). Uses the same
// sqrt-scale formula but with a higher base so groups don't dwarf the canvas.
function metricGroupDiam(aggVal, mode) {
  const base = sizeBaseFor(mode);
  return +(METRIC_BASE_DIAM * Math.sqrt(Math.max(aggVal, base) / base)).toFixed(3);
}
function fmtMetricShort(v) {
  if (v >= 1_000_000) return Math.round(v / 1_000_000) + 'M';
  if (v >= 1_000)     return Math.round(v / 1_000) + 'K';
  return String(Math.round(v));
}
const metricFontSize = d => Math.max(6, Math.round(d * 26));

// The grouping ladder (`grouperForDig`) lives in grouping.js; layout consumes it.

function buildDOT(nodes, edges, level, viewport) {
  const sizeMode   = window.nodeSizeMode || null;
  const drillGroup = window.drillGroup   || null;
  // Any active size mode renders nodes as sized circles (else default boxes).
  const isMetric   = sizeMode !== null;
  // Cache the size-scale base for this render (data-driven for custom metrics);
  // metricNodeDiam/metricGroupDiam read it via sizeBaseFor, including the
  // post-layout per-side resize (applySideSizing) which runs after this.
  if (isMetric) window._sizeBaseCache = { mode: sizeMode, base: metricSizeBase(nodes, sizeMode) };
  // Overview granularity follows the relative zoom; a drilled (focus) view filters
  // by the zoom that was active when the user drilled in.
  const activeDig  = drillGroup === null ? (window.dig || 0) : (window.drillDig ?? 0);
  const gOf        = grouperForDig(level, activeDig);
  // CYCLES is keyed [lang][level]; resolve active language before indexing.
  const _langForCycles = (typeof currentLang === 'function' ? currentLang() : null)
                      || Object.keys(window.CYCLES || {})[0];
  const _langCycles    = _langForCycles ? window.CYCLES?.[_langForCycles] : null;
  const cycleOf    = _langCycles?.[level]?.nodeCycleStatus;
  // Node filter (data-driven from `ui.filter`): when a key is active keep
  // only nodes where that metric has signal. `cycle` is special (uses the cycle
  // membership set); any other key keeps nodes whose attribute value is non-zero.
  const isCyc      = id => !!(cycleOf && cycleOf.has(id));
  const nodeFilter = window.nodeFilter || null;
  const passFilter = n => {
    if (!nodeFilter) return true;
    if (nodeFilter === 'cycle') return isCyc(n.id);
    return Number(n[nodeFilter] ?? 0) > 0;
  };

  // Fan-in/out section data is recomputed each render; overview leaves it empty.
  window._fanData = { in: [], out: [] };

  let dot = 'digraph {\n';
  dot += '  rankdir=LR\n';
  // No `ratio=fill` / `size`: let graphviz lay out at natural size with packed
  // nodes (tiny nodesep/ranksep), then the SVG viewBox scales uniformly to the
  // frame — so the gaps between nodes stay small instead of being stretched.
  // Tighter rank/node spacing + roomier box padding so nodes occupy more of the
  // frame relative to whitespace (edges route less prettily — an accepted trade
  // for bigger, more legible nodes).
  dot += '  graph [bgcolor="white" fontname="Helvetica" pad="0.1" nodesep="0.12" ranksep="0.6"]\n';
  // Smaller arrowheads — graphviz default (arrowsize=1) reads oversized once the
  // SVG viewBox is scaled up to fill the frame on sparse graphs.
  dot += '  edge  [arrowsize=0.6]\n';
  if (isMetric) {
    dot += '  node  [shape=circle style=filled fixedsize=true width=0.3]\n\n';
  } else {
    dot += '  node  [shape=box style=filled fontname="Helvetica" fontsize=11 margin="0.044,0.022" height=0 width=0]\n\n';
  }

  // ── Group view: one node per group, deduped inter-group edges ─────────────────
  if (drillGroup === null) {
    const nodeGroup  = new Map();
    const groupNodes = new Map();
    for (const n of nodes) {
      if (!passFilter(n)) continue;   // node filter: drop nodes without signal
      const g = gOf(n);
      nodeGroup.set(n.id, g);
      if (!groupNodes.has(g)) groupNodes.set(g, []);
      groupNodes.get(g).push(n);
    }

    const baselineById = new Map((window.BASELINE?.graphs?.[level]?.nodes || []).map(n => [n.id, n]));
    const currentById  = new Map((window.CURRENT?.graphs?.[level]?.nodes  || []).map(n => [n.id, n]));

    // Crate-tier groups (zoom 0) are pink; any other grouping (folders, or the
    // file tier) is a uniform neutral white, so the colour signals "these are crates".
    const isCrateTier = window.viewTier(level) === 'crate' && activeDig === 0 && !!(levelUi(level).grouping?.key);
    // Files mode (one step past the deepest folders): each group IS a single file,
    // so drop the member-count suffix and render like a plain file node.
    const filesMode   = window.isFilesDig(level, activeDig);
    const groupFill   = isCrateTier ? '#ffd4d4' : '#ffffff';
    // Metric circles are always filled — red for the crate tier, blue otherwise
    // (white reads as "empty" / unfinished on the folder tiers).
    const circleFill  = isCrateTier ? '#ffd4d4' : N_FILL;

    // One DOT statement for a single group box (circle in metric mode, box otherwise).
    const groupBoxDot = (g, gNodes) => {
      // A group is red when any member sits in a dependency cycle (aggregated
      // per side); reuses the same cycle-status CSS as individual nodes.
      const gCyc = aggCycleStatus(gNodes.map(n => cycleOf?.get(n.id) || 'none'));
      const cyc  = `class="cycle-status-${gCyc}"`;
      // Group label: crate name at dig 0, the full folder path when digging in
      // or collapsing (see grouping.js).
      const leaf = groupLabel(level, g, activeDig);
      if (isMetric) {
        const aggB = gNodes.reduce((s, n) => s + metricNodeVal(baselineById.get(n.id), sizeMode), 0);
        const aggC = gNodes.reduce((s, n) => s + metricNodeVal(currentById.get(n.id),  sizeMode), 0);
        const agg  = Math.max(aggB, aggC) || gNodes.reduce((s, n) => s + metricNodeVal(n, sizeMode), 0);
        const d    = metricGroupDiam(agg, sizeMode);
        const lbl  = agg > 0 ? fmtMetricShort(agg) : '';
        const fs   = metricFontSize(d);
        return `${dotId(g)} [label=${dotId(lbl)} fontsize=${fs} fontcolor="#333" fillcolor="${circleFill}" color="${N_COLOR}" width=${d} shape=circle style=filled fixedsize=true ${cyc}]`;
      }
      // Group box: name + the count of member nodes (what opens on drill-in).
      const lbl = `${leaf} (${gNodes.length})`;
      return `${dotId(g)} [label=${dotId(lbl)} fillcolor="${groupFill}" color="${N_COLOR}" shape=box style=filled fontname="Helvetica" fontsize=11 ${cyc}]`;
    };

    // At dig IN (>0) with crate grouping, wrap each crate's folder-groups in a
    // labelled crate cluster — so folders read as "inside their crate", mirroring
    // the drilled view's directory sub-clusters. dig 0 / dig OUT render flat.
    // Files mode keys groups by full workspace path (not `crate/under`), so the
    // crate-cluster wrapper can't parse them → render flat.
    const clusterByCrate = window.viewTier(level) === 'crate' && activeDig > 0 && !filesMode && !!(levelUi(level).grouping?.key);
    if (filesMode) {
      // Files level: keep the deepest SINGLE folder level as clusters (no nested
      // folders), but draw the real file nodes inside each — mirroring the drilled
      // view's directory sub-clusters. Internal edges (below) are file↔file via the
      // per-file grouping; external nodes stay as their plural group box.
      const folderDig = window.filesDig(level) - 1;
      const fileDot = n => {
        const ks   = nodeKindSpec(level, n.kind);
        const fill = ks.fill   || N_FILL;
        const col  = ks.stroke || N_COLOR;
        const cls  = `class="node-${n.kind || 'unknown'} status-${n.status} cycle-status-${cycleOf?.get(n.id) || 'none'}"`;
        if (isMetric) {
          const db = baselineById.has(n.id) ? metricNodeDiam(baselineById.get(n.id), sizeMode) : 0;
          const da = currentById.has(n.id)  ? metricNodeDiam(currentById.get(n.id),  sizeMode) : 0;
          const d  = Math.max(db, da) || metricNodeDiam(n, sizeMode);
          const v  = metricNodeVal(n, sizeMode);
          const lbl = v > 0 ? fmtMetricShort(v) : '';
          const fs  = metricFontSize(d);
          return `label=${dotId(lbl)} fontsize=${fs} fontcolor="#333" fillcolor="${fill}" color="${col}" width=${d} shape=circle style=filled fixedsize=true ${cls}`;
        }
        return `label=${dotId(n.name)} fillcolor="${fill}" color="${col}" shape=box style=filled fontname="Helvetica" fontsize=11 ${cls}`;
      };
      const folders = new Map();   // deepest-folder key → [internal file nodes]
      const extGrp  = new Map();   // plural → [external nodes]
      for (const [g, gNodes] of groupNodes) {
        for (const n of gNodes) {
          if (isExternalNode(n, level)) { (extGrp.get(g) || extGrp.set(g, []).get(g)).push(n); continue; }
          const fk = groupKeyAtDig(level, n, folderDig);
          (folders.get(fk) || folders.set(fk, []).get(fk)).push(n);
        }
      }
      let fi = 0;
      for (const [fk, ns] of folders) {
        dot += `  subgraph cluster_files_${fi++} {\n`;
        dot += `    label=${dotId(groupLabel(level, fk, folderDig))} style=filled fillcolor="#f7f7f7" color="#cccccc" fontcolor="#666666" fontname="Helvetica" fontsize=11\n`;
        for (const n of ns) dot += `    ${dotId(n.id)} [${fileDot(n)}]\n`;
        dot += '  }\n';
      }
      for (const [g, gNodes] of extGrp) dot += `  ${groupBoxDot(g, gNodes)}\n`;
    } else if (clusterByCrate) {
      const crateOf = g => { const i = g.indexOf('/'); return i >= 0 ? g.slice(0, i) : g; };
      const byCrate = new Map();   // crate → [[g, gNodes], …]
      const loose   = [];          // external / crate-less groups stay outside clusters
      for (const [g, gNodes] of groupNodes) {
        if (gNodes.every(n => isExternalNode(n, level))) { loose.push([g, gNodes]); continue; }
        const c = crateOf(g);
        (byCrate.get(c) || byCrate.set(c, []).get(c)).push([g, gNodes]);
      }
      let ci = 0;
      for (const [crate, entries] of byCrate) {
        dot += `  subgraph cluster_crate_${ci++} {\n`;
        // Red signals "crate" only at the overview (dig 0). Once dug in (dig>0) the
        // crate is just a container around its folders — render it neutral, matching
        // the folder sub-clusters, so the red is reserved for the top-level boxes.
        dot += `    label=${dotId(crate)} style=filled fillcolor="#f7f7f7" color="#cccccc" fontname="Helvetica" fontsize=11 fontcolor="#666666"\n`;
        for (const [g, gNodes] of entries) dot += `    ${groupBoxDot(g, gNodes)}\n`;
        dot += '  }\n';
      }
      for (const [g, gNodes] of loose) dot += `  ${groupBoxDot(g, gNodes)}\n`;
    } else {
      for (const [g, gNodes] of groupNodes) dot += `  ${groupBoxDot(g, gNodes)}\n`;
    }

    const seenGroupEdge = new Set();
    for (const e of edges) {
      if (!edgeIsFlow(level, e.kind)) continue;
      const sg = nodeGroup.get(e.source);
      const tg = nodeGroup.get(e.target);
      if (!sg || !tg || sg === tg) continue;
      const key = sg + '\x00' + tg;
      if (seenGroupEdge.has(key)) continue;
      seenGroupEdge.add(key);
      dot += `  ${dotId(sg)} -> ${dotId(tg)} [color="${E_COLOR}" style="solid"]\n`;
    }
    // Non-flow inter-group edges (contains / reexports): dashed + hidden until a
    // group hover reveals them; skip pairs already linked by a flow edge.
    const seenGroupNF = new Set();
    for (const e of edges) {
      if (edgeIsFlow(level, e.kind)) continue;
      const sg = nodeGroup.get(e.source);
      const tg = nodeGroup.get(e.target);
      if (!sg || !tg || sg === tg) continue;
      const key = sg + '\x00' + tg;
      if (seenGroupEdge.has(key) || seenGroupNF.has(key)) continue;
      seenGroupNF.add(key);
      dot += `  ${dotId(sg)} -> ${dotId(tg)} [color="${E_COLOR}" style="dashed" constraint=false class="edge-nonflow"]\n`;
    }

    dot += '}';
    return dot;
  }

  // ── Drilled file view: only files in the selected group ───────────────────────
  const drillNodes = nodes.filter(n => gOf(n) === drillGroup && passFilter(n));
  const drillIds   = new Set(drillNodes.map(n => n.id));
  dot += '  newrank=true\n';

  const baselineById = new Map((window.BASELINE?.graphs?.[level]?.nodes || []).map(n => [n.id, n]));
  const currentById  = new Map((window.CURRENT?.graphs?.[level]?.nodes  || []).map(n => [n.id, n]));
  const allNodesById = new Map(nodes.map(n => [n.id, n]));

  // ── Focus level-of-detail (`window.focusDig`) ─────────────────────────────────
  // 0 = individual files (default); a negative value collapses the focus's files
  // into folder boxes, deepest folders first (mirrors the overview's dig out → in).
  const underDepth = n => underDepthOf(level, n);   // shared (see map-interactions.js)
  const maxFocusD  = drillNodes.length ? Math.max(...drillNodes.map(underDepth)) : 0;
  const fz         = window.focusDig || 0;
  // Reveal depth D (0 = the most-collapsed landing, up to maxRel = all files). A
  // node is shown as an individual FILE when it sits at or above the revealed
  // frontier (its folder level under the focus ≤ D); deeper nodes collapse into a
  // folder box at the frontier (focus + D + 1 levels). So depth 0 shows the focus's
  // direct files (in their dir cluster) plus its immediate subfolders as boxes.
  const minFz      = -Math.max(0, maxFocusD - activeDig);
  const D          = fz - minFz;
  const frontierDig = activeDig + D + 1;
  // The focus's PARENT dir — subtracted from folder labels so a drilled view shows
  // paths relative to where you are while keeping the focus folder's own name
  // (focus `…/sdk/src` → `/src`, children `/src/render`), not the long ancestor path.
  const focusBase  = focusStripBase(level);
  const relLevel   = n => underDepth(n) - activeDig;
  const isFileNode = n => relLevel(n) <= D;
  const renderId   = id => { const n = allNodesById.get(id); return (n && !isFileNode(n)) ? groupKeyAtDig(level, n, frontierDig) : id; };
  const anyBoxed   = drillNodes.some(n => !isFileNode(n));
  // _FOCUS.focusD is the dig the collapsed folder boxes are keyed at (for the
  // tooltip/click handlers); folderMode flags that some boxes are present.
  window._FOCUS = { folderMode: anyBoxed, focusD: frontierDig, maxFocusD };

  const layoutDiam = n => {
    const db = baselineById.has(n.id) ? metricNodeDiam(baselineById.get(n.id), sizeMode) : 0;
    const da = currentById.has(n.id)  ? metricNodeDiam(currentById.get(n.id),  sizeMode) : 0;
    return Math.max(db, da) || metricNodeDiam(n, sizeMode);
  };

  const edgeCycleOf = _langCycles?.[level]?.edgeCycleStatus;
  // Non-flow edges (contains / reexports) render DASHED and tagged `edge-nonflow`
  // so CSS keeps them hidden until a node hover reveals the connected ones; flow
  // edges stay solid and always visible.
  const eAttr = e => {
    const flow = edgeIsFlow(level, e.kind);
    return `color="${E_COLOR}" style="${flow ? 'solid' : 'dashed'}" class="edge-${e.kind || 'unknown'} status-${e.status} cycle-status-${edgeCycleOf ? edgeCycleOf(e.source, e.target) : 'none'}${flow ? '' : ' edge-nonflow'}"`;
  };

  const nAttr = n => {
    const ks   = nodeKindSpec(level, n.kind);
    const ext  = isExternalNode(n, level);
    const fill = ks.fill   || (ext ? EXT_FILL  : N_FILL);
    const col  = ks.stroke || (ext ? EXT_COLOR : N_COLOR);
    const cls  = `class="node-${n.kind || 'unknown'} status-${n.status} cycle-status-${cycleOf?.get(n.id) || 'none'}"`;
    if (isMetric) {
      const d   = layoutDiam(n);
      const v   = metricNodeVal(n, sizeMode);
      const lbl = v > 0 ? fmtMetricShort(v) : '';
      const fs  = metricFontSize(d);
      return `label=${dotId(lbl)} fontsize=${fs} fontcolor="#333" fillcolor="${fill}" color="${col}" width=${d} ${cls}`;
    }
    // File box: just the file name, no connection counts.
    return `label=${dotId(n.name)} fillcolor="${fill}" color="${col}" ${cls}`;
  };

  // ── Collect neighbour CRATES (callers / dependencies, no 3rd-party) ───────────
  // Group every cross-boundary edge by the OTHER end's **crate** (regardless of
  // tier / focus depth), so the boxes are a stable list of crates. Both **flow**
  // (uses) and **non-flow** (contains / reexports) edges are included. Per crate:
  //   • `their` — the distinct neighbour-side files coupled via **flow** edges (the
  //     box's `(N)` count); a crate reached only by non-flow edges counts `(0)`.
  //   • `our`   — our render-ids the connector edges fan to, with per-diff-side
  //     presence (Baseline/Current toggle) and `flow` = does ANY edge to that file
  //     flow (flow wins → solid connector; else dashed).
  // A crate that is both a caller and a dependency appears on the left only.
  const crateOf = n => crateIdOf(level, n) ?? gOf(n);
  const inGrp  = new Map();   // crate → { their:Set<flow-their-file>, our:Map<our-id,{b,c,flow}> }
  const outGrp = new Map();
  const touch = (m, crate, theirFile, ourId, e, flow) => {
    let r = m.get(crate);
    if (!r) { r = { their: new Set(), our: new Map() }; m.set(crate, r); }
    if (flow) r.their.add(theirFile);   // count flow-coupled files only
    let rec = r.our.get(ourId);
    if (!rec) { rec = { b: false, c: false, flow: false }; r.our.set(ourId, rec); }
    rec.b = rec.b || e.status !== 'added';    // present in baseline
    rec.c = rec.c || e.status !== 'removed';  // present in current
    rec.flow = rec.flow || flow;              // flow priority: solid if any flow edge
  };
  for (const e of edges) {
    const flow = edgeIsFlow(level, e.kind);
    const sIn = drillIds.has(e.source), tIn = drillIds.has(e.target);
    if (!sIn && tIn) {
      const src = allNodesById.get(e.source);
      if (!src || isExternalNode(src, level)) continue;
      touch(inGrp, crateOf(src), e.source, renderId(e.target), e, flow);
    } else if (sIn && !tIn) {
      const tgt = allNodesById.get(e.target);
      if (!tgt || isExternalNode(tgt, level)) continue;
      touch(outGrp, crateOf(tgt), e.target, renderId(e.source), e, flow);
    }
  }
  for (const c of inGrp.keys()) outGrp.delete(c);   // a crate in both → callers only

  // Diff side-presence → status class (drives the overlay's Baseline/Current hide).
  const statusClass = (b, c) => (b && c) ? 'unchanged' : c ? 'added' : 'removed';
  const grpStatus = r => { let b = false, c = false; for (const rec of r.our.values()) { b = b || rec.b; c = c || rec.c; } return statusClass(b, c); };

  // The Fan-in (callers) / Fan-out (dependencies) neighbour sections are NOT laid
  // out by graphviz. The internal file/folder graph is rendered alone (so its node
  // positions are fixed), and the sections + their real arrows are composed into the
  // SVG afterwards (composeFanSections in map-interactions.js) — that way a +/−
  // collapse never reflows the graph, the viewBox, or the pan/zoom. Here we only
  // stash the per-crate data the overlay needs: the crate, its flow-coupled file
  // count, and our render-ids the arrows attach to (with flow → solid, diff status).
  const fanSerialize = grp => [...grp].map(([crate, r]) => ({
    crate, count: r.their.size, status: grpStatus(r),
    our: [...r.our].map(([fid, rec]) => ({ fid, flow: rec.flow, status: statusClass(rec.b, rec.c) })),
  }));
  window._fanData = { in: fanSerialize(inGrp), out: fanSerialize(outGrp) };

  // Reveal frontier: nodes at/above depth D render as individual files inside
  // their directory sub-cluster; deeper nodes collapse into a folder box at the
  // frontier. Both kinds can appear together — e.g. the focus's direct files in a
  // "/src" cluster alongside collapsed "/src/render", "/src/scan" boxes.
  const fileNodes = drillNodes.filter(isFileNode);
  const boxNodes  = drillNodes.filter(n => !isFileNode(n));

  // Collapsed folder boxes (deeper than the frontier), deduped by box key.
  const boxes = new Map();
  for (const n of boxNodes) { const k = groupKeyAtDig(level, n, frontierDig); (boxes.get(k) || boxes.set(k, []).get(k)).push(n); }
  for (const [k, ns] of boxes) {
    const gCyc = aggCycleStatus(ns.map(n => cycleOf?.get(n.id) || 'none'));
    // In a metric size-mode (loc/hk) EVERY node is a sized circle — including a
    // collapsed folder, drawn at the aggregate metric of the files it hides (same
    // math as an overview group circle), so the metric reads consistently across
    // overview groups, revealed files and collapsed folders. Kept grey so it still
    // reads as a folder, distinct from the blue file circles.
    if (isMetric) {
      const aggB = ns.reduce((s, n) => s + metricNodeVal(baselineById.get(n.id), sizeMode), 0);
      const aggC = ns.reduce((s, n) => s + metricNodeVal(currentById.get(n.id),  sizeMode), 0);
      const agg  = Math.max(aggB, aggC) || ns.reduce((s, n) => s + metricNodeVal(n, sizeMode), 0);
      const d    = metricGroupDiam(agg, sizeMode);
      const lbl  = agg > 0 ? fmtMetricShort(agg) : '';
      const fs   = metricFontSize(d);
      dot += `  ${dotId(k)} [label=${dotId(lbl)} fontsize=${fs} fontcolor="#555555" fillcolor="#ececec" color="#bbbbbb" width=${d} shape=circle style=filled fixedsize=true class="cycle-status-${gCyc}"]\n`;
      continue;
    }
    const lbl  = `${stripDirPrefix(focusBase, groupLabel(level, k, frontierDig))} (${ns.length})`;
    // Default (box) mode: collapsed folders are grey (matching the expanded dir
    // sub-clusters) so they read as folders, distinct from the file nodes.
    dot += `  ${dotId(k)} [label=${dotId(lbl)} fillcolor="#ececec" color="#bbbbbb" fontcolor="#555555" shape=box style=filled fontname="Helvetica" fontsize=11 class="cycle-status-${gCyc}"]\n`;
  }

  // Revealed files: directory sub-clusters labelled with the full workspace-relative
  // path (e.g. "/libs/modkit-odata-macros/src"), faint-filled so the folder area is
  // hoverable/clickable to drill in.
  const subGroups = new Map();
  fileNodes.forEach(n => { const d = nodeFullDir(n); (subGroups.get(d) || subGroups.set(d, []).get(d)).push(n); });
  let si = 0;
  for (const [label, ns] of subGroups) {
    dot += `  subgraph cluster_${si++} {\n`;
    dot += `    label=${dotId(stripDirPrefix(focusBase, label))} style=filled fillcolor="#f7f7f7" color="#cccccc" fontcolor="#666666" fontname="Helvetica" fontsize=11\n`;
    for (const n of ns) dot += `    ${dotId(n.id)} [${nAttr(n)}]\n`;
    dot += '  }\n';
  }

  // (Fan-in / Fan-out sections are composed into the SVG after layout — see
  // composeFanSections — not emitted here, so the internal graph lays out alone.)

  // ── Edges ─────────────────────────────────────────────────────────────────────
  // Internal edges (within the drilled group). Flow edges (solid) are laid out
  // normally; non-flow edges (contains / reexports) are added DASHED with
  // `constraint=false` (so they don't distort the layout) and hidden by CSS until
  // a node hover reveals the connected ones. A non-flow pair already linked by a
  // flow edge is skipped to avoid a doubled line.
  const flowPairs = new Set();
  for (const e of edges) {
    if (!edgeIsFlow(level, e.kind)) continue;
    if (!drillIds.has(e.source) || !drillIds.has(e.target)) continue;
    const s = renderId(e.source), t = renderId(e.target);
    if (s === t) continue;   // collapsed into the same folder box
    const key = s + '\x00' + t;
    if (flowPairs.has(key)) continue;
    flowPairs.add(key);
    dot += `  ${dotId(s)} -> ${dotId(t)} [${eAttr(e)}]\n`;
  }
  const seenNonFlow = new Set();
  for (const e of edges) {
    if (edgeIsFlow(level, e.kind)) continue;
    if (!drillIds.has(e.source) || !drillIds.has(e.target)) continue;
    const s = renderId(e.source), t = renderId(e.target);
    if (s === t) continue;
    const key = s + '\x00' + t;
    if (flowPairs.has(key) || seenNonFlow.has(key)) continue;
    seenNonFlow.add(key);
    dot += `  ${dotId(s)} -> ${dotId(t)} [${eAttr(e)} constraint=false]\n`;
  }

  // (Connectors from our files to the Fan-in/out crate boxes are drawn by the
  // post-layout overlay — composeFanSections — as real SVG arrows.)

  dot += '}';
  return dot;
}

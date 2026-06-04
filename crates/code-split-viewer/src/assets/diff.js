// ── Diff ──────────────────────────────────────────────────────────────────────
function computeDiff(baseline, current) {
  const result = {};
  for (const level of ['files']) {
    const bg = (baseline.graphs || {})[level] || { nodes: [], edges: [] };
    const ag = (current.graphs  || {})[level] || { nodes: [], edges: [] };

    const bgMap = new Map(bg.nodes.filter(n => !isExternalNode(n, level)).map(n => [n.id, n]));
    const agMap = new Map(ag.nodes.filter(n => !isExternalNode(n, level)).map(n => [n.id, n]));

    const nodeStatus = new Map();
    for (const id of agMap.keys()) nodeStatus.set(id, bgMap.has(id) ? 'unchanged' : 'added');
    for (const id of bgMap.keys()) if (!agMap.has(id)) nodeStatus.set(id, 'removed');

    const edgeKey = e => `${e.source}\x00${e.target}\x00${e.kind}`;
    const localEdges = edges => edges.filter(e => nodeStatus.has(e.source) && nodeStatus.has(e.target));

    const bgEdgeMap = new Map(localEdges(bg.edges).map(e => [edgeKey(e), e]));
    const agEdgeMap = new Map(localEdges(ag.edges).map(e => [edgeKey(e), e]));

    const edgeStatus = new Map();
    for (const key of agEdgeMap.keys()) edgeStatus.set(key, bgEdgeMap.has(key) ? 'unchanged' : 'added');
    for (const key of bgEdgeMap.keys()) if (!agEdgeMap.has(key)) edgeStatus.set(key, 'removed');

    const nodes = [];
    for (const [id, status] of nodeStatus)
      nodes.push({ ...(status === 'removed' ? bgMap.get(id) : agMap.get(id)), status });

    const edges = [];
    for (const [key, status] of edgeStatus)
      edges.push({ ...(status === 'removed' ? bgEdgeMap.get(key) : agEdgeMap.get(key)), status });

    for (const e of edges) {
      if (e.status === 'unchanged') continue;
      if (nodeStatus.get(e.source) === 'unchanged') nodeStatus.set(e.source, 'affected');
      if (nodeStatus.get(e.target) === 'unchanged') nodeStatus.set(e.target, 'affected');
    }
    nodes.forEach(n => { n.status = nodeStatus.get(n.id); });
    edges.forEach(e => {
      if (e.status === 'unchanged' &&
          (nodeStatus.get(e.source) !== 'unchanged' || nodeStatus.get(e.target) !== 'unchanged'))
        e.status = 'affected';
    });

    result[level] = { nodes, edges };
  }
  return result;
}

// ── Cycles ────────────────────────────────────────────────────────────────────

// Build SCC membership map from the level's backend-computed `graph.cycles`.
// The backend is the single source of truth for cycle detection and kind
// classification — no cycles in the JSON means no cycles in the UI.
function buildSCCOf(graph, level) {
  const ids = new Set(graph.nodes.filter(n => !isExternalNode(n, level)).map(n => n.id));
  const sccOf = new Map(); // nodeId → scc group index
  const cycles = graph.cycles || [];
  cycles.forEach((group, i) => {
    for (const id of group.nodes) if (ids.has(id)) sccOf.set(id, i);
  });
  return { sccOf, sccCount: cycles.length };
}

function computeCycles(baseline, current) {
  const result = {};
  for (const level of ['files']) {
    const bg = (baseline.graphs || {})[level] || { nodes: [], edges: [], cycles: [] };
    const ag = (current.graphs  || {})[level] || { nodes: [], edges: [], cycles: [] };

    const { sccOf: bgSCCOf } = buildSCCOf(bg, level);
    const { sccOf: agSCCOf } = buildSCCOf(ag, level);

    const nodeCycleStatus = new Map();
    for (const id of new Set([...bgSCCOf.keys(), ...agSCCOf.keys()])) {
      const b = bgSCCOf.has(id), a = agSCCOf.has(id);
      nodeCycleStatus.set(id, b && a ? 'both' : b ? 'baseline-only' : 'current-only');
    }

    const edgeCycleStatus = (from, to) => {
      const inB = bgSCCOf.has(from) && bgSCCOf.get(from) === bgSCCOf.get(to);
      const inA = agSCCOf.has(from) && agSCCOf.get(from) === agSCCOf.get(to);
      if (!inB && !inA) return 'none';
      return inB && inA ? 'both' : inB ? 'baseline-only' : 'current-only';
    };

    let cycleBaseline = 0, cycleCurrent = 0, cycleBoth = 0;
    for (const cs of nodeCycleStatus.values()) {
      if (cs === 'baseline-only') cycleBaseline++;
      else if (cs === 'current-only') cycleCurrent++;
      else cycleBoth++;
    }

    result[level] = { nodeCycleStatus, edgeCycleStatus, cycleBaseline, cycleCurrent, cycleBoth };
  }
  return result;
}

// ── Meta ──────────────────────────────────────────────────────────────────────
function computeMeta(baseline, current) {
  const label  = s => s?.git?.branch || s?.target?.split('/').pop() || 'snapshot';
  const commit = s => s?.git?.commit?.slice(0, 8) || '';
  const date   = s => s?.generated_at || '';
  const meta   = s => ({ name: label(s), commit: commit(s), date: date(s) });
  const primary = current || baseline;
  return {
    target: (primary?.target || primary?.workspace || 'snapshot').split('/').pop(),
    baseline: baseline ? meta(baseline) : null,
    current:  current  ? meta(current)  : null,
  };
}

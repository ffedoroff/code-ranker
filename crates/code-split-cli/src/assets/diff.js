// ── Tarjan SCC ────────────────────────────────────────────────────────────────
function findSCCs(ids, adjList) {
  let idx = 0;
  const index = new Map(), lowlink = new Map(), onStack = new Set();
  const stack = [], sccs = [];

  function dfs(v) {
    index.set(v, idx); lowlink.set(v, idx); idx++;
    stack.push(v); onStack.add(v);
    for (const w of (adjList.get(v) || [])) {
      if (!index.has(w)) {
        dfs(w);
        lowlink.set(v, Math.min(lowlink.get(v), lowlink.get(w)));
      } else if (onStack.has(w)) {
        lowlink.set(v, Math.min(lowlink.get(v), index.get(w)));
      }
    }
    if (lowlink.get(v) === index.get(v)) {
      const scc = [];
      let w;
      do { w = stack.pop(); onStack.delete(w); scc.push(w); } while (w !== v);
      if (scc.length > 1) sccs.push(scc);
    }
  }

  for (const id of ids) if (!index.has(id)) dfs(id);
  return sccs;
}

// ── Diff ──────────────────────────────────────────────────────────────────────
function computeDiff(before, after) {
  const result = {};
  for (const level of ['files']) {
    const bg = (before.graphs || {})[level] || { nodes: [], edges: [] };
    const ag = (after.graphs  || {})[level] || { nodes: [], edges: [] };

    const bgMap = new Map(bg.nodes.filter(n => !n.external).map(n => [n.id, n]));
    const agMap = new Map(ag.nodes.filter(n => !n.external).map(n => [n.id, n]));

    const nodeStatus = new Map();
    for (const id of agMap.keys()) nodeStatus.set(id, bgMap.has(id) ? 'unchanged' : 'added');
    for (const id of bgMap.keys()) if (!agMap.has(id)) nodeStatus.set(id, 'removed');

    const edgeKey = e => `${e.from}\x00${e.to}\x00${e.kind}`;
    const localEdges = edges => edges.filter(e => nodeStatus.has(e.from) && nodeStatus.has(e.to));

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
      if (nodeStatus.get(e.from) === 'unchanged') nodeStatus.set(e.from, 'affected');
      if (nodeStatus.get(e.to)   === 'unchanged') nodeStatus.set(e.to,   'affected');
    }
    nodes.forEach(n => { n.status = nodeStatus.get(n.id); });
    edges.forEach(e => {
      if (e.status === 'unchanged' &&
          (nodeStatus.get(e.from) !== 'unchanged' || nodeStatus.get(e.to) !== 'unchanged'))
        e.status = 'affected';
    });

    result[level] = { nodes, edges };
  }
  return result;
}

// ── Cycles ────────────────────────────────────────────────────────────────────

// Build SCC membership map from a graph level.
// Prefers backend-computed `graph.cycles` when present (accurate cycle_kind
// classification including TestEmbed); falls back to Tarjan SCC on edges.
function buildSCCOf(graph) {
  const ids = new Set(graph.nodes.filter(n => !n.external).map(n => n.id));
  const sccOf = new Map(); // nodeId → scc group index

  if (graph.cycles && graph.cycles.length > 0) {
    // Use backend pre-computed SCCs
    graph.cycles.forEach((group, i) => {
      for (const id of group.nodes) if (ids.has(id)) sccOf.set(id, i);
    });
    return { sccOf, sccCount: graph.cycles.length };
  }

  // Fallback: Tarjan SCC on non-contains edges
  const adj = new Map([...ids].map(id => [id, []]));
  for (const e of graph.edges)
    if (e.kind !== 'contains' && adj.has(e.from) && adj.has(e.to))
      adj.get(e.from).push(e.to);
  const sccs = findSCCs([...ids], adj);
  sccs.forEach((scc, i) => scc.forEach(n => sccOf.set(n, i)));
  return { sccOf, sccCount: sccs.length };
}

function computeCycles(before, after) {
  const result = {};
  for (const level of ['files']) {
    const bg = (before.graphs || {})[level] || { nodes: [], edges: [], cycles: [] };
    const ag = (after.graphs  || {})[level] || { nodes: [], edges: [], cycles: [] };

    const { sccOf: bgSCCOf } = buildSCCOf(bg);
    const { sccOf: agSCCOf } = buildSCCOf(ag);

    const nodeCycleStatus = new Map();
    for (const id of new Set([...bgSCCOf.keys(), ...agSCCOf.keys()])) {
      const b = bgSCCOf.has(id), a = agSCCOf.has(id);
      nodeCycleStatus.set(id, b && a ? 'both' : b ? 'before-only' : 'after-only');
    }

    const edgeCycleStatus = (from, to) => {
      const inB = bgSCCOf.has(from) && bgSCCOf.get(from) === bgSCCOf.get(to);
      const inA = agSCCOf.has(from) && agSCCOf.get(from) === agSCCOf.get(to);
      if (!inB && !inA) return 'none';
      return inB && inA ? 'both' : inB ? 'before-only' : 'after-only';
    };

    let cycleBefore = 0, cycleAfter = 0, cycleBoth = 0;
    for (const cs of nodeCycleStatus.values()) {
      if (cs === 'before-only') cycleBefore++;
      else if (cs === 'after-only') cycleAfter++;
      else cycleBoth++;
    }

    result[level] = {
      nodeCycleStatus, edgeCycleStatus,
      cycleBefore, cycleAfter, cycleBoth,
    };
  }
  return result;
}

// ── Meta ──────────────────────────────────────────────────────────────────────
function computeMeta(before, after) {
  const label  = s => s?.git?.branch || s?.target?.split('/').pop() || 'snapshot';
  const commit = s => s?.git?.commit?.slice(0, 8) || '';
  const date   = s => s?.generated_at || '';
  return {
    target: (before?.target || before?.workspace || 'snapshot').split('/').pop(),
    before: { name: label(before), commit: commit(before), date: date(before) },
    after:  after ? { name: label(after), commit: commit(after), date: date(after) } : null,
  };
}

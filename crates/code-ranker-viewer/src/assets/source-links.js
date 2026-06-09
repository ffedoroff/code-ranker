// source-links.js — git-host source URLs for nodes/connections, plus absolute
// on-disk path reconstruction. Pure helpers (no DOM). Split out of diagram.js.

// Convert a git remote `origin` URL into its web base (https://host/group/proj),
// handling scp-style SSH (git@host:group/proj.git), ssh:// and https remotes.
function gitWebBase(origin) {
  if (!origin) return null;
  const s = String(origin).trim();
  if (/^https?:\/\//i.test(s)) {
    return s.replace(/^(https?:\/\/)[^@/]+@/i, '$1')  // drop embedded credentials
            .replace(/\.git\/?$/i, '')
            .replace(/\/$/, '');
  }
  // scp-like (`git@host:group/proj.git`) or `ssh://git@host/group/proj.git`.
  const m = s.match(/^(?:ssh:\/\/)?(?:[^@]+@)?([^:/]+)[:/](.+?)(?:\.git)?\/?$/);
  return m ? `https://${m[1]}/${m[2]}` : null;
}

// Build a blob link to a project file at the analysed commit. `relPath` is the
// repo-relative path (the displayed path, with the `{root}/` token stripped).
// The node id IS the relativized path. An optional `line` adds a `#L<n>` anchor
// (GitHub and GitLab both use that form).
function gitSourceUrl(git, relPath, line) {
  const base = gitWebBase(git?.origin);
  if (!base || !relPath) return null;
  const ref  = git.commit || git.branch || 'HEAD';
  const enc  = relPath.split('/').map(encodeURIComponent).join('/');
  const blob = /(^|\/)github\.com\//i.test(base) ? 'blob' : '-/blob';   // GitLab uses /-/blob/
  const anchor = (line != null && Number.isFinite(+line)) ? `#L${line}` : '';
  return `${base}/${blob}/${ref}/${enc}${anchor}`;
}

// Git-host source URL for a node: only project files (external nodes live
// elsewhere). The node id IS its relativized path; strip the leading `{...}/`
// root token to get the repo-relative path. Returns null for external nodes.
// An optional `line` adds a `#L<n>` anchor to the blob URL.
function nodeSourceUrl(node, level, line) {
  if (!node) return null;
  if (level != null && isExternalNode(node, level)) return null;
  // Fallback for callers that don't pass level: check node.external flag.
  if (node.external === true) return null;
  // Use node.id as the path (strip the root token).
  const rel = (node.id || '').replace(/^\{[^}]+\}\//, '');
  if (!rel) return null;
  return gitSourceUrl(activeSnap()?.git, rel, line);
}
// Expose on window so modal.js can use it from click handlers.
window.nodeSourceUrl = nodeSourceUrl;

// Line to anchor when opening a fan-in neighbour's source from the popup. Only
// edges where the neighbour is the *source* and the central node is the target
// are considered — for those the edge's `line` (the `use` site) lives in the
// neighbour's own file. Pick the first flow edge (e.g. `uses`) that carries a
// line, else the edge with the largest line. Returns null when there is no such
// edge (e.g. a pure fan-out card, where the line would belong to the central
// file instead) so the caller opens the URL without an anchor.
function connSourceLine(neighbourId, centralId, level) {
  const edges = (activeGraph(level).edges || [])
    .filter(e => e.source === neighbourId && e.target === centralId && e.line != null);
  if (!edges.length) return null;
  const flow = edges.find(e => edgeIsFlow(level, e.kind));
  if (flow) return flow.line;
  return edges.reduce((m, e) => (e.line > m.line ? e : m)).line;
}
window.connSourceLine = connSourceLine;

// Reconstruct the absolute on-disk path from a relativized id/path: replace the
// leading `{token}/` with the snapshot's real root — `{target}` → the analyzed
// project dir, a named root (`{registry}` …) → `roots[token]`. Returns the input
// unchanged when there is no token or the root is unknown. Used for the path
// tooltip in the node popup.
function absPath(idOrPath) {
  const snap = activeSnap();
  const m = /^\{([^}]+)\}\/(.*)$/.exec(idOrPath || '');
  if (!snap || !m) return idOrPath || '';
  const base = m[1] === 'target' ? (snap.target ?? snap.roots?.target) : snap.roots?.[m[1]];
  return base ? `${base}/${m[2]}` : (idOrPath || '');
}

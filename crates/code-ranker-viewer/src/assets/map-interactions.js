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

// ── Navigation breadcrumb ──────────────────────────────────────────────────────
// One always-visible trail driving two orthogonal axes (see grouping.js + docs):
//   [tier ▾] › chip › chip … › cur      ⟨ ⊟ depth N/max ⊞ ⟩
//   • the tier dropdown anchor switches the dimension (crates ⇄ files) and its
//     label drills out to that tier's overview;
//   • each path chip drills to itself;
//   • the trailing lens chip controls the reveal depth — the overview's `window.dig`
//     or, while focused, the focus's `window.focusDig` — through setDig.

// The reveal-depth lens bounds for the current context, normalised so the lens
// shows a non-negative `depth N / max` (0 = coarsest). Overview: depth measured
// from the dig floor (single root) up to where digging deeper stops splitting.
// Focus: from the most-collapsed folder view (minFz) up to individual files (0).
function lensInfo(level) {
  if (window.drillGroup !== null) {
    const minFz = focusMinFz(level);
    const fz    = window.focusDig || 0;
    // depth 0 = the drill landing (the focus's direct children: its files in a dir
    // cluster + immediate subfolders as boxes); ⊞ (right) reveals one level deeper,
    // depth growing up to every file individually (fz = 0).
    return { depth: fz - minFz, canDown: fz > minFz, canUp: fz < 0,
             cur: focusRenderCount(level, fz),
             down: fz > minFz ? focusRenderCount(level, fz - 1) : null,
             up:   fz < 0     ? focusRenderCount(level, fz + 1) : null };
  }
  const z     = window.dig || 0;
  const floor = digFloor(level);
  // Ceiling: the highest dig that still increases the box count (capped at DIG_MAX).
  let ceil = Math.max(z, 0), prev = window.groupCountAtDig(level, ceil);
  while (ceil < DIG_MAX) {
    const c = window.groupCountAtDig(level, ceil + 1);
    if (c == null || c === prev) break;
    ceil++; prev = c;
  }
  if (z > ceil) ceil = z;
  // depth 0 = the overview landing (overviewBaseDig); + reveals finer, − coarser.
  return { depth: z - overviewBaseDig(level), canDown: z > floor, canUp: z < ceil,
           cur: window.groupCountAtDig(level, z),
           down: z > floor ? window.groupCountAtDig(level, z - 1) : null,
           up:   z < ceil  ? window.groupCountAtDig(level, z + 1) : null };
}

// A node's folder depth on the active tier's dig ladder (mirrors layout.js's
// underDepth): crate tier → depth under the crate root; file tier (or crate-less)
// → the absolute file-dig position (`dirs.length - maxFileDepth`, negative).
function underDepthOf(level, n) {
  const dirs  = nodeDirSegs(n.id);
  // File tier ignores the crate; crate tier measures depth under the crate root.
  const crate = window.viewTier(level) === 'file' ? null : crateIdOf(level, n);
  return crate == null
    ? dirs.length - maxFileDepth(level)
    : Math.max(0, dirs.length - (crateRoots(level).get(crate) || []).length);
}

// The deepest folder nesting under the current focus (mirrors layout.js's
// maxFocusD), used to find the most-collapsed focus view without a render. Seed
// null and take the true max — file-tier underDepth is negative, so a 0 seed wins.
function focusMaxDepth(level) {
  const grp = window.drillGroup;
  if (grp == null) return 0;
  const gOf = grouperForDig(level, window.drillDig ?? 0);
  let m = null;
  for (const n of unionGraph(level).nodes) {
    if (gOf(n) !== grp) continue;
    const ud = underDepthOf(level, n);
    if (m === null || ud > m) m = ud;
  }
  return m == null ? 0 : m;
}
// The focus-dig of the most-collapsed focus view (reveal depth 0). focusDig ranges
// [minFz, 0]: minFz = the focus's direct children, 0 = every file revealed.
function focusMinFz(level) {
  return -Math.max(0, focusMaxDepth(level) - (window.drillDig ?? 0));
}
window.focusMinFz = focusMinFz;

// Rendered element count at a focus-dig fz — file nodes (folder level ≤ reveal
// depth D) plus the distinct collapsed folder boxes deeper than the frontier.
// Mirrors layout.js's hybrid view so the lens hover previews are accurate.
function focusRenderCount(level, fz) {
  const grp     = window.drillGroup;
  const baseDig = window.drillDig ?? 0;
  const D       = fz - focusMinFz(level);
  const gOf     = grouperForDig(level, baseDig);
  let files = 0; const boxes = new Set();
  for (const n of unionGraph(level).nodes) {
    if (gOf(n) !== grp) continue;
    if (underDepthOf(level, n) - baseDig <= D) files++;
    else boxes.add(groupKeyAtDig(level, n, baseDig + D + 1));
  }
  return files + boxes.size;
}

// Node budget for the auto-chosen landing depth when drilling in.
const FOCUS_NODE_BUDGET = 20;
// The focusDig to land on when drilling into a crate/folder: the DEEPEST (most-
// revealed) view whose rendered element count stays under FOCUS_NODE_BUDGET. Falls
// back to the most-collapsed view (`minFz`, depth 0) when even that is already
// at/over budget. The count is non-decreasing as the reveal depth grows, so the
// deepest under-budget depth is also the richest. Keying on the highest *count*
// instead would wrongly stop early when a deeper reveal adds no elements — e.g. a
// lone file nested in folders keeps the count at 1 at every depth, so the folder
// would never expand. e.g. counts {d1:3, d2:18, d3:34} → land on d2 (18 < 20).
function landingFocusDig(level) {
  const minFz = focusMinFz(level);
  let best = minFz;
  for (let fz = minFz; fz <= 0; fz++) {
    if (focusRenderCount(level, fz) < FOCUS_NODE_BUDGET) best = fz;
  }
  return best;
}
window.landingFocusDig = landingFocusDig;

// The dig at which a focus `key` is a group key, for a tier — used as drillDig.
function digOfKeyForTier(level, key, tier) {
  if (tier === 'file') return key.split('/').length - maxFileDepth(level);
  const cut = key.indexOf('/');
  return cut >= 0 ? key.slice(cut + 1).split('/').length : 0;   // 0 = bare crate
}
window.digOfKeyForTier = digOfKeyForTier;

// The dig a breadcrumb path chip `i` drills into (its key has i+1 segments).
function chipDig(level, i, tier) {
  return tier === 'file' ? (i + 1) - maxFileDepth(level) : i;
}

// The tier-dropdown anchor (crates ⇄ files) — shared by the map breadcrumb and the
// file-modal header so both render identically. A plain label when no crates.
function tierAnchorHtml(level, tier) {
  const tierLabel = tier === 'file' ? 'files' : (levelUi(level).grouping?.key || 'crate') + 's';
  if (!levelUi(level).grouping?.key) return `<span class="drill-crumb-cur tier-label">${escHtml(tierLabel)}</span>`;
  return `<button class="drill-crumb tier-label" data-tier-toggle type="button" title="Switch dimension (crates ⇄ files)">${escHtml(tierLabel)} ▾</button>` +
    `<span class="tier-menu" hidden>` +
    `<button class="tier-opt${tier === 'crate' ? ' on' : ''}" data-tier="crate" type="button">crates</button>` +
    `<button class="tier-opt${tier === 'file' ? ' on' : ''}" data-tier="file" type="button">files</button>` +
    `</span>`;
}
window.tierAnchorHtml = tierAnchorHtml;

// Tier-menu toggle on a breadcrumb click; returns true when it handled the event.
// Shared by the map breadcrumb and the modal header click handlers.
function handleTierToggle(e) {
  const tg = e.target.closest('[data-tier-toggle]');
  if (!tg) return false;
  tg.parentElement.querySelector('.tier-menu')?.toggleAttribute('hidden');
  e.stopPropagation();
  return true;
}
window.handleTierToggle = handleTierToggle;

function renderBreadcrumb(level) {
  level = level || currentLevel();
  const grp  = window.drillGroup;
  const tier = window.viewTier(level);
  const uNodes = (typeof unionGraph === 'function' ? unionGraph(level).nodes : []);
  const filesUnder = (key, dg) => uNodes.reduce((c, n) => c + (groupKeyAtDig(level, n, dg) === key ? 1 : 0), 0);
  const col = (inner, count) =>
    `<span class="crumb-col">${inner}<span class="crumb-count">${count == null ? '' : count}</span></span>`;

  document.querySelectorAll(`.view[data-view="${level}"] .drill-breadcrumb`).forEach(bc => {
    bc.style.display = '';

    // ── Anchor: the tier dropdown (▾ switches dimension; the root chip drills out) ──
    const rootLabel = tier === 'file' ? 'root' : 'all';
    const rootCount = tier === 'crate' ? window.groupCountAtDig?.(level, 0)
                                       : uNodes.filter(n => !isExternalNode(n, level)).length;
    const parts = [col(`<span class="crumb-tier">${tierAnchorHtml(level, tier)}</span>`, null)];

    // ── Root element: "all" (crates) / "root" (files) — the whole-tree overview ──
    parts.push('<span class="drill-sep">›</span>');
    parts.push(grp == null
      ? col(`<span class="drill-crumb-cur">${rootLabel}</span>`, rootCount)
      : col(`<button class="drill-crumb" data-crumb-root="1" type="button" title="Show the whole overview">${rootLabel}</button>`, rootCount));

    // ── Path chips ─────────────────────────────────────────────────────────────
    if (grp != null) {
      const segs = String(grp).split('/');
      for (let i = 0; i < segs.length; i++) {
        const key  = segs.slice(0, i + 1).join('/');
        const dg   = chipDig(level, i, tier);
        const last = i === segs.length - 1;
        parts.push('<span class="drill-sep">›</span>');
        if (last) parts.push(col(`<span class="drill-crumb-cur">${escHtml(segs[i])}</span>`, filesUnder(key, dg)));
        else      parts.push(col(`<button class="drill-crumb" data-crumb-key="${escAttr(key)}" data-crumb-dig="${dg}" type="button">${escHtml(segs[i])}</button>`, filesUnder(key, dg)));
      }
    }

    // ── Lens chip: reveal depth (⊟ depth N/max ⊞) ──────────────────────────────
    const li = lensInfo(level);
    if (li.canDown || li.canUp) {
      parts.push('<span class="crumb-lens">' +
        col(`<button class="lens-btn" data-lens-step="-1" type="button"${li.canDown ? '' : ' disabled'} title="Collapse one level">⊟</button>`, li.canDown ? li.down : null) +
        col(`<span class="lens-depth" title="reveal depth">depth ${li.depth}</span>`, li.cur) +
        col(`<button class="lens-btn" data-lens-step="1" type="button"${li.canUp ? '' : ' disabled'} title="Reveal one level deeper">⊞</button>`, li.canUp ? li.up : null) +
        '</span>');
    }

    bc.innerHTML = parts.join(' ');
    if (!bc.dataset.crumbInit) {
      bc.dataset.crumbInit = '1';
      bc.addEventListener('click', e => {
        if (handleTierToggle(e)) return;
        const opt = e.target.closest('[data-tier]');
        if (opt) { switchTier(opt.dataset.tier, level); return; }
        const step = e.target.closest('.lens-btn');
        if (step) { if (!step.disabled) setDig(Number(step.dataset.lensStep), level); return; }
        const btn = e.target.closest('.drill-crumb');
        if (!btn) return;
        if (btn.dataset.crumbRoot) { drillOutOfGroup(level); return; }
        drillIntoGroup(btn.dataset.crumbKey, level, Number(btn.dataset.crumbDig) || 0);
      });
    }
  });
}
window.renderBreadcrumb = renderBreadcrumb;

// Close any open tier menu on an outside click.
document.addEventListener('click', e => {
  if (e.target.closest('[data-tier-toggle]') || e.target.closest('.tier-menu')) return;
  document.querySelectorAll('.tier-menu:not([hidden])').forEach(m => m.setAttribute('hidden', ''));
});

// Switch the grouping dimension (crates ⇄ files), mapping the current focus across
// the crate-root boundary when possible; otherwise fall back to the nearest
// representable ancestor, else the tier overview. Reveal depth resets at the focus
// (per-node overrides — Stage 2 — would be dropped here too).
function switchTier(tier, level) {
  level = level || currentLevel();
  document.querySelectorAll('.tier-menu:not([hidden])').forEach(m => m.setAttribute('hidden', ''));
  if (tier === window.viewTier(level)) {   // same dimension → go to its overview
    if (window.drillGroup !== null) drillOutOfGroup(level);
    return;
  }

  const cur = window.drillGroup;
  let mapped = null;
  if (cur != null) {
    const map = k => tier === 'file' ? crateKeyToFileKey(level, k) : fileKeyToCrateKey(level, k);
    mapped = map(cur);
    if (mapped == null) {   // climb ancestors until one maps
      const segs = String(cur).split('/');
      for (let k = segs.length - 1; k > 0 && mapped == null; k--) mapped = map(segs.slice(0, k).join('/'));
    }
  }

  window.tier = tier;
  if (mapped != null && mapped !== '_root') {
    window.drillGroup = mapped;
    window.drillDig   = digOfKeyForTier(level, mapped, tier);
    window.focusDig   = landingFocusDig(level);   // land at the depth that fits the node budget
  } else {
    window.drillGroup = null;
    // Land the overview at a coarse top-level grouping: crates at dig 0; the file
    // tier one level below root (top directories) instead of the finest per-folder
    // grouping that dig 0 would give there.
    window.dig = tier === 'file' ? clampDig(digFloor(level) + 1) : 0;
  }
  renderBreadcrumb(level);
  window.navReplaceView?.();
  document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
  const active = document.querySelector('.view.active');
  if (active && window.gv) renderView(active, { preserve: false });
}
window.switchTier = switchTier;

function drillIntoGroup(groupId, level, dig) {
  window.drillGroup = groupId;
  // The drilled view filters by the grouper that produced this group key, so
  // remember the dig it came from — caller may override (a crate cluster drills
  // into the whole crate → crate-tier grouper, dig 0).
  window.drillDig  = (dig != null) ? dig : (window.dig || 0);
  window.focusDig  = landingFocusDig(level);   // land at the depth that fits the node budget
  renderBreadcrumb(level);
  window.navPushView?.();
  document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
  const active = document.querySelector('.view.active');
  if (active && window.gv) renderView(active, { preserve: false });
}

function drillOutOfGroup(level) {
  window.drillGroup = null;
  window.focusDig   = 0;
  renderBreadcrumb(level);
  window.navPushView?.();
  document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
  const active = document.querySelector('.view.active');
  if (active && window.gv) renderView(active, { preserve: false });
}

// ── Fan-in / Fan-out overlay ───────────────────────────────────────────────────
// The internal file/folder graph is laid out by graphviz ALONE, so its node
// positions are fixed. The Fan-in (callers, top) / Fan-out (dependencies, bottom)
// sections + their real arrows are composed into the SVG afterwards. We reserve
// vertical bands for the fully-EXPANDED grids once (so the +/− toggle never changes
// the viewBox, the graph, or the pan/zoom); a toggle re-runs ONLY the overlay build
// — nothing moves but the section content and its arrows.
const SVGNS   = 'http://www.w3.org/2000/svg';
const FAN_GEO = { BOXH: 22, BOXMINW: 52, BOXPADX: 18, GAPX: 8, GAPY: 8, LBLH: 18, PAD: 12, BTN: 18, PILLW: 128 };
const FAN_PAL = {
  in:  { fill: '#edf7ed', stroke: '#88bb88', text: '#447744' },
  out: { fill: '#fdf3e3', stroke: '#ccaa77', text: '#886633' },
};
const svgEl = (name, attrs) => {
  const e = document.createElementNS(SVGNS, name);
  for (const k in attrs) if (attrs[k] != null) e.setAttribute(k, attrs[k]);
  return e;
};
function fanCollapsed(dir) {
  const st = window._fanCollapsed || (window._fanCollapsed = { in: true, out: true });
  return st[dir];
}
// A node's centre in the SVG's user (viewBox) coordinates — independent of pan/zoom,
// so it stays valid after the viewBox is expanded and across toggles.
function nodeCenterUser(svg, el) {
  const m = svg.getScreenCTM();
  if (!m) return null;
  // In metric size-modes (loc/hk) graphviz draws the node as an <ellipse>; measure
  // the shape itself, not the group bbox (which would include the text label and
  // skew the half-extents). Box nodes have no ellipse → fall back to the group bbox.
  const shape = el.querySelector('ellipse');
  const r = (shape || el).getBoundingClientRect();
  const p = svg.createSVGPoint();
  p.x = r.left + r.width / 2; p.y = r.top + r.height / 2;
  const o = p.matrixTransform(m.inverse());
  // half-extents in user units (graphviz applies no rotation → a/d are the scale)
  return { x: o.x, y: o.y, hw: (r.width / 2) / (m.a || 1), hh: (r.height / 2) / (m.d || 1), circle: !!shape };
}
// Point on a node's border toward `from`, so an arrow lands on the edge, not the
// centre. Circular/elliptical nodes use the ellipse boundary; box nodes use the
// bbox edge (a diagonal would otherwise hit the circumscribed square's corner).
function nodeEdgePoint(t, from) {
  const dx = from.x - t.x, dy = from.y - t.y;
  if (!dx && !dy) return { x: t.x, y: t.y };
  const s = t.circle
    ? 1 / Math.hypot(dx / t.hw, dy / t.hh)
    : Math.min(dx ? t.hw / Math.abs(dx) : Infinity, dy ? t.hh / Math.abs(dy) : Infinity);
  return { x: t.x + dx * s, y: t.y + dy * s };
}
// Measure a label's rendered width (cached) so crate chips size to their text, the
// way the old graphviz boxes did (auto-width, full crate name).
const _fanTextW = new Map();
function fanMeasure(svg, s) {
  if (_fanTextW.has(s)) return _fanTextW.get(s);
  const t = svgEl('text', { 'font-size': 11, 'font-family': 'Helvetica', x: -9999, y: -9999 });
  t.textContent = s;
  svg.appendChild(t);
  let w = 0; try { w = t.getComputedTextLength(); } catch {}
  t.remove();
  if (!w) w = s.length * 6.6;
  _fanTextW.set(s, w);
  return w;
}
// Flow-lay a section's crate chips left→right, wrapping when the next chip would
// exceed availW (variable widths, like graphviz). Returns chip rects (x within the
// row, row index, width) + total flow height.
function chipFlow(svg, secData, availW) {
  const chips = [];
  const rowW = [];   // content width per row (for centring)
  let x = 0, row = 0;
  for (const c of secData) {
    const label = `${c.crate} (${c.count})`;
    const w = Math.max(FAN_GEO.BOXMINW, Math.ceil(fanMeasure(svg, label)) + FAN_GEO.BOXPADX);
    if (x > 0 && x + w > availW) { row++; x = 0; }
    chips.push({ c, label, x, row, w });
    rowW[row] = x + w;
    x += w + FAN_GEO.GAPX;
  }
  const rows = secData.length ? row + 1 : 0;
  return { chips, rows, rowW, h: rows * FAN_GEO.BOXH + Math.max(0, rows - 1) * FAN_GEO.GAPY };
}
// Reserved band height for a section's fully-EXPANDED flow — constant regardless of
// collapse state, so the band never resizes on toggle.
function fanReservedH(svg, secData, availW) {
  if (!secData.length) return 0;
  return FAN_GEO.PAD * 2 + FAN_GEO.LBLH + chipFlow(svg, secData, availW).h;
}
// A bigger framed +/− button (rect + symbol). `onClick` optional (the collapsed
// pill handles its own click, so its + needs none).
function fanBtn(sym, x, y, pal, onClick) {
  const g = svgEl('g', { class: 'fan-btn' });
  // Transparent hit area (no visible frame — just the symbol; a faint bg on hover).
  g.appendChild(svgEl('rect', { x, y, width: FAN_GEO.BTN, height: FAN_GEO.BTN, rx: 4, fill: 'transparent' }));
  const t = svgEl('text', { x: x + FAN_GEO.BTN / 2, y: y + FAN_GEO.BTN / 2 + 5, 'text-anchor': 'middle', 'font-size': 15, 'font-weight': 700, fill: pal.text, 'font-family': 'Helvetica' });
  t.textContent = sym;
  g.appendChild(t);
  if (onClick) g.addEventListener('click', e => { e.stopPropagation(); onClick(); });
  return g;
}

function composeFanSections(svgFrame, level) {
  const svg = svgFrame?.querySelector('svg');
  if (!svg) return;
  svg.querySelector('#fan-overlay')?.remove();
  const data = window._fanData || { in: [], out: [] };
  if (!data.in.length && !data.out.length) return;

  // Capture the bare-graph viewBox + node anchors and reserve the expanded bands
  // ONCE; toggles reuse them and never touch the viewBox (so pan/zoom is preserved).
  if (!svgFrame._fanBase) {
    const vb = (svg.getAttribute('viewBox') || '').split(/\s+/).map(Number);
    if (!(vb.length === 4 && vb.every(Number.isFinite))) return;
    const anchors = new Map();
    svg.querySelectorAll('g.node').forEach(g => {
      const id = g.querySelector('title')?.textContent?.trim();
      if (!id) return;
      const c = nodeCenterUser(svg, g);
      if (c) anchors.set(id, c);
    });
    const base = { x: vb[0], y: vb[1], w: vb[2], h: vb[3] };
    // Expanded sections are at least 250 wide; on a narrow graph they extend past it
    // (centred) so a lone short chip's box still fits. The viewBox widens to match.
    base.secW = Math.max(base.w, 250);
    base.secX = base.x + (base.w - base.secW) / 2;
    const availW = base.secW - FAN_GEO.PAD * 2;
    base.topH = fanReservedH(svg, data.in,  availW);
    base.botH = fanReservedH(svg, data.out, availW);
    svgFrame._fanBase = base; svgFrame._fanAnchors = anchors;
    svg.setAttribute('viewBox', `${base.secX} ${base.y - base.topH} ${base.secW} ${base.h + base.topH + base.botH}`);
  }
  const base = svgFrame._fanBase, anchors = svgFrame._fanAnchors;

  const overlay = svgEl('g', { id: 'fan-overlay' });
  const defs = svgEl('defs', {});
  for (const dir of ['in', 'out']) {
    const m = svgEl('marker', { id: `fan-ah-${dir}`, markerWidth: 7, markerHeight: 6, refX: 6, refY: 3, orient: 'auto' });
    m.appendChild(svgEl('path', { d: 'M0,0 L0,6 L7,3 z', fill: FAN_PAL[dir].stroke }));
    defs.appendChild(m);
  }
  overlay.appendChild(defs);
  const trunc = (s, n) => s.length > n ? s.slice(0, n - 1) + '…' : s;

  const buildSection = (dir, secData, bandTop, bandH) => {
    if (!secData.length) return;
    const pal = FAN_PAL[dir];
    const cx  = base.x + base.w / 2;
    const g   = svgEl('g', { class: `fan-section fan-${dir}` });
    const collapsed = secData.length > 1 && fanCollapsed(dir);
    // Shared +/− button position — top-right of the section, identical in both states.
    const btnX = base.secX + base.secW - FAN_GEO.PAD - FAN_GEO.BTN;
    const btnY = bandTop + FAN_GEO.PAD / 2 + (FAN_GEO.BOXH - FAN_GEO.BTN) / 2;
    // Shared Fan-in/out label position (centred in the top row) — same in both states
    // so the text doesn't jump on collapse/expand.
    const lblX = base.secX + base.secW / 2;
    const lblY = bandTop + FAN_GEO.PAD / 2 + FAN_GEO.BOXH / 2 + 4;

    // Arrows from anchor (ax,ay) to each coupled file's EDGE — hidden by default,
    // shown only while the section is hovered (CSS), like the old connectors.
    const drawArrows = (our, ax, ay, parent) => {
      for (const o of our) {
        const fa = anchors.get(o.fid);
        if (!fa) continue;
        const ep = nodeEdgePoint(fa, { x: ax, y: ay });
        const d  = dir === 'in' ? `M${ax},${ay} L${ep.x},${ep.y}` : `M${ep.x},${ep.y} L${ax},${ay}`;
        parent.appendChild(svgEl('path', {
          d, fill: 'none', stroke: pal.stroke, 'stroke-width': 1.2,
          'stroke-dasharray': o.flow ? null : '4,3',
          'marker-end': `url(#fan-ah-${dir})`,
          'data-fid': o.fid,
          class: `fan-arrow status-${o.status || 'unchanged'}`,
        }));
      }
    };

    if (collapsed) {
      // One pill "Fan-in N" + a framed + button; the whole pill expands on click.
      const pillH = FAN_GEO.BOXH, pillW = base.secW - 8;
      // Sit at the band's TOP (same top as the expanded section's background), so
      // expanding grows only downward and the top edge never shifts.
      const px = base.secX + 4, py = bandTop + FAN_GEO.PAD / 2;
      const agg = new Map();
      for (const c of secData) for (const o of c.our) {
        const a = agg.get(o.fid) || { fid: o.fid, flow: false, status: o.status };
        a.flow = a.flow || o.flow; agg.set(o.fid, a);
      }
      const pill = svgEl('g', { class: 'fan-pill' });
      drawArrows([...agg.values()], cx, dir === 'in' ? py + pillH : py, pill);
      pill.appendChild(svgEl('rect', { x: px, y: py, width: pillW, height: pillH, fill: pal.fill, stroke: pal.stroke }));
      const t = svgEl('text', { x: lblX, y: lblY, 'text-anchor': 'middle', 'font-size': 11, fill: pal.text, 'font-family': 'Helvetica' });
      t.textContent = `Fan-${dir} ${secData.length}`;
      pill.appendChild(t);
      pill.appendChild(fanBtn('+', btnX, btnY, pal));
      pill.addEventListener('click', e => { e.stopPropagation(); toggleFanSection(dir); });
      g.appendChild(pill);
    } else {
      const lbl = svgEl('text', { x: lblX, y: lblY, 'text-anchor': 'middle', 'font-size': 11, fill: pal.text, 'font-family': 'Helvetica' });
      lbl.textContent = `Fan-${dir}`;
      g.appendChild(lbl);
      if (secData.length > 1)
        g.appendChild(fanBtn('−', btnX, btnY, pal, () => toggleFanSection(dir)));

      const startX  = base.secX + FAN_GEO.PAD;
      const gridTop = bandTop + FAN_GEO.PAD + FAN_GEO.LBLH;
      const flow    = chipFlow(svg, secData, base.secW - FAN_GEO.PAD * 2);
      const contentH = FAN_GEO.LBLH + flow.h;
      // Tinted full-width section background (behind label/boxes/arrows). Hovering
      // its exposed area (not a crate, which sits on top) reveals ALL the section's
      // arrows; the crate boxes still reveal only their own on hover.
      const bg = svgEl('rect', { class: 'fan-bg', x: base.secX + 4, y: bandTop + FAN_GEO.PAD / 2, width: base.secW - 8, height: contentH + FAN_GEO.PAD, fill: pal.fill, stroke: pal.stroke });
      bg.addEventListener('mouseenter', () => g.classList.add('fan-show-all'));
      bg.addEventListener('mouseleave', () => g.classList.remove('fan-show-all'));
      g.insertBefore(bg, g.firstChild);
      const availW = base.secW - FAN_GEO.PAD * 2;
      for (const chip of flow.chips) {
        const c = chip.c;
        const bx = startX + (availW - flow.rowW[chip.row]) / 2 + chip.x;
        const by = gridTop + chip.row * (FAN_GEO.BOXH + FAN_GEO.GAPY);
        const box = svgEl('g', { class: `fan-crate status-${c.status}` });
        box.appendChild(svgEl('rect', { x: bx, y: by, width: chip.w, height: FAN_GEO.BOXH, fill: pal.fill, stroke: pal.stroke, 'stroke-dasharray': c.count === 0 ? '4,3' : null }));
        const ct = svgEl('text', { x: bx + chip.w / 2, y: by + FAN_GEO.BOXH / 2 + 4, 'text-anchor': 'middle', 'font-size': 11, fill: pal.text, 'font-family': 'Helvetica' });
        ct.textContent = chip.label;
        box.appendChild(ct);
        // Clicking a crate box drills into that crate's folder (as the old boxes did).
        box.addEventListener('click', e => {
          e.stopPropagation();
          const t = crateFocusTarget(level, c.crate);
          drillIntoGroup(t.key, level, t.dig);
        });
        drawArrows(c.our, bx + chip.w / 2, dir === 'in' ? by + FAN_GEO.BOXH : by, box);
        g.appendChild(box);
      }
    }
    overlay.appendChild(g);
  };

  buildSection('in',  data.in,  base.y - base.topH, base.topH);
  buildSection('out', data.out, base.y + base.h,    base.botH);
  svg.appendChild(overlay);
}
window.composeFanSections = composeFanSections;

// Collapse/expand a section — re-runs ONLY the overlay (no graphviz, no viewBox
// change), so the graph and the pan/zoom stay put; only the section + its arrows move.
function toggleFanSection(dir) {
  const st = window._fanCollapsed || (window._fanCollapsed = { in: true, out: true });
  st[dir] = !st[dir];
  if (window._fanFrame) composeFanSections(window._fanFrame, window._fanFrame.dataset.fanLevel);
}
window.toggleFanSection = toggleFanSection;

// Hovering a file node reveals the Fan-in/out arrows that attach to it (matched by
// the arrow's data-fid = the file's render-id).
function fanHighlightFile(on, fid) {
  const ov = window._fanFrame?.querySelector('#fan-overlay');
  if (!ov) return;
  // Always clear the previous file/box highlight first, so a missed `mouseleave`
  // (fast pointer movement between nodes) can never leave stale arrows lit — the
  // next `mouseenter` resets the set. Mirrors the internal edge-highlight, which
  // also clears everything before applying.
  ov.querySelectorAll('.fan-arrow.fan-arrow-on').forEach(a => a.classList.remove('fan-arrow-on'));
  if (!on || !fid) return;
  const ids = Array.isArray(fid) ? new Set(fid) : null;
  ov.querySelectorAll('.fan-arrow').forEach(a => {
    const f = a.getAttribute('data-fid');
    if (ids ? ids.has(f) : f === fid) a.classList.add('fan-arrow-on');
  });
}
window.fanHighlightFile = fanHighlightFile;

// Drill target (group key + dig) for the folder a node sits in directly — its
// depth on the active tier's ladder (`underDepthOf`), so a folder/dir-cluster
// drills into itself.
function focusFolderTarget(level, n) {
  const dig = underDepthOf(level, n);
  return { key: groupKeyAtDig(level, n, dig), dig };
}

// Drill target (key + dig) for a neighbour **crate** box (callers/dependencies) —
// the crate's folder in the current tier: the crate itself at the crate tier, its
// source directory at the file tier.
function crateFocusTarget(level, crate) {
  if (window.viewTier(level) === 'file') {
    const key = crateKeyToFileKey(level, crate);
    if (key && key !== '_root') return { key, dig: digOfKeyForTier(level, key, 'file') };
  }
  return { key: crate, dig: 0 };
}

// Clamp a focus-dig (collapse level inside a focused group): 0 = individual files,
// down to -(folder depth below the focus) where only top-level folders remain.
function clampFocusDig(z) {
  const maxFocusD = window._FOCUS?.maxFocusD ?? 0;
  const baseDig   = window.drillDig ?? 0;
  return Math.max(-Math.max(0, maxFocusD - baseDig), Math.min(0, z | 0));
}

// Reveal-depth step from the lens chip. In the overview `delta` (+1 reveal / -1
// collapse) steps the crate/folder grouping (`window.dig`); while focused it steps
// `window.focusDig` — collapsing the focus's files into folder boxes (−) or
// expanding back to individual files (+). See grouping.js.
function setDig(delta, level) {
  level = level || currentLevel();
  if (window.drillGroup !== null) {
    const fz = clampFocusDig((window.focusDig || 0) + delta);
    if (fz === (window.focusDig || 0)) return;
    window.focusDig = fz;
  } else {
    const z = clampDig((window.dig || 0) + delta);
    if (z === (window.dig || 0)) return;
    window.dig = z;
  }
  renderBreadcrumb(level);
  window.navReplaceView?.();
  document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
  const active = document.querySelector('.view.active');
  if (active && window.gv) renderView(active, { preserve: false });
}
window.setDig = setDig;

// Back-compat alias: callers (view-state recompute / restore) sync the dig UI via
// this name; the control now lives entirely in the breadcrumb.
function updateDigLabel(level) { renderBreadcrumb(level || currentLevel()); }
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
    const dir = nodeDirSegs(n.id);
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
  // `_root` is the collapse sentinel (no path segments) — show it as "/".
  const parts = [stats.name === '_root' ? '/' : stats.name];
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

// Hover smoothing + paint order ───────────────────────────────────────────────
// SVG has no z-index, so a hovered node's glow would be painted under its later
// siblings. Move it to the end of its parent ONCE on first hover (never restored
// — paint order doesn't affect layout, so leaving it on top is harmless).
function raisePaint(el) {
  if (el && !el._raised) { el.parentNode?.appendChild(el); el._raised = true; }
}

const HOVER_DELAY = 70;   // ms before a hover effect applies — avoids flicker on quick passes

// Wire a node's hover with the glow class + paint raise, debounced so dragging
// the cursor across many nodes doesn't flash. `onEnter` runs once when settled;
// `onLeave` always runs (its clears are safe even if `onEnter` never fired).
function wireNodeHover(el, onEnter, onLeave) {
  let timer = null, active = false;
  el.addEventListener('mouseenter', () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null; active = true;
      // Always drop any prior highlight first — a missed mouseleave (fast move,
      // or a paint-raise reparent) must never leave two nodes glowing at once.
      (el.ownerSVGElement || el.closest('svg'))
        ?.querySelectorAll('.node-hl').forEach(n => { if (n !== el) n.classList.remove('node-hl'); });
      raisePaint(el);
      el.classList.add('node-hl');
      onEnter?.();
    }, HOVER_DELAY);
  });
  el.addEventListener('mouseleave', e => {
    if (timer) { clearTimeout(timer); timer = null; }
    if (active) { active = false; el.classList.remove('node-hl'); }
    onLeave?.(e);
  });
}

// Build edge-highlight behaviour: on node/cluster hover dim unrelated edges and
// show connected ones; if IN/OUT cluster edges exceed 10, hide them until the
// cluster zone is hovered. Must be called BEFORE setupTooltips (reads titles).
function setupEdgeHighlight(svgFrame, level) {
  const allEdgeEls = [...svgFrame.querySelectorAll('g.edge')];
  const allNodeEls = [...svgFrame.querySelectorAll('g.node')];
  if (allEdgeEls.length === 0) return;
  // Node lookup so a dir sub-cluster's edges can be matched by the same
  // focus-relative dir label that layout.js prints (the focus path is subtracted —
  // see focusDirPath/stripDirPrefix).
  const nodeById = new Map((typeof unionGraph === 'function' ? unionGraph(level).nodes : []).map(n => [n.id, n]));
  const focusBase = window.focusStripBase?.(level) ?? '';
  const nodeRelDir = n => stripDirPrefix(focusBase, nodeFullDir(n));

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
  // `isLeaf` = hovering a single leaf node (an individual file, or a collapsed
  // folder/group box with no rendered children) — as opposed to a directory
  // sub-cluster that already shows its files. Only a leaf hover reveals the dashed
  // non-flow edges (gated on `.leaf-hovered` in CSS); hovering a cluster of visible
  // files does not.
  const applyHighlight = (connected, isLeaf = false) => {
    svgFrame.classList.add('node-hovered');
    svgFrame.classList.toggle('leaf-hovered', !!isLeaf);
    for (const e of allEdgeEls) {
      e.classList.remove('edge-connected', 'edge-dim');
      if (connected.has(e)) e.classList.add('edge-connected');
      else                   e.classList.add('edge-dim');
    }
  };
  const clearHighlight = () => {
    svgFrame.classList.remove('node-hovered', 'leaf-hovered');
    for (const e of allEdgeEls) e.classList.remove('edge-connected', 'edge-dim');
  };
  // Reveal the (default-hidden) green/orange caller/dependency connector edges.
  const setShowInOut = (showIn, showOut) => {
    svgFrame.classList.toggle('show-in-edges', !!showIn);
    svgFrame.classList.toggle('show-out-edges', !!showOut);
  };

  // ONE shared debounce timer for EVERY edge-highlight change — nodes AND clusters.
  // A hover that supersedes a pending one cancels it, so crossing node/cluster
  // boundaries never flashes the arrows back to "all visible".
  let ehTimer = null;
  const ehSchedule = fn => {
    if (ehTimer) clearTimeout(ehTimer);
    ehTimer = setTimeout(() => { ehTimer = null; fn(); }, HOVER_DELAY);
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

    let edges, nc, memberIds = null;
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
      // The crate container is EXPANDED (its folders/files are visible), so a click
      // on its background does nothing; only its name/path label drills into the
      // whole crate — crate-tier grouper, so the focus shows all its files.
      const crateLabelEl = clusterEl.querySelector('text');
      if (crateLabelEl) crateLabelEl.style.cursor = 'pointer';
      clusterEl.addEventListener('click', e => {
        if (e.target.closest('g.node')) return;   // a folder box handles its own click
        if (!e.target.closest('text')) return;    // only the path label navigates
        e.stopPropagation();
        drillIntoGroup(label, level, 0);
      });
    } else {
      // Directory sub-cluster: label is the focus-relative dir layout.js prints
      // (the focus path subtracted), so match against the same relative form.
      const matchIds = [...edgeMap.keys()].filter(k => {
        const node = nodeById.get(k);
        return node ? nodeRelDir(node) === label : false;
      });
      edges = new Set();
      for (const id of matchIds) {
        for (const e of (edgeMap.get(id) ?? new Set())) edges.add(e);
      }
      nc = matchIds.length;
      memberIds = matchIds;
      // The folder cluster is EXPANDED (its files are visible), so a click on its
      // background does nothing; only its path label drills into it. Find a
      // representative node by the folder's full dir (robust regardless of whether
      // graphviz nests the member nodes in the cluster <g>, and even when the
      // folder's files have no edges) — the old `querySelector('g.node title')`
      // returned null in those cases, leaving the folder unclickable.
      const sample = [...nodeById.values()].find(n => nodeRelDir(n) === label);
      if (sample) {
        const tgt = focusFolderTarget(level, sample);
        const dirLabelEl = clusterEl.querySelector('text');
        if (dirLabelEl) dirLabelEl.style.cursor = 'pointer';
        clusterEl.addEventListener('click', e => {
          if (e.target.closest('g.node')) return;   // a file handles its own click
          if (!e.target.closest('text')) return;    // only the path label navigates
          e.stopPropagation();
          drillIntoGroup(tgt.key, level, tgt.dig);
        });
      }
    }

    const ec = edges.size;
    const statusText = [label,
      nc ? `${nc} node${nc !== 1 ? 's' : ''}` : '',
      ec ? `${ec} edge${ec !== 1 ? 's' : ''}` : '',
    ].filter(Boolean).join('  ·  ');
    const isIn = cTitle === 'cluster_in', isOut = cTitle === 'cluster_out';
    clusterData.set(clusterEl, { edges, statusText, isIn, isOut });

    clusterEl.addEventListener('mouseenter', () =>
      ehSchedule(() => { applyHighlight(edges); showSB(statusText); setShowInOut(isIn, isOut); }));
    clusterEl.addEventListener('mouseleave', () =>
      ehSchedule(() => { clearHighlight(); hideSB(); setShowInOut(false, false); }));
  }

  // ── IN/OUT edges are always hidden by default; revealed on cluster/node hover ──
  // (The reveal itself is folded into the cluster's debounced hover handler above
  // via setShowInOut, so it stays in sync with the highlight.)
  inEdges.forEach(e  => e.classList.add('cluster-edge-hidden'));
  outEdges.forEach(e => e.classList.add('cluster-edge-hidden'));

  // ── Node hover ───────────────────────────────────────────────────────────────
  // Routed through the same shared `ehSchedule` debounce as clusters: leaving a
  // node schedules a clear, but entering the next node (or a cluster) cancels it
  // and schedules its own highlight — so the arrows never flash between targets.
  for (const nodeEl of allNodeEls) {
    const nodeId = nodeEl.querySelector('title')?.textContent?.trim();
    if (!nodeId) continue;

    nodeEl.addEventListener('mouseenter', () => {
      // Status bar is updated by setupTooltips handlers (fire after these). A node
      // is a leaf (file / collapsed box) → reveal its dashed non-flow edges.
      ehSchedule(() => { applyHighlight(edgeMap.get(nodeId) ?? new Set(), true); setShowInOut(false, false); });
    });

    nodeEl.addEventListener('mouseleave', e => {
      // Moving back onto a cluster background re-applies that cluster's full state
      // (highlight + in/out reveal); otherwise clear. All via the shared debounce.
      const destCluster = e.relatedTarget?.closest?.('g.cluster');
      const cd = destCluster ? clusterData.get(destCluster) : null;
      if (cd) ehSchedule(() => { applyHighlight(cd.edges); showSB(cd.statusText); setShowInOut(cd.isIn, cd.isOut); });
      else    ehSchedule(() => { clearHighlight(); setShowInOut(false, false); });
    });
  }
}

function setupTooltips(svgFrame, level) {
  svgFrame.querySelectorAll('g.edge title, g.cluster title').forEach(t => t.remove());

  const drillGroup = window.drillGroup || null;
  const section    = svgFrame.closest('.view');
  const gNodeMap   = new Map();
  // Maps a Details aggregate row's highlight key (`group:<crate>` / `folder:<dir>`)
  // to its on-map SVG element, so hovering/selecting one lights up the other.
  const gAggMap    = new Map();
  const aggRow = key => section?.querySelector(`tr[data-agg-key="${(window.CSS?.escape ? CSS.escape(key) : key)}"]`);

  const sb = svgFrame._statusBar;
  const showStatus = text => { if (sb) { sb.textContent = text; sb.hidden = false; } };
  const hideStatus = ()   => { if (sb) { sb.hidden = true; sb.textContent = ''; } };

  if (drillGroup !== null) {
    // ── Drilled file view: wire up individual file nodes ─────────────────────────
    // Map EVERY union node so baseline-only / current-only nodes get handlers too.
    const nodeMap = new Map(unionGraph(level).nodes.map(n => [n.id, n]));
    // Neighbour boxes are keyed by the OTHER end's crate (same as layout.js) —
    // aggregate per-crate stats so a hover shows crate-style details.
    const drillG = grouperForDig(level, window.drillDig ?? 0);
    const neighbourStats = computeGroupStats(level, n => crateIdOf(level, n) ?? drillG(n));
    // Focus folder mode: the rendered boxes are folder groups (not files) keyed by
    // the focus-dig grouper — stats + drill-in keyed by the same depth.
    const focusFolder = window._FOCUS?.folderMode ? window._FOCUS : null;
    const focusStats  = focusFolder ? computeGroupStats(level, grouperForDig(level, focusFolder.focusD)) : null;

    svgFrame.querySelectorAll('g.node').forEach(g => {
      const titleEl = g.querySelector('title');
      const nodeId  = titleEl?.textContent?.trim();
      titleEl?.remove();

      // External neighbor node (caller / dependency from another group)?
      const neighborPrefix = nodeId?.startsWith('IN\x01') ? 'IN\x01'
                           : nodeId?.startsWith('OUT\x01') ? 'OUT\x01' : null;
      if (neighborPrefix) {
        const neighborGroup = nodeId.slice(neighborPrefix.length);   // the neighbour crate
        const dir   = neighborPrefix === 'IN\x01' ? 'in' : 'out';
        const arrow = dir === 'in' ? '← ' : '→ ';
        // Collapsed-section summary box (`FAN_ALL` marker, '\x02') → expand on click.
        if (neighborGroup === '\x02') {
          g.addEventListener('click', e => { e.stopPropagation(); toggleFanSection(dir); });
          wireNodeHover(g,
            () => showStatus(`${arrow}Fan-${dir} — click to expand`),
            e => { if (!e.relatedTarget?.closest?.('g.cluster')) hideStatus(); });
          return;
        }
        g.addEventListener('click', e => {
          e.stopPropagation();
          const t = crateFocusTarget(level, neighborGroup);
          drillIntoGroup(t.key, level, t.dig);
        });
        wireNodeHover(g,
          () => {
            const st = neighbourStats.get(neighborGroup);
            showStatus(st ? statusLineForGroup({ ...st, name: arrow + st.name })
                          : arrow + neighborGroup);
          },
          e => { if (!e.relatedTarget?.closest?.('g.cluster')) hideStatus(); });
        return;
      }

      // Focus folder box (collapsed files): clicking drills into that folder.
      if (focusFolder && !nodeMap.has(nodeId)) {
        g.addEventListener('click', e => {
          e.stopPropagation();
          drillIntoGroup(nodeId, level, focusFolder.focusD);
        });
        wireNodeHover(g,
          () => { const st = focusStats?.get(nodeId); showStatus(st ? statusLineForGroup(st) : nodeId); window.fanHighlightFile?.(true, nodeId); },
          e => { window.fanHighlightFile?.(false, nodeId); if (!e.relatedTarget?.closest?.('g.cluster')) hideStatus(); });
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

      wireNodeHover(g,
        () => {
          section?.querySelector(`tr[data-node-id="${nodeId.replace(/\\/g,'\\\\').replace(/"/g,'\\"')}"]`)
                  ?.classList.add('row-hl');
          showStatus(statusLineFor(node, level));
          window.fanHighlightFile?.(true, nodeId);
        },
        e => {
          section?.querySelector(`tr[data-node-id="${nodeId.replace(/\\/g,'\\\\').replace(/"/g,'\\"')}"]`)
                  ?.classList.remove('row-hl');
          window.fanHighlightFile?.(false, nodeId);
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

      // Sync with the Details table's group (crate) aggregate row, when grouping by
      // the crate tier (dig 0) — its row key is `group:<crate>`.
      const aggKey = (window.dig || 0) === 0 ? 'group:' + groupId : null;
      if (aggKey) {
        gAggMap.set(aggKey, g);
        if (section?.querySelector(`tr[data-agg-key="${(window.CSS?.escape ? CSS.escape(aggKey) : aggKey)}"].row-selected`))
          g.classList.add('node-selected');
      }

      g.addEventListener('click', e => {
        e.stopPropagation();
        drillIntoGroup(groupId, level);
      });
      wireNodeHover(g,
        () => { if (aggKey) aggRow(aggKey)?.classList.add('row-hl'); showStatus(statusLineForGroup(stats)); },
        e => { if (aggKey) aggRow(aggKey)?.classList.remove('row-hl'); if (!e.relatedTarget?.closest?.('g.cluster')) hideStatus(); });
    });
  }

  if (section) { section._gNodeMap = gNodeMap; section._gAggMap = gAggMap; }
}

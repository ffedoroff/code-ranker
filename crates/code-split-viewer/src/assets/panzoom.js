function setupPanZoom(frame, svg) {
  const vbAttr = svg.getAttribute('viewBox');
  if (!vbAttr) return;
  const [ox, oy, ow, oh] = vbAttr.split(/[ ,]+/).map(Number);
  // The fit-all viewBox for this render — renderView compares against it to decide
  // whether the user has panned/zoomed (and thus whether to preserve on re-render).
  frame.dataset.naturalVB = `${ox} ${oy} ${ow} ${oh}`;
  let pan = null, didDrag = false, animFrame = null;

  function getVB() { return svg.getAttribute('viewBox').split(/[ ,]+/).map(Number); }
  let lastVBW = ow;
  function setVB(x, y, w, h) {
    svg.setAttribute('viewBox', `${x} ${y} ${w} ${h}`);
    // Zoom changes the fit factor → re-normalize arrowheads + hover-halo scale.
    // (Pan keeps width, so this is skipped during drags.)
    if (Math.abs(w - lastVBW) > 0.5) { lastVBW = w; window.normalizeArrows?.(frame, svg); }
  }

  function animate(tx, ty, tw, th, ms) {
    if (animFrame) cancelAnimationFrame(animFrame);
    const [sx, sy, sw, sh] = getVB();
    const t0 = performance.now();
    (function step(now) {
      const t = Math.min(1, (now - t0) / ms);
      const e = 1 - Math.pow(1 - t, 3);
      setVB(sx+(tx-sx)*e, sy+(ty-sy)*e, sw+(tw-sw)*e, sh+(th-sh)*e);
      animFrame = t < 1 ? requestAnimationFrame(step) : null;
    })(t0);
  }

  function zoomOut()     { animate(ox, oy, ow, oh, 250); frame.classList.remove('zoomed', 'panning'); }
  function zoomInCenter() {
    const [vx, vy, vw, vh] = getVB();
    const nw = vw * 0.667, nh = vh * 0.667;
    animate(vx + (vw - nw) / 2, vy + (vh - nh) / 2, nw, nh, 200);
    frame.classList.add('zoomed');
  }
  function zoomOutStep() {
    const [vx, vy, vw, vh] = getVB();
    const nw = Math.min(ow * 4, vw * 1.5), nh = Math.min(oh * 4, vh * 1.5);
    animate(vx + (vw - nw) / 2, vy + (vh - nh) / 2, nw, nh, 200);
    frame.classList.toggle('zoomed', Math.abs(nw - ow) > 1);
  }

  // ── Drag-to-pan ─────────────────────────────────────────────────────────────
  function onDragMove(e) {
    if (!pan) return;
    const dx = e.clientX - pan.x, dy = e.clientY - pan.y;
    if (!didDrag && (Math.abs(dx) > 3 || Math.abs(dy) > 3)) {
      didDrag = true;
      frame.classList.add('panning');
    }
    if (didDrag) {
      const ctm = svg.getScreenCTM();
      if (ctm && ctm.a !== 0 && ctm.d !== 0)
        setVB(pan.vx - dx / ctm.a, pan.vy - dy / ctm.d, pan.vw, pan.vh);
    }
  }

  function onDragEnd() {
    if (!pan) return;
    pan = null;
    frame.classList.remove('panning');
    document.removeEventListener('mousemove', onDragMove);
    document.removeEventListener('mouseup',   onDragEnd);
    window.removeEventListener('blur',        onDragEnd);
  }

  svg.addEventListener('dblclick', e => {
    e.preventDefault();
    const [vx, vy, vw, vh] = getVB();
    const ctm = svg.getScreenCTM();
    if (!ctm || ctm.a === 0) { zoomInCenter(); return; }
    const cx = (e.clientX - ctm.e) / ctm.a;
    const cy = (e.clientY - ctm.f) / ctm.d;
    const nw = vw / 2, nh = vh / 2;
    animate(cx - nw / 2, cy - nh / 2, nw, nh, 200);
    frame.classList.add('zoomed');
  });

  svg.addEventListener('mousedown', e => {
    e.preventDefault();
    if (animFrame) { cancelAnimationFrame(animFrame); animFrame = null; }
    didDrag = false;
    const [vx, vy, vw, vh] = getVB();
    pan = { x: e.clientX, y: e.clientY, vx, vy, vw, vh };
    document.addEventListener('mousemove', onDragMove);
    document.addEventListener('mouseup',   onDragEnd);
    window.addEventListener('blur',        onDragEnd);
  });

  // ── Zoom buttons ─────────────────────────────────────────────────────────────
  const wrap = frame.parentElement;

  // Store fresh zoom closures on frame so they pick up the new svg/viewBox
  // each render while the click listeners on wrap are registered only once.
  frame._zoomIn  = zoomInCenter;
  frame._zoomOut = zoomOutStep;
  frame._zoomFit = zoomOut;

  if (wrap && !wrap.dataset.pzInit) {
    wrap.dataset.pzInit = '1';

    wrap.querySelector('[data-zoom="in"]' )?.addEventListener('click', () => frame._zoomIn?.());
    wrap.querySelector('[data-zoom="out"]')?.addEventListener('click', () => frame._zoomOut?.());
    wrap.querySelector('[data-zoom="fit"]')?.addEventListener('click', () => frame._zoomFit?.());
    wrap.querySelector('[data-zoom="fullscreen"]')?.addEventListener('click', () => {
      if (!document.fullscreenElement) wrap.requestFullscreen?.();
      else document.exitFullscreen?.();
    });

    wrap.addEventListener('mousemove', e => {
      const r = wrap.getBoundingClientRect();
      const sc = wrap.querySelector('.size-controls');
      const zoneW = sc ? sc.offsetWidth + 24 : 248;
      wrap.classList.toggle('show-zoom', e.clientX >= r.right - zoneW);
    });
    wrap.addEventListener('mouseleave', () => wrap.classList.remove('show-zoom'));

    // Metric row: ■ (dot=null) | SLOC (loc) | HK (hk).
    // Clicking the active SLOC/HK deselects back to ■ (null).
    const modeFor = size => (size === 'dot' ? null : size);
    wrap.querySelectorAll('.size-row[data-row="metric"] .size-mode-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        const clicked  = modeFor(btn.dataset.size);
        const newMode  = (window.nodeSizeMode === clicked && clicked !== null) ? null : clicked;
        window.nodeSizeMode = newMode;
        btn.closest('.size-row').querySelectorAll('.size-mode-btn').forEach(b =>
          b.classList.toggle('active', modeFor(b.dataset.size) === newMode));
        window.navReplaceView?.();
        document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
        const active = document.querySelector('.view.active');
        if (active && window.gv) renderView(active, { preserve: true });
      });
    });

    // Cycle filter toggle: show only nodes in dependency cycles (+ connections).
    wrap.querySelector('[data-filter="cycle"]')?.addEventListener('click', e => {
      window.cycleOnly = !window.cycleOnly;
      e.currentTarget.classList.toggle('active', window.cycleOnly);
      document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
      const active = document.querySelector('.view.active');
      if (active && window.gv) renderView(active, { preserve: false });
    });

    // Drill back button: return from file view to group view.
    wrap.querySelector('[data-drill="back"]')?.addEventListener('click', () => {
      const lv = wrap.closest('.view')?.dataset.view || 'files';
      drillOutOfGroup(lv);
    });

    // Relative-zoom (level-of-detail) buttons: + finer / − coarser grouping.
    wrap.querySelector('.dig-lod [data-lod="in"]')?.addEventListener('click', () => {
      window.setDig?.(1, wrap.closest('.view')?.dataset.view || 'files');
    });
    wrap.querySelector('.dig-lod [data-lod="out"]')?.addEventListener('click', () => {
      window.setDig?.(-1, wrap.closest('.view')?.dataset.view || 'files');
    });

    document.addEventListener('fullscreenchange', () => {
      if (document.fullscreenElement === wrap) enterFS();
      else if (fsBarEl) exitFS();
    });
  }

  // ── Fullscreen overlay ────────────────────────────────────────────────────────
  // In fullscreen only `wrap` (the frame) is visible, so the page `<header>` and
  // the body-attached overlays (node modal, snapshot popup, metric tooltip) are
  // moved under `wrap` for the duration and restored on exit. The header rides a
  // slide-in `.fs-bar` revealed when the cursor nears the top edge.
  let fsBarEl = null, fsMoveHandler = null;
  let headerEl = null, headerParent = null, headerNext = null;
  let fsMoved = [];   // relocated overlays: { el, parent, next }

  const relocate = el => {
    if (!el) return;
    fsMoved.push({ el, parent: el.parentElement, next: el.nextSibling });
    wrap.appendChild(el);
  };

  function enterFS() {
    fsBarEl = document.createElement('div');
    fsBarEl.className = 'fs-bar';

    headerEl = document.querySelector('header');
    if (headerEl) {
      headerParent = headerEl.parentElement;
      headerNext = headerEl.nextSibling;
      fsBarEl.append(headerEl);
    }
    wrap.appendChild(fsBarEl);

    fsMoved = [];
    ['node-modal-overlay', 'snap-popup', 'tt'].forEach(id => relocate(document.getElementById(id)));

    fsMoveHandler = e => {
      const barH = fsBarEl.offsetHeight;
      const topShow = e.clientY < barH + 52 + 52;
      fsBarEl.classList.toggle('visible', topShow);
      const topPx = topShow ? (barH + 12) + 'px' : '';
      wrap.querySelector('.size-controls')?.style.setProperty('top', topPx || null);
      const r = wrap.getBoundingClientRect();
      const sc2 = wrap.querySelector('.size-controls');
      const zoneW2 = sc2 ? sc2.offsetWidth + 24 : 248;
      wrap.classList.toggle('show-zoom', topShow || e.clientX >= r.right - zoneW2);
    };
    document.addEventListener('mousemove', fsMoveHandler);
  }

  function exitFS() {
    if (fsMoveHandler) { document.removeEventListener('mousemove', fsMoveHandler); fsMoveHandler = null; }
    wrap.classList.remove('show-zoom');
    wrap.querySelector('.size-controls')?.style.removeProperty('top');
    if (headerEl && headerParent) headerParent.insertBefore(headerEl, headerNext);
    headerEl = null;
    fsMoved.forEach(({ el, parent, next }) => { if (parent) parent.insertBefore(el, next); });
    fsMoved = [];
    if (fsBarEl) { fsBarEl.remove(); fsBarEl = null; }
  }

}

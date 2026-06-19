function setupPanZoom(frame, svg) {
  const vbAttr = svg.getAttribute('viewBox');
  if (!vbAttr) return;
  const [ox, oy, ow, oh] = vbAttr.split(/[ ,]+/).map(Number);
  // The fit-all viewBox (set below from fitVB) is the framing renderView compares
  // against to decide whether the user has panned/zoomed (→ preserve on re-render).
  let pan = null, didDrag = false, animFrame = null;

  // Capped fit-all viewBox: the default framing never zooms IN past 1.3× absolute
  // (frame px per SVG unit). It also keeps the TOP strip free — the area the
  // breadcrumb occupies plus a little padding — so the diagram is fit/centred into
  // the space *below* the breadcrumb, never under it.
  const MAX_FIT_ZOOM = 1.3;
  // Screen px to keep clear at the top: the breadcrumb's bottom (relative to the
  // frame) + ~12px padding; ~50 as a fallback before it is laid out.
  function topReservePx() {
    const fr = frame.getBoundingClientRect();
    const bc = frame.parentElement?.querySelector('.drill-breadcrumb');
    if (bc && fr.height) {
      const r = (bc.getBoundingClientRect().bottom - fr.top) + 12;
      if (isFinite(r) && r > 0) return r;
    }
    return 50;
  }
  function fitVB() {
    const fw = frame.clientWidth || frame.offsetWidth || 0;
    const fh = frame.clientHeight || frame.offsetHeight || 0;
    if (!fw || !fh || !ow || !oh) return [ox, oy, ow, oh];
    const R = Math.min(topReservePx(), fh * 0.6);   // never eat more than 60% of the height
    const avail = Math.max(1, fh - R);
    // Fit the content into the area BELOW the reserve, capped at 1.3× absolute.
    const s = Math.min(fw / ow, avail / oh, MAX_FIT_ZOOM);
    // viewBox exactly fills the frame at scale s (aspect = frame → no letterbox), so
    // vx/vy place the content precisely: centred horizontally, centred in the area
    // below the reserve (the top R px left empty for the breadcrumb).
    const vw = fw / s, vh = fh / s;
    const vx = ox - (vw - ow) / 2;
    const contentTop = R + (avail - oh * s) / 2;
    const vy = oy - contentTop / s;
    return [vx, vy, vw, vh];
  }
  // Default framing for this fresh render = the capped fit. renderView's preserve
  // step overrides this afterwards when the user had zoomed/panned.
  { const [fx, fy, fw, fh] = fitVB();
    frame.dataset.naturalVB = `${fx} ${fy} ${fw} ${fh}`;
    svg.setAttribute('viewBox', `${fx} ${fy} ${fw} ${fh}`); }

  function getVB() { return svg.getAttribute('viewBox').split(/[ ,]+/).map(Number); }
  function setVB(x, y, w, h) { svg.setAttribute('viewBox', `${x} ${y} ${w} ${h}`); }

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

  function zoomOut()     { const [fx, fy, fw, fh] = fitVB(); animate(fx, fy, fw, fh, 250); frame.classList.remove('zoomed', 'panning'); }
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

    // The size-mode + filter buttons are built per render (renderMapControls)
    // from the level's `ui.size` / `ui.filter`, so handle clicks
    // by DELEGATION on the controls container (one listener, survives rebuilds).
    // Metric row: ■ (data-size="dot" → null) toggles back to box mode; any other
    //   data-size key is the attribute the circle area scales with. Filter row:
    //   data-filter key toggles the single active node filter.
    wrap.querySelector('.size-controls')?.addEventListener('click', e => {
      const btn = e.target.closest('.size-mode-btn');
      if (!btn) return;
      const rerenderMap = preserve => {
        window.navReplaceView?.();
        document.querySelectorAll('.view').forEach(sec => { sec.dataset.rendered = 'false'; });
        const active = document.querySelector('.view.active');
        if (active && window.gv) renderView(active, { preserve });
      };
      if (btn.dataset.size !== undefined) {
        const clicked = btn.dataset.size === 'dot' ? null : btn.dataset.size;
        // Re-clicking the active mode toggles back to box mode (■).
        window.nodeSizeMode =
          window.nodeSizeMode === clicked && clicked !== null ? null : clicked;
        rerenderMap(true);   // size change keeps pan/zoom
      } else if (btn.dataset.filter !== undefined) {
        const key = btn.dataset.filter;
        window.nodeFilter = window.nodeFilter === key ? null : key;
        rerenderMap(false);  // filter changes the node set → relayout
      }
    });

    // Drill back button: return from file view to group view.
    wrap.querySelector('[data-drill="back"]')?.addEventListener('click', () => {
      const lv = wrap.closest('.view')?.dataset.view || 'files';
      drillOutOfGroup(lv);
    });

    // The reveal-depth (level-of-detail) control now lives in the breadcrumb's
    // lens chip (see renderBreadcrumb), not a standalone panzoom button.

    document.addEventListener('fullscreenchange', () => {
      if (document.fullscreenElement === wrap) enterFS();
      else if (fsBarEl) exitFS();
    });
  }

  // ── Fullscreen overlay ────────────────────────────────────────────────────────
  // In fullscreen only `wrap` (the frame) is visible, so the page `<header>` and
  // the body-attached overlays (node modal, snapshot popup, metric tooltip) are
  // moved under `wrap` for the duration and restored on exit. The header sits in a
  // persistent `.fs-bar` at the top — always visible, no slide-in.
  let fsBarEl = null, fsMoveHandler = null;
  let headerEl = null, headerParent = null, headerNext = null;
  let fsMoved = [];   // relocated overlays: { el, parent, next }

  const relocate = el => {
    if (!el) return;
    fsMoved.push({ el, parent: el.parentElement, next: el.nextSibling });
    wrap.appendChild(el);
  };

  // Floating top controls that must clear the always-on header bar.
  const FS_TOP_CTRLS = ['.size-controls', '.drill-breadcrumb'];

  function enterFS() {
    // The header stays visible for the WHOLE fullscreen session (not just on a
    // top-edge hover) — `.fs-bar` is created already `.visible`.
    fsBarEl = document.createElement('div');
    fsBarEl.className = 'fs-bar visible';

    headerEl = document.querySelector('header');
    if (headerEl) {
      headerParent = headerEl.parentElement;
      headerNext = headerEl.nextSibling;
      fsBarEl.append(headerEl);
    }
    wrap.appendChild(fsBarEl);

    fsMoved = [];
    ['node-modal-overlay', 'snap-popup', 'tt'].forEach(id => relocate(document.getElementById(id)));

    // Offset the top-left/right floating controls below the persistent header.
    const offsetControls = () => {
      const top = (fsBarEl.offsetHeight + 12) + 'px';
      FS_TOP_CTRLS.forEach(sel => wrap.querySelector(sel)?.style.setProperty('top', top));
    };
    requestAnimationFrame(offsetControls);

    // Keep the zoom controls' right-edge reveal working; the header no longer toggles.
    fsMoveHandler = e => {
      const r = wrap.getBoundingClientRect();
      const sc2 = wrap.querySelector('.size-controls');
      const zoneW2 = sc2 ? sc2.offsetWidth + 24 : 248;
      wrap.classList.toggle('show-zoom', e.clientX >= r.right - zoneW2);
    };
    document.addEventListener('mousemove', fsMoveHandler);
  }

  function exitFS() {
    if (fsMoveHandler) { document.removeEventListener('mousemove', fsMoveHandler); fsMoveHandler = null; }
    wrap.classList.remove('show-zoom');
    FS_TOP_CTRLS.forEach(sel => wrap.querySelector(sel)?.style.removeProperty('top'));
    if (headerEl && headerParent) headerParent.insertBefore(headerEl, headerNext);
    headerEl = null;
    fsMoved.forEach(({ el, parent, next }) => { if (parent) parent.insertBefore(el, next); });
    fsMoved = [];
    if (fsBarEl) { fsBarEl.remove(); fsBarEl = null; }
  }

}

function fmtDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  if (isNaN(d)) return '';
  const p = n => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth()+1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function fmtDuration(ms) {
  const s = Math.floor(ms / 1000) % 60;
  const m = Math.floor(ms / 60000) % 60;
  const h = Math.floor(ms / 3600000);
  const parts = [];
  if (h) parts.push(`${h}h`);
  if (m || h) parts.push(`${m}m`);
  parts.push(`${s}s`);
  return parts.join(' ');
}

function fmtNum(n) {
  if (n === 0) return '0';
  // Abbreviated magnitudes are rounded to a whole number — the suffix already
  // signals "approximate", so a single decimal is false precision (1K, 2M, 5B).
  if (n >= 1e9) return Math.round(n / 1e9) + 'B';
  if (n >= 1e6) return Math.round(n / 1e6) + 'M';
  if (n >= 1e4) return Math.round(n / 1e3) + 'K';
  const sep = v => String(v).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  if (n >= 100) return sep(Math.round(n));
  if (n === Math.round(n)) return sep(n);
  return sep(parseFloat(n.toFixed(1)));
}

// Verbatim number: thousands-separated, NO rounding or abbreviation. Used where
// space is ample (the node popup's field table and central card), so every
// stored digit shows. Ints and floats alike; negatives handled.
function fmtFull(v) {
  if (v == null) return null;
  const s = String(v);
  const neg = s.startsWith('-');
  const body = neg ? s.slice(1) : s;
  const [int, dec] = body.includes('.') ? body.split('.') : [body, ''];
  const fi = int.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return (neg ? '-' : '') + (dec ? `${fi}.${dec}` : fi);
}

function fmtMs(ms) {
  if (ms < 1000)  return `${ms} ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`;
}

function escHtml(s) {
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}
function escAttr(s) {
  return escHtml(s).replace(/"/g,'&quot;');
}

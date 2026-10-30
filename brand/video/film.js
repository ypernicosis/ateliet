'use strict';
// ateliet — a 15 s film. Every frame is drawn as a function of time t (seconds).
const W = 1920, H = 1080, FPS = 60, DUR = 15, NF = FPS * DUR;
const cv = document.getElementById('c'), out = cv.getContext('2d');
const buf = document.createElement('canvas'); buf.width = W; buf.height = H;
let g = buf.getContext('2d');

// ── palette: paper, ink, non-photo blue. Nothing else. ──
const PAPER = [244, 242, 236], INK = [24, 24, 28], BLUE = [155, 211, 234];
const rgba = (c, a = 1) => `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a})`;
const mixc = (a, b, t) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
const ink = a => rgba(INK, a), pap = a => rgba(PAPER, a), blu = a => rgba(BLUE, a);

// ── math ──
const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const lerp = (a, b, t) => a + (b - a) * t;
const seg = (t, a, b) => clamp((t - a) / (b - a));
const E = {
  lin: x => x,
  inC: x => x * x * x,
  outC: x => 1 - (1 - x) ** 3,
  ioC: x => x < .5 ? 4 * x * x * x : 1 - (-2 * x + 2) ** 3 / 2,
  outQ: x => 1 - (1 - x) ** 5,
  inQ: x => x ** 5,
  ioQ: x => x < .5 ? 16 * x ** 5 : 1 - (-2 * x + 2) ** 5 / 2,
  outX: x => x >= 1 ? 1 : 1 - 2 ** (-10 * x),
  inX: x => x <= 0 ? 0 : 2 ** (10 * x - 10),
  ioX: x => x <= 0 ? 0 : x >= 1 ? 1 : x < .5 ? 2 ** (20 * x - 10) / 2 : (2 - 2 ** (-20 * x + 10)) / 2,
  outB: (x, s = 1.70158) => 1 + (s + 1) * (x - 1) ** 3 + s * (x - 1) ** 2,
  // damped spring: overshoots, then settles
  spring: (x, f = 1.6, d = 6.5) => x <= 0 ? 0 : x >= 1 ? 1 : 1 - Math.exp(-d * x) * Math.cos(f * Math.PI * 2 * x) * (1 - x) - 0 * x,
};
function mulberry32(a) { return () => { a |= 0; a = a + 0x6D2B79F5 | 0; let t = Math.imul(a ^ a >>> 15, 1 | a); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; }; }
const P = (x, y) => ({ x, y });
const lerpP = (a, b, t) => P(lerp(a.x, b.x, t), lerp(a.y, b.y, t));

// chained keys: each key starts from the value the previous one left
function track(t, v0, keys, log = false) {
  let v = log ? Math.log(v0) : v0;
  for (const [a, b, to, e] of keys) v = lerp(v, log ? Math.log(to) : to, (e || E.ioQ)(seg(t, a, b)));
  return log ? Math.exp(v) : v;
}

// clips a polygon by a half-plane (Sutherland–Hodgman); keeps f(p) >= 0
function clipPoly(poly, f) {
  const res = [];
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i], b = poly[(i + 1) % poly.length], fa = f(a), fb = f(b);
    if (fa >= 0) res.push(a);
    if ((fa >= 0) !== (fb >= 0)) { const k = fa / (fa - fb); res.push(lerpP(a, b, k)); }
  }
  return res;
}
function lineDist(A, B) { // signed distance to line AB, normal = left perpendicular
  const dx = B.x - A.x, dy = B.y - A.y, L = Math.hypot(dx, dy), nx = -dy / L, ny = dx / L;
  return p => (p.x - A.x) * nx + (p.y - A.y) * ny;
}
function hull(pts) {
  const p = pts.slice().sort((a, b) => a.x - b.x || a.y - b.y), cr = (o, a, b) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const lo = [], up = [];
  for (const q of p) { while (lo.length >= 2 && cr(lo[lo.length - 2], lo[lo.length - 1], q) <= 0) lo.pop(); lo.push(q); }
  for (const q of p.reverse()) { while (up.length >= 2 && cr(up[up.length - 2], up[up.length - 1], q) <= 0) up.pop(); up.push(q); }
  return lo.slice(0, -1).concat(up.slice(0, -1));
}
function pathPoly(ctx, poly) { ctx.beginPath(); poly.forEach((p, i) => i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)); ctx.closePath(); }
function rrect(ctx, x, y, w, h, r) { ctx.beginPath(); ctx.roundRect(x, y, Math.max(0, w), Math.max(0, h), Math.min(r, Math.abs(w) / 2, Math.abs(h) / 2)); }
function bbox(poly) { const xs = poly.map(p => p.x), ys = poly.map(p => p.y); return { x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) }; }
// orders 4 vertices: top-left, top-right, bottom-right, bottom-left
function quadOrder(poly) {
  const c = poly.reduce((s, p) => P(s.x + p.x / poly.length, s.y + p.y / poly.length), P(0, 0));
  const q = poly.slice().sort((a, b) => Math.atan2(a.y - c.y, a.x - c.x) - Math.atan2(b.y - c.y, b.x - c.x));
  let k = 0, best = 1e9; q.forEach((p, i) => { const s = p.x + p.y; if (s < best) { best = s; k = i; } });
  return q.slice(k).concat(q.slice(0, k));
}

// ═════════════════════════ fixed layout ═════════════════════════
const FONT_SENT = '400 64px NR';
const ZOOM_A = 4.4;
const SRC = 'toda obra começa numa [[nota]].';
let SENT; // sentence geometry, in px of the 64 px font
const PAGE = { cx: 960, cy: 540, w: 420, h: 594 }; // book page (scene space)
const PG = { x0: 750, x1: 1170, y0: 243, y1: 837 };
const TITLE_BL = 329;
let PARA_L, PARA_R;
const MANGA = { w: 500, h: 708 };
let MG; // manga page geometry
let TILES, MATCH_ORDER, PANEL_TILE;
let GRAIN;

function layout() {
  g.font = FONT_SENT;
  const chars = [...SRC];
  const isBr = chars.map(c => c === '[' || c === ']');
  const plain = chars.filter((c, i) => !isBr[i]).join('');
  const brW = g.measureText('[').width;
  const xs = [], adv = [];
  let nb = '', brBefore = 0;
  for (let i = 0; i < chars.length; i++) {
    xs.push({ nb: g.measureText(nb).width, br: brBefore });
    if (isBr[i]) { brBefore += brW; adv.push(brW); }
    else { const w0 = g.measureText(nb).width; nb += chars[i]; adv.push(g.measureText(nb).width - w0); }
  }
  const finalW = g.measureText(plain).width;
  // typing times, at a human pace
  const r = mulberry32(7); const tt = []; let tc = 0.42;
  for (let i = 0; i < chars.length; i++) { tt.push(tc); tc += 0.03 + r() * 0.014 + (chars[i] === ' ' ? 0.018 : 0); }
  // words (no brackets, no period)
  const words = []; let cur = null;
  chars.forEach((c, i) => {
    if (/[a-zçã]/i.test(c)) { if (!cur) { cur = { i0: i, i1: i }; words.push(cur); } cur.i1 = i; }
    else if (!isBr[i]) cur = null;
  });
  SENT = { chars, isBr, xs, adv, finalW, tt, words, brW, tEnd: tc };

  // paragraphs (justified bars)
  const mkPara = (seed, x0, x1, y0, y1, paras, firstIndent) => {
    const rr = mulberry32(seed), lines = []; const lh = 17; let y = y0, pi = 0, li = 0;
    while (y <= y1) {
      const nL = paras[pi % paras.length];
      const last = li === nL - 1, indent = (li === 0 && (pi > 0 || firstIndent)) ? 18 : 0;
      const target = last ? (x1 - x0 - indent) * (0.35 + rr() * 0.4) : (x1 - x0 - indent);
      const ws = []; let sum = 0;
      while (true) { const w = 10 + rr() * 46; if (sum + w + (ws.length ? 5 : 0) > target && ws.length) break; ws.push(w); sum += w + (ws.length > 1 ? 5 : 0); }
      const gap = (!last && ws.length > 1) ? (target - ws.reduce((a, b) => a + b, 0)) / (ws.length - 1) : 5;
      let x = x0 + indent; const row = [];
      for (const w of ws) { row.push({ x, w }); x += w + gap; }
      lines.push({ y, words: row });
      y += lh; li++; if (last) { li = 0; pi++; }
    }
    return lines;
  };
  PARA_L = mkPara(11, 806, 1114, 375, 751, [6, 8, 5, 7], false);
  PARA_R = mkPara(23, 1226, 1534, 291, 751, [7, 5, 9, 6], true);

  // ── manga: page, inner frame and cuts ──
  const mx0 = 960 - MANGA.w / 2, my0 = 540 - MANGA.h / 2;
  const fr = { x0: mx0 + 46, x1: mx0 + MANGA.w - 46, y0: my0 + 52, y1: my0 + MANGA.h - 70 };
  const frame = [P(fr.x0, fr.y0), P(fr.x1, fr.y0), P(fr.x1, fr.y1), P(fr.x0, fr.y1)];
  const A1 = P(fr.x0 - 70, 478), B1 = P(fr.x1 + 70, 452);
  const A2 = P(1000, 470), B2 = P(924, fr.y1 + 70);
  MG = { mx0, my0, fr, frame, A1, B1, A2, B2, d1: lineDist(A1, B1), d2: lineDist(A2, B2) };
  const polys = panelPolys(18, 14);
  MG.final = polys.map(quadOrder);
  MG.fbox = MG.final.map(bbox);
  // focus lines of the right panel
  const rf = mulberry32(5); MG.speed = [];
  for (let i = 0; i < 64; i++) MG.speed.push({ a: i / 64 * Math.PI * 2 + rf() * 0.06, rin: 34 + rf() * 52, w: 2 + rf() * 5, d: rf() * 0.4 });

  // ── image mosaic ──
  const rm = mulberry32(31); TILES = [];
  const RH = 150, GUT = 14, ROWS = 6, Y0 = (H - (ROWS * RH + (ROWS - 1) * GUT)) / 2;
  const types = [1, 2, 3, 4, 6, 7, 2, 3, 6, 1, 4, 5, 2, 6, 4, 3];
  const panelAsp = MG.fbox.map(b => b.w / b.h);
  for (let r = 0; r < ROWS; r++) {
    const y = Y0 + r * (RH + GUT);
    const center = []; // the tiles centred in this row
    if (r === 2) center.push({ panel: 0, asp: panelAsp[0] });
    if (r === 3) center.push({ panel: 1, asp: panelAsp[1] }, { panel: 2, asp: panelAsp[2] });
    let cw = center.reduce((s, c) => s + c.asp * RH, 0) + Math.max(0, center.length - 1) * GUT;
    let xl = 960 - cw / 2 - (center.length ? 0 : (rm() * 200)), xr = xl;
    for (const c of center) { TILES.push({ r, x: xr, y, w: c.asp * RH, h: RH, panel: c.panel, type: -1, seed: 0 }); xr += c.asp * RH + GUT; }
    if (!center.length) xr = xl;
    else xl -= GUT;
    let k = 0;
    while (xr < W + 420) { const w = RH * (0.62 + rm() * 1.15); TILES.push({ r, x: xr, y, w, h: RH, type: types[(r * 5 + k++) % types.length], seed: rm() * 1e6 | 0 }); xr += w + GUT; }
    while (xl > -420) { const w = RH * (0.62 + rm() * 1.15); TILES.push({ r, x: xl - w, y, w, h: RH, type: types[(r * 3 + k++) % types.length], seed: rm() * 1e6 | 0 }); xl -= w + GUT; }
  }
  for (const tl of TILES) { tl.cx = tl.x + tl.w / 2; tl.cy = tl.y + tl.h / 2; tl.dist = Math.hypot(tl.cx - 960, tl.cy - 540); }
  // the four "moons" the search finds
  const want = [[1, 520], [4, 1380], [1, 1420], [4, 560]];
  MATCH_ORDER = [];
  for (const [r, x] of want) {
    let best = null; for (const tl of TILES) if (tl.r === r && tl.panel === undefined && !tl.match) { const d = Math.abs(tl.cx - x); if (!best || d < best.d) best = { tl, d }; }
    best.tl.type = 0; best.tl.match = true; MATCH_ORDER.push(best.tl);
  }
  PANEL_TILE = TILES.find(t => t.panel === 0); PANEL_TILE.match = true;
  const order = [MATCH_ORDER[0], MATCH_ORDER[3], PANEL_TILE, MATCH_ORDER[1], MATCH_ORDER[2]];
  const TH = 232, TG = 18; let tw = order.reduce((s, t) => s + t.w / t.h * TH, 0) + TG * 4, x = 960 - tw / 2;
  for (const tl of order) { const w = tl.w / tl.h * TH; tl.tgt = { x, y: 560 - TH / 2, w, h: TH }; x += w + TG; }
  TILES.forEach(t => { if (t.match) t.ord = order.indexOf(t); });
  // a different tone for each moon
  MATCH_ORDER.forEach((t, i) => t.seed = 1000 + i * 77);

  // grain
  GRAIN = document.createElement('canvas'); GRAIN.width = GRAIN.height = 512;
  const gc = GRAIN.getContext('2d'), id = gc.createImageData(512, 512), rg = mulberry32(99);
  for (let i = 0; i < id.data.length; i += 4) { const v = rg() * 255; id.data[i] = id.data[i + 1] = id.data[i + 2] = v; id.data[i + 3] = 255; }
  gc.putImageData(id, 0, 0);
}

function panelPolys(g1, g2) {
  const { frame, d1, d2 } = MG;
  const top = clipPoly(frame, p => -d1(p) - g1 / 2);
  const bot = clipPoly(frame, p => d1(p) - g1 / 2);
  const bl = clipPoly(bot, p => d2(p) - g2 / 2);
  const br = clipPoly(bot, p => -d2(p) - g2 / 2);
  return [top, bl, br];
}

// ═════════════════════════ camera ═════════════════════════
function camera(t) {
  const x = track(t, 900, [[0.4, 1.7, 960, E.outC], [2.0, 2.95, 960], [3.3, 4.15, 1110], [4.3, 5.0, 820], [8.7, 9.45, 960]]);
  const y = track(t, TITLE_BL - 8, [[2.0, 2.95, 540, E.ioQ]]);
  let z = track(t, ZOOM_A, [[2.0, 2.95, 1, E.ioQ], [3.3, 4.15, 0.9], [4.3, 5.0, 1], [6.3, 8.7, 1.045, E.ioC], [8.7, 9.45, 1], [9.45, 11.3, 1.03, E.ioC], [11.3, 11.95, 1]], true);
  z *= 1 + 0.004 * Math.sin(t * 0.9); // breathing
  const rot = 0.0;
  return { x, y, z, rot };
}
function applyCam(c) { g.translate(W / 2, H / 2); g.rotate(c.rot); g.scale(c.z, c.z); g.translate(-c.x, -c.y); }

// ═════════════════════════ scene 1: note → book → stack ═════════════════════════
function drawSentence(t) {
  const S = SENT, collapse = E.outX(seg(t, 1.66, 2.0));
  const X0 = -S.finalW / 2;
  g.save();
  g.translate(960, TITLE_BL); g.scale(1 / ZOOM_A, 1 / ZOOM_A);
  g.font = FONT_SENT; g.textBaseline = 'alphabetic';
  const xOf = i => X0 + S.xs[i].nb + (1 - collapse) * S.xs[i].br;
  const word = S.words[4]; // "nota"
  const wx0 = xOf(word.i0), wx1 = xOf(word.i1) + S.adv[word.i1];
  // word → bar
  const mW = S.words.map((w, k) => E.ioQ(seg(t, 2.02 + k * 0.035, 2.5 + k * 0.035)));
  const hl = E.outQ(seg(t, 1.78, 2.12));
  if (hl > 0) { // blue highlight behind the link
    const m = mW[4], x0 = lerp(wx0 - 6, wx0, m), y0 = lerp(-40, -28, m), hh = lerp(50, 28, m), ww = lerp(wx1 - wx0 + 12, wx1 - wx0, m);
    g.fillStyle = blu(1); rrect(g, x0, y0, ww * hl, hh, lerp(4, 6, m)); g.fill();
  }
  // bars
  S.words.forEach((w, k) => {
    const m = mW[k]; if (m <= 0 || k === 4) return;
    const x0 = xOf(w.i0), x1 = xOf(w.i1) + S.adv[w.i1];
    g.fillStyle = ink(0.82 * seg(m, 0.1, 0.55)); rrect(g, x0, lerp(-34, -28, m), x1 - x0, lerp(34, 28, m), 6); g.fill();
  });
  // letters
  for (let i = 0; i < S.chars.length; i++) {
    const a = seg(t, S.tt[i], S.tt[i] + 0.09); if (a <= 0) continue;
    let alpha = E.outC(a), dy = (1 - E.outC(a)) * 10;
    const wk = S.words.findIndex(w => i >= w.i0 && i <= w.i1);
    const fade = wk >= 0 ? 1 - seg(mW[wk], 0, 0.45) : 1 - seg(t, 2.0, 2.2);
    let sx = 1;
    if (S.isBr[i]) { alpha *= 0.4 * (1 - collapse); sx = 1 - collapse; }
    alpha *= fade; if (alpha <= 0.002) continue;
    g.fillStyle = ink(alpha);
    g.save(); g.translate(xOf(i), dy); g.scale(sx, 1); g.fillText(S.chars[i], 0, 0); g.restore();
  }
  // text caret
  let cx = X0;
  for (let i = 0; i < S.chars.length; i++) cx += (S.isBr[i] ? S.brW * (1 - collapse) : S.adv[i]) * E.outC(seg(t, S.tt[i], S.tt[i] + 0.06));
  const typing = t > S.tt[0] - 0.05 && t < S.tEnd + 0.05;
  const blink = typing || (Math.floor((t + 0.2) / 0.5) % 2 === 0);
  const ca = E.outB(seg(t, 0.1, 0.32)) * (1 - seg(t, 1.95, 2.05));
  if (ca > 0 && blink) { g.fillStyle = ink(1); g.fillRect(cx + 2, -50 + 64 * (1 - ca) / 2, 3.2, 64 * ca); }
  g.restore();
}

function drawPara(lines, t0, t, scale = 1) {
  for (let k = 0; k < lines.length; k++) {
    const L = lines[k];
    for (let j = 0; j < L.words.length; j++) {
      const w = L.words[j], a = E.outX(seg(t, t0 + k * 0.026 + j * 0.011, t0 + 0.5 + k * 0.026 + j * 0.011));
      if (a <= 0) continue;
      g.fillStyle = ink(0.8); rrect(g, w.x, L.y - 6, w.w * a, 6, 2); g.fill();
    }
  }
}

function pageShadow(alpha) { g.shadowColor = ink(0.16 * alpha); g.shadowBlur = 50; g.shadowOffsetY = 18; }
function noShadow() { g.shadowColor = 'transparent'; g.shadowBlur = 0; g.shadowOffsetY = 0; g.shadowOffsetX = 0; }

function drawBook(t) {
  if (t > 4.97) return;
  const vis = seg(t, 2.1, 2.8);
  // left page
  g.save(); pageShadow(vis); g.fillStyle = pap(1); g.fillRect(PG.x0, PG.y0, PAGE.w, PAGE.h); noShadow();
  g.strokeStyle = ink(0.1 * vis); g.lineWidth = 1; g.strokeRect(PG.x0 + .5, PG.y0 + .5, PAGE.w - 1, PAGE.h - 1);
  drawSentence(t);
  drawPara(PARA_L, 2.28, t);
  g.font = '400 10px NR'; g.textAlign = 'center'; g.fillStyle = ink(0.55 * seg(t, 2.6, 3.0)); g.fillText('1', 960, PG.y1 - 44); g.textAlign = 'left';
  const open = t < 4.3 ? E.ioQ(seg(t, 3.35, 4.1)) : 1 - 2 * E.ioC(seg(t, 4.3, 4.95));
  if (open > 0 || t > 4.3) {
    // spine shadow on the left page
    const sp = Math.max(0, Math.min(1, open)) * (t > 4.3 ? 1 : 1);
    const gr = g.createLinearGradient(PG.x1, 0, PG.x1 - 34, 0); gr.addColorStop(0, ink(0.09 * sp)); gr.addColorStop(1, ink(0));
    g.fillStyle = gr; g.fillRect(PG.x1 - 34, PG.y0, 34, PAGE.h);
  }
  g.restore();
  // right page: opens, then closes over the left one
  if (t > 3.35) {
    const sx = t < 4.3 ? E.ioQ(seg(t, 3.35, 4.1)) : 1 - 2 * E.ioC(seg(t, 4.3, 4.95));
    const shade = 1 - Math.abs(sx);
    g.save();
    if (sx > 0.001) {
      g.translate(PG.x1, 0); g.scale(sx, 1); g.translate(-PG.x1, 0);
      pageShadow(1); g.fillStyle = pap(1); g.fillRect(PG.x1, PG.y0, PAGE.w, PAGE.h); noShadow();
      g.strokeStyle = ink(0.1); g.lineWidth = 1; g.strokeRect(PG.x1 + .5, PG.y0 + .5, PAGE.w - 1, PAGE.h - 1);
      drawPara(PARA_R, 3.62, t);
      g.fillStyle = ink(0.35); rrect(g, 1380 - 34, 262, 68, 3, 1.5); g.fill();
      g.font = '400 10px NR'; g.textAlign = 'center'; g.fillStyle = ink(0.55); g.fillText('2', 1380, PG.y1 - 44); g.textAlign = 'left';
      const gr = g.createLinearGradient(PG.x1, 0, PG.x1 + 40, 0); gr.addColorStop(0, ink(0.1)); gr.addColorStop(1, ink(0));
      g.fillStyle = gr; g.fillRect(PG.x1, PG.y0, 40, PAGE.h);
      g.fillStyle = ink(0.16 * shade); g.fillRect(PG.x1, PG.y0, PAGE.w, PAGE.h);
    } else if (sx < -0.001) {
      const w = -sx * PAGE.w;
      // shadow cast on the page below
      const sh = g.createLinearGradient(PG.x1 - w, 0, PG.x1 - w - 60 * (1 - shade * 0.5), 0); sh.addColorStop(0, ink(0.14 * (1 - Math.abs(sx)) + 0.03)); sh.addColorStop(1, ink(0));
      g.fillStyle = sh; g.fillRect(PG.x1 - w - 60, PG.y0, 60, PAGE.h);
      g.fillStyle = pap(1); g.fillRect(PG.x1 - w, PG.y0, w, PAGE.h);
      const gr = g.createLinearGradient(PG.x1 - w, 0, PG.x1, 0); gr.addColorStop(0, ink(0.02 + 0.1 * shade)); gr.addColorStop(1, ink(0.08 + 0.12 * shade));
      g.fillStyle = gr; g.fillRect(PG.x1 - w, PG.y0, w, PAGE.h);
      g.strokeStyle = ink(0.1); g.lineWidth = 1; g.strokeRect(PG.x1 - w + .5, PG.y0 + .5, w - 1, PAGE.h - 1);
    }
    g.restore();
  }
}

// a sheet in isometric view (rotation + squash + thickness)
function sheetParams(t) {
  const th = track(t, 0, [[4.95, 5.5, Math.PI / 4, E.ioQ], [5.72, 6.28, 0, t => E.outB(t, 1.2)]]);
  const sy = track(t, 1, [[5.0, 5.5, 0.5774, E.ioQ], [5.72, 6.22, 1, E.ioQ]]);
  const d = track(t, 0, [[5.08, 5.55, 46, E.outQ], [5.7, 6.05, 0, E.ioC]]);
  const w = track(t, PAGE.w, [[5.72, 6.3, MANGA.w, E.ioQ]]);
  const h = track(t, PAGE.h, [[5.72, 6.3, MANGA.h, E.ioQ]]);
  const fl = Math.sin(Math.PI * seg(t, 5.2, 6.0)) * -10;
  // the closed book sits where the left page was: centre at x=960
  return { cx: 960, cy: 540 + fl - d / 2, th, sy, d, w, h };
}
function sheetCorners(sp) {
  const c = Math.cos(sp.th), s = Math.sin(sp.th);
  return [[-1, -1], [1, -1], [1, 1], [-1, 1]].map(([u, v]) => { const x = u * sp.w / 2, y = v * sp.h / 2; return P(sp.cx + x * c - y * s, sp.cy + (x * s + y * c) * sp.sy); });
}
function drawSheet(t, inner, fillA = 1) {
  const sp = sheetParams(t), top = sheetCorners(sp);
  const isoAmt = Math.sin(sp.th * 2) ;
  // long shadow, a table lit from the side
  if (sp.d > 0.5 || isoAmt > 0.01) {
    const base = top.map(p => P(p.x, p.y + sp.d));
    const off = P(300 * isoAmt, 170 * isoAmt);
    const hp = hull(base.concat(base.map(p => P(p.x + off.x, p.y + off.y))));
    const c0 = base.reduce((s, p) => P(s.x + p.x / 4, s.y + p.y / 4), P(0, 0));
    const gr = g.createLinearGradient(c0.x, c0.y, c0.x + off.x * 1.1, c0.y + off.y * 1.1);
    gr.addColorStop(0, ink(0.2 * isoAmt)); gr.addColorStop(1, ink(0));
    g.save(); g.filter = 'blur(8px)'; g.fillStyle = gr; pathPoly(g, hp); g.fill(); g.restore();
  }
  if (sp.d > 0.3) {
    for (let i = 0; i < 4; i++) {
      const a = top[i], b = top[(i + 1) % 4], my = (a.y + b.y) / 2;
      if (my <= sp.cy + 0.01) continue;
      const left = (a.x + b.x) / 2 < sp.cx;
      const quad = [a, b, P(b.x, b.y + sp.d), P(a.x, a.y + sp.d)];
      const gr = g.createLinearGradient(a.x, a.y, b.x, b.y);
      if (left) { gr.addColorStop(0, mixCss(0.06)); gr.addColorStop(1, mixCss(0.16)); }
      else { gr.addColorStop(0, mixCss(0.42)); gr.addColorStop(1, mixCss(0.2)); }
      g.fillStyle = gr; pathPoly(g, quad); g.fill();
      // edges of the stacked sheets
      const n = 11; g.strokeStyle = ink(left ? 0.16 : 0.22); g.lineWidth = 1;
      for (let k = 1; k < n; k++) { const yy = sp.d * k / n; g.beginPath(); g.moveTo(a.x, a.y + yy); g.lineTo(b.x, b.y + yy); g.stroke(); }
    }
  }
  // top face
  g.save();
  const flat = sp.th < 0.0005 && Math.abs(sp.sy - 1) < 0.0005 && sp.d < 0.3;
  if (flat) pageShadow(fillA);
  g.fillStyle = pap(fillA); pathPoly(g, top); g.fill(); noShadow();
  g.strokeStyle = ink(0.1 * fillA); g.lineWidth = 1; pathPoly(g, top); g.stroke();
  if (inner) {
    g.translate(sp.cx, sp.cy); g.scale(1, sp.sy); g.rotate(sp.th); g.translate(-sp.cx, -sp.cy);
    inner(sp);
  }
  g.restore();
}
const mixCss = k => rgba(mixc(PAPER, INK, k));

// ═════════════════════════ scene 2: manga storyboard ═════════════════════════
function cutState(t) {
  const g1 = t < 6.97 ? 0 : 18 * E.spring(seg(t, 6.97, 7.55), 1.4, 6);
  const g2 = t < 7.42 ? 0 : 14 * E.spring(seg(t, 7.42, 7.95), 1.4, 6);
  return { g1, g2, cut1: t >= 6.97, cut2: t >= 7.42 };
}
function mangaPanels(t) {
  const { g1, g2, cut1, cut2 } = cutState(t);
  if (!cut1) return [MG.frame];
  const [top, bl, br] = panelPolys(g1, g2);
  if (!cut2) return [top, clipPoly(MG.frame, p => MG.d1(p) - g1 / 2)];
  return [top, bl, br];
}
// a stroke that draws itself (k = fraction of its length)
function strokePart(pts, k) {
  if (k <= 0) return;
  let L = 0; const segs = [];
  for (let i = 1; i < pts.length; i++) { const l = Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y); segs.push(l); L += l; }
  let rem = L * k; g.beginPath(); g.moveTo(pts[0].x, pts[0].y);
  for (let i = 1; i < pts.length && rem > 0; i++) { const f = Math.min(1, rem / segs[i - 1]); g.lineTo(lerp(pts[i - 1].x, pts[i].x, f), lerp(pts[i - 1].y, pts[i].y, f)); rem -= segs[i - 1]; }
  g.stroke();
}
const circlePts = (cx, cy, r, a0 = -Math.PI / 2, n = 64, jit = 0, seed = 1) => { const rr = mulberry32(seed); const p = []; for (let i = 0; i <= n; i++) { const a = a0 + i / n * Math.PI * 2; const j = 1 + (rr() - .5) * jit; p.push(P(cx + Math.cos(a) * r * j, cy + Math.sin(a) * r * j)); } return p; };

// top panel: moon over the sea — blue underdrawing first, then ink
function contentMoon(t, ct) {
  const b = MG.fbox[0];
  const hy = b.y + b.h * 0.64, mx = b.x + b.w * 0.72, my = b.y + b.h * 0.34, mr = b.h * 0.17;
  const sk = ct < 0 ? 1 : seg(ct, 7.5, 8.05), ik = ct < 0 ? 1 : seg(ct, 7.95, 8.5), bf = ct < 0 ? 0 : 1 - seg(ct, 8.35, 8.75);
  const horizon = [P(b.x - 10, hy + 6), P(b.x + b.w + 10, hy - 4)];
  const isl = [P(b.x + b.w * 0.06, hy + 3), P(b.x + b.w * 0.16, hy - b.h * 0.12), P(b.x + b.w * 0.26, hy - b.h * 0.16), P(b.x + b.w * 0.38, hy - 1)];
  const refl = [0, 1, 2, 3].map(i => { const yy = hy + 16 + i * 13, ww = (64 - i * 14); return [P(mx - ww / 2, yy), P(mx + ww / 2, yy)]; });
  const sketch = (k, col, lw, j) => {
    g.strokeStyle = col; g.lineWidth = lw; g.lineCap = 'round'; g.lineJoin = 'round';
    strokePart(horizon.map((p, i) => P(p.x, p.y + j * (i ? 1 : -1))), E.outC(seg(k, 0, 0.5)));
    strokePart(circlePts(mx + j, my, mr, -2.2, 64, j ? 0.05 : 0, 3), E.ioC(seg(k, 0.15, 0.75)));
    strokePart(isl.map(p => P(p.x + j, p.y)), E.outC(seg(k, 0.3, 0.8)));
    refl.forEach((r, i) => strokePart(r, E.outC(seg(k, 0.55 + i * 0.1, 0.85 + i * 0.05))));
  };
  if (bf > 0 && sk > 0) { g.save(); g.globalAlpha = bf; sketch(sk, blu(1), 2.2, 0); sketch(sk * 0.97, blu(0.7), 1.4, 2.5); g.restore(); }
  if (ik > 0) {
    // sea in an ink wash
    g.fillStyle = ink(0.07 * ik); g.beginPath(); g.moveTo(b.x - 10, hy + 6); g.lineTo(b.x + b.w + 10, hy - 4); g.lineTo(b.x + b.w + 10, b.y + b.h + 10); g.lineTo(b.x - 10, b.y + b.h + 10); g.fill();
    g.fillStyle = ink(0.92 * seg(ik, 0.35, 0.8)); g.beginPath(); g.arc(mx, my, mr, 0, Math.PI * 2); g.fill();
    sketch(ik, ink(0.95), 2.6, 0);
  }
}
function contentBalloon(t, ct) {
  const b = MG.fbox[1];
  const cx = b.x + b.w * 0.5, cy = b.y + b.h * 0.42, rx = b.w * 0.36, ry = b.h * 0.24;
  const s = ct < 0 ? 1 : E.spring(seg(ct, 7.78, 8.3), 1.5, 6);
  if (s <= 0) return;
  g.save(); g.translate(cx, cy); g.scale(s, s);
  g.fillStyle = pap(1); g.strokeStyle = ink(0.95); g.lineWidth = 3; g.lineJoin = 'round';
  g.beginPath(); g.ellipse(0, 0, rx, ry, 0, 0.62 * Math.PI, 0.58 * Math.PI + Math.PI * 2 - 0.12);
  g.lineTo(-rx * 0.5, ry * 1.75); g.closePath(); g.fill(); g.stroke();
  const ws = [0.92, 1.1, 0.62];
  ws.forEach((w, i) => { const a = ct < 0 ? 1 : E.outX(seg(ct, 8.0 + i * 0.07, 8.35 + i * 0.07)); g.fillStyle = ink(0.85); rrect(g, -rx * 0.5 * w, -ry * 0.38 + i * ry * 0.38 - 3, rx * w * a, 5, 2.5); g.fill(); });
  g.restore();
  // a minimal figure: the line of a shoulder
  const fk = ct < 0 ? 1 : E.outC(seg(ct, 8.1, 8.6));
  g.strokeStyle = ink(0.9); g.lineWidth = 2.6; g.lineCap = 'round';
  strokePart([P(b.x - 10, b.y + b.h * 0.98), P(b.x + b.w * 0.22, b.y + b.h * 0.86), P(b.x + b.w * 0.3, b.y + b.h * 0.7), P(b.x + b.w * 0.42, b.y + b.h * 0.66), P(b.x + b.w * 0.52, b.y + b.h * 0.74), P(b.x + b.w * 0.58, b.y + b.h * 0.9), P(b.x + b.w * 0.9, b.y + b.h + 10)], fk);
}
function contentSpeed(t, ct) {
  const b = MG.fbox[2], cx = b.x + b.w * 0.56, cy = b.y + b.h * 0.48, R = 420;
  g.fillStyle = ink(0.95);
  for (const L of MG.speed) {
    const k = ct < 0 ? 1 : E.outQ(seg(ct, 7.95 + L.d, 8.3 + L.d));
    if (k <= 0) continue;
    const rin = lerp(R, L.rin, k), ca = Math.cos(L.a), sa = Math.sin(L.a), nx = -sa, ny = ca;
    g.beginPath(); g.moveTo(cx + ca * R + nx * L.w, cy + sa * R + ny * L.w); g.lineTo(cx + ca * rin, cy + sa * rin); g.lineTo(cx + ca * R - nx * L.w, cy + sa * R - ny * L.w); g.fill();
  }
}
const CONTENT = [contentMoon, contentBalloon, contentSpeed];

function drawManga(t) {
  if (t < 5.72 || t > 9.6) return;
  const pageA = 1 - E.ioC(seg(t, 8.75, 9.25));
  drawSheet(t, () => {
    if (t < 6.12) return;
    const polys = mangaPanels(t);
    const frameK = E.ioC(seg(t, 6.12, 6.55));
    if (t < 8.75) {
      // contents
      if (t >= 7.42) polys.forEach((poly, i) => { g.save(); pathPoly(g, poly); g.clip(); CONTENT[i](t, t); g.restore(); });
      else if (t >= 6.97) { g.save(); pathPoly(g, polys[0]); g.clip(); contentMoon(t, t); g.restore(); }
      g.strokeStyle = ink(1); g.lineWidth = 4; g.lineJoin = 'miter';
      if (frameK < 1) {
        const f = MG.frame, pts = [f[0], f[1], f[2], f[3], f[0]]; g.lineCap = 'square'; strokePart(pts, frameK);
      } else polys.forEach(poly => { pathPoly(g, poly); g.stroke(); });
      // the cut line, in blue, while it is dragged
      const cutLine = (A, B, t0, t1) => {
        if (t < t0 || t > t1 + 0.35) return;
        const k = E.ioC(seg(t, t0, t1)), fade = 1 - seg(t, t1, t1 + 0.3);
        g.strokeStyle = blu(fade); g.lineWidth = 2.5; g.setLineDash([]); g.beginPath(); g.moveTo(A.x, A.y); const e = lerpP(A, B, k); g.lineTo(e.x, e.y); g.stroke();
      };
      cutLine(MG.A1, MG.B1, 6.6, 6.97); cutLine(MG.A2, MG.B2, 7.12, 7.42);
    }
  }, pageA);
}

// ═════════════════════════ scene 3: image library ═════════════════════════
function tileArt(tl, x, y, w, h, a) {
  const r = mulberry32(tl.seed + 1);
  g.save(); g.beginPath(); g.rect(x, y, w, h); g.clip(); g.globalAlpha *= a;
  const T = tl.type;
  if (T === 0) { // moon
    const night = (tl.seed % 2) === 0;
    g.fillStyle = night ? ink(0.88) : ink(0.08); g.fillRect(x, y, w, h);
    const hy = y + h * (0.58 + r() * 0.12);
    g.fillStyle = night ? pap(0.1) : ink(0.14); g.fillRect(x, hy, w, h);
    g.fillStyle = night ? pap(0.95) : ink(0.85); g.beginPath(); g.arc(x + w * (0.3 + r() * 0.4), y + h * (0.28 + r() * 0.12), h * (0.1 + r() * 0.06), 0, Math.PI * 2); g.fill();
  } else if (T === 1) { g.fillStyle = ink(0.06); g.fillRect(x, y, w, h); g.fillStyle = ink(0.32); g.beginPath(); g.moveTo(x + w, y + h * r()); g.lineTo(x + w, y + h); g.lineTo(x + w * r() * 0.5, y + h); g.fill(); }
  else if (T === 2) { g.fillStyle = ink(0.05); g.fillRect(x, y, w, h); const aw = Math.min(w, h) * 0.5; g.fillStyle = ink(0.2); g.beginPath(); g.moveTo(x + w / 2 - aw / 2, y + h); g.lineTo(x + w / 2 - aw / 2, y + h * 0.45); g.arc(x + w / 2, y + h * 0.45, aw / 2, Math.PI, 0); g.lineTo(x + w / 2 + aw / 2, y + h); g.fill(); }
  else if (T === 3) { const n = 3 + (r() * 4 | 0); for (let i = 0; i < n; i++) { g.fillStyle = ink(0.04 + r() * 0.22); g.fillRect(x, y + h * i / n, w, h / n + 1); } }
  else if (T === 4) { g.fillStyle = ink(0.05); g.fillRect(x, y, w, h); g.fillStyle = ink(0.4); const s = 16; for (let yy = y + 14; yy < y + h; yy += s) for (let xx = x + 14; xx < x + w; xx += s) { g.beginPath(); g.arc(xx, yy, 2, 0, 7); g.fill(); } }
  else if (T === 5) { g.fillStyle = ink(0.72); g.fillRect(x, y, w, h); g.strokeStyle = pap(0.85); g.lineWidth = 2; g.beginPath(); g.moveTo(x + w * 0.2, y + h * 0.75); g.lineTo(x + w * 0.8, y + h * 0.75); g.stroke(); g.fillStyle = pap(0.85); g.fillRect(x + w * 0.45, y + h * 0.3, 3, h * 0.45); }
  else if (T === 6) { g.fillStyle = ink(0.06); g.fillRect(x, y, w, h); g.fillStyle = ink(0.24); g.beginPath(); g.moveTo(x - 5, y + h); g.lineTo(x + w * (0.3 + r() * 0.3), y + h * 0.35); g.lineTo(x + w + 5, y + h); g.fill(); g.fillStyle = ink(0.5); g.beginPath(); g.arc(x + w * 0.78, y + h * 0.26, h * 0.07, 0, 7); g.fill(); }
  else if (T === 7) { g.fillStyle = ink(0.07); g.fillRect(x, y, w, h); g.fillStyle = ink(0.34); const n = 4 + (r() * 5 | 0); for (let i = 0; i < n; i++) g.fillRect(x + w * (i + 0.5) / n - 1, y + h * 0.2, 2, h * 0.6); }
  g.restore();
}

function searchState(t) {
  const q = 'lua sobre o mar';
  const t0 = 9.95, per = 0.033;
  const n = Math.max(0, Math.min(q.length, Math.floor((t - t0) / per) + 1));
  return { q, shown: t < t0 ? '' : q.slice(0, n), tEnd: t0 + q.length * per };
}
function drift(r, t) { return (r % 2 ? 1 : -1) * (t - 9.0) * 26; }

function drawMosaic(t) {
  if (t < 8.75 || t > 12.0) return;
  const filt = E.ioQ(seg(t, 10.5, 11.0));
  // ordinary tiles
  for (const tl of TILES) {
    if (tl.panel !== undefined) continue;
    const app = E.outQ(seg(t, 9.0 + tl.dist / 1500 * 0.55, 9.55 + tl.dist / 1500 * 0.55));
    if (app <= 0) continue;
    let x = tl.x + drift(tl.r, t), y = tl.y, w = tl.w, h = tl.h, a = app, s = lerp(0.82, 1, app);
    if (tl.match) {
      const m = E.ioQ(seg(t, 10.5 + tl.ord * 0.035, 11.0 + tl.ord * 0.035));
      x = lerp(x, tl.tgt.x, m); y = lerp(y, tl.tgt.y, m); w = lerp(w, tl.tgt.w, m); h = lerp(h, tl.tgt.h, m);
      const ex = E.inC(seg(t, 11.4 + Math.abs(tl.ord - 2) * 0.05, 11.75 + Math.abs(tl.ord - 2) * 0.05));
      a *= 1 - ex; y += 40 * ex; s *= 1 - 0.08 * ex;
    } else {
      a *= lerp(1, 0.1, filt); s *= lerp(1, 0.94, filt);
      a *= 1 - seg(t, 11.4, 11.7);
    }
    if (a <= 0.003) continue;
    const cx = x + w / 2, cy = y + h / 2;
    tileArt(tl, cx - w * s / 2, cy - h * s / 2, w * s, h * s, a);
  }
  // the three manga panels become pictures
  const m = E.ioQ(seg(t, 8.75, 9.5));
  MG.final.forEach((poly, i) => {
    const tl = TILES.find(u => u.panel === i);
    let tx = tl.x + drift(tl.r, t), ty = tl.y, tw = tl.w, th = tl.h;
    if (i === 0) { const mm = E.ioQ(seg(t, 10.5 + 2 * 0.035, 11.0 + 2 * 0.035)); tx = lerp(tx, tl.tgt.x, mm); ty = lerp(ty, tl.tgt.y, mm); tw = lerp(tw, tl.tgt.w, mm); th = lerp(th, tl.tgt.h, mm); }
    let a = 1;
    if (i > 0) a = lerp(1, 0.1, filt) * (1 - seg(t, 11.4, 11.7));
    if (a <= 0.003) return;
    const fb = MG.fbox[i];
    // the rectangle the contents are drawn into
    let R = { x: lerp(fb.x, tx, m), y: lerp(fb.y, ty, m), w: lerp(fb.w, tw, m), h: lerp(fb.h, th, m) };
    let inkCover = 0, contentA = 1;
    if (i === 0 && t > 11.4) { // the chosen picture becomes the mark's page
      const k = E.ioQ(seg(t, 11.4, 11.95));
      const S = 380, sq = { x: 960 - S / 2, y: 520 - S / 2, w: S, h: S };
      R = { x: lerp(R.x, sq.x, k), y: lerp(R.y, sq.y, k), w: lerp(R.w, sq.w, k), h: lerp(R.h, sq.h, k) };
      inkCover = E.ioC(seg(t, 11.45, 11.82)); contentA = 1 - seg(t, 11.45, 11.7);
    }
    const rc = [P(R.x, R.y), P(R.x + R.w, R.y), P(R.x + R.w, R.y + R.h), P(R.x, R.y + R.h)];
    const sx = R.w / fb.w, sy = R.h / fb.h;
    const pp = poly.map((p, k) => lerpP(P(R.x + (p.x - fb.x) * sx, R.y + (p.y - fb.y) * sy), rc[k], m));
    g.save(); g.globalAlpha = a;
    g.fillStyle = pap(1); pathPoly(g, pp); g.fill();
    g.save(); pathPoly(g, pp); g.clip();
    g.globalAlpha = a * contentA;
    g.translate(R.x, R.y); g.scale(sx, sy); g.translate(-fb.x, -fb.y);
    CONTENT[i](t, -1);
    g.restore();
    if (inkCover > 0) { g.fillStyle = ink(inkCover); pathPoly(g, pp); g.fill(); }
    g.strokeStyle = ink(1 - m * 0.85); g.lineWidth = lerp(4, 1, m); if (inkCover < 1) { pathPoly(g, pp); g.stroke(); }
    // selection, in blue
    if (i === 0 && t > 11.15) {
      const s = E.spring(seg(t, 11.15, 11.5), 1.4, 6) * (1 - seg(t, 11.6, 11.88));
      if (s > 0) { g.strokeStyle = blu(1); g.lineWidth = 5 * s; const o = 7 * s - 2; g.strokeRect(R.x - o, R.y - o, R.w + 2 * o, R.h + 2 * o); }
    }
    g.restore();
  });
}

function drawSearch(t) {
  if (t < 9.6 || t > 11.75) return;
  const a = E.outX(seg(t, 9.6, 10.05)), wv = E.ioX(seg(t, 9.7, 10.15)), out_ = E.inQ(seg(t, 11.35, 11.68));
  const w = lerp(64, 640, wv), h = 64, x = 960 - w / 2, y = lerp(-80, 96, a) - 170 * out_;
  g.save(); g.globalAlpha = 1 - out_;
  g.shadowColor = ink(0.14); g.shadowBlur = 40; g.shadowOffsetY = 12;
  g.fillStyle = pap(1); rrect(g, x, y, w, h, 32); g.fill(); noShadow();
  g.strokeStyle = ink(0.16); g.lineWidth = 1; rrect(g, x + .5, y + .5, w - 1, h - 1, 31.5); g.stroke();
  // magnifier
  const ix = x + 32 + (w - 64) * 0 , iy = y + 32;
  g.strokeStyle = ink(0.85); g.lineWidth = 2.2; g.beginPath(); g.arc(ix - 2, iy - 2, 8.5, 0, 7); g.stroke();
  g.beginPath(); g.moveTo(ix + 4.2, iy + 4.2); g.lineTo(ix + 10, iy + 10); g.stroke();
  g.beginPath(); g.rect(x + 54, y, w - 70, h); g.clip();
  const st = searchState(t);
  g.font = '400 22px IN'; g.textBaseline = 'middle';
  if (!st.shown) { g.fillStyle = ink(0.35 * seg(t, 10.1, 10.3) ); }
  g.fillStyle = ink(0.9); g.fillText(st.shown, x + 58, y + 33);
  const tw = g.measureText(st.shown).width;
  const blink = t < st.tEnd + 0.05 || Math.floor((t - st.tEnd) / 0.45) % 2 === 1;
  if (t > 9.9 && blink && t < 10.5) { g.fillStyle = ink(0.9); g.fillRect(x + 60 + tw, y + 19, 2, 28); }
  const rk = E.outC(seg(t, 10.55, 10.85));
  if (rk > 0) { g.font = '400 15px IN'; g.textAlign = 'right'; g.fillStyle = ink(0.45 * rk); g.fillText('5 imagens', x + w - 28 + (1 - rk) * 10, y + 33); g.textAlign = 'left'; }
  g.restore();
}

// ═════════════════════════ scene 4: the mark ═════════════════════════
function markTf(t) {
  const L = lockup();
  const k = E.ioX(seg(t, 13.12, 13.66));
  const S = lerp(380, L.markS, k), cx = lerp(960, L.markCx, k), cy = lerp(520, L.markCy, k);
  return { S, cx, cy, u: S / 560 };
}
function lockup() {
  const sc = 820 / 718.4, x0 = 960 - 410 + 4 * sc, base = 520 + 73.2 * sc;
  const markS = 560 * 0.24886 * sc;
  return { sc, x0, base, markS, markCx: x0 + markS / 2, markCy: base - markS / 2 };
}
function drawMark(t) {
  if (t < 11.92) return;
  const M = markTf(t);
  g.save(); g.translate(M.cx, M.cy); g.scale(M.u, M.u); g.translate(-512, -512);
  // the inked page
  g.fillStyle = ink(1); g.fillRect(232, 232, 560, 560);
  g.save(); g.beginPath(); g.rect(232, 232, 560, 560); g.clip();
  // round panel: paper gutter + blue counter
  const rk = E.spring(seg(t, 12.62, 13.12), 1.25, 5.5);
  if (rk > 0) {
    g.fillStyle = pap(1); g.beginPath(); g.arc(481.41, 545.6, 182 * rk, 0, Math.PI * 2); g.fill();
    g.fillStyle = blu(1); g.beginPath(); g.arc(481.41, 545.6, 138 * rk, 0, Math.PI * 2); g.fill();
  }
  // stem: a cut slanted at 13° that runs out through the bottom edge
  const sk = E.ioQ(seg(t, 12.12, 12.5));
  if (sk > 0) {
    const yb = lerp(363.6, 792, sk), f = (yb - 363.6) / 428.4;
    g.fillStyle = pap(1); g.beginPath();
    g.moveTo(665.06, 363.6); g.lineTo(724.59, 363.6); g.lineTo(724.59 + (625.68 - 724.59) * f, yb); g.lineTo(665.06 + (566.16 - 665.06) * f, yb); g.closePath(); g.fill();
  }
  g.restore();
  // at rest, the exact shapes from the mark's file
  if (t > 13.14) {
    g.fillStyle = ink(1); g.fill(new Path2D(MARK_INK));
    g.fillStyle = pap(1); g.fill(new Path2D(MARK_PAPER));
    g.fillStyle = blu(1); g.fill(new Path2D(MARK_BLUE));
  }
  g.restore();
}
const LETTER_OF = [0, 1, 1, 2, 3, 4, 4, 5, 6, 6];
let WM_PATHS;
function drawWordmark(t) {
  if (t < 13.3) return;
  const L = lockup();
  WM_PATHS = WM_PATHS || WORDMARK.map(s => new Path2D(s.d));
  g.save(); g.translate(L.x0, L.base); g.scale(L.sc, L.sc); g.translate(197.89, 0);
  g.beginPath(); g.rect(-60, -170, 640, 178); g.clip(); // mask at the baseline
  g.fillStyle = ink(1);
  WM_PATHS.forEach((p, i) => {
    const k = E.outX(seg(t, 13.5 + LETTER_OF[i] * 0.04, 14.08 + LETTER_OF[i] * 0.04));
    if (k <= 0) return;
    g.save(); g.translate(-22 * (1 - k), 150 * (1 - k)); g.fill(p); g.restore();
  });
  g.restore();
}
function drawTagline(t) {
  if (t < 13.6) return;
  const L = lockup();
  g.save(); g.textAlign = 'left'; g.textBaseline = 'alphabetic';
  const line = (txt, font, y, col, t0, st, ls) => {
    g.font = font; g.letterSpacing = ls || '0px';
    const tw = g.measureText(txt).width; let x = 960 - tw / 2;
    const chars = [...txt];
    let acc = '';
    chars.forEach((c, i) => {
      const x_ = 960 - tw / 2 + g.measureText(acc).width; acc += c;
      const k = E.outC(seg(t, t0 + i * st, t0 + 0.5 + i * st)); if (k <= 0) return;
      g.fillStyle = ink(col * k); g.fillText(c, x_, y + 10 * (1 - k));
    });
    g.letterSpacing = '0px';
  };
  line('a oficina de quem escreve e desenha', 'italic 400 34px NR', L.base + 98, 0.82, 13.62, 0.012);
  line('notas  ·  livros  ·  storyboards  ·  imagens  —  no mesmo vault do obsidian', '400 14px IN', L.base + 150, 0.5, 13.82, 0.003, '1.5px');
  g.restore();
}

// ═════════════════════════ pointer ═════════════════════════
const ARROW = [[0, 0], [0, 23], [5.6, 17.6], [9.4, 26.4], [13, 24.8], [9.3, 16.2], [16.6, 16.2]];
function drawCursor(x, y, a, press) {
  if (a <= 0) return;
  g.save(); g.translate(x, y); const s = 1.25 * (1 - 0.1 * press); g.scale(s, s);
  g.globalAlpha = a;
  g.shadowColor = ink(0.25); g.shadowBlur = 8; g.shadowOffsetY = 3;
  g.beginPath(); ARROW.forEach(([px, py], i) => i ? g.lineTo(px, py) : g.moveTo(px, py)); g.closePath();
  g.fillStyle = ink(1); g.fill(); noShadow();
  g.strokeStyle = pap(1); g.lineWidth = 1.6; g.lineJoin = 'round'; g.stroke();
  g.restore();
}
function ripple(x, y, t, t0) {
  const k = seg(t, t0, t0 + 0.5); if (k <= 0 || k >= 1) return;
  g.strokeStyle = ink(0.35 * (1 - k)); g.lineWidth = 1.5; g.beginPath(); g.arc(x, y, 8 + 34 * E.outC(k), 0, 7); g.stroke();
}
function path(t, keys) { // chained [[t0,t1,P,ease]]
  let p = keys[0][2];
  for (let i = 1; i < keys.length; i++) { const [a, b, q, e] = keys[i]; p = lerpP(p, q, (e || E.ioC)(seg(t, a, b))); }
  return p;
}
function cursors(t) {
  // cutting the manga panels
  if (t > 6.3 && t < 7.9) {
    const p = path(t, [[0, 0, P(1330, 820)], [6.32, 6.6, MG.A1, E.ioQ], [6.6, 6.97, MG.B1, E.ioC], [6.99, 7.12, MG.A2, E.ioQ], [7.12, 7.42, MG.B2, E.ioC], [7.45, 7.85, P(1250, 1010), E.inC]]);
    const press = (t > 6.6 && t < 6.97) || (t > 7.12 && t < 7.42) ? 1 : 0;
    ripple(MG.A1.x, MG.A1.y, t, 6.6); ripple(MG.A2.x, MG.A2.y, t, 7.12);
    drawCursor(p.x, p.y, seg(t, 6.3, 6.45) * (1 - seg(t, 7.6, 7.85)), press);
  }
  // picking the picture
  if (t > 10.7 && t < 11.75) {
    const tg = PANEL_TILE.tgt, c = P(tg.x + tg.w * 0.62, tg.y + tg.h * 0.6);
    const p = path(t, [[0, 0, P(1500, 960)], [10.72, 11.12, c, E.ioQ], [11.3, 11.65, P(c.x + 120, 1040), E.inC]]);
    ripple(c.x, c.y, t, 11.15);
    drawCursor(p.x, p.y, seg(t, 10.72, 10.85) * (1 - seg(t, 11.45, 11.65)), t > 11.12 && t < 11.24 ? 1 : 0);
  }
  // building the mark with the panel tool
  if (t > 11.9 && t < 13.3) {
    const M = markTf(t), mp = (x, y) => P(M.cx + (x - 512) * M.u, M.cy + (y - 512) * M.u);
    const rk = E.spring(seg(t, 12.62, 13.12), 1.25, 5.5);
    const ringP = mp(481.41 + 182 * 0.72 * rk, 545.6 + 182 * 0.69 * rk);
    let p;
    if (t < 12.52) p = path(t, [[0, 0, mp(820, 300)], [11.92, 12.12, mp(695, 363.6), E.ioQ], [12.12, 12.5, mp(587, 830), E.ioQ]]);
    else if (t < 12.62) p = lerpP(mp(587, 830), mp(481.41, 545.6), E.ioQ(seg(t, 12.5, 12.62)));
    else p = ringP;
    if (t > 12.98) p = lerpP(p, P(p.x + 160, p.y + 220), E.inC(seg(t, 12.98, 13.28)));
    const press = (t > 12.12 && t < 12.5) || (t > 12.62 && t < 12.92) ? 1 : 0;
    ripple(mp(695, 363.6).x, mp(695, 363.6).y, t, 12.12);
    drawCursor(p.x, p.y, seg(t, 11.92, 12.06) * (1 - seg(t, 13.05, 13.25)), press);
  }
}

// ═════════════════════════ captions and HUD ═════════════════════════
const CAPS = [
  { t0: 3.05, t1: 5.15, n: '02', w: 'componha', d: ['diagramação com knuth–plass,', 'hifenização e pdf para impressão'] },
  { t0: 6.15, t1: 8.6, n: '03', w: 'desenhe', d: ['quadros cortados na medianiz,', 'balões e storyboards'] },
];
function drawCaptions(t) {
  for (const C of CAPS) {
    if (t < C.t0 || t > C.t1 + 0.6) continue;
    const x = 150, base = 562;
    g.save();
    g.font = '500 13px IN'; g.letterSpacing = '2px';
    const ia = E.outC(seg(t, C.t0, C.t0 + 0.4)) * (1 - seg(t, C.t1, C.t1 + 0.3));
    g.fillStyle = ink(0.45 * ia); g.fillText(C.n + '  —', x + 4, base - 108 + 8 * (1 - ia));
    g.letterSpacing = '0px';
    // the word: letters rise from behind a mask
    g.save(); g.beginPath(); g.rect(x - 20, base - 110, 600, 136); g.clip();
    g.font = 'italic 400 88px NR';
    let acc = '';
    [...C.w].forEach((c, i) => {
      const xx = x + g.measureText(acc).width; acc += c;
      const kin = E.outX(seg(t, C.t0 + 0.08 + i * 0.03, C.t0 + 0.78 + i * 0.03));
      const kout = E.inX(seg(t, C.t1 + i * 0.022, C.t1 + 0.42 + i * 0.022));
      const dy = 120 * (1 - kin) - 120 * kout;
      g.fillStyle = ink(1); g.fillText(c, xx, base + dy);
    });
    g.restore();
    g.font = '400 16px IN';
    C.d.forEach((ln, i) => {
      const k = E.outC(seg(t, C.t0 + 0.35 + i * 0.07, C.t0 + 0.9 + i * 0.07)) * (1 - seg(t, C.t1 + 0.05 * i, C.t1 + 0.3 + 0.05 * i));
      g.fillStyle = ink(0.55 * k); g.fillText(ln, x + 4, base + 50 + i * 24 + 10 * (1 - k));
    });
    g.restore();
  }
}
const GLYPHS = 'abcdefghijklmnopqrstuvwxyz0123456789—·';
function scramble(txt, t, t0, seed) {
  const k = seg(t, t0, t0 + 0.45); if (k >= 1) return txt; if (k <= 0) return '';
  const r = mulberry32(seed + Math.floor(t * 30)); let s = '';
  for (let i = 0; i < txt.length; i++) { const f = i / txt.length; s += f < k - 0.25 ? txt[i] : f < k ? (txt[i] === ' ' ? ' ' : GLYPHS[r() * GLYPHS.length | 0]) : ''; }
  return s;
}
const SECTIONS = [[0.15, '01  notas'], [2.55, '02  livro'], [5.6, '03  storyboard'], [8.95, '04  imagens'], [11.85, '05  ateliet']];
function drawHUD(t) {
  const a = 1 - seg(t, 12.9, 13.3); if (a <= 0) return;
  g.save(); g.font = '500 13px IN'; g.letterSpacing = '1.5px'; g.textBaseline = 'alphabetic';
  const col = ink(0.5 * a); g.fillStyle = col;
  const M = 60, top = 64, bot = H - 52;
  g.fillText(scramble('ateliet', t, 0.05, 1), M, top);
  g.textAlign = 'center'; g.fillStyle = ink(0.5 * a * (1 - seg(t, 9.55, 9.75) + seg(t, 11.5, 11.8))); g.fillText(scramble('oficina de escrita e desenho', t, 0.15, 2), W / 2, top); g.fillStyle = col;
  g.textAlign = 'right';
  const f = Math.floor(t * FPS + 1e-6), ss = Math.floor(f / FPS), ff = f % FPS;
  const tc = `00:${String(ss).padStart(2, '0')}:${String(ff).padStart(2, '0')}`;
  g.fillText(t < 0.3 ? scramble(tc, t, 0.0, 3) : tc, W - M, top);
  g.fillText(scramble('nativo  ·  abre o vault do obsidian', t, 0.25, 4), W - M, bot);
  g.textAlign = 'left';
  // current section, swapped with a scramble
  let cur = SECTIONS[0]; for (const s of SECTIONS) if (t >= s[0]) cur = s;
  g.fillText(scramble(cur[1], t, cur[0], 5 + SECTIONS.indexOf(cur)), M, bot);
  // timeline ruler
  const y = bot - 26, x0 = M, x1 = W - M, k = E.outQ(seg(t, 0.0, 0.6));
  g.fillStyle = ink(0.14 * a); g.fillRect(x0, y, (x1 - x0) * k, 1);
  g.fillStyle = ink(0.5 * a); g.fillRect(x0, y - 0.5, (x1 - x0) * clamp(t / DUR) * k, 2);
  SECTIONS.forEach(([s]) => { const xx = x0 + (x1 - x0) * s / DUR; g.fillStyle = ink(0.3 * a * k); g.fillRect(xx, y - 4, 1, 9); });
  g.restore();
}

// ═════════════════════════ composition ═════════════════════════
function drawScene(t) {
  g.setTransform(1, 0, 0, 1, 0, 0); g.globalAlpha = 1; g.filter = 'none'; noShadow();
  // the table: the background darkens a little when the page appears
  const tbl = 0.055 * E.ioC(seg(t, 2.15, 2.8)) * (1 - E.ioC(seg(t, 8.75, 9.3)));
  g.fillStyle = rgba(mixc(PAPER, INK, tbl)); g.fillRect(0, 0, W, H);
  const cam = camera(t);
  g.save(); applyCam(cam);
  drawBook(t);
  if (t >= 4.97 && t < 5.72) drawSheet(t, null);
  drawManga(t);
  drawMosaic(t);
  drawMark(t);
  drawWordmark(t);
  cursors(t);
  g.restore();
  drawSearch(t);
  drawTagline(t);
  drawCaptions(t);
}

function renderFrame(f, S = 8, shutter = 0.5) {
  out.globalAlpha = 1;
  for (let s = 0; s < S; s++) {
    const t = clamp((f + (S === 1 ? 0 : (s + 0.5) / S * shutter - shutter / 2)) / FPS, 0, DUR - 1e-4);
    drawScene(t);
    out.globalAlpha = 1 / (s + 1); out.drawImage(buf, 0, 0);
  }
  // grain and vignette (outside the motion blur)
  const r = mulberry32(f * 7 + 3);
  out.globalAlpha = 0.045; out.globalCompositeOperation = 'overlay';
  const ox = -(r() * 512 | 0), oy = -(r() * 512 | 0);
  for (let y = oy; y < H; y += 512) for (let x = ox; x < W; x += 512) out.drawImage(GRAIN, x, y);
  out.globalCompositeOperation = 'source-over'; out.globalAlpha = 1;
  const vg = out.createRadialGradient(W / 2, H / 2, H * 0.45, W / 2, H / 2, H * 1.05);
  vg.addColorStop(0, ink(0)); vg.addColorStop(1, ink(0.07));
  out.fillStyle = vg; out.fillRect(0, 0, W, H);
  // the HUD stays sharp: drawn once, outside the blur
  const gb = g; g = out; g.save(); drawHUD(f / FPS); g.restore(); g = gb;
}

window.ready = (async () => {
  await Promise.all(['400 64px NR', 'italic 400 64px NR', '400 20px IN', '500 20px IN'].map(f => document.fonts.load(f)));
  await document.fonts.ready;
  layout();
  window.renderFrame = renderFrame;
  window.NF = NF;
  window.typing = { tt: SENT.tt, chars: SENT.chars };
  return true;
})();

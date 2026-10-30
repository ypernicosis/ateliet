#!/usr/bin/env node
// Renders the ateliet film: Chromium draws each frame (film.js), ffmpeg encodes,
// sound.py synthesises the sound track, and ffmpeg muxes both.
//
//   node brand/video/render.js                     # writes brand/video/ateliet.mp4
//   node brand/video/render.js --stills 2.5,7.6 dir  # PNG stills at those times, for checking
//
// Needs Node with Playwright (and its Chromium), ffmpeg, and python3 with numpy.
// Newsreader is fetched from Google Fonts into ~/.cache/ateliet-brand, like brand/build.py.
const { chromium } = require('playwright');
const http = require('http'), fs = require('fs'), os = require('os'), path = require('path');
const { spawn, execFileSync } = require('child_process');

const HERE = __dirname, ROOT = path.resolve(HERE, '..', '..');
const CACHE = process.env.ATELIET_FONT_CACHE || path.join(os.homedir(), '.cache', 'ateliet-brand');
const SAMPLES = 8; // motion-blur subframes per frame

function newsreader() {
  const files = { 'Newsreader-latin.woff2': 'normal', 'NewsreaderItalic-latin.woff2': 'italic' };
  fs.mkdirSync(CACHE, { recursive: true });
  for (const [name, style] of Object.entries(files)) {
    const dst = path.join(CACHE, name);
    if (fs.existsSync(dst)) continue;
    const css = execFileSync('curl', ['-sSfL', '-A', 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36',
      'https://fonts.googleapis.com/css2?family=Newsreader:ital,opsz,wght@0,6..72,300..500;1,6..72,300..500']).toString();
    const block = css.split('@font-face').find(b => b.includes(`font-style: ${style}`) && b.includes('U+0000-00FF'));
    execFileSync('curl', ['-sSfL', '-o', dst, block.match(/url\((https[^)]+)\)/)[1]]);
  }
}

// the mark's paths and the wordmark's subpaths, straight from the lockup
function brandJs() {
  const svg = fs.readFileSync(path.join(ROOT, 'brand', 'lockup.svg'), 'utf8');
  const ds = [...svg.matchAll(/<path[^>]* d="([^"]*)"/g)].map(m => m[1]);
  const words = ds[3].split('M').filter(s => s.trim()).map(s => {
    const n = s.match(/-?\d+\.?\d*/g).map(Number);
    return { d: 'M' + s, x: n[0], y: n[1] };
  });
  return `const MARK_PAPER=${JSON.stringify(ds[0])};\nconst MARK_INK=${JSON.stringify(ds[1])};\nconst MARK_BLUE=${JSON.stringify(ds[2])};\nconst WORDMARK=${JSON.stringify(words)};\n`;
}

function serve() {
  const brand = brandJs();
  const files = {
    '/': path.join(HERE, 'index.html'),
    '/film.js': path.join(HERE, 'film.js'),
    '/fonts/Inter-Regular.ttf': path.join(ROOT, 'assets', 'fonts', 'Inter-Regular.ttf'),
    '/fonts/Inter-Medium.ttf': path.join(ROOT, 'assets', 'fonts', 'Inter-Medium.ttf'),
    '/fonts/Newsreader-latin.woff2': path.join(CACHE, 'Newsreader-latin.woff2'),
    '/fonts/NewsreaderItalic-latin.woff2': path.join(CACHE, 'NewsreaderItalic-latin.woff2'),
  };
  const mime = { '.html': 'text/html', '.js': 'text/javascript', '.woff2': 'font/woff2', '.ttf': 'font/ttf' };
  const srv = http.createServer((q, r) => {
    const url = q.url.split('?')[0];
    if (url === '/brand.js') { r.writeHead(200, { 'Content-Type': 'text/javascript' }); r.end(brand); return; }
    const f = files[url];
    if (!f) { r.writeHead(404); r.end(); return; }
    r.writeHead(200, { 'Content-Type': mime[path.extname(f)] }); r.end(fs.readFileSync(f));
  });
  return new Promise(res => srv.listen(0, '127.0.0.1', () => res(srv)));
}

function run(cmd, args) {
  return new Promise((res, rej) => spawn(cmd, args, { stdio: 'inherit' }).on('close', c => c ? rej(new Error(`${cmd} exited ${c}`)) : res()));
}

(async () => {
  newsreader();
  const srv = await serve();
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  page.on('pageerror', e => { console.error(e.message); process.exit(1); });
  await page.goto(`http://127.0.0.1:${srv.address().port}/`);
  await page.evaluate(() => window.ready);
  const grab = async (f, S) => Buffer.from(await page.evaluate(([f, S]) => {
    renderFrame(f, S); return document.getElementById('c').toDataURL('image/png').split(',')[1];
  }, [f, S]), 'base64');

  if (process.argv[2] === '--stills') {
    const dir = process.argv[4] || '.';
    fs.mkdirSync(dir, { recursive: true });
    for (const t of process.argv[3].split(',').map(Number))
      fs.writeFileSync(path.join(dir, `t${t.toFixed(2).padStart(5, '0')}.png`), await grab(Math.round(t * 60), SAMPLES));
  } else {
    const out = process.argv[2] || path.join(HERE, 'ateliet.mp4');
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'ateliet-film-'));
    const silent = path.join(tmp, 'silent.mp4'), typing = path.join(tmp, 'typing.json'), wav = path.join(tmp, 'sound.wav');
    const NF = await page.evaluate(() => window.NF);
    const ff = spawn('ffmpeg', ['-y', '-v', 'error', '-f', 'image2pipe', '-framerate', '60', '-c:v', 'png', '-i', '-',
      '-c:v', 'libx264', '-preset', 'slow', '-crf', '14', '-pix_fmt', 'yuv420p', '-tune', 'animation', silent], { stdio: ['pipe', 'inherit', 'inherit'] });
    const done = new Promise(r => ff.on('close', r));
    for (let f = 0; f < NF; f++) {
      if (!ff.stdin.write(await grab(f, SAMPLES))) await new Promise(r => ff.stdin.once('drain', r));
      if (f % 60 === 0) process.stdout.write(`\rframe ${f}/${NF}`);
    }
    ff.stdin.end(); await done; console.log(`\rframe ${NF}/${NF}`);
    fs.writeFileSync(typing, JSON.stringify(await page.evaluate(() => window.typing)));
    await run('python3', ['-I', path.join(HERE, 'sound.py'), typing, wav]);
    await run('ffmpeg', ['-y', '-v', 'error', '-i', silent, '-i', wav, '-map', '0:v', '-map', '1:a', '-c:v', 'copy',
      '-af', 'loudnorm=I=-16:TP=-1.5:LRA=11', '-ar', '48000', '-c:a', 'aac', '-b:a', '256k', '-movflags', '+faststart', '-shortest', out]);
    fs.rmSync(tmp, { recursive: true, force: true });
    console.log(out);
  }
  await browser.close(); srv.close();
})();

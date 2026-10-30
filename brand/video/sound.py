# Synthesised sound design, synced to the film's events.
# usage: python3 sound.py typing.json out.wav
import json, wave, sys
import numpy as np

SR = 48000
DUR = 15.0
N = int(SR * DUR)
L = np.zeros(N); R = np.zeros(N)
rng = np.random.default_rng(4)
T = json.load(open(sys.argv[1]))


def add(sig, t0, gain=1.0, pan=0.0):
    i0 = int(t0 * SR)
    if i0 >= N: return
    sig = sig[: N - i0] * gain
    gl, gr = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    L[i0:i0 + len(sig)] += sig * gl * 1.414
    R[i0:i0 + len(sig)] += sig * gr * 1.414


def onepole(x, a):  # one-pole low-pass
    y = np.empty_like(x); s = 0.0
    for i in range(len(x)):
        s += a * (x[i] - s); y[i] = s
    return y


def lp(x, fc):
    a = 1 - np.exp(-2 * np.pi * fc / SR)
    return onepole(x, a)


def bp_sweep(n, f0, f1, q=2.0):  # noise through a sweeping band-pass (state-variable filter)
    x = rng.standard_normal(n)
    f = np.geomspace(f0, f1, n)
    y = np.empty(n); lo = bd = 0.0
    for i in range(n):
        F = 2 * np.sin(np.pi * f[i] / SR)
        hi = x[i] - lo - bd / q
        bd += F * hi; lo += F * bd
        y[i] = bd
    return y / (np.max(np.abs(y)) + 1e-9)


def env(n, a, d, shape=2.0):
    t = np.arange(n) / SR
    e = np.minimum(1, t / max(a, 1e-4)) * np.exp(-np.maximum(0, t - a) / d)
    return e ** (shape / 2)


def click(gain=1.0, bright=6000):
    n = int(0.035 * SR)
    x = rng.standard_normal(n) * env(n, 0.0005, 0.004)
    x = x - lp(x, bright)  # drop the lows: a dry tick
    body = np.sin(2 * np.pi * (1800 + rng.uniform(-200, 200)) * np.arange(n) / SR) * env(n, 0.0003, 0.006) * 0.25
    return (x * 0.7 + body) * gain


def whoosh(dur, f0, f1, peak=0.6, q=1.6):
    n = int(dur * SR)
    t = np.linspace(0, 1, n)
    e = np.sin(np.pi * np.clip(t / peak, 0, 1) / 2) ** 2 * np.where(t > peak, np.cos(np.pi / 2 * (t - peak) / (1 - peak)) ** 2, 1)
    return bp_sweep(n, f0, f1, q) * e


def paper(dur=0.35):
    n = int(dur * SR)
    x = rng.standard_normal(n)
    x = lp(lp(lp(x, 3800), 3800), 3800) - lp(x, 600)
    flutter = 0.6 + 0.4 * np.sin(2 * np.pi * 23 * np.arange(n) / SR + rng.uniform(0, 6)) ** 2
    t = np.linspace(0, 1, n)
    return x * flutter * np.sin(np.pi * t) ** 1.5 * 1.6


def mixs(*xs):
    n = max(len(x) for x in xs); y = np.zeros(n)
    for x in xs: y[:len(x)] += x
    return y


def pop(f0=420, f1=180, dur=0.18):
    n = int(dur * SR); t = np.arange(n) / SR
    f = f1 + (f0 - f1) * np.exp(-t * 40)
    ph = 2 * np.pi * np.cumsum(f) / SR
    return np.sin(ph) * env(n, 0.002, 0.05)


def thump(f=58, dur=0.5):
    n = int(dur * SR); t = np.arange(n) / SR
    ph = 2 * np.pi * np.cumsum(f * (1 + 1.5 * np.exp(-t * 30))) / SR
    return np.sin(ph) * env(n, 0.003, 0.14)


def bell(f, dur=3.0, gain=1.0):
    n = int(dur * SR); t = np.arange(n) / SR
    parts = [(1, 1, 1.4), (2.0, 0.35, 0.9), (2.76, 0.18, 0.6), (5.4, 0.06, 0.3), (8.93, 0.025, 0.2)]
    x = sum(a * np.sin(2 * np.pi * f * r * t) * np.exp(-t / d) for r, a, d in parts)
    x *= np.minimum(1, t / 0.004)
    return x * gain


def pad(freqs, t0, t1, gain):
    n = int((t1 - t0) * SR); t = np.arange(n) / SR
    x = np.zeros(n)
    for i, f in enumerate(freqs):
        det = 1 + 0.0015 * np.sin(2 * np.pi * (0.13 + i * 0.05) * t + i)
        x += np.sin(2 * np.pi * f * np.cumsum(det) / SR) / len(freqs)
        x += 0.25 * np.sin(2 * np.pi * 2 * f * np.cumsum(det) / SR + i) / len(freqs)
    fade = np.minimum(1, t / 1.2) * np.minimum(1, (t[-1] - t) / 1.0)
    return x * fade * gain


# ── harmonic bed in D, opening up at the wordmark ──
D = 146.83
add(pad([D / 2, D, D * 1.5, D * 2 * 1.122], 0.0, 13.2, 0.05), 0.0)
add(pad([D / 2, D, D * 1.5, D * 1.25 * 2, D * 3], 12.9, 15.0, 0.06), 12.9)

# ── note: typing ──
for i, (t, c) in enumerate(zip(T['tt'], T['chars'])):
    add(click(0.18 if c != ' ' else 0.24, 5000 if c != ' ' else 3000), t, pan=-0.25 + i / len(T['tt']) * 0.5)
add(click(0.1), 0.12)  # the caret appears
add(pop(1400, 900, 0.08) * 0.08, 1.78)  # highlight

# ── book ──
add(whoosh(1.1, 200, 1800, 0.55, 1.2), 1.95, 0.22)
for k in range(14):  # lines being set
    add(click(0.035, 9000), 2.32 + k * 0.03 + rng.uniform(0, 0.01), pan=rng.uniform(-0.3, 0.3))
add(paper(0.45), 3.38, 0.13, 0.35)
for k in range(10):
    add(click(0.03, 9000), 3.66 + k * 0.025, pan=0.4)
add(paper(0.5), 4.33, 0.14, 0.1)
add(thump(70, 0.4), 4.92, 0.25)
add(whoosh(0.7, 300, 1400, 0.5, 1.4), 4.9, 0.16)
add(whoosh(0.7, 1200, 300, 0.4, 1.4), 5.68, 0.14)
add(thump(64, 0.4), 6.22, 0.2)

# ── storyboard ──
add(whoosh(0.4, 800, 3000, 0.7, 3), 6.12, 0.06)  # inner frame
add(click(0.25, 3000), 6.6)
add(whoosh(0.37, 2500, 6000, 0.8, 4), 6.6, 0.1, -0.1)
add(mixs(pop(260, 120, 0.25) * 0.3, click(0.25, 2500)), 6.97)
add(click(0.25, 3000), 7.12)
add(whoosh(0.3, 2500, 6000, 0.8, 4), 7.12, 0.1, 0.1)
add(mixs(pop(240, 110, 0.25) * 0.3, click(0.25, 2500)), 7.42)
add(whoosh(0.55, 4000, 9000, 0.5, 5), 7.5, 0.035, -0.2)  # blue pencil
add(pop(520, 260, 0.2), 7.78, 0.22, -0.3)  # balloon
for k in range(24):  # focus lines
    add(click(0.03, 10000), 7.95 + rng.uniform(0, 0.4), pan=0.35)
add(whoosh(0.6, 3500, 8000, 0.5, 5), 7.95, 0.03, 0.1)  # ink

# ── pictures ──
add(whoosh(0.9, 250, 2200, 0.5, 1.2), 8.7, 0.2)
for k in range(40):
    add(click(0.025 + rng.uniform(0, 0.02), 10000), 9.0 + rng.uniform(0, 0.75) ** 1.4, pan=rng.uniform(-0.8, 0.8))
add(whoosh(0.4, 600, 2500, 0.6, 2), 9.6, 0.08)  # search
q = 'lua sobre o mar'
for i, c in enumerate(q):
    add(click(0.13 if c != ' ' else 0.17, 5500), 9.95 + i * 0.033, pan=-0.05 + i * 0.01)
add(bell(1174.66, 0.9, 0.05), 10.52)
add(whoosh(0.6, 400, 1600, 0.5, 1.5), 10.48, 0.1)
add(click(0.3, 2500), 11.15)
add(pop(900, 500, 0.12) * 0.12, 11.16)
add(whoosh(0.6, 2000, 250, 0.35, 1.3), 11.4, 0.18)
add(thump(55, 0.6), 11.92, 0.3)

# ── mark ──
add(click(0.25, 3000), 12.12)
add(whoosh(0.38, 2000, 6000, 0.8, 4), 12.12, 0.1)
add(pop(260, 120, 0.25) * 0.4, 12.5)
add(click(0.25, 3000), 12.62)
add(pop(600, 300, 0.3), 12.62, 0.18)
add(whoosh(0.6, 400, 1800, 0.45, 1.3), 13.08, 0.12)
add(bell(D * 4, 2.6, 0.16), 13.5, 0.0)
add(bell(D * 6, 2.4, 0.08), 13.56, 0.15)
add(bell(D * 2 * 1.25 * 2, 2.2, 0.06), 13.62, -0.15)
add(thump(48, 0.9), 13.5, 0.35)

# a short echo, for air
d = int(0.11 * SR)
for ch in (L, R):
    e = np.zeros_like(ch); e[d:] = ch[:-d] * 0.18; e[2 * d:] += ch[:-2 * d] * 0.07
    ch += lp(e, 4000)

# fades and normalisation
fade = np.ones(N); fi, fo = int(0.02 * SR), int(0.35 * SR)
fade[:fi] = np.linspace(0, 1, fi); fade[-fo:] = np.linspace(1, 0, fo) ** 2
L *= fade; R *= fade
pk = max(np.abs(L).max(), np.abs(R).max())
L, R = L / pk * 0.84, R / pk * 0.84
st = (np.stack([L, R], 1) * 32767).astype('<i2')
with wave.open(sys.argv[2], 'wb') as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR); w.writeframes(st.tobytes())
print('ok', pk)

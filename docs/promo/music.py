#!/usr/bin/env python3
"""Original background music for the Headphones Control commercial.

Everything is synthesised here (no samples, no third-party recordings), so the
result can be published freely with the project.

    python3 docs/promo/music.py music.wav

A minor, 120 BPM (one beat = 0.5 s), so every scene cut of the video
(4, 9.5, 15, 20, 25 and 29.5 s) falls on a beat and gets a whoosh + hit.
Needs only numpy.
"""
import sys
import wave

import numpy as np

SR = 44100
DURATION = 34.0
N = int(SR * DURATION)
BEAT = 0.5
CUTS = [4.0, 9.5, 15.0, 20.0, 25.0, 29.5]
GROOVE = (4.0, 29.5)  # drums play in this range
rng = np.random.default_rng(7)


def midi(m):
    return 440.0 * 2 ** ((m - 69) / 12)


# Am - F - C - G, two bars (4 s) each, voiced around middle C.
CHORDS = [
    [57, 60, 64, 69],  # Am
    [53, 57, 60, 65],  # F
    [55, 60, 64, 67],  # C
    [55, 59, 62, 67],  # G
]
ROOTS = [45, 41, 48, 43]


def chord_at(t):
    return int(t // 4.0) % 4


class Bus:
    def __init__(self):
        self.l = np.zeros(N)
        self.r = np.zeros(N)

    def add(self, sig, start, gain=1.0, pan=0.0):
        i = int(start * SR)
        if i >= N:
            return
        sig = sig[: N - i]
        a = (pan + 1) * np.pi / 4
        self.l[i:i + len(sig)] += sig * gain * np.cos(a)
        self.r[i:i + len(sig)] += sig * gain * np.sin(a)

    def stereo(self):
        return np.stack([self.l, self.r])


def tt(n):
    return np.arange(n) / SR


def saw(freq, n, harmonics=10, phase=0.0):
    t = tt(n)
    out = np.zeros(n)
    for k in range(1, harmonics + 1):
        if k * freq > SR / 2 - 1000:
            break
        out += np.sin(2 * np.pi * k * freq * t + phase * k) / k
    return out


def lowpass(x, cutoff, order=2):
    spec = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / SR)
    spec *= 1 / np.sqrt(1 + (f / cutoff) ** (2 * order))
    return np.fft.irfft(spec, len(x))


def highpass(x, cutoff, order=2):
    spec = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / SR)
    with np.errstate(divide="ignore"):
        spec *= 1 / np.sqrt(1 + (cutoff / np.maximum(f, 1e-6)) ** (2 * order))
    return np.fft.irfft(spec, len(x))


def sweep_filter(noise, start_hz, end_hz, frame=2048):
    """Band-pass-ish noise whose centre frequency glides (overlap-add)."""
    hop = frame // 2
    win = np.hanning(frame)
    out = np.zeros(len(noise) + frame)
    f = np.fft.rfftfreq(frame, 1 / SR)
    frames = max(1, (len(noise) - frame) // hop + 1)
    for j in range(frames):
        seg = noise[j * hop:j * hop + frame]
        if len(seg) < frame:
            seg = np.pad(seg, (0, frame - len(seg)))
        c = start_hz * (end_hz / start_hz) ** (j / max(1, frames - 1))
        resp = np.exp(-0.5 * (np.log2(np.maximum(f, 1) / c) / 0.9) ** 2)
        out[j * hop:j * hop + frame] += np.fft.irfft(np.fft.rfft(seg * win) * resp, frame)
    return out[: len(noise)]


# ----------------------------------------------------------------- instruments
def pad_voice(freq, length, detune):
    n = int(length * SR)
    s = saw(freq * 2 ** (detune / 1200), n, 12, rng.uniform(0, 6)) + saw(
        freq * 2 ** (-detune / 1200), n, 12, rng.uniform(0, 6))
    t = tt(n)
    env = np.minimum(1, t / 0.9) * np.minimum(1, (length - t) / 1.2).clip(0, 1)
    return s * env


def pluck(freq, length=0.6):
    n = int(length * SR)
    t = tt(n)
    s = sum(np.sin(2 * np.pi * freq * k * t) * np.exp(-t * (6 + 5 * k)) / k for k in range(1, 6))
    return s * np.minimum(1, t / 0.003)


def kick():
    n = int(0.45 * SR)
    t = tt(n)
    f = 44 + 90 * np.exp(-t / 0.035)
    phase = 2 * np.pi * np.cumsum(f) / SR
    body = np.sin(phase) * np.exp(-t / 0.22)
    click = rng.standard_normal(n) * np.exp(-t / 0.002) * 0.25
    return body + click


HAT = highpass(rng.standard_normal(int(0.08 * SR)), 7000, 3) * np.exp(-tt(int(0.08 * SR)) / 0.018)
CLAP_N = int(0.25 * SR)
_clap = sweep_filter(rng.standard_normal(CLAP_N), 1400, 1600)
_ct = tt(CLAP_N)
CLAP = _clap * (np.exp(-_ct / 0.09) + 0.6 * np.exp(-np.abs(_ct - 0.012) / 0.003) + 0.5 * np.exp(-np.abs(_ct - 0.024) / 0.003))


def bass_note(freq, length):
    n = int(length * SR)
    t = tt(n)
    s = np.sin(2 * np.pi * freq * t) + 0.35 * np.sin(4 * np.pi * freq * t) + 0.12 * saw(freq, n, 6)
    return s * np.minimum(1, t / 0.005) * np.exp(-t / 0.35)


def impact(length=1.6):
    n = int(length * SR)
    t = tt(n)
    f = 34 + 40 * np.exp(-t / 0.08)
    sub = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / 0.55)
    air = lowpass(rng.standard_normal(n), 2500) * np.exp(-t / 0.25) * 0.5
    return sub + air


def whoosh(length):
    n = int(length * SR)
    t = tt(n)
    s = sweep_filter(rng.standard_normal(n), 400, 6000)
    return s * (t / length) ** 2.2 * np.minimum(1, (length - t) / 0.02)


def bell(freq, length=4.0):
    n = int(length * SR)
    t = tt(n)
    partials = [(1, 1.0, 2.2), (2.01, 0.45, 1.4), (3.0, 0.25, 0.9), (4.17, 0.15, 0.6)]
    s = sum(a * np.sin(2 * np.pi * freq * r * t) * np.exp(-t / d) for r, a, d in partials)
    return s * np.minimum(1, t / 0.004)


# ----------------------------------------------------------------- arrangement
pad, arp, drums, bass, fx, verb_send = Bus(), Bus(), Bus(), Bus(), Bus(), Bus()

# Pads: one chord per 4 s, overlapping tails; the final chord rings out.
for i, start in enumerate(np.arange(0, 29.5, 4.0)):
    chord = CHORDS[i % 4]
    length = min(4.0, 29.5 - start) + 1.2
    for j, m in enumerate(chord):
        pan = (-0.5, 0.5, -0.25, 0.25)[j]
        pad.add(pad_voice(midi(m), length, 7 + j), start, 0.05, pan)
for j, m in enumerate([57, 64, 69, 71, 76]):  # Am(add9) final chord
    pad.add(pad_voice(midi(m), 4.5, 6 + j), 29.5, 0.055, (-0.6, -0.2, 0.2, 0.6, 0)[j])

# Arpeggio: eighth notes over chord tones, from 1 s until the final hit.
pattern = [0, 1, 2, 3, 2, 1, 2, 3]
step = BEAT / 2
for k, start in enumerate(np.arange(1.0, 29.5, step)):
    chord = CHORDS[chord_at(start)]
    note = chord[pattern[k % len(pattern)]] + 12
    gain = 0.34 * min(1, (start - 1.0) / 3 + 0.3)
    p = pluck(midi(note))
    arp.add(p, start, gain, 0.3 * np.sin(k * 0.9))
    # Ping-pong echo (3/8 beat... dotted eighth).
    arp.add(p, start + 0.375, gain * 0.38, -0.6)
    arp.add(p, start + 0.75, gain * 0.16, 0.6)

# Drums and bass in the groove section.
K = kick()
pump = np.ones(N)
t_all = tt(N)
for b in np.arange(GROOVE[0], GROOVE[1], BEAT):
    drums.add(K, b, 0.55)
    i = int(b * SR)
    seg = t_all[i:i + int(0.4 * SR)] - b
    pump[i:i + len(seg)] = np.minimum(pump[i:i + len(seg)], 1 - 0.5 * np.exp(-seg / 0.11))
    drums.add(HAT, b + BEAT / 2, 0.13, 0.25)
    if b >= 9.5 and int(round((b - GROOVE[0]) / BEAT)) % 2 == 1:
        drums.add(CLAP, b, 0.20, -0.1)
        verb_send.add(CLAP, b, 0.12)
    root = ROOTS[chord_at(b)]
    for off in (0, BEAT / 2):
        bass.add(bass_note(midi(root), 0.24), b + off, 0.30 if off == 0 else 0.22)
# Build-up: sixteenth hats + claps getting louder in the last second.
for k, b in enumerate(np.arange(28.5, 29.5, BEAT / 4)):
    drums.add(HAT, b, 0.08 + 0.02 * k, 0.1)
    drums.add(CLAP, b, 0.05 + 0.025 * k)

# Transitions: whoosh into every cut, soft impact on it; big one at the end card.
fx.add(whoosh(2.0), 2.0, 0.22)
for c in CUTS:
    if c > 4.0:
        fx.add(whoosh(0.7), c - 0.7, 0.16, 0.2)
    big = c in (4.0, 29.5)
    fx.add(impact(2.4 if big else 1.2), c, 0.55 if big else 0.25)
for j, m in enumerate([69, 76, 81]):
    fx.add(bell(midi(m), 4.5), 29.5, 0.10, (-0.4, 0.4, 0)[j])
    verb_send.add(bell(midi(m), 4.5), 29.5, 0.08)

# ----------------------------------------------------------------- mix
pad_st = np.stack([lowpass(pad.l, 1900, 2), lowpass(pad.r, 1900, 2)])
in_groove = (t_all >= GROOVE[0]) & (t_all < GROOVE[1])
pad_st *= np.where(in_groove, pump, 1.0)
bass_st = np.stack([lowpass(bass.l, 900), lowpass(bass.r, 900)]) * np.where(in_groove, pump, 1.0)

verb_in = verb_send.stereo() + 0.35 * arp.stereo() + 0.25 * pad_st
ir_n = int(2.6 * SR)
irt = tt(ir_n)
mix = pad_st + arp.stereo() + drums.stereo() + bass_st + fx.stereo()
for ch in range(2):
    ir = rng.standard_normal(ir_n) * np.exp(-irt / 0.55)
    ir = lowpass(ir, 5000) / np.sqrt(np.sum(ir ** 2))
    size = 1 << int(np.ceil(np.log2(N + ir_n)))
    wet = np.fft.irfft(np.fft.rfft(verb_in[ch], size) * np.fft.rfft(ir, size), size)[:N]
    mix[ch] += 0.28 * wet

mix = highpass(mix[0], 28), highpass(mix[1], 28)
mix = np.stack(mix)
fade = np.ones(N)
fade[: int(0.05 * SR)] = np.linspace(0, 1, int(0.05 * SR))
tail = int(2.2 * SR)
fade[-tail:] = np.linspace(1, 0, tail) ** 1.5
mix *= fade
mix = np.tanh(mix * 1.1) / np.tanh(1.1)
mix *= 0.89 / np.max(np.abs(mix))  # peak about -1 dBFS

out = sys.argv[1] if len(sys.argv) > 1 else "music.wav"
with wave.open(out, "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes((mix.T * 32767).astype("<i2").tobytes())
print(f"wrote {out}: {DURATION:.1f} s, peak {20 * np.log10(np.max(np.abs(mix))):.1f} dBFS")

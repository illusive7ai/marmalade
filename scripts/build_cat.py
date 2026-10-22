"""Turn the Dreamina clips into Marmalade sprite packs (256x256 RGBA lossless WebP + anim.toml).

Run:  python -I build_cat.py [anim ...]     (no args = all)
"""
import os
import subprocess
import sys

import numpy as np
from PIL import Image

SCRATCH = r'C:\Users\User\AppData\Local\Temp\claude\C--Users-User-OneDrive-Desktop-PROJECTS-CLAUDE-IDEAS\40331c1d-71f7-46e7-9f9e-bb7753b400ef\scratchpad'
A = SCRATCH + r'\clips'    # first batch: c1..c6
B = SCRATCH + r'\clips2'   # second batch: d1..d5
OUT = r'C:\dev\marmalade\assets\cat'
PREVIEW = SCRATCH + r'\preview'

SW, SH = 832, 1120        # source size
CANVAS = 256
FOOT = (128, 248)         # where the feet go in the 256x256 frame
S = 0.33                  # shared source->frame scale (same for every clip so the cat is the same size)
SCALE = {'carry': 0.29, 'lie': 0.30}   # per-animation overrides where the cat is wider than the frame
S_PEEK = 0.18             # the peek clip is framed much closer; shrink it so the head matches the others
FPS_SRC = 24

# name: (file, t0, t1, mode, loop-search or None, fps)
#   mode: 'stand' (feet on the ground, centred), 'follow' (centred per frame), 'leap', 'peek'
ANIMS = {
    'walk':  (B + r'\d1.mp4', 0.0, 4.0, 'stand', (30, 72), 12),
    'sleep': (B + r'\d4.mp4', 0.0, 4.0, 'stand', (48, 90), 12),
    'sit':   (A + r'\c5.mp4', 1.7, 4.0, 'stand', (36, 54), 12),
    'lie':   (A + r'\c3.mp4', 1.7, 4.0, 'stand', (30, 54), 12),
    'carry': (B + r'\d5.mp4', 2.9, 4.0, 'follow', (16, 25), 12),
    'leap':  (B + r'\d2.mp4', 1.3, 2.45, 'leap', None, 12),
    'peek':  (B + r'\d3.mp4', 0.3, 3.9, 'peek', None, 15),
}


def read_frames(path, t0, t1):
    cmd = ['ffmpeg', '-v', 'error', '-ss', str(t0), '-t', str(t1 - t0), '-i', path,
           '-vf', f'fps={FPS_SRC}', '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-']
    raw = subprocess.run(cmd, capture_output=True, check=True).stdout
    n = len(raw) // (SW * SH * 3)
    return np.frombuffer(raw, np.uint8)[: n * SW * SH * 3].reshape(n, SH, SW, 3)


def key(rgb):
    """Hue key: the backdrop is saturated green, the fur is neutral. Returns straight-alpha RGBA uint8."""
    a = rgb.astype(np.float32)
    r, g, b = a[..., 0], a[..., 1], a[..., 2]
    greenness = g - np.maximum(r, b)
    alpha = np.clip(1.0 - (greenness - 6.0) / 22.0, 0.0, 1.0)
    g2 = np.minimum(g, np.maximum(r, b))                      # despill
    # Generator badge (top-left) and watermark (bottom-right): the cat never reaches these corners.
    alpha[: int(0.06 * SH), : int(0.14 * SW)] = 0
    alpha[int(0.93 * SH):, int(0.66 * SW):] = 0
    out = np.empty((SH, SW, 4), np.uint8)
    out[..., 0], out[..., 1], out[..., 2] = r, g2, b
    out[..., 3] = (alpha * 255 + 0.5).astype(np.uint8)
    return out


def bbox(rgba):
    ys, xs = np.nonzero(rgba[..., 3] > 128)
    if len(xs) == 0:
        return None
    return xs.min(), xs.max(), ys.min(), ys.max()


def scale_rgba(rgba, s):
    w, h = round(SW * s), round(SH * s)
    im = Image.fromarray(rgba, 'RGBA').convert('RGBa').resize((w, h), Image.LANCZOS).convert('RGBA')
    return np.asarray(im)


def place(scaled, ox, oy):
    canvas = np.zeros((CANVAS, CANVAS, 4), np.uint8)
    h, w = scaled.shape[:2]
    x0, y0 = max(ox, 0), max(oy, 0)
    x1, y1 = min(ox + w, CANVAS), min(oy + h, CANVAS)
    if x1 > x0 and y1 > y0:
        canvas[y0:y1, x0:x1] = scaled[y0 - oy: y1 - oy, x0 - ox: x1 - ox]
    return canvas


def smooth(v, k=5):
    pad = np.pad(v, (k // 2, k // 2), mode='edge')
    return np.convolve(pad, np.ones(k) / k, mode='valid')


def build_frames(name):
    path, t0, t1, mode, _, _ = ANIMS[name]
    src = read_frames(path, t0, t1)
    keyed = [key(f) for f in src]
    if name == 'peek':   # the ledge the cat is behind: everything below its top edge is ledge, not cat
        for k in keyed:
            k[int(0.922 * SH):] = 0
    boxes = [bbox(k) for k in keyed]
    keep = [i for i, b in enumerate(boxes) if b is not None]
    keyed = [keyed[i] for i in keep]
    boxes = [boxes[i] for i in keep]
    left = min(b[0] for b in boxes)
    right = max(b[1] for b in boxes)
    bottom = max(b[3] for b in boxes)
    cx_union = (left + right) / 2

    S_ = SCALE.get(name, S)
    if mode == 'stand':
        ox = round(FOOT[0] - S_ * cx_union)
        oy = round(FOOT[1] - S_ * bottom)
        offs = [(ox, oy)] * len(keyed)
        s = S_
    elif mode == 'follow':
        cxs = smooth(np.array([(b[0] + b[1]) / 2 for b in boxes]), 7)
        oy = round(FOOT[1] - S_ * bottom)
        offs = [(round(FOOT[0] - S_ * cx), oy) for cx in cxs]
        s = S_
    elif mode == 'leap':
        b0 = boxes[0]
        ox = round(98 - S * (b0[0] + b0[1]) / 2)
        oy = round(FOOT[1] - S * b0[3])
        offs = [(ox, oy)] * len(keyed)
        s = S
    elif mode == 'peek':
        ox = round(FOOT[0] - S_PEEK * cx_union)
        oy = round((CANVAS - 1) - S_PEEK * (SH - 1))
        offs = [(ox, oy)] * len(keyed)
        s = S_PEEK
    else:
        raise ValueError(mode)
    return [place(scale_rgba(k, s), *o) for k, o in zip(keyed, offs)]


def small(c):
    f = c[::4, ::4].astype(np.float32) / 255.0
    f[..., :3] *= f[..., 3:4]
    return f


def find_loop(frames, lo, hi, a_min=0):
    sm = [small(f) for f in frames]
    best = None
    for L in range(lo, hi + 1, 2):
        for a in range(a_min, len(frames) - L):
            d = float(np.mean((sm[a] - sm[a + L]) ** 2))
            if best is None or d < best[0]:
                best = (d, a, L)
    return best


def mouth_point(frames):
    pts = []
    for c in frames:
        al = c[..., 3] > 128
        ys, xs = np.nonzero(al)
        if len(xs) == 0:
            continue
        top, bot = ys.min(), ys.max()
        rows = np.nonzero(al.any(axis=1))[0]
        rows = rows[rows < top + 0.6 * (bot - top)]
        best_x, best_y = -1, 0
        for y in rows:
            xr = np.nonzero(al[y])[0].max()
            if xr > best_x:
                best_x, best_y = xr, y
        pts.append((best_x, best_y))
    p = np.mean(pts, axis=0)
    return int(round(p[0])) - 3, int(round(p[1])) + 5


def save_pack(name, frames, fps, loop, mouth=None):
    d = os.path.join(OUT, name)
    os.makedirs(d, exist_ok=True)
    for f in os.listdir(d):
        os.remove(os.path.join(d, f))
    for i, f in enumerate(frames, 1):
        Image.fromarray(f, 'RGBA').save(os.path.join(d, f'{i:04d}.webp'), 'WEBP', lossless=True, quality=100, method=6)
    toml = f'fps = {fps}\nloop = {str(loop).lower()}\nframes = {len(frames)}\n\n[anchor]\nx = {FOOT[0]}\ny = {FOOT[1]}\n'
    if mouth:
        toml += f'\n[mouth]\nx = {mouth[0]}\ny = {mouth[1]}\n'
    with open(os.path.join(d, 'anim.toml'), 'w', newline='\n') as fh:
        fh.write(toml)


def preview(name, frames):
    os.makedirs(PREVIEW, exist_ok=True)
    n = len(frames)
    idx = np.linspace(0, n - 1, min(n, 8)).round().astype(int)
    tiles = []
    for i in idx:
        bg = Image.new('RGBA', (CANVAS, CANVAS), (70, 110, 170, 255))
        bg.alpha_composite(Image.fromarray(frames[i], 'RGBA'))
        tiles.append(bg.convert('RGB').resize((160, 160)))
    sheet = Image.new('RGB', (160 * len(tiles), 160))
    for k, t in enumerate(tiles):
        sheet.paste(t, (160 * k, 0))
    sheet.save(os.path.join(PREVIEW, f'{name}.png'))


def main(names):
    for name in names:
        _, _, _, mode, loop_cfg, fps = ANIMS[name]
        frames = build_frames(name)
        info = f'{name}: {len(frames)} source frames'
        step = max(1, round(FPS_SRC / fps))
        if loop_cfg:
            a_min = 0
            if name in ('sit', 'lie'):
                a_min = 0
            d, a, L = find_loop(frames, *loop_cfg, a_min=a_min)
            L -= L % 2
            seg = frames[a:a + L][::step]
            info += f', loop a={a} L={L} seam_mse={d:.5f}'
            loop = True
        elif name == 'peek':
            # 1.5x faster than the source, sampled on a 15 fps grid
            n = int((len(frames) - 1) / FPS_SRC / 0.1) + 1
            seg = [frames[min(len(frames) - 1, round(k * 0.1 * FPS_SRC))] for k in range(n)]
            loop = False
        else:
            seg = frames[::step]
            loop = False
        mouth = mouth_point(seg) if name == 'carry' else None
        save_pack(name, seg, fps, loop, mouth)
        preview(name, seg)
        size = sum(os.path.getsize(os.path.join(OUT, name, f)) for f in os.listdir(os.path.join(OUT, name)))
        print(info, f'-> {len(seg)} frames @ {fps} fps, {size / 1024:.0f} KB', f'mouth={mouth}' if mouth else '')


if __name__ == '__main__':
    main(sys.argv[1:] or list(ANIMS))


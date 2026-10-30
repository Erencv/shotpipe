#!/usr/bin/env python3
"""Generate the Shotpipe icon set (assets/) — pure stdlib.

Design: deep midnight-navy rounded square; a screenshot crop-frame (cyan
brackets, image glyph inside) streams a luminous cyan beam into a terminal
window (lavender accents). Soft glows, vignette, top sheen for dimension.

Writes: assets/Shotpipe.icns, preview-app-icon.png (1024), preview-sizes.png.
"""

import math
import os
import struct
import subprocess
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = os.path.join(ROOT, "assets")
S = 1024  # master size

CYAN = (0.133, 0.827, 0.933)
CYAN_HI = (0.55, 0.95, 1.0)
LAV = (0.647, 0.706, 0.988)
NAVY_TOP = (0.062, 0.105, 0.192)
NAVY_BOT = (0.024, 0.047, 0.102)
TERM_TOP = (0.106, 0.169, 0.290)
TERM_BOT = (0.055, 0.094, 0.188)


# ------------------------------------------------------------- png writer --

def write_png(path, rgba, w, h):
    def chunk(t, d):
        return (struct.pack(">I", len(d)) + t + d
                + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF))
    raw = b"".join(
        b"\x00" + bytes(rgba[y * w * 4:(y + 1) * w * 4]) for y in range(h)
    )
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9))
           + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(png)


# ------------------------------------------------------------- primitives --

def smooth(a, b, x):
    t = max(0.0, min(1.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


def rr_sd(x, y, cx, cy, hw, hh, r):
    qx = abs(x - cx) - (hw - r)
    qy = abs(y - cy) - (hh - r)
    ux, uy = max(qx, 0.0), max(qy, 0.0)
    return math.hypot(ux, uy) + min(max(qx, qy), 0.0) - r


def seg_dist(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    t = max(0.0, min(1.0, (wx * vx + wy * vy) / (vx * vx + vy * vy)))
    return math.hypot(px - (ax + t * vx), py - (ay + t * vy))


def in_tri(px, py, a, b, c):
    def cross(p, q, r):
        return (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    d1, d2, d3 = cross(a, b, (px, py)), cross(b, c, (px, py)), cross(c, a, (px, py))
    return not ((d1 < 0 or d2 < 0 or d3 < 0) and (d1 > 0 or d2 > 0 or d3 > 0))


# --------------------------------------------------------------- canvas ----

class Canvas:
    def __init__(self, w, h):
        self.w, self.h = w, h
        self.px = [0.0] * (w * h * 3)
        self.glow = [0.0] * (w * h * 3)

    def blend(self, x, y, color, a):
        i = (y * self.w + x) * 3
        p = self.px
        ia = 1.0 - a
        p[i] = p[i] * ia + color[0] * a
        p[i + 1] = p[i + 1] * ia + color[1] * a
        p[i + 2] = p[i + 2] * ia + color[2] * a

    def darken(self, x, y, f):
        i = (y * self.w + x) * 3
        self.px[i] *= f
        self.px[i + 1] *= f
        self.px[i + 2] *= f

    def add_glow(self, x, y, color, k):
        i = (y * self.w + x) * 3
        self.glow[i] += color[0] * k
        self.glow[i + 1] += color[1] * k
        self.glow[i + 2] += color[2] * k

    def rgba(self):
        out = bytearray(self.w * self.h * 4)
        p, g = self.px, self.glow
        for i in range(self.w * self.h):
            j = i * 3
            r = min(1.0, p[j] + g[j])
            gg = min(1.0, p[j + 1] + g[j + 1])
            b = min(1.0, p[j + 2] + g[j + 2])
            k = i * 4
            out[k], out[k + 1], out[k + 2], out[k + 3] = int(r * 255), int(gg * 255), int(b * 255), 255
        return out


def region(c, x0, y0, x1, y1):
    return [(x, y) for y in range(max(0, y0), min(c.h, y1))
            for x in range(max(0, x0), min(c.w, x1))]


# -------------------------------------------------------------- drawing ----

def bez(p0, p1, p2, n):
    return [((1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t ** 2 * p2[0],
             (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t ** 2 * p2[1])
            for t in (i / n for i in range(n + 1))]


def draw():
    c = Canvas(S, S)
    C = S / 2

    # ---- background: navy gradient + center lavender tint + sheen + vignette
    for y in range(S):
        gy = y / S
        br, bg, bb = (NAVY_TOP[i] + (NAVY_BOT[i] - NAVY_TOP[i]) * gy for i in range(3))
        for x in range(S):
            xf = x + 0.5
            dx, dy = xf - C, y + 0.5 - C
            d2 = dx * dx + dy * dy
            tint = 0.040 * max(0.0, 1.0 - d2 / (460.0 * 460.0))
            sheen = 0.016 * (1.0 - y / S)
            vig = 1.0 - 0.28 * smooth(390.0 * 390.0, 780.0 * 780.0, d2)
            i = (y * S + x) * 3
            c.px[i] = (br + LAV[0] * tint + CYAN[0] * sheen) * vig
            c.px[i + 1] = (bg + LAV[1] * tint + CYAN[1] * sheen) * vig
            c.px[i + 2] = (bb + LAV[2] * tint + CYAN[2] * sheen) * vig

    # ---- soft shadows under the two elements (dimensionality)
    for (sx, sy, rx, ry, k) in [(292, 700, 210, 42, 0.30), (708, 706, 240, 46, 0.34)]:
        for x, y in region(c, sx - rx, sy - ry, sx + rx, sy + ry):
            dx, dy = (x + 0.5 - sx) / rx, (y + 0.5 - sy) / ry
            c.darken(x, y, 1.0 - k * max(0.0, 1.0 - dx * dx - dy * dy))

    # ================= screenshot frame (left) =================
    fx, fy, fh = 292.0, 512.0, 140.0  # center + half-size
    arms = []
    armlen, stroke = 96.0, 28.0
    for (bx, by, sxs, sys_) in [(fx - fh, fy - fh, 1, 1), (fx + fh, fy - fh, -1, 1),
                                (fx - fh, fy + fh, 1, -1), (fx + fh, fy + fh, -1, -1)]:
        cx0 = bx if sxs > 0 else bx - stroke
        cy0 = by if sys_ > 0 else by - stroke
        ax0, ax1 = (bx, bx + armlen) if sxs > 0 else (bx - armlen, bx)
        ay0, ay1 = (by, by + armlen) if sys_ > 0 else (by - armlen, by)
        arms.append((ax0, cy0, ax1, cy0 + stroke))       # horizontal
        arms.append((cx0, ay0, cx0 + stroke, ay1))       # vertical

    # bracket drop shadow, then brackets (cyan with subtle gradient)
    for x, y in region(c, int(fx - fh - 10), int(fy - fh - 10), int(fx + fh + 14), int(fy + fh + 14)):
        xf, yf = x + 0.5, y + 0.5
        for (a, b, cc, d) in arms:
            if rr_sd(xf + 5, yf + 7, (a + cc) / 2, (b + d) / 2,
                     abs(cc - a) / 2 + 8, abs(d - b) / 2 + 8, 10.0) < 0:
                c.blend(x, y, (0.0, 0.0, 0.0), 0.38)
                break
    for x, y in region(c, int(fx - fh - 4), int(fy - fh - 4), int(fx + fh + 4), int(fy + fh + 4)):
        xf, yf = x + 0.5, y + 0.5
        for (a, b, cc, d) in arms:
            sd = rr_sd(xf, yf, (a + cc) / 2, (b + d) / 2,
                       abs(cc - a) / 2, abs(d - b) / 2, 8.0)
            if sd < 1.5:
                mix = 0.22 * smooth(fy - fh, fy + fh, yf)  # cyan → cyan-lavender
                col = (CYAN[0] + (LAV[0] - CYAN[0]) * mix,
                       CYAN[1] + (LAV[1] - CYAN[1]) * mix,
                       CYAN[2] + (LAV[2] - CYAN[2]) * mix)
                c.blend(x, y, col, smooth(1.5, -1.5, sd))

    # image glyph inside the frame: sun + two mountains, faint lavender
    sun = (fx + 62, fy - 58, 30.0)
    m1 = ((fx - 112, fy + 72), (fx - 40, fy - 34), (fx + 32, fy + 72))
    m2 = ((fx - 34, fy + 72), (fx + 34, fy - 12), (fx + 104, fy + 72))
    for x, y in region(c, int(fx - 130), int(fy - 96), int(fx + 130), int(fy + 92)):
        xf, yf = x + 0.5, y + 0.5
        a = 0.30 if in_tri(xf, yf, *m1) else 0.0
        b = 0.45 if in_tri(xf, yf, *m2) else 0.0
        if a or b:
            c.blend(x, y, LAV, max(a, b))
        d = math.hypot(xf - sun[0], yf - sun[1])
        if d < sun[2] + 1.5:
            c.blend(x, y, LAV, 0.55 * smooth(sun[2] + 1.5, sun[2] - 2.0, d))

    # ================= terminal window (right) =================
    tx, ty, thw, thh, tr = 708.0, 512.0, 168.0, 142.0, 26.0
    for x, y in region(c, int(tx - thw - 4), int(ty - thh - 4), int(tx + thw + 5), int(ty + thh + 5)):
        xf, yf = x + 0.5, y + 0.5
        sd = rr_sd(xf, yf, tx, ty, thw, thh, tr)
        if sd < 1.5:
            g = smooth(ty - thh, ty + thh, yf)
            fill = tuple(TERM_TOP[i] + (TERM_BOT[i] - TERM_TOP[i]) * g for i in range(3))
            c.blend(x, y, fill, smooth(1.5, -1.5, sd))
            if sd > -4.0:  # hairline lavender border
                c.blend(x, y, LAV, 0.28 * smooth(1.5, -1.5, sd) * smooth(-1.5, -4.0, sd))
        # header divider
        if abs(yf - 428.0) < 2.0 and rr_sd(xf, yf, tx, 428.0, thw - 18, 1.5, 1.5) < 0:
            c.blend(x, y, LAV, 0.22)
        # traffic dots (recolored)
        for i, (dx, col, al) in enumerate([(-132, CYAN, 0.85), (-100, LAV, 0.75), (-68, LAV, 0.40)]):
            d = math.hypot(xf - (tx + dx), yf - 399.0)
            if d < 8.5 + 1.5:
                c.blend(x, y, col, al * smooth(10.0, 7.0, d))
        # prompt lines
        for (yy, x0, x1, col, al, r) in [
            (470.0, -148, -78, CYAN, 0.85, 9.0), (470.0, -60, 96, LAV, 0.55, 9.0),
            (528.0, -148, 50, LAV, 0.35, 9.0), (586.0, -148, -18, LAV, 0.22, 9.0),
        ]:
            d = seg_dist(xf, yf, tx + x0, yy, tx + x1, yy) - r
            if d < 1.5:
                c.blend(x, y, col, al * smooth(1.5, -1.5, d))

    # ================= luminous flow: frame → terminal =================
    p0, p1, p2 = (fx + fh + 12, fy - 12), (497.0, 434.0), (tx - thw - 4, fy - 2)
    path = bez(p0, p1, p2, 34)
    segs = list(zip(path[:-1], path[1:]))
    for x, y in region(c, 430, 400, 570, 560):
        xf, yf = x + 0.5, y + 0.5
        dmin, tmin = 1e9, 0.0
        for i, ((ax, ay), (bx, by)) in enumerate(segs):
            d = seg_dist(xf, yf, ax, ay, bx, by)
            if d < dmin:
                dmin, tmin = d, i / len(segs)
        w = 19.0 - 10.0 * tmin  # taper
        if dmin < w + 2:
            hot = math.exp(-(dmin / (w * 0.5)) ** 2)
            col = (CYAN[0] + (1 - CYAN[0]) * hot, CYAN[1] + (1 - CYAN[1]) * hot,
                   CYAN[2] + (1 - CYAN[2]) * hot)
            c.blend(x, y, col, 0.92 * smooth(w + 2.0, w - 3.0, dmin))
        # glow (subtle)
        gk = math.exp(-(dmin / 30.0) ** 2) * 0.30 + math.exp(-(dmin / 85.0) ** 2) * 0.10
        c.add_glow(x, y, CYAN, gk)
    # node at the frame edge, bloom where the beam enters the terminal
    for (nx, ny, r, k) in [(p0[0] + 2, p0[1], 26.0, 0.45), (p2[0] + 6, p2[1], 46.0, 0.60)]:
        for x, y in region(c, int(nx - 3 * r), int(ny - 3 * r), int(nx + 3 * r), int(ny + 3 * r)):
            d = math.hypot(x + 0.5 - nx, y + 0.5 - ny)
            c.add_glow(x, y, CYAN_HI, k * math.exp(-(d / r) ** 2))
    # two travelling particles
    for (px, py, r) in [(path[13][0], path[13][1], 7.0), (path[24][0], path[24][1], 5.0)]:
        for x, y in region(c, int(px - 30), int(py - 30), int(px + 30), int(py + 30)):
            d = math.hypot(x + 0.5 - px, y + 0.5 - py)
            if d < r + 1.5:
                c.blend(x, y, (1, 1, 1), 0.9 * smooth(r + 1.5, r - 1.5, d))
            c.add_glow(x, y, CYAN_HI, 0.5 * math.exp(-(d / 14.0) ** 2))

    # ---- rounded-square mask (macOS squircle)
    out = c.rgba()
    for y in range(S):
        for x in range(S):
            sd = rr_sd(x + 0.5, y + 0.5, C, C, C, C, 0.2375 * S)
            if sd > 0.5:
                out[(y * S + x) * 4 + 3] = 0
            elif sd > -0.5:
                out[(y * S + x) * 4 + 3] = int(255 * smooth(0.5, -0.5, sd))
    return out


# ------------------------------------------------------------- png decode --

def png_decode_rgba(path):
    """Minimal PNG decoder: 8-bit RGB/RGBA, non-interlaced (Figma exports)."""
    data = open(path, "rb").read()
    pos, idat, w = 8, b"", 0
    while pos < len(data):
        ln = struct.unpack(">I", data[pos:pos + 4])[0]
        typ, ch = data[pos + 4:pos + 8], data[pos + 8:pos + 8 + ln]
        if typ == b"IHDR":
            w, h, depth, ct, _, _, inter = struct.unpack(">IIBBBBB", ch[:13])
            assert depth == 8 and inter == 0 and ct in (2, 6), "need 8-bit RGB/RGBA"
        elif typ == b"IDAT":
            idat += ch
        pos += 12 + ln
    raw = zlib.decompress(idat)
    bpp = 4 if ct == 6 else 3
    stride = w * bpp
    out = bytearray(w * h * 4)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        f = raw[p]
        p += 1
        line = bytearray(raw[p:p + stride])
        p += stride
        if f == 1:
            for i in range(bpp, stride):
                line[i] = (line[i] + line[i - bpp]) & 255
        elif f == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif f == 3:
            for i in range(stride):
                a = line[i - bpp] if i >= bpp else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 255
        elif f == 4:
            for i in range(stride):
                a = line[i - bpp] if i >= bpp else 0
                b, c = prev[i], prev[i - bpp] if i >= bpp else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        prev = line
        for x in range(w):
            i, k = x * bpp, (y * w + x) * 4
            out[k], out[k + 1], out[k + 2] = line[i], line[i + 1], line[i + 2]
            out[k + 3] = line[i + 3] if ct == 6 else 255
    return out, w, h


def bilinear(src, sw, sh, dw, dh):
    out = bytearray(dw * dh * 4)
    for y in range(dh):
        fy = y * (sh - 1) / max(1, dh - 1)
        y0, ty = int(fy), fy - int(fy)
        y1 = min(y0 + 1, sh - 1)
        for x in range(dw):
            fx = x * (sw - 1) / max(1, dw - 1)
            x0, tx = int(fx), fx - int(fx)
            x1 = min(x0 + 1, sw - 1)
            k = (y * dw + x) * 4
            for ch in range(4):
                a = src[(y0 * sw + x0) * 4 + ch] * (1 - tx) + src[(y0 * sw + x1) * 4 + ch] * tx
                b = src[(y1 * sw + x0) * 4 + ch] * (1 - tx) + src[(y1 * sw + x1) * 4 + ch] * tx
                out[k + ch] = int(a * (1 - ty) + b * ty)
    return out


def apply_rounded_mask(rgba, size):
    C = size / 2
    for y in range(size):
        for x in range(size):
            sd = rr_sd(x + 0.5, y + 0.5, C, C, C, C, 0.2375 * size)
            if sd > 0.5:
                rgba[(y * size + x) * 4 + 3] = 0
            elif sd > -0.5:
                rgba[(y * size + x) * 4 + 3] = int(rgba[(y * size + x) * 4 + 3] * smooth(0.5, -0.5, sd))
    return rgba


def fit_master(src, sw, sh):
    """Normalize any square-ish design PNG into a 1024 RGBA master."""
    if sw != sh:  # letterbox to square, centered
        side = max(sw, sh)
        tmp = bytearray(side * side * 4)
        ox, oy = (side - sw) // 2, (side - sh) // 2
        for y in range(sh):
            tmp[((oy + y) * side + ox) * 4:((oy + y) * side + ox + sw) * 4] = src[y * sw * 4:(y + 1) * sw * 4]
        src, sw, sh = tmp, side, side
    if sw < S:
        src = bilinear(src, sw, sh, S, S)
    elif sw > S:
        src = downsample(src, sw, S) if sw % S == 0 else bilinear(src, sw, sh, S, S)
    corners_opaque = all(src[i * 4 + 3] > 250 for i in (3, S - 4, (S - 4) * S + 3, (S - 1) * S + S - 4))
    if corners_opaque:  # design shipped square; give it macOS corners
        src = apply_rounded_mask(src, S)
    return src


# ------------------------------------------------------------- resample ----

def downsample(src, sw, dw):
    box = sw // dw
    out = bytearray(dw * dw * 4)
    for y in range(dw):
        for x in range(dw):
            r = g = b = a = 0.0
            for yy in range(y * box, (y + 1) * box):
                for xx in range(x * box, (x + 1) * box):
                    j = (yy * sw + xx) * 4
                    pa = src[j + 3] / 255.0
                    r += src[j] * pa
                    g += src[j + 1] * pa
                    b += src[j + 2] * pa
                    a += pa
            n = box * box
            k = (y * dw + x) * 4
            if a > 0:
                out[k], out[k + 1], out[k + 2] = int(r / a), int(g / a), int(b / a)
            out[k + 3] = int(a / n * 255)
    return out


def paste(dst, src, sw, dw, ox, oy):
    for y in range(dw):
        for x in range(dw):
            si = (y * dw + x) * 4
            di = ((oy + y) * sw + ox + x) * 4
            a = src[si + 3] / 255.0
            for ch in range(3):
                dst[di + ch] = int(dst[di + ch] * (1 - a) + src[si + ch] * a)
            dst[di + 3] = max(dst[di + 3], src[si + 3])


# ---------------------------------------------------------------- build ----

def main():
    os.makedirs(ASSETS, exist_ok=True)
    design = os.path.join(ASSETS, "design", "app-icon.png")
    if os.path.exists(design):
        src, sw, sh = png_decode_rgba(design)
        print(f"using provided design app-icon.png ({sw}x{sh})")
        master = fit_master(src, sw, sh)
    else:
        master = draw()
    write_png(os.path.join(ASSETS, "preview-app-icon.png"), master, S, S)

    # size-legibility strip
    sizes = [256, 128, 64, 48, 32, 16]
    strip_w = sum(sizes) + 40 * (len(sizes) + 1)
    strip = bytearray([13, 21, 38, 255] * (strip_w * 300))
    ox = 40
    for sz in sizes:
        paste(strip, downsample(master, S, sz) if sz < S else master,
              strip_w, sz, ox, (300 - sz) // 2)
        ox += sz + 40
    write_png(os.path.join(ASSETS, "preview-sizes.png"), strip, strip_w, 300)

    iconset = os.path.join(ASSETS, "Shotpipe.iconset")
    os.makedirs(iconset, exist_ok=True)
    for sz in [16, 32, 128, 256, 512]:
        for suffix, actual in [("", sz), ("@2x", sz * 2)]:
            data = master if actual > S else downsample(master, S, actual)
            write_png(os.path.join(iconset, f"icon_{sz}x{sz}{suffix}.png"),
                      data, actual, actual)
    subprocess.run(["iconutil", "-c", "icns", iconset,
                    "-o", os.path.join(ASSETS, "Shotpipe.icns")], check=True)
    subprocess.run(["rm", "-rf", iconset], check=True)
    print("wrote assets/Shotpipe.icns, preview-app-icon.png, preview-sizes.png")


if __name__ == "__main__":
    main()

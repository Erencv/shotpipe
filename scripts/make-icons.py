#!/usr/bin/env python3
"""Generate the Shotpipe icon set (assets/) — pure stdlib.

Draws the 1024px master (rounded-square gradient + screenshot brackets +
arrow), box-downsamples to every icns size, and builds assets/Shotpipe.icns
with iconutil. Also writes preview PNGs so humans can eyeball the result.
"""

import math
import os
import struct
import subprocess
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = os.path.join(ROOT, "assets")
S = 1024  # master size

TOP = (0.259, 0.212, 0.898)   # indigo
BOT = (0.024, 0.678, 0.800)   # teal/cyan


# ------------------------------------------------------------- png writer --

def write_png(path, rgba, w, h):
    def chunk(t, d):
        return (struct.pack(">I", len(d)) + t + d
                + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF))
    raw = b"".join(
        b"\x00" + bytes(rgba[(y * w + x) * 4:(y * w + x) * 4 + 4])
        for y in range(h) for x in range(w)
    )
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9))
           + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(png)


# ------------------------------------------------------------- geometry ----

def rounded_rect_sd(x, y, cx, cy, hw, hh, r):
    qx = abs(x - cx) - (hw - r)
    qy = abs(y - cy) - (hh - r)
    ux, uy = max(qx, 0.0), max(qy, 0.0)
    return math.hypot(ux, uy) + min(max(qx, qy), 0.0) - r


def in_rect(x, y, x0, y0, x1, y1):
    return x0 <= x < x1 and y0 <= y < y1


# ------------------------------------------------------------- drawing -----

def draw_master():
    rgba = bytearray(S * S * 4)
    c = S / 2
    # Same proportions as the menu bar icon (36px: m=3, t=4, a=10, dot r=4.5)
    bi, t, a = S * 3 / 36, S * 4 / 36, S * 10 / 36  # bracket inset, stroke, arm
    dot_r = S * 4.5 / 36
    corners = [(bi, bi, 1, 1), (S - bi, bi, -1, 1),
               (bi, S - bi, 1, -1), (S - bi, S - bi, -1, -1)]
    for y in range(S):
        for x in range(S):
            xf, yf = x + 0.5, y + 0.5
            i = (y * S + x) * 4
            # rounded-square mask, macOS-ish radius
            if rounded_rect_sd(xf, yf, c, c, c, c, 0.2375 * S) > 0:
                continue
            # diagonal-ish vertical gradient
            g = clamp((yf / S * 0.82 + xf / S * 0.18))
            r = lerp(TOP[0], BOT[0], g) * 255
            gr = lerp(TOP[1], BOT[1], g) * 255
            b = lerp(TOP[2], BOT[2], g) * 255
            white = False
            # screenshot brackets (menu bar motif)
            for (bx, by, sx, sy) in corners:
                cx0 = bx if sx > 0 else bx - t
                cy0 = by if sy > 0 else by - t
                ax0, ax1 = (bx, bx + a) if sx > 0 else (bx - a, bx)
                ay0, ay1 = (by, by + a) if sy > 0 else (by - a, by)
                if (in_rect(xf, yf, ax0, cy0, ax1, cy0 + t)
                        or in_rect(xf, yf, cx0, ay0, cx0 + t, ay1)):
                    white = True
                    break
            # filled center dot (the "active" state of the menu bar icon)
            if math.hypot(xf - c, yf - c) <= dot_r:
                white = True
            if white:
                rgba[i:i + 3] = b"\xff\xff\xff"
            else:
                rgba[i] = int(r)
                rgba[i + 1] = int(gr)
                rgba[i + 2] = int(b)
            rgba[i + 3] = 255
    return rgba


def clamp(v):
    return max(0.0, min(1.0, v))


def lerp(a, b, t):
    return a + (b - a) * t


def downsample(src, sw, dw):
    dh = dw
    box = sw // dw
    out = bytearray(dw * dh * 4)
    for y in range(dh):
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
            if a > 0:
                k = (y * dw + x) * 4
                out[k] = int(r / a)
                out[k + 1] = int(g / a)
                out[k + 2] = int(b / a)
            out[(y * dw + x) * 4 + 3] = int(a / n * 255)
    return out


# --------------------------------------------------------------- build -----

def main():
    os.makedirs(ASSETS, exist_ok=True)
    master = draw_master()
    write_png(os.path.join(ASSETS, "preview-app-icon.png"), master, S, S)

    iconset = os.path.join(ASSETS, "Shotpipe.iconset")
    os.makedirs(iconset, exist_ok=True)
    sizes = [16, 32, 128, 256, 512]
    for sz in sizes:
        for suffix, actual in [("", sz), ("@2x", sz * 2)]:
            if actual > S:
                data = master
            else:
                data = downsample(master, S, actual)
            write_png(os.path.join(iconset, f"icon_{sz}x{sz}{suffix}.png"),
                      data, actual, actual)
    subprocess.run(["iconutil", "-c", "icns", iconset,
                    "-o", os.path.join(ASSETS, "Shotpipe.icns")], check=True)
    subprocess.run(["rm", "-rf", iconset], check=True)
    print("wrote assets/Shotpipe.icns + assets/preview-app-icon.png")


if __name__ == "__main__":
    main()

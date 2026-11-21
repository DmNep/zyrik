"""Рисует значок «о_О» → assets/zyrik.ico (16, 20, 24, 32, 40, 48, 64, 256 px).

Два глаза: левый обычный «о», правый вытаращен «О» и «зырит» (маленький зрачок в большом белке),
между ними «_». Белые глаза на синей плашке читаются и на светлой, и на тёмной панели задач.
Только стандартная библиотека Python. Запуск: python tools/make_icon.py [--preview out.png]
"""
import struct
import sys
import zlib
from pathlib import Path

INK = (24, 24, 32)
WHITE = (255, 255, 255)
BADGE = (45, 108, 223)  # синяя плашка: белые глаза на ней видны и на светлой, и на тёмной панели
SS = 8  # сглаживание: 8×8 проб на пиксель


def in_rounded_square(x, y, radius):
    cx = min(max(x, radius), 1 - radius)
    cy = min(max(y, radius), 1 - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius * radius


def eye(x, y, cx, cy, r, pupil_r, pdx, pdy):
    """Белок и зрачок, без контура: контраст даёт плашка."""
    if (x - cx) ** 2 + (y - cy) ** 2 > r * r:
        return None
    if (x - cx - pdx) ** 2 + (y - cy - pdy) ** 2 <= pupil_r ** 2:
        return INK
    return WHITE


def sample(x, y, size):
    if not (0 <= x <= 1 and 0 <= y <= 1) or not in_rounded_square(x, y, 0.22):
        return None
    # левый «о» — обычный глаз; правый «О» — вытаращенный, зрачок крошечный
    small = eye(x, y, 0.27, 0.58, 0.17, max(0.085, 1.0 / size), 0.015, 0.0)
    if small:
        return small
    big = eye(x, y, 0.665, 0.43, 0.27, max(0.06, 1.0 / size), -0.02, -0.01)
    if big:
        return big
    # «_» между глазами
    mouth_h = max(0.075, 1.1 / size)
    if 0.37 <= x <= 0.56 and 0.87 - mouth_h <= y <= 0.87:
        return WHITE
    return BADGE


def render(size):
    px = []
    for py in range(size):
        row = []
        for qx in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(SS):
                for sx in range(SS):
                    c = sample((qx + (sx + 0.5) / SS) / size, (py + (sy + 0.5) / SS) / size, size)
                    if c:
                        acc[0] += c[0]
                        acc[1] += c[1]
                        acc[2] += c[2]
                        acc[3] += 1
            n = acc[3]
            if n:
                row.append((acc[0] // n, acc[1] // n, acc[2] // n, n * 255 // (SS * SS)))
            else:
                row.append((0, 0, 0, 0))
        px.append(row)
    return px


def png(px):
    h, w = len(px), len(px[0])
    raw = b"".join(b"\0" + bytes(v for p in row for v in p) for row in px)

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def bmp(px):
    """32-битный DIB для ICO: снизу вверх, BGRA, плюс пустая 1-битная маска."""
    h, w = len(px), len(px[0])
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = b"".join(bytes((b, g, r, a)) for row in reversed(px) for (r, g, b, a) in row)
    mask_row = ((w + 31) // 32) * 4
    return header + pixels + b"\0" * (mask_row * h)


def ico(images):
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    blobs = []
    for size, data in images:
        d = size if size < 256 else 0
        out += struct.pack("<BBBBHHII", d, d, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
        blobs.append(data)
    return out + b"".join(blobs)


def main():
    root = Path(__file__).resolve().parent.parent
    images = []
    for size in (16, 20, 24, 32, 40, 48, 64, 256):
        px = render(size)
        images.append((size, png(px) if size == 256 else bmp(px)))
    (root / "assets" / "zyrik.ico").write_bytes(ico(images))
    if "--preview" in sys.argv:
        # все размеры в ряд на светлом и тёмном фоне, увеличено ×4 для 16–64
        out = Path(sys.argv[sys.argv.index("--preview") + 1])
        sizes = (16, 24, 32, 48, 64)
        rendered = {s: render(s) for s in sizes}
        scale, pad = 4, 8
        width = sum(s * scale + pad for s in sizes) + pad
        height = 2 * (64 * scale + pad) + pad
        canvas = []
        for y in range(height):
            bg = (243, 243, 243) if y < height // 2 else (32, 32, 32)
            canvas.append([bg + (255,)] * width)
        for half in (0, 1):
            x0 = pad
            for s in sizes:
                y0 = pad + half * (64 * scale + pad)
                for y in range(s * scale):
                    for x in range(s * scale):
                        r, g, b, a = rendered[s][y // scale][x // scale]
                        br, bg_, bb, _ = canvas[y0 + y][x0 + x]
                        canvas[y0 + y][x0 + x] = (
                            (r * a + br * (255 - a)) // 255,
                            (g * a + bg_ * (255 - a)) // 255,
                            (b * a + bb * (255 - a)) // 255,
                            255,
                        )
                x0 += s * scale + pad
        out.write_bytes(png(canvas))


if __name__ == "__main__":
    main()

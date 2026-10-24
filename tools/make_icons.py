#!/usr/bin/env python3
"""生成 Craboard 的多尺寸 ico、窗口/托盘 RGBA 和预览图。

图形与界面里的图标方案 A「蟹壳勾」一致：暖橙圆角底、深色蟹壳、奶白勾。
小尺寸不缩大图：16/20 只保留壳和勾，24/32 保留实心蟹钳，48 以上才出现钳口和腿。
"""

from __future__ import annotations

import io
import struct
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
PROBE = ROOT / ".probe"

BG = (201, 129, 90, 255)
INK = (43, 33, 26, 255)
CREAM = (246, 238, 220, 255)


def stroke(d: ImageDraw.ImageDraw, points, width: float, fill) -> None:
    w = max(1, round(width))
    d.line(points, fill=fill, width=w, joint="curve")
    r = w / 2
    for x, y in points:
        d.ellipse((x - r, y - r, x + r, y + r), fill=fill)


def render(size: int) -> Image.Image:
    ss = 16 if size <= 24 else 8 if size <= 64 else 4
    canvas = size * ss
    img = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    pad = max(1, round(size * 0.04))
    inner = size - pad * 2
    k = inner / 256
    radius = max(3, round(inner * 0.22))

    d.rounded_rectangle(
        (pad * ss, pad * ss, (size - pad) * ss, (size - pad) * ss),
        radius=radius * ss,
        fill=BG,
    )

    def p(x: float, y: float) -> tuple[float, float]:
        return ((pad + x * k) * ss, (pad + y * k) * ss)

    def rect(x, y, w, h, r, fill) -> None:
        d.rounded_rectangle(
            (*p(x, y), *p(x + w, y + h)),
            radius=r * k * ss,
            fill=fill,
        )

    def line(points, width, fill) -> None:
        stroke(d, [p(x, y) for x, y in points], width * k * ss, fill)

    def circle(cx, cy, r, fill) -> None:
        d.ellipse(
            (p(cx - r, cy - r), p(cx + r, cy + r)),
            fill=fill,
        )

    if size <= 20:
        rect(44, 86, 168, 108, 48, INK)
        check = [(86, 138), (116, 168), (174, 100)]
        check_width = 28 if size == 16 else 26
    elif size <= 32:
        line([(76, 112), (48, 86)], 16, INK)
        line([(180, 112), (208, 86)], 16, INK)
        circle(42, 78, 18, INK)
        circle(214, 78, 18, INK)
        rect(56, 92, 144, 100, 46, INK)
        check = [(92, 138), (116, 162), (166, 108)]
        check_width = 24 if size == 24 else 22
    else:
        full = size >= 48
        legs = size >= 64
        if legs:
            for start, end in (
                ((86, 188), (77, 210)),
                ((111, 194), (107, 214)),
                ((145, 194), (149, 214)),
                ((170, 188), (179, 210)),
            ):
                line([start, end], 12, INK)
        line([(80, 112), (54, 88)], 17, INK)
        line([(176, 112), (202, 88)], 17, INK)
        circle(46, 80, 21, INK)
        circle(210, 80, 21, INK)
        if full:
            circle(56, 72, 10, BG)
            circle(200, 72, 10, BG)
        rect(64, 94, 128, 96, 45, INK)
        check = [(99, 139), (118, 158), (158, 114)]
        check_width = 18 if full else 19

    line(check, check_width, CREAM)
    return img.resize((size, size), Image.LANCZOS)


def dib_bytes(img: Image.Image) -> bytes:
    w, h = img.size
    px = img.load()
    xor = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = px[x, y]
            xor += bytes((b, g, r, a))
    row = ((w + 31) // 32) * 4
    and_mask = bytes(row * h)
    header = struct.pack(
        "<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, w * h * 4, 0, 0, 0, 0
    )
    return header + bytes(xor) + and_mask


def png_bytes(img: Image.Image) -> bytes:
    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def write_ico(images: dict[int, Image.Image], path: Path) -> None:
    entries = []
    blobs = []
    offset = 6 + 16 * len(images)
    for size in sorted(images):
        data = png_bytes(images[size]) if size >= 128 else dib_bytes(images[size])
        pad = (-offset) % 4
        blobs.append((b"\0" * pad, data))
        offset += pad + len(data)
        entries.append((size, len(data), offset - len(data)))
    out = bytearray(struct.pack("<HHH", 0, 1, len(images)))
    for size, length, data_offset in entries:
        dim = 0 if size == 256 else size
        out += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, length, data_offset)
    for pad, data in blobs:
        out += pad + data
    path.write_bytes(out)


def preview(images: dict[int, Image.Image], path: Path) -> None:
    sizes = [16, 24, 32, 48, 64, 128]
    cell = 150
    canvas = Image.new("RGBA", (cell * len(sizes), 370), (35, 33, 29, 255))
    d = ImageDraw.Draw(canvas)
    for row, (label, bg) in enumerate(
        (("light taskbar", (239, 232, 216, 255)), ("dark taskbar", (26, 25, 22, 255)))
    ):
        top = 10 + row * 180
        d.rounded_rectangle(
            (10, top, canvas.width - 10, top + 170), radius=18, fill=bg
        )
        for i, size in enumerate(sizes):
            x = i * cell + (cell - size) // 2
            y = top + 18 + (128 - size) // 2
            canvas.alpha_composite(images[size], (x, y))
            d.text((i * cell + 12, top + 145), f"{size}px", fill=(120, 108, 92, 255))
    d.text((12, 350), "Craboard icon - same artwork, hand-tuned per size", fill=(200, 190, 175, 255))
    canvas.save(path)


def main() -> None:
    ASSETS.mkdir(exist_ok=True)
    PROBE.mkdir(exist_ok=True)
    images = {size: render(size) for size in (16, 20, 24, 32, 40, 48, 64, 128, 256)}
    write_ico(images, ASSETS / "craboard.ico")
    images[256].save(ASSETS / "craboard-256.png")
    (ASSETS / "tray-32.rgba").write_bytes(images[32].tobytes())
    (ASSETS / "window-64.rgba").write_bytes(images[64].tobytes())
    preview(images, PROBE / "icon-preview.png")
    print(f"ico: {(ASSETS / 'craboard.ico').stat().st_size} bytes")
    print(f"preview: {PROBE / 'icon-preview.png'}")


if __name__ == "__main__":
    main()

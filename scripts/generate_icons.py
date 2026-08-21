#!/usr/bin/env python3
"""Generate Athenaeum's application icons.

These are deliberately generated rather than committed as opaque binaries:
the mark is a placeholder until real branding exists, and a script makes that
obvious and easy to replace. Pure standard library — no Pillow, no ImageMagick
— so it runs in CI on any of the three target platforms.

    python3 scripts/generate_icons.py

Writes src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png plus icon.ico and
icon.icns, which is exactly the set tauri.conf.json references.
"""

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

ICON_DIR = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"

# A knowledge graph: connected nodes, which is what the app is about.
BACKGROUND = (18, 20, 28, 255)      # near-black indigo
EDGE = (94, 106, 138, 255)          # muted slate
NODE = (232, 217, 176, 255)         # parchment
ACCENT = (240, 176, 96, 255)        # the hub node

# Normalized coordinates in a 0..1 square.
NODES = [
    (0.50, 0.24, 0.070, ACCENT),
    (0.24, 0.46, 0.055, NODE),
    (0.76, 0.46, 0.055, NODE),
    (0.36, 0.76, 0.048, NODE),
    (0.64, 0.76, 0.048, NODE),
]
EDGES = [(0, 1), (0, 2), (1, 3), (2, 4), (3, 4), (1, 2)]

SUPERSAMPLE = 4
CORNER_RADIUS = 0.185
EDGE_WIDTH = 0.016


def blend(dst: tuple[int, ...], src: tuple[int, ...], alpha: float) -> tuple[int, ...]:
    """Composite `src` over `dst` at coverage `alpha`."""
    return tuple(int(round(d + (s - d) * alpha)) for d, s in zip(dst, src))


def in_rounded_square(x: float, y: float, radius: float) -> bool:
    """True when (x, y) in 0..1 lies inside a rounded square."""
    cx = min(max(x, radius), 1.0 - radius)
    cy = min(max(y, radius), 1.0 - radius)
    return math.hypot(x - cx, y - cy) <= radius or (
        radius <= x <= 1.0 - radius or radius <= y <= 1.0 - radius
    ) and 0.0 <= x <= 1.0 and 0.0 <= y <= 1.0


def distance_to_segment(px: float, py: float, ax: float, ay: float, bx: float, by: float) -> float:
    dx, dy = bx - ax, by - ay
    length_sq = dx * dx + dy * dy
    if length_sq == 0.0:
        return math.hypot(px - ax, py - ay)
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / length_sq))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def render(size: int) -> bytes:
    """Render the mark at `size` pixels square, returning RGBA bytes."""
    hi = size * SUPERSAMPLE
    # Render supersampled, then box-filter down. Cheap, and good enough for
    # an icon that is mostly circles and straight lines.
    buffer = [[(0, 0, 0, 0)] * hi for _ in range(hi)]

    for py in range(hi):
        y = (py + 0.5) / hi
        row = buffer[py]
        for px in range(hi):
            x = (px + 0.5) / hi
            if not in_rounded_square(x, y, CORNER_RADIUS):
                continue
            pixel = BACKGROUND

            for a, b in EDGES:
                ax, ay = NODES[a][0], NODES[a][1]
                bx, by = NODES[b][0], NODES[b][1]
                if distance_to_segment(x, y, ax, ay, bx, by) <= EDGE_WIDTH:
                    pixel = blend(pixel, EDGE, 1.0)

            for nx, ny, radius, color in NODES:
                if math.hypot(x - nx, y - ny) <= radius:
                    pixel = blend(pixel, color, 1.0)

            row[px] = pixel

    out = bytearray()
    for y in range(size):
        for x in range(size):
            r = g = b = a = 0
            for sy in range(SUPERSAMPLE):
                for sx in range(SUPERSAMPLE):
                    pr, pg, pb, pa = buffer[y * SUPERSAMPLE + sy][x * SUPERSAMPLE + sx]
                    r += pr * pa
                    g += pg * pa
                    b += pb * pa
                    a += pa
            if a == 0:
                out += bytes(4)
            else:
                out += bytes((r // a, g // a, b // a, a // (SUPERSAMPLE * SUPERSAMPLE)))
    return bytes(out)


def encode_png(size: int, rgba: bytes) -> bytes:
    def chunk(tag: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + tag
            + payload
            + struct.pack(">I", zlib.crc32(tag + payload) & 0xFFFFFFFF)
        )

    stride = size * 4
    # Filter type 0 (None) on every scanline.
    raw = b"".join(b"\x00" + rgba[y * stride : (y + 1) * stride] for y in range(size))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def encode_ico(pngs: dict[int, bytes]) -> bytes:
    """ICO with PNG payloads (Vista+), which is what Tauri's bundler emits."""
    sizes = sorted(pngs)
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = len(header) + 16 * len(sizes)
    entries, blobs = b"", b""
    for size in sizes:
        data = pngs[size]
        entries += struct.pack(
            "<BBBBHHII", size if size < 256 else 0, size if size < 256 else 0,
            0, 0, 1, 32, len(data), offset,
        )
        blobs += data
        offset += len(data)
    return header + entries + blobs


def encode_icns(pngs: dict[int, bytes]) -> bytes:
    """ICNS with PNG payloads."""
    # Only PNG-capable OSTypes; sizes must match what each type means.
    types = {32: b"ic11", 64: b"ic12", 128: b"ic07", 256: b"ic08", 512: b"ic09"}
    body = b""
    for size, tag in sorted(types.items()):
        if size not in pngs:
            continue
        data = pngs[size]
        body += tag + struct.pack(">I", len(data) + 8) + data
    return b"icns" + struct.pack(">I", len(body) + 8) + body


def main() -> None:
    ICON_DIR.mkdir(parents=True, exist_ok=True)
    sizes = [32, 64, 128, 256, 512]
    pngs = {size: encode_png(size, render(size)) for size in sizes}

    for size, name in [(32, "32x32.png"), (128, "128x128.png"),
                       (256, "128x128@2x.png"), (512, "icon.png")]:
        (ICON_DIR / name).write_bytes(pngs[size])

    (ICON_DIR / "icon.ico").write_bytes(encode_ico({s: pngs[s] for s in (32, 64, 128, 256)}))
    (ICON_DIR / "icon.icns").write_bytes(encode_icns(pngs))

    for path in sorted(ICON_DIR.iterdir()):
        print(f"  {path.name:<18} {path.stat().st_size:>7} bytes")


if __name__ == "__main__":
    main()

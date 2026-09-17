#!/usr/bin/env python3
"""Generate clAIre PNG/ICO/ICNS icons without extra Python packages."""

from __future__ import annotations

import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "src-tauri" / "icons"


def chunk(tag: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


def write_png(path: Path, size: int, rgba: bytes) -> None:
    raw = bytearray()
    stride = size * 4
    for y in range(size):
        raw.append(0)
        raw.extend(rgba[y * stride : (y + 1) * stride])
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    path.write_bytes(png)


def pixel(x: int, y: int, size: int) -> bytes:
    nx = (x + 0.5) / size * 2 - 1
    ny = (y + 0.5) / size * 2 - 1
    r2 = nx * nx + ny * ny
    # Rounded square background
    ax, ay = abs(nx), abs(ny)
    square = max(ax, ay)
    aa = max(0.0, min(1.0, (0.86 - square) * size / 3))
    bg = (18, 22, 30)
    teal = (126, 200, 195)
    # Inner disc
    disc = max(0.0, min(1.0, (0.62 - r2**0.5) * size / 4))
    r = int(bg[0] * (1 - disc) + teal[0] * disc)
    g = int(bg[1] * (1 - disc) + teal[1] * disc)
    b = int(bg[2] * (1 - disc) + teal[2] * disc)
    # Stylized C cutout
    in_ring = 0.28 < (r2**0.5) < 0.46
    right_gap = nx > 0.12 and abs(ny) < 0.22
    if in_ring and not right_gap:
        r, g, b = 244, 241, 234
    alpha = int(aa * 255)
    return bytes((r, g, b, alpha))


def raster(size: int) -> bytes:
    return b"".join(pixel(x, y, size) for y in range(size) for x in range(size))


def write_ico(path: Path, images: list[tuple[int, bytes]]) -> None:
    count = len(images)
    header = struct.pack("<HHH", 0, 1, count)
    entries = b""
    payload = b""
    offset = 6 + 16 * count
    for size, png in images:
        w = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(png), offset)
        payload += png
        offset += len(png)
    path.write_bytes(header + entries + payload)


def write_icns(path: Path, images: dict[bytes, bytes]) -> None:
    body = b""
    for tag, png in images.items():
        body += tag + struct.pack(">I", 8 + len(png)) + png
    path.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)


def main() -> None:
    ROOT.mkdir(parents=True, exist_ok=True)
    pngs: dict[int, bytes] = {}
    for size, name in [
        (32, "32x32.png"),
        (128, "128x128.png"),
        (256, "128x128@2x.png"),
        (512, "icon.png"),
        (1024, "icon-1024.png"),
    ]:
        buf = raster(size)
        dest = ROOT / name
        write_png(dest, size, buf)
        pngs[size] = dest.read_bytes()
        print(f"wrote {dest}")

    write_ico(ROOT / "icon.ico", [(32, pngs[32]), (256, pngs[256])])
    print(f"wrote {ROOT / 'icon.ico'}")
    write_icns(
        ROOT / "icon.icns",
        {
            b"icp5": pngs[32],
            b"ic07": pngs[128],
            b"ic08": pngs[256],
            b"ic09": pngs[512],
            b"ic10": pngs[1024],
        },
    )
    print(f"wrote {ROOT / 'icon.icns'}")


if __name__ == "__main__":
    main()

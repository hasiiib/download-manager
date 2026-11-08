"""Generate a small deterministic RGBA PNG before the Tauri CLI validates icon paths.

Uses only Python's standard library so CI does not require Pillow.
"""
from pathlib import Path
import struct
import zlib


def chunk(kind: bytes, data: bytes) -> bytes:
    return (struct.pack(">I", len(data)) + kind + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))


def icon_bytes() -> bytes:
    pixels = bytearray()
    for y in range(64):
        pixels.append(0)  # PNG filter method: none
        for x in range(64):
            tile = 5 <= x < 59 and 5 <= y < 59
            arrow = (29 <= x < 35 and 14 <= y < 42
                     or (22 <= y < 42 and abs(x - 32) + abs(y - 42) < 21)
                     or (21 <= x < 43 and 47 <= y < 52))
            pixels.extend((255, 255, 255, 255) if arrow else
                          (113, 89, 236, 255) if tile else (0, 0, 0, 0))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", 64, 64, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(bytes(pixels), level=9))
            + chunk(b"IEND", b""))


if __name__ == "__main__":
    output = Path(__file__).resolve().parents[1] / "src-tauri" / "icons" / "icon.png"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(icon_bytes())
    print(f"Generated {output} ({output.stat().st_size} bytes)")

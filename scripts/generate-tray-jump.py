#!/usr/bin/env python3
"""Bake a short tray jump from logo.png without redrawing the orca.

Only Python's standard library is required. The source is the repository's
256px RGBA logo; it has no checked-in vector original. Runtime playback uses
64px frames at 30ms intervals and restores the actual original icon at 720ms.
The first and last frames equal the same area-resampled static source.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import sys
import zlib


INTERVAL_MS = 30
INTERVALS = 24
OUTPUT_SIZE = 64
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
# fraction, y translation, clockwise angle, horizontal scale, vertical scale
# A small anticipation, nose-up launch, apex, landing, then a settled rebound.
KEYS = (
    (0.00, 0.0, 0.0, 1.0, 1.0),
    (0.12, 5.0, 3.0, 1.025, 0.975),
    (0.30, -24.0, -18.0, 1.0, 1.0),
    (0.46, -34.0, -8.0, 1.0, 1.0),
    (0.68, 0.0, 7.0, 1.0, 1.0),
    (0.78, 4.0, 1.0, 1.025, 0.975),
    (0.90, -4.0, -2.0, 1.0, 1.0),
    (1.00, 0.0, 0.0, 1.0, 1.0),
)


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload))
    )


def read_png(payload: bytes) -> tuple[int, int, bytes]:
    """Read the fixed source's 8-bit, non-interlaced RGBA PNG."""
    if not payload.startswith(PNG_SIGNATURE):
        raise ValueError("the source is not a PNG")
    offset, compressed, header = 8, bytearray(), None
    while offset < len(payload):
        length = struct.unpack_from(">I", payload, offset)[0]
        kind = payload[offset + 4 : offset + 8]
        data = payload[offset + 8 : offset + 8 + length]
        checksum = struct.unpack_from(">I", payload, offset + 8 + length)[0]
        if zlib.crc32(kind + data) != checksum:
            raise ValueError("the source PNG has an invalid checksum")
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", data)
        elif kind == b"IDAT":
            compressed.extend(data)
        offset += length + 12
    if header is None or header[2:] != (8, 6, 0, 0, 0):
        raise ValueError("the source must be 8-bit non-interlaced RGBA")
    width, height = header[:2]
    stride = width * 4
    raw = zlib.decompress(compressed)
    if len(raw) != (stride + 1) * height:
        raise ValueError("the source PNG has an invalid pixel payload")
    out = bytearray(stride * height)
    for y in range(height):
        filter_kind = raw[y * (stride + 1)]
        if filter_kind > 4:
            raise ValueError("the source PNG has an invalid row filter")
        for x in range(stride):
            value = raw[y * (stride + 1) + x + 1]
            left = out[y * stride + x - 4] if x >= 4 else 0
            up = out[(y - 1) * stride + x] if y else 0
            upper_left = out[(y - 1) * stride + x - 4] if y and x >= 4 else 0
            if filter_kind == 1:
                value += left
            elif filter_kind == 2:
                value += up
            elif filter_kind == 3:
                value += (left + up) // 2
            elif filter_kind == 4:
                p = left + up - upper_left
                distances = (abs(p - left), abs(p - up), abs(p - upper_left))
                value += (left, up, upper_left)[distances.index(min(distances))]
            out[y * stride + x] = value & 255
    return width, height, bytes(out)


def write_png(width: int, height: int, pixels: bytes) -> bytes:
    stride = width * 4
    rows = b"".join(
        b"\0" + pixels[y * stride : (y + 1) * stride] for y in range(height)
    )
    return (
        PNG_SIGNATURE
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(rows, level=9))
        + png_chunk(b"IEND", b"")
    )


def pose(t: float) -> tuple[float, ...]:
    for before, after in zip(KEYS, KEYS[1:]):
        if t <= after[0]:
            fraction = (t - before[0]) / (after[0] - before[0])
            eased = fraction * fraction * (3.0 - 2.0 * fraction)
            return tuple(a + (b - a) * eased for a, b in zip(before[1:], after[1:]))
    return KEYS[-1][1:]


def transform(
    width: int, height: int, pixels: bytes, values: tuple[float, ...]
) -> bytes:
    dy, angle, sx, sy = values
    cosine, sine = math.cos(math.radians(angle)), math.sin(math.radians(angle))
    cx, cy = width / 2.0, height / 2.0
    out = bytearray(width * height * 4)
    for y in range(height):
        for x in range(width):
            rx, ry = x + 0.5 - cx, y + 0.5 - cy - dy
            # Inverse rotate, inverse scale, then sample source pixel centers.
            px = (cosine * rx + sine * ry) / sx + cx - 0.5
            py = (-sine * rx + cosine * ry) / sy + cy - 0.5
            x0, y0 = math.floor(px), math.floor(py)
            if x0 < -1 or x0 >= width or y0 < -1 or y0 >= height:
                continue
            fx, fy = px - x0, py - y0
            alpha = red = green = blue = 0.0
            for ox, oy, weight in (
                (0, 0, (1.0 - fx) * (1.0 - fy)),
                (1, 0, fx * (1.0 - fy)),
                (0, 1, (1.0 - fx) * fy),
                (1, 1, fx * fy),
            ):
                qx, qy = x0 + ox, y0 + oy
                if not (0 <= qx < width and 0 <= qy < height):
                    continue
                offset = (qy * width + qx) * 4
                r, g, b, a = pixels[offset : offset + 4]
                weighted_alpha = a * weight
                alpha += weighted_alpha
                red += r * weighted_alpha
                green += g * weighted_alpha
                blue += b * weighted_alpha
            # Premultiplied-alpha interpolation keeps the transparent border clean.
            if round(alpha):
                offset = (y * width + x) * 4
                out[offset : offset + 4] = bytes(
                    (
                        round(red / alpha),
                        round(green / alpha),
                        round(blue / alpha),
                        round(alpha),
                    )
                )
    return bytes(out)


def alpha_bounds(width: int, height: int, pixels: bytes) -> tuple[int, int, int, int]:
    opaque = [
        (i % width, i // width)
        for i in range(width * height)
        if pixels[i * 4 + 3]
    ]
    if not opaque:
        raise ValueError("an animation frame is empty")
    return (
        min(x for x, _ in opaque),
        min(y for _, y in opaque),
        max(x for x, _ in opaque) + 1,
        max(y for _, y in opaque) + 1,
    )


def downsample(width: int, height: int, pixels: bytes) -> bytes:
    """Area-resample the 256px canvas with premultiplied alpha, preserving margins."""
    factor = width // OUTPUT_SIZE
    if width != height or width % OUTPUT_SIZE:
        raise ValueError("the source must divide into the output canvas exactly")
    out = bytearray(OUTPUT_SIZE * OUTPUT_SIZE * 4)
    for y in range(OUTPUT_SIZE):
        for x in range(OUTPUT_SIZE):
            alpha = red = green = blue = 0
            for oy in range(factor):
                for ox in range(factor):
                    i = ((y * factor + oy) * width + x * factor + ox) * 4
                    r, g, b, a = pixels[i : i + 4]
                    alpha += a
                    red += r * a
                    green += g * a
                    blue += b * a
            a = round(alpha / (factor * factor))
            if a:
                i = (y * OUTPUT_SIZE + x) * 4
                out[i : i + 4] = bytes(
                    (round(red / alpha), round(green / alpha), round(blue / alpha), a)
                )
    return bytes(out)


def generate(source: bytes) -> dict[str, bytes]:
    width, height, original = read_png(source)
    if (width, height) != (256, 256):
        raise ValueError("logo.png must retain its 256px source canvas")
    outputs, frames = {}, []
    static_pixels = downsample(width, height, original)
    for number in range(INTERVALS + 1):
        values = pose(number / INTERVALS)
        high_resolution = (
            original
            if number in (0, INTERVALS)
            else transform(width, height, original, values)
        )
        pixels = downsample(width, height, high_resolution)
        bounds = alpha_bounds(OUTPUT_SIZE, OUTPUT_SIZE, pixels)
        if (
            bounds[0] < 2
            or bounds[1] < 2
            or bounds[2] > OUTPUT_SIZE - 2
            or bounds[3] > OUTPUT_SIZE - 2
        ):
            raise ValueError(f"frame {number} touches the canvas border: {bounds}")
        payload = write_png(OUTPUT_SIZE, OUTPUT_SIZE, pixels)
        name = f"jump-{number:02}.png"
        outputs[name] = payload
        frames.append(
            {
                "file": name,
                "time_ms": number * INTERVAL_MS,
                "pose": [round(value, 6) for value in values],
                "alpha_bounds": bounds,
                "sha256": hashlib.sha256(payload).hexdigest(),
            }
        )
    report = {
        "source": "logo.png",
        "source_sha256": hashlib.sha256(source).hexdigest(),
        "source_canvas": [width, height],
        "canvas": [OUTPUT_SIZE, OUTPUT_SIZE],
        "interval_ms": INTERVAL_MS,
        "duration_ms": INTERVALS * INTERVAL_MS,
        "runtime_frames": INTERVALS,
        "restore_resource": "logo.png",
        "endpoints_match_static_pixels": (
            read_png(outputs["jump-00.png"])[2]
            == read_png(outputs["jump-24.png"])[2]
            == static_pixels
        ),
        "frames": frames,
    }
    outputs["manifest.json"] = (json.dumps(report, indent=2) + "\n").encode()
    return outputs


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="verify committed assets without writing"
    )
    args = parser.parse_args()
    repo_root = Path(__file__).resolve().parents[1]
    output_dir = repo_root / "ui/assets/tray-jump"
    expected = generate((repo_root / "logo.png").read_bytes())
    if args.check:
        stale = [
            name
            for name, data in expected.items()
            if not (output_dir / name).exists()
            or (output_dir / name).read_bytes() != data
        ]
        extra = sorted(
            path.name
            for path in output_dir.glob("jump-*.png")
            if path.name not in expected
        )
        if stale or extra:
            print(f"Tray jump assets differ: stale={stale}, extra={extra}", file=sys.stderr)
            return 1
        print(
            "Tray jump assets are current; all 25 frames have clear borders "
            "and equal static endpoints."
        )
        return 0
    output_dir.mkdir(parents=True, exist_ok=True)
    for name, data in expected.items():
        (output_dir / name).write_bytes(data)
    print(f"Wrote 25 64px tray jump frames and manifest to {output_dir}; duration 720ms.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

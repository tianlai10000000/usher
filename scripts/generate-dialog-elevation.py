#!/usr/bin/env python3
"""Generate the fixed-size alpha masks used by shared elevation primitives."""

from __future__ import annotations

import argparse
import math
from pathlib import Path
import struct
import sys
import zlib


PROFILES = {
    "dialog-shadow-mask.png": {
        "size": 145,
        "surface_origin": 24,
        "surface_size": 97,
        "corner_radius": 12,
        "slice_inset": 72,
        "minimum_target": 96,
        "blur_sigma": 12.0,
        "blur_radius": 36,
    },
    "floating-shadow-mask.png": {
        "size": 75,
        "surface_origin": 18,
        "surface_size": 39,
        "corner_radius": 12,
        "slice_inset": 37,
        "minimum_target": 40,
        "blur_sigma": 9.0,
        "blur_radius": 27,
    },
    "compact-shadow-mask.png": {
        "size": 55,
        "surface_origin": 12,
        "surface_size": 31,
        "corner_radius": 8,
        "slice_inset": 27,
        "minimum_target": 32,
        "blur_sigma": 6.0,
        "blur_radius": 18,
    },
}


def validate_profile(profile: dict[str, int | float]) -> None:
    size = int(profile["size"])
    surface_origin = int(profile["surface_origin"])
    surface_size = int(profile["surface_size"])
    corner_radius = int(profile["corner_radius"])
    slice_inset = int(profile["slice_inset"])
    minimum_target = int(profile["minimum_target"])

    # Render each profile around a representative small surface and leave one
    # center pixel for nine-slice stretching. Consumers suppress elevation below
    # minimum_target so the two fixed caps can never overlap.
    if size != surface_origin * 2 + surface_size:
        raise ValueError("surface plus shadow outsets must fill the source image")
    if size != slice_inset * 2 + 1:
        raise ValueError("nine-slice profile must retain exactly one center pixel")
    if minimum_target + surface_origin * 2 < slice_inset * 2:
        raise ValueError("nine-slice caps overlap at the minimum target size")
    if surface_size < corner_radius * 2:
        raise ValueError("source surface must contain both rounded corners")


def rounded_rect_mask(profile: dict[str, int | float]) -> list[list[float]]:
    size = int(profile["size"])
    surface_origin = int(profile["surface_origin"])
    surface_size = int(profile["surface_size"])
    corner_radius = int(profile["corner_radius"])
    mask = [[0.0] * size for _ in range(size)]
    left = surface_origin
    top = surface_origin
    right = left + surface_size
    bottom = top + surface_size
    inner_left = left + corner_radius
    inner_top = top + corner_radius
    inner_right = right - corner_radius
    inner_bottom = bottom - corner_radius
    radius_squared = corner_radius * corner_radius

    for y in range(top, bottom):
        py = y + 0.5
        for x in range(left, right):
            px = x + 0.5
            nearest_x = min(max(px, inner_left), inner_right)
            nearest_y = min(max(py, inner_top), inner_bottom)
            dx = px - nearest_x
            dy = py - nearest_y
            if dx * dx + dy * dy <= radius_squared:
                mask[y][x] = 1.0
    return mask


def gaussian_kernel(profile: dict[str, int | float]) -> list[float]:
    blur_radius = int(profile["blur_radius"])
    blur_sigma = float(profile["blur_sigma"])
    values = [
        math.exp(-(offset * offset) / (2.0 * blur_sigma * blur_sigma))
        for offset in range(-blur_radius, blur_radius + 1)
    ]
    total = sum(values)
    return [value / total for value in values]


def blur(
    mask: list[list[float]], profile: dict[str, int | float]
) -> list[list[float]]:
    size = int(profile["size"])
    blur_radius = int(profile["blur_radius"])
    kernel = gaussian_kernel(profile)
    horizontal = [[0.0] * size for _ in range(size)]
    result = [[0.0] * size for _ in range(size)]

    for y in range(size):
        for x in range(size):
            horizontal[y][x] = sum(
                mask[y][source_x] * weight
                for offset, weight in enumerate(kernel, start=-blur_radius)
                if 0 <= (source_x := x + offset) < size
            )

    for y in range(size):
        for x in range(size):
            result[y][x] = sum(
                horizontal[source_y][x] * weight
                for offset, weight in enumerate(kernel, start=-blur_radius)
                if 0 <= (source_y := y + offset) < size
            )
    return result


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    checksum = zlib.crc32(kind)
    checksum = zlib.crc32(payload, checksum)
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", checksum)


def render_png(profile: dict[str, int | float]) -> bytes:
    validate_profile(profile)
    size = int(profile["size"])
    alpha = blur(rounded_rect_mask(profile), profile)
    rows = bytearray()
    for row in alpha:
        rows.append(0)
        for value in row:
            rows.extend((255, 255, 255, round(max(0.0, min(1.0, value)) * 255)))

    signature = b"\x89PNG\r\n\x1a\n"
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"".join(
        (
            signature,
            png_chunk(b"IHDR", header),
            png_chunk(b"IDAT", zlib.compress(bytes(rows), level=9)),
            png_chunk(b"IEND", b""),
        )
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    repo_root = Path(__file__).resolve().parents[1]
    output_dir = repo_root / "ui/assets/elevation"
    expected = {name: render_png(profile) for name, profile in PROFILES.items()}

    if args.check:
        stale = [
            output_dir / name
            for name, payload in expected.items()
            if not (output_dir / name).exists()
            or (output_dir / name).read_bytes() != payload
        ]
        if stale:
            for output in stale:
                print(f"elevation asset is stale: {output}", file=sys.stderr)
            return 1
        print("Elevation assets are current.")
        return 0

    output_dir.mkdir(parents=True, exist_ok=True)
    for name, payload in expected.items():
        output = output_dir / name
        output.write_bytes(payload)
        print(f"Wrote {output} ({len(payload)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

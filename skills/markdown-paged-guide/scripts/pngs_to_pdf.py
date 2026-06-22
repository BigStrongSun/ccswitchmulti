#!/usr/bin/env python
"""Convert a folder of rendered PNG guide pages into a single PDF."""
from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image, JpegImagePlugin  # noqa: F401
from pypdf import PdfReader


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Convert rendered guide PNG pages to PDF.")
    parser.add_argument("--pages-dir", required=True, help="Directory containing ordered PNG pages.")
    parser.add_argument("--output", required=True, help="Output PDF path.")
    parser.add_argument("--resolution", type=float, default=144.0, help="PDF image resolution.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    pages_dir = Path(args.pages_dir)
    output = Path(args.output)
    page_files = sorted(pages_dir.glob("*.png"))
    if not page_files:
        raise SystemExit(f"No PNG pages found in {pages_dir}")

    output.parent.mkdir(parents=True, exist_ok=True)
    images = [Image.open(page).convert("RGB") for page in page_files]
    try:
        images[0].save(output, save_all=True, append_images=images[1:], resolution=args.resolution)
    finally:
        for image in images:
            image.close()

    reader = PdfReader(str(output))
    if len(reader.pages) != len(page_files):
        raise SystemExit(f"PDF page count mismatch: {len(reader.pages)} != {len(page_files)}")

    with Image.open(page_files[0]) as first:
        print(f"Wrote {output.resolve()}")
        print(f"pages={len(page_files)}")
        print(f"first_page_size={first.size[0]}x{first.size[1]}")
        print(f"bytes={output.stat().st_size}")


if __name__ == "__main__":
    main()

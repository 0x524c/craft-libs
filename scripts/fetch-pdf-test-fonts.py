#!/usr/bin/env python3
"""Fetch the pinned, licensed PDF test fixtures outside the checkout."""

import argparse
import hashlib
import json
from pathlib import Path
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    destination = args.destination.resolve()
    if destination == root or root in destination.parents:
        parser.error("fixtures must be stored outside the checkout")
    fixtures = json.loads((root / "crates/pdf/tests/fonts.json").read_text())
    destination.mkdir(parents=True, exist_ok=True)
    for fixture in fixtures:
        name, url, digest = fixture["name"], fixture["source"], fixture["sha256"]
        if Path(name).name != name or name in ("", ".", "..") or not url.startswith("https://"):
            raise ValueError("invalid fixture name or source")
        path = destination / name
        if path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == digest:
            print(f"Verified {name}")
            continue
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(16 * 1024 * 1024 + 1)
        if len(data) > 16 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != digest:
            raise ValueError(f"fixture size or SHA-256 mismatch: {name}")
        path.write_bytes(data)
        print(f"Fetched and verified {name}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Generate or verify a portable SHA-256 manifest for release assets."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def parse_manifest(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        checksum, separator, name = line.partition("  ")
        if not separator or len(checksum) != 64 or not name:
            raise SystemExit(f"invalid checksum entry: {line!r}")
        values[name] = checksum
    return values


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()

    assets = sorted(path for path in args.directory.iterdir() if path.is_file() and path.name != args.manifest.name)
    if not assets:
        raise SystemExit("no release assets found")
    expected = {path.name: digest(path) for path in assets}
    if args.verify:
        actual = parse_manifest(args.manifest)
        if actual != expected:
            raise SystemExit(f"checksum manifest mismatch; expected {expected}, got {actual}")
        print(f"verified {args.manifest}")
        return
    args.manifest.write_text(
        "".join(f"{checksum}  {name}\n" for name, checksum in expected.items()), encoding="utf-8"
    )
    print(args.manifest)


if __name__ == "__main__":
    main()

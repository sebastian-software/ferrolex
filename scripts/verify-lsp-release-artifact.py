#!/usr/bin/env python3
"""Assert that a ferrolex-lsp archive contains exactly its documented payload."""

from __future__ import annotations

import argparse
import tarfile
import zipfile
from pathlib import PurePosixPath


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--binary-name", required=True)
    return parser.parse_args()


def archive_files(artifact: str) -> list[str]:
    if artifact.endswith(".tar.gz"):
        with tarfile.open(artifact, "r:gz") as archive:
            return [member.name for member in archive.getmembers() if member.isfile()]
    if artifact.endswith(".zip"):
        with zipfile.ZipFile(artifact) as archive:
            return [member.filename for member in archive.infolist() if not member.is_dir()]
    raise SystemExit(f"unsupported archive format: {artifact}")


def main() -> None:
    args = parse_args()
    version = args.version.removeprefix("ferrolex-v")
    root = f"ferrolex-lsp-{version}-{args.target}"
    expected = {
        f"{root}/{args.binary_name}",
        f"{root}/LICENSE-APACHE",
        f"{root}/LICENSE-MIT",
        f"{root}/NOTICE.txt",
    }
    actual = set(archive_files(args.artifact))
    unsafe = [name for name in actual if PurePosixPath(name).is_absolute() or ".." in PurePosixPath(name).parts]
    if unsafe:
        raise SystemExit(f"archive contains unsafe paths: {unsafe}")
    if actual != expected:
        raise SystemExit(f"unexpected archive contents; expected {sorted(expected)}, got {sorted(actual)}")
    print(f"verified {args.artifact}")


if __name__ == "__main__":
    main()

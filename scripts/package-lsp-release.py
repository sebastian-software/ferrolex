#!/usr/bin/env python3
"""Create the documented, dictionary-free ferrolex-lsp release archive."""

from __future__ import annotations

import argparse
import shutil
import stat
import tarfile
import tempfile
import zipfile
from pathlib import Path


NOTICE = """ferrolex-lsp {version}

This archive contains only the ferrolex-lsp server binary and its license notices.
It does not contain, download, or select dictionaries. The LSP client controls
dictionary configuration and paths; see docs/adr/0007-dictionary-distribution.md.
"""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--format", choices=("tar.gz", "zip"), required=True)
    return parser.parse_args()


def copy_archive_contents(root: Path, binary: Path, version: str) -> list[Path]:
    root.mkdir(parents=True, exist_ok=True)
    binary_name = binary.name
    copied_binary = root / binary_name
    shutil.copy2(binary, copied_binary)
    copied_binary.chmod(copied_binary.stat().st_mode | stat.S_IXUSR)

    repository = Path(__file__).resolve().parent.parent
    files = [copied_binary]
    for license_name in ("LICENSE-APACHE", "LICENSE-MIT"):
        destination = root / license_name
        shutil.copy2(repository / license_name, destination)
        files.append(destination)
    notice = root / "NOTICE.txt"
    notice.write_text(NOTICE.format(version=version), encoding="utf-8")
    files.append(notice)
    return files


def create_tarball(archive: Path, root_name: str, files: list[Path]) -> None:
    with tarfile.open(archive, "w:gz") as output:
        for file in files:
            output.add(file, arcname=f"{root_name}/{file.name}")


def create_zip(archive: Path, root_name: str, files: list[Path]) -> None:
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
        for file in files:
            info = zipfile.ZipInfo(f"{root_name}/{file.name}")
            info.external_attr = (file.stat().st_mode & 0xFFFF) << 16
            output.writestr(info, file.read_bytes())


def main() -> None:
    args = parse_args()
    if not args.binary.is_file():
        raise SystemExit(f"server binary does not exist: {args.binary}")

    version = args.version.removeprefix("ferrolex-v")
    root_name = f"ferrolex-lsp-{version}-{args.target}"
    extension = "tar.gz" if args.format == "tar.gz" else "zip"
    archive = args.out_dir / f"{root_name}.{extension}"
    args.out_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="ferrolex-lsp-release-") as temporary:
        contents = copy_archive_contents(Path(temporary) / root_name, args.binary, version)
        if args.format == "tar.gz":
            create_tarball(archive, root_name, contents)
        else:
            create_zip(archive, root_name, contents)

    print(archive)


if __name__ == "__main__":
    main()

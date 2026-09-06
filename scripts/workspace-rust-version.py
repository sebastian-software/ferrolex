#!/usr/bin/env python3
"""Print, verify, or synchronize the workspace MSRV declared in Cargo.toml.

``workspace.package.rust-version`` in the root ``Cargo.toml`` is the single
source of truth for the supported Rust version. Two surfaces cannot read it at
use time: ``rust-toolchain.toml`` is consumed literally by rustup, and the
README badge is static Markdown. ``--check`` fails when either copy drifts and
``--sync`` rewrites them, so raising the MSRV stays a one-file edit.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
TOOLCHAIN = ROOT / "rust-toolchain.toml"
README = ROOT / "README.md"

TOOLCHAIN_PATTERN = re.compile(r'^(channel = ")([^"]*)(")$', re.MULTILINE)
BADGE_PATTERN = re.compile(
    r"^(\[!\[MSRV: )([^\]]*)(\]\(https://img\.shields\.io/badge/MSRV-)([^-]*)"
    r"(-blue\.svg\)\]\(CONTRIBUTING\.md#rust-toolchain-and-msrv\))$",
    re.MULTILINE,
)


def workspace_rust_version() -> str:
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        document = tomllib.load(manifest)
    version = document["workspace"]["package"]["rust-version"]
    if not isinstance(version, str) or not version:
        raise SystemExit("workspace.package.rust-version must be a non-empty string")
    return version


def substitute(path: Path, pattern: re.Pattern[str], version: str) -> tuple[str, str]:
    """Return the current text of ``path`` and the text the MSRV implies."""
    original = path.read_text(encoding="utf-8")
    matches = list(pattern.finditer(original))
    if len(matches) != 1:
        raise SystemExit(f"{path.name}: expected exactly one MSRV reference")
    match = matches[0]
    rendered = "".join(
        version if index % 2 else group
        for index, group in enumerate(match.groups())
    )
    return original, f"{original[: match.start()]}{rendered}{original[match.end() :]}"


def derived_copies(version: str) -> list[tuple[Path, str, str]]:
    return [
        (path, *substitute(path, pattern, version))
        for path, pattern in ((TOOLCHAIN, TOOLCHAIN_PATTERN), (README, BADGE_PATTERN))
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group()
    group.add_argument(
        "--check",
        action="store_true",
        help="fail when a derived MSRV copy does not match Cargo.toml",
    )
    group.add_argument(
        "--sync",
        action="store_true",
        help="rewrite the derived MSRV copies from Cargo.toml",
    )
    args = parser.parse_args()

    version = workspace_rust_version()
    if not (args.check or args.sync):
        print(version)
        return 0

    stale = [
        (path, rendered)
        for path, original, rendered in derived_copies(version)
        if original != rendered
    ]
    if args.sync:
        for path, rendered in stale:
            path.write_text(rendered, encoding="utf-8")
            print(f"updated {path.name} to Rust {version}")
        return 0

    for path, _ in stale:
        print(
            f"workspace MSRV error: {path.name} does not declare Rust {version}; "
            "run python3 scripts/workspace-rust-version.py --sync",
            file=sys.stderr,
        )
    if stale:
        return 1
    print(f"workspace MSRV ok: rust-toolchain.toml and the README badge use {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

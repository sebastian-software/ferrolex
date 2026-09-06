#!/usr/bin/env python3
"""Print the workspace MSRV declared in Cargo.toml."""

from pathlib import Path
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        document = tomllib.load(manifest)
    version = document["workspace"]["package"]["rust-version"]
    if not isinstance(version, str) or not version:
        raise SystemExit("workspace.package.rust-version must be a non-empty string")
    print(version)


if __name__ == "__main__":
    main()

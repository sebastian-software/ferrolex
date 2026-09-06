#!/usr/bin/env python3
"""Print the pinned fuzzing toolchain from fuzz/rust-toolchain.toml."""

from pathlib import Path
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    with (ROOT / "fuzz" / "rust-toolchain.toml").open("rb") as manifest:
        document = tomllib.load(manifest)
    channel = document["toolchain"]["channel"]
    if not isinstance(channel, str) or not channel.startswith("nightly-"):
        raise SystemExit("fuzz toolchain must be a pinned nightly channel")
    print(channel)


if __name__ == "__main__":
    main()

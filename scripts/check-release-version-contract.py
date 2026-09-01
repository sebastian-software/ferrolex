#!/usr/bin/env python3
"""Verify ferrolex's single-version Cargo workspace release contract."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tomllib
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as source:
        return json.load(source)


def cargo_metadata() -> dict[str, Any]:
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def main() -> int:
    errors: list[str] = []
    with (ROOT / "Cargo.toml").open("rb") as source:
        root_version = tomllib.load(source)["package"]["version"]

    release_config = load_json(ROOT / "release-please-config.json")
    configured_packages = release_config.get("packages", {})
    if set(configured_packages) != {"."}:
        errors.append(
            "release-please must keep one root package so the workspace has one release record"
        )
    if release_config.get("release-type") != "rust":
        errors.append("release-please must use the rust release strategy")

    workspace_plugins = [
        plugin
        for plugin in release_config.get("plugins", [])
        if isinstance(plugin, dict) and plugin.get("type") == "cargo-workspace"
    ]
    if len(workspace_plugins) != 1:
        errors.append("release-please must configure exactly one cargo-workspace plugin")
    elif workspace_plugins[0].get("updateAllPackages") is not True:
        errors.append("cargo-workspace must set updateAllPackages to true")
    elif workspace_plugins[0].get("merge", True) is not True:
        errors.append("cargo-workspace must keep its release candidate merged")

    release_manifest = load_json(ROOT / ".release-please-manifest.json")
    if release_manifest != {".": root_version}:
        errors.append(
            ".release-please-manifest.json must contain only the root workspace version "
            f"{root_version}"
        )

    metadata = cargo_metadata()
    workspace_ids = set(metadata["workspace_members"])
    packages = [
        package for package in metadata["packages"] if package["id"] in workspace_ids
    ]
    workspace_names = {package["name"] for package in packages}
    internal_requirements = 0

    for package in sorted(packages, key=lambda item: item["name"]):
        if package["version"] != root_version:
            errors.append(
                f"{package['name']} is {package['version']}, expected workspace version "
                f"{root_version}"
            )
        for dependency in package["dependencies"]:
            if dependency.get("path") is None or dependency["name"] not in workspace_names:
                continue
            internal_requirements += 1
            expected = f"^{root_version}"
            if dependency["req"] != expected:
                errors.append(
                    f"{package['name']} requires {dependency['name']} {dependency['req']}, "
                    f"expected {expected}"
                )

    if errors:
        for error in errors:
            print(f"release version contract error: {error}", file=sys.stderr)
        return 1

    print(
        "release version contract ok: "
        f"{len(packages)} workspace packages and {internal_requirements} internal "
        f"requirements use {root_version}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

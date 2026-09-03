#!/usr/bin/env python3
"""Verify ferrolex's single-version Cargo and Node.js release contract."""

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

    expected_node_release_paths = {
        ("crates/ferrolex-node/package.json", "$.version"),
        (
            "crates/ferrolex-node/package.json",
            "$['optionalDependencies']['@ferrolex/node-darwin-arm64']",
        ),
        (
            "crates/ferrolex-node/package.json",
            "$['optionalDependencies']['@ferrolex/node-linux-x64-gnu']",
        ),
        (
            "crates/ferrolex-node/package.json",
            "$['optionalDependencies']['@ferrolex/node-win32-x64-msvc']",
        ),
        ("crates/ferrolex-node/package-lock.json", "$.version"),
        (
            "crates/ferrolex-node/package-lock.json",
            "$['packages']['']['version']",
        ),
        (
            "crates/ferrolex-node/package-lock.json",
            "$['packages']['']['optionalDependencies']['@ferrolex/node-darwin-arm64']",
        ),
        (
            "crates/ferrolex-node/package-lock.json",
            "$['packages']['']['optionalDependencies']['@ferrolex/node-linux-x64-gnu']",
        ),
        (
            "crates/ferrolex-node/package-lock.json",
            "$['packages']['']['optionalDependencies']['@ferrolex/node-win32-x64-msvc']",
        ),
    }
    node_release_paths = {
        (entry.get("path"), entry.get("jsonpath"))
        for entry in configured_packages.get(".", {}).get("extra-files", [])
        if isinstance(entry, dict) and entry.get("type") == "json"
    }
    if node_release_paths != expected_node_release_paths:
        errors.append(
            "release-please extra-files must update every Node.js package and "
            "lockfile version field"
        )

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

    node_package = load_json(ROOT / "crates/ferrolex-node/package.json")
    node_lock = load_json(ROOT / "crates/ferrolex-node/package-lock.json")
    if node_package.get("name") != "@ferrolex/node":
        errors.append("the supported Node.js package must be named @ferrolex/node")
    if node_package.get("version") != root_version:
        errors.append(
            "@ferrolex/node is "
            f"{node_package.get('version')}, expected workspace version {root_version}"
        )
    if node_lock.get("name") != node_package.get("name"):
        errors.append("the Node.js lockfile package name is out of sync")
    if node_lock.get("version") != root_version:
        errors.append("the Node.js lockfile package version is out of sync")
    locked_root = node_lock.get("packages", {}).get("", {})
    if locked_root.get("name") != node_package.get("name"):
        errors.append("the Node.js lockfile root package name is out of sync")
    if locked_root.get("version") != root_version:
        errors.append("the Node.js lockfile root package version is out of sync")

    node_targets = {
        "@ferrolex/node-darwin-arm64",
        "@ferrolex/node-linux-x64-gnu",
        "@ferrolex/node-win32-x64-msvc",
    }
    optional_dependencies = node_package.get("optionalDependencies", {})
    if set(optional_dependencies) != node_targets:
        errors.append(
            "@ferrolex/node optionalDependencies must exactly match the supported "
            "prebuilt packages"
        )
    for name, version in optional_dependencies.items():
        if version != root_version:
            errors.append(
                f"{name} is pinned to {version}, expected workspace version {root_version}"
            )

    node_loader = (ROOT / "crates/ferrolex-node/index.js").read_text(encoding="utf-8")
    if "const packageVersion = require('./package.json').version" not in node_loader:
        errors.append("the generated Node.js loader must read its version dynamically")
    if root_version in node_loader:
        errors.append("the generated Node.js loader must not embed the release version")

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
        f"requirements plus @ferrolex/node use {root_version}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

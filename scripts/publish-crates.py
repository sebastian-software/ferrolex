#!/usr/bin/env python3
"""Publish ferrolex's public crates to crates.io in dependency order."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import time
from typing import Any
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]
PUBLISH_ORDER = (
    "ferrolex-core",
    "ferrolex-dictionaries",
    "ferrolex-code",
    "ferrolex-suggest",
    "ferrolex-text",
    "ferrolex-compiler",
    "ferrolex-hunspell",
    "ferrolex-cli",
    "ferrolex",
)
INDEX_TIMEOUT_SECONDS = 300
INDEX_POLL_SECONDS = 10


def cargo_metadata() -> dict[str, Any]:
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def public_packages(metadata: dict[str, Any]) -> dict[str, dict[str, Any]]:
    workspace_ids = set(metadata["workspace_members"])
    return {
        package["name"]: package
        for package in metadata["packages"]
        if package["id"] in workspace_ids and package.get("publish") != []
    }


def validate_release_graph(
    metadata: dict[str, Any], packages: dict[str, dict[str, Any]]
) -> list[str]:
    errors: list[str] = []
    expected = set(PUBLISH_ORDER)
    actual = set(packages)
    if actual != expected:
        missing = sorted(actual - expected)
        stale = sorted(expected - actual)
        if missing:
            errors.append(f"publish order is missing public packages: {', '.join(missing)}")
        if stale:
            errors.append(f"publish order contains non-public packages: {', '.join(stale)}")

    positions = {name: position for position, name in enumerate(PUBLISH_ORDER)}
    workspace_ids = set(metadata["workspace_members"])
    workspace_names = {
        package["name"]
        for package in metadata["packages"]
        if package["id"] in workspace_ids
    }
    for name, package in packages.items():
        for dependency in package["dependencies"]:
            dependency_name = dependency["name"]
            if (
                dependency.get("path") is None
                or dependency.get("kind") == "dev"
                or dependency_name not in workspace_names
            ):
                continue
            if dependency_name not in packages:
                errors.append(
                    f"public package {name} depends on unpublished workspace package "
                    f"{dependency_name}"
                )
            elif positions.get(dependency_name, sys.maxsize) >= positions.get(
                name, sys.maxsize
            ):
                errors.append(
                    f"{dependency_name} must appear before dependent package {name}"
                )
    return errors


def validate_package_contents() -> None:
    for name in PUBLISH_ORDER:
        subprocess.run(
            [
                "cargo",
                "package",
                "--locked",
                "--allow-dirty",
                "--no-verify",
                "--list",
                "--package",
                name,
            ],
            cwd=ROOT,
            check=True,
            stdout=subprocess.DEVNULL,
        )


def version_exists(name: str, version: str) -> bool:
    url = f"https://crates.io/api/v1/crates/{quote(name)}/{quote(version)}"
    request = Request(url, headers={"User-Agent": "ferrolex-release-workflow"})
    try:
        with urlopen(request, timeout=30):
            return True
    except HTTPError as error:
        if error.code == 404:
            return False
        raise


def wait_for_index(name: str, version: str) -> None:
    deadline = time.monotonic() + INDEX_TIMEOUT_SECONDS
    while True:
        completed = subprocess.run(
            ["cargo", "info", f"{name}@{version}", "--registry", "crates-io"],
            cwd=ROOT,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if completed.returncode == 0:
            return
        if time.monotonic() >= deadline:
            raise RuntimeError(
                f"timed out waiting for {name} {version} in the crates.io index"
            )
        time.sleep(INDEX_POLL_SECONDS)


def publish(name: str, version: str) -> None:
    if version_exists(name, version):
        print(f"already published: {name} {version}")
        wait_for_index(name, version)
        return

    print(f"publishing: {name} {version}", flush=True)
    completed = subprocess.run(
        [
            "cargo",
            "publish",
            "--locked",
            "--registry",
            "crates-io",
            "--package",
            name,
        ],
        cwd=ROOT,
        check=False,
    )
    if completed.returncode != 0 and not version_exists(name, version):
        raise RuntimeError(f"cargo publish failed for {name} {version}")
    wait_for_index(name, version)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--check",
        action="store_true",
        help="validate the public release graph without contacting crates.io",
    )
    args = parser.parse_args()

    metadata = cargo_metadata()
    packages = public_packages(metadata)
    errors = validate_release_graph(metadata, packages)
    if errors:
        for error in errors:
            print(f"publish contract error: {error}", file=sys.stderr)
        return 1

    versions = {packages[name]["version"] for name in PUBLISH_ORDER}
    if len(versions) != 1:
        print("publish contract error: public package versions differ", file=sys.stderr)
        return 1
    version = versions.pop()
    validate_package_contents()
    print(
        f"publish contract ok: {len(PUBLISH_ORDER)} public packages at {version} "
        "are in dependency order"
    )
    if args.check:
        return 0

    for name in PUBLISH_ORDER:
        publish(name, version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

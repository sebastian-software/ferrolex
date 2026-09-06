#!/usr/bin/env python3
"""Check or render the Ferramenta family blocks used by ferrolex READMEs."""

from __future__ import annotations

import argparse
import difflib
from pathlib import Path
import re
import sys


TOOLS = (
    ("ferroni", "Oniguruma-compatible regex engine"),
    ("ferriki", "Shiki-compatible syntax highlighting"),
    ("ferromark", "CommonMark/GFM Markdown to HTML"),
    ("ferralk", "Glob matching and parallel filesystem walking"),
    ("ferrovia", "SVGO-compatible SVG optimizer"),
    ("ferrocat", "Translation catalog engine"),
    ("ferrolex", "Spell checking for text and code"),
    ("ferrugo", "Rust-native PDF previews"),
)
FAMILY_URL = "https://ferramenta.dev"
COMPANY_URL = "https://oss.sebastian-software.com"
LOGO_URL = (
    "https://raw.githubusercontent.com/sebastian-software/ferramenta/main/"
    "app/assets/logos/sebastian-software.svg"
)

MARKERS = {
    "github": ("<!-- ferramenta-family:start -->", "<!-- ferramenta-family:end -->"),
    "registry": (
        "<!-- ferramenta-family:registry:start -->",
        "<!-- ferramenta-family:registry:end -->",
    ),
}
FOOTER_MARKERS = (
    "<!-- sebastian-software-branding:start -->",
    "<!-- sebastian-software-branding:end -->",
)


def repository_url(name: str) -> str:
    return f"https://github.com/sebastian-software/{name}"


def github_block(current: str) -> str:
    rows = [
        "<!-- ferramenta-family:start -->",
        "## The Ferramenta family",
        "",
        (
            "This project is part of [Ferramenta](https://ferramenta.dev) — the "
            "family of Rust-native developer tools by [Sebastian Software]"
            f"({COMPANY_URL}) that keep the APIs the ecosystem already knows:"
        ),
        "",
        "| Tool | Job |",
        "| --- | --- |",
    ]
    rows.extend(
        f"| {'**' if name == current else ''}[{name}]({repository_url(name)})"
        f"{'**' if name == current else ''} | {job} |"
        for name, job in TOOLS
    )
    rows.append("<!-- ferramenta-family:end -->")
    return "\n".join(rows)


def registry_block(current: str) -> str:
    siblings = [
        f"[{name}]({repository_url(name)})"
        for name, _ in TOOLS
        if name != current
    ]
    return "\n".join(
        (
            "<!-- ferramenta-family:registry:start -->",
            (
                f"Part of the [Ferramenta]({FAMILY_URL}) family of Rust-native "
                f"developer tools by [Sebastian Software]({COMPANY_URL})."
            ),
            f"Siblings: {', '.join(siblings[:-1])}, and {siblings[-1]}.",
            "<!-- ferramenta-family:registry:end -->",
        )
    )


def footer() -> str:
    return "\n".join(
        (
            FOOTER_MARKERS[0],
            '<p align="center">',
            f'  <a href="{COMPANY_URL}">',
            f'    <img src="{LOGO_URL}" alt="Sebastian Software" width="240" />',
            "  </a>",
            "</p>",
            "",
            '<p align="center">',
            f'  <a href="{COMPANY_URL}">Open Source at Sebastian Software</a><br />',
            "  Copyright &copy; 2026 Sebastian Software GmbH",
            "</p>",
            FOOTER_MARKERS[1],
        )
    )


def replace_block(
    text: str, markers: tuple[str, str], replacement: str, path: Path
) -> str:
    start, end = (re.escape(marker) for marker in markers)
    pattern = re.compile(f"{start}.*?{end}", re.DOTALL)
    matches = list(pattern.finditer(text))
    if len(matches) != 1:
        raise ValueError(f"{path}: expected exactly one {markers[0]} block")
    match = matches[0]
    return f"{text[:match.start()]}{replacement}{text[match.end():]}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--current", required=True, choices=[name for name, _ in TOOLS])
    parser.add_argument("--variant", required=True, choices=sorted(MARKERS))
    parser.add_argument("--readme", required=True, type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    original = args.readme.read_text(encoding="utf-8")
    rendered = replace_block(
        original,
        MARKERS[args.variant],
        github_block(args.current)
        if args.variant == "github"
        else registry_block(args.current),
        args.readme,
    )
    rendered = replace_block(rendered, FOOTER_MARKERS, footer(), args.readme)
    if args.check:
        if rendered == original:
            return 0
        sys.stdout.writelines(
            difflib.unified_diff(
                original.splitlines(keepends=True),
                rendered.splitlines(keepends=True),
                fromfile=str(args.readme),
                tofile=f"{args.readme} (generated)",
            )
        )
        return 1

    args.readme.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

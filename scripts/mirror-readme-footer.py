#!/usr/bin/env python3
"""Mirror the company footer from the root README into the crate READMEs.

The footer is the ``sebastian-software-branding`` section owned by
`@sebastian-software/standards`_: ``standards apply`` writes it into the root
README and is its only writer. This script copies that block verbatim into the
READMEs that crates.io renders, so both surfaces stay identical without
duplicating standards-owned content here.

The Ferramenta family block above it is a separate concern with a separate
source of truth; ``scripts/readme-family.sh`` renders it from the family
registry.

.. _`@sebastian-software/standards`:
   https://github.com/sebastian-software/standards
"""

from __future__ import annotations

import argparse
import difflib
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
SOURCE_README = ROOT / "README.md"

# The crate READMEs that crates.io renders. `@ferrolex/node` is published to
# npm from crates/ferrolex-node/README.md and deliberately stays out: the npm
# page carries the family block only.
MIRRORED_READMES = (
    "crates/ferrolex-core/README.md",
    "crates/ferrolex-dictionaries/README.md",
    "crates/ferrolex-code/README.md",
    "crates/ferrolex-suggest/README.md",
    "crates/ferrolex-text/README.md",
    "crates/ferrolex-compiler/README.md",
    "crates/ferrolex-hunspell/README.md",
    "crates/ferrolex-cli/README.md",
)

MARKERS = (
    "<!-- sebastian-software-branding:start -->",
    "<!-- sebastian-software-branding:end -->",
)


def find_block(text: str, path: Path) -> re.Match[str]:
    start, end = (re.escape(marker) for marker in MARKERS)
    pattern = re.compile(f"{start}.*?{end}", re.DOTALL)
    matches = list(pattern.finditer(text))
    if len(matches) != 1:
        raise ValueError(f"{path}: expected exactly one {MARKERS[0]} block")
    return matches[0]


def footer() -> str:
    """Return the standards-owned footer, read from the root README."""
    text = SOURCE_README.read_text(encoding="utf-8")
    return find_block(text, SOURCE_README).group(0)


def mirror(path: Path, replacement: str, check: bool) -> bool:
    """Write or verify one README. Returns True when it is up to date."""
    original = path.read_text(encoding="utf-8")
    match = find_block(original, path)
    rendered = f"{original[: match.start()]}{replacement}{original[match.end() :]}"
    if rendered == original:
        return True
    if check:
        sys.stdout.writelines(
            difflib.unified_diff(
                original.splitlines(keepends=True),
                rendered.splitlines(keepends=True),
                fromfile=str(path),
                tofile=f"{path} (mirrored)",
            )
        )
        return False
    path.write_text(rendered, encoding="utf-8")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="report drift instead of rewriting the mirrored READMEs",
    )
    args = parser.parse_args()

    replacement = footer()
    ok = True
    for relative in MIRRORED_READMES:
        ok &= mirror(ROOT / relative, replacement, args.check)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())

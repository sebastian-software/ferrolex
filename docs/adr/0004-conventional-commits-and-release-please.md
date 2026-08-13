# ADR-0004: Conventional Commits and Release Please

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-10
- Deciders: Sebastian Werner

## Context

The Sebastian Software organization centralizes repository governance in
[`sebastian-software/standards`](https://github.com/sebastian-software/standards),
which provides versioned configuration and release blueprints (including
Release Please setups for Rust crates, npm packages, and hybrid workspaces).
This project will grow into a multi-crate Rust workspace and needs automated,
low-ceremony releases from day one.

## Decision

- Commit messages follow the
  [Conventional Commits](https://www.conventionalcommits.org/) specification.
- Releases are automated with Release Please, adopting the shared setup and
  blueprints from `sebastian-software/standards` rather than a hand-rolled
  configuration.
- Version bumps and changelogs are derived from commit messages; commit
  messages are therefore treated as a machine-read API, not free-form prose.

## Consequences

- CI should enforce commit-message format once CI exists.
- The exact Release Please configuration lives in this repository's config
  files, synchronized from the standards repository — not in this ADR.
- Squash-merge discipline matters: the merged message must be a valid
  conventional commit.

## Validation and review triggers

- Reopen if the standards repository changes its release tooling or if the
  workspace's crate-release strategy (unified vs. independent versions) needs
  to diverge from the shared blueprint.

## References

- [Release readiness epic](https://github.com/sebastian-software/ferrolex/issues/79)
- <https://github.com/sebastian-software/standards>

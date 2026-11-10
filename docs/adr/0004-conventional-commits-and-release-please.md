# ADR-0004: Conventional Commits and Release Please

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-11-10
- Deciders: Sebastian Werner

## Context

The repository needs automated releases for its coordinated Rust workspace and
Node packages. Shared infrastructure guidance comes from
[`project-infra`](https://github.com/sebastian-software/project-infra); native
configuration and executable release contracts remain owned by this project.

## Decision

- Commit messages follow the
  [Conventional Commits](https://www.conventionalcommits.org/) specification.
- Releases are automated with Release Please using the project-owned
  configuration and coordinated release contracts.
- The single root Rust strategy owns the coordinated workspace release. Its
  native manifest and lockfile updates are combined with explicit TOML extra-files
  for inherited internal requirements. The independent-package `cargo-workspace`
  plugin is incompatible with the root-only release record and is not enabled.
- Version bumps and changelogs are derived from commit messages; commit
  messages are therefore treated as a machine-read API, not free-form prose.

## Consequences

- CI should enforce commit-message format once CI exists.
- The exact Release Please configuration lives in this repository's config
  files and is verified by `scripts/check-release-version-contract.py`.
- Repository-specific contributor and automation guidance lives in
  [`AGENTS.md`](../../AGENTS.md) and [`CONTRIBUTING.md`](../../CONTRIBUTING.md);
  shared infrastructure conventions remain in `project-infra`.
- Squash-merge discipline matters: the merged message must be a valid
  conventional commit.

## Validation and review triggers

- Reopen if the workspace's coordinated crate-release strategy or publication
  contract needs to change.

## References

- [Release readiness epic](https://github.com/sebastian-software/ferrolex/issues/79)
- <https://github.com/sebastian-software/project-infra>

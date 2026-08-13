# ADR-0005: Project, crates, and CLI binary are named ferrolex

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-10
- Deciders: Sebastian Werner

## Context

Early planning used `spell` as a placeholder CLI name, which collides with the
classic Unix `spell` tool, and `spell-*` as placeholder crate names. The
repository's working title was already `ferrolex` (ferro/iron alluding to
Rust, lex to lexicon).

Checks on 2026-08-10: the `ferrolex` crate name is unregistered on crates.io.
A FERROLEX trademark registration exists in India, apparently outside the
software sector; no software-related use was found. Low risk for an
open-source developer tool, not a legal clearance.

## Decision

One name everywhere: the project, the crates.io namespace (`ferrolex-core`,
`ferrolex-hunspell`, …), and the CLI binary are all named `ferrolex`.

## Consequences

- No collision with Unix `spell`; the name is unique and greppable.
- The `ferrolex` crate name should be reserved on crates.io before the first
  public release.
- Examples and future docs use `ferrolex` consistently.

## Validation and review triggers

- Reopen if a software-sector trademark conflict for "ferrolex" surfaces
  before the first public release.

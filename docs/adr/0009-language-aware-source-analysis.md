# ADR-0009: Keep document parsing outside ferrolex

- Status: accepted
- Date: 2026-08-13
- Last updated: 2026-08-14
- Deciders: Ferrolex maintainers

## Context

ferrolex started with a generic source-token analyzer and later selected Rust
as the first parser-backed language. That direction added a grammar dependency,
syntax-error recovery, language-specific fixtures, editor-oriented behavior,
and a new compatibility surface. The selection was based on the repository's
own Rust source volume rather than evidence that ferrolex users needed Rust
parsing.

The focused product is a native spell-checking engine: it loads dictionaries,
checks words, and returns deterministic suggestions. Markdown, PO, TypeScript,
and other formats already have owning tools with better syntax knowledge.

## Decision

ferrolex will not own programming-language or document parsers. It will not add
Tree-sitter grammars, semantic analysis, compiler integration, or named
language-support tiers.

Format-aware consumers select human-language content and call the ferrolex Rust
or Node.js API:

- Ferromark owns Markdown parsing and passes prose to ferrolex.
- Ferrocat owns PO parsing and passes translatable strings to ferrolex.
- OXC owns TypeScript parsing and passes selected comments, strings, or
  identifiers to ferrolex.

The existing `ferrolex-code` crate may remain as a generic, parser-independent
helper while the workspace is simplified. Its presence is not a promise that
ferrolex will become a general source-analysis framework.

## Consequences

- Parser-backed Rust analysis is removed from the product and dependency graph.
- Format-specific accuracy and configuration stay with the projects that own
  those formats.
- All integrations share one dictionary, recognition, and suggestion contract.
- ferrolex avoids language-version tracking, parser error recovery, incremental
  syntax state, and editor-specific test matrices.

## Review triggers

Revisit this boundary only when a concrete consumer cannot integrate through
the word, text, or token API and can demonstrate that centralizing a parser in
ferrolex reduces total ownership across multiple real callers.

## References

- [Architecture](../../ARCHITECTURE.md)
- [Generic token analysis](../source-code-analysis.md)

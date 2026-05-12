# ADR 0001: Use Hunspell As Import Source, Not Runtime Boundary

## Status

Accepted

## Context

Ferrolex needs broad multilingual dictionary coverage. Hunspell-compatible
dictionaries are widely available and practical. Alternatives are either
wrappers, older standalone systems, Java-heavy style systems, or
language-specific engines.

## Decision

Ferrolex will use Hunspell-compatible `.aff` and `.dic` dictionaries as v0.1
source material. The runtime checker will use a Ferrolex-owned dictionary API
and compiled artifact format.

## Consequences

- Dictionary import needs to preserve source license metadata.
- The runtime lookup layer can evolve independently of Hunspell file details.
- Full Hunspell compatibility is not required before the rest of the pipeline
  can become useful.

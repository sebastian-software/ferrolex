# Security policy

## Supported versions

Security fixes are applied to the current `main` branch until ferrolex has its
first stable release. Pre-release versions and unreleased commits should be
updated to the latest `main` revision before reporting a duplicate issue.

## Reporting a vulnerability

Please use GitHub's private vulnerability-reporting flow for this repository:
[report a vulnerability](https://github.com/sebastian-software/ferrolex/security/advisories/new).
Do not open a public issue for an unpatched vulnerability.

Include a minimal reproducer, the ferrolex revision, affected platform, and an
assessment of practical impact. The maintainers will acknowledge reports,
validate the impact, and coordinate disclosure through a GitHub Security
Advisory where appropriate.

## Scope

ferrolex treats dictionaries, compiled artifacts, source files, and suggestion
queries as untrusted input. In scope are memory-safety defects, panics,
unbounded resource consumption, and integrity failures in importers, artifact
loaders, source analysis, and bounded suggestion processing. Unsupported
dictionary semantics that are diagnosed rather than silently accepted are not
compatibility vulnerabilities by themselves.

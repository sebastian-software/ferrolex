# Performance

## Lookup characterization

`cargo bench -p ferrolex-core` measures present and absent exact lookups over
deterministically generated dictionaries of 1,000, 10,000, and 100,000 words.
Dictionary construction is outside the measured closure; only `contains()` is
timed. The benchmark uses Criterion's black-box input and reports its estimates
and confidence intervals locally.

The harness establishes a reproducible baseline for data-structure decisions.
It does not yet compare ferrolex to another engine, measure process startup, or
support a portable performance claim. Those lanes require a versioned dictionary
corpus and semantically equivalent comparison configuration.

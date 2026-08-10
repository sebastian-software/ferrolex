# Compatibility reporting

ferrolex measures compatibility by named features and recognition results, not
by reproducing another spell checker's internal algorithm or suggestion order.

## Levels

1. **Format:** the importer can parse the source and return diagnostics.
2. **Recognition:** basic stems and supported generated forms are accepted or
   rejected according to ferrolex's documented semantics.
3. **Morphology:** continuation, capitalization, and compound rules operate.
4. **Ecosystem:** selected real-world dictionaries work without modification.

The initial test suite uses independently authored, minimal `.aff`/`.dic`
fixtures. Every fixture states the accepted and rejected words that distinguish
the rule under test. Real dictionary tests are added only after their source
and redistribution license are explicitly recorded.

## Feature status

The importer reports every encountered unsupported directive. A caller can
select strict mode for CI or lenient mode to inspect a partial import. Future
compiled dictionaries will record the features required for their recognition
semantics so that a runtime can reject an incompatible artifact before lookup.

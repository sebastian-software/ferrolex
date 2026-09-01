use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferrolex_core::WordList;
use ferrolex_suggest::{Completeness, SuggestConfig, SuggestScratch, Suggester, Suggestion};

const CORPUS_SIZE: usize = 100_000;
const TARGETS: [&str; 4] = [
    "accommodation",
    "receive",
    "characteristically",
    "rainbowbridge",
];

fn corpus() -> WordList {
    let mut words = (0..CORPUS_SIZE - TARGETS.len())
        .map(|index| format!("lexeme{index:06}"))
        .collect::<Vec<_>>();
    words.extend(TARGETS.map(str::to_owned));
    WordList::new(words).expect("benchmark corpus contains valid words")
}

fn suggestions(c: &mut Criterion) {
    let dictionary = corpus();
    assert_eq!(dictionary.len(), CORPUS_SIZE, "benchmark corpus size");
    let suggester = Suggester::new(&dictionary, SuggestConfig::default());
    let lanes = [
        ("single-edit", "accommodatoin", Some("accommodation")),
        ("transposition", "recieve", Some("receive")),
        ("long-word", "characteristicaly", Some("characteristically")),
        ("compound-typo", "rainbowbrigde", Some("rainbowbridge")),
        ("no-useful-suggestion", "zzzzzzzzzzzz", None),
    ];
    let mut group = c.benchmark_group("suggestions");
    group.throughput(Throughput::Elements(CORPUS_SIZE as u64));

    for (lane, query, expected) in lanes {
        let mut output: Vec<Suggestion> = Vec::new();
        let mut scratch = SuggestScratch::default();
        let completeness = suggester.suggest_into(query, &mut output, &mut scratch);
        assert_eq!(
            completeness,
            Completeness::Complete,
            "benchmark lane {lane} must not time a budget-truncated search"
        );
        if let Some(expected) = expected {
            assert!(
                output
                    .iter()
                    .any(|suggestion| suggestion.word() == expected),
                "benchmark lane {lane} must reach {expected}"
            );
        } else {
            assert!(
                output.is_empty(),
                "benchmark lane {lane} must remain a stable empty result"
            );
        }
        group.bench_with_input(
            BenchmarkId::new("scratch-reused", lane),
            query,
            |bench, query| {
                bench.iter(|| {
                    let completeness =
                        suggester.suggest_into(black_box(query), &mut output, &mut scratch);
                    black_box((completeness, output.len()));
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, suggestions);
criterion_main!(benches);

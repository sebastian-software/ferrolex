use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferrolex_core::WordList;
use ferrolex_suggest::{SuggestConfig, SuggestScratch, Suggester, Suggestion};

const CORPUS_SIZE: usize = 100_000;

fn corpus() -> WordList {
    let mut words = (0..CORPUS_SIZE)
        .map(|index| format!("lexeme{index:06}"))
        .collect::<Vec<_>>();
    words.extend([
        "accommodation".to_owned(),
        "receive".to_owned(),
        "characteristically".to_owned(),
        "rainbowbridge".to_owned(),
    ]);
    WordList::new(words).expect("benchmark corpus contains valid words")
}

fn suggestions(c: &mut Criterion) {
    let dictionary = corpus();
    let suggester = Suggester::new(&dictionary, SuggestConfig::default());
    let lanes = [
        ("single-edit", "accommodatoin"),
        ("transposition", "recieve"),
        ("long-word", "characteristicaly"),
        ("compound-typo", "rainbowbrigde"),
        ("no-useful-suggestion", "zzzzzzzzzzzz"),
    ];
    let mut group = c.benchmark_group("suggestions");
    group.throughput(Throughput::Elements(CORPUS_SIZE as u64));

    for (lane, query) in lanes {
        group.bench_with_input(
            BenchmarkId::new("scratch-reused", lane),
            query,
            |bench, query| {
                let mut output: Vec<Suggestion> = Vec::new();
                let mut scratch = SuggestScratch::default();
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

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ferrolex_core::{Dictionary, WordList};

fn lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("word-list lookup");

    for size in [1_000_usize, 10_000, 100_000] {
        let dictionary = WordList::new((0..size).map(|index| format!("word{index:06}")))
            .expect("generated benchmark entries are non-empty");
        let present = format!("word{:06}", size / 2);

        group.bench_with_input(
            BenchmarkId::new("present", size),
            &dictionary,
            |bench, dictionary| {
                bench.iter(|| dictionary.contains(black_box(&present)));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("absent", size),
            &dictionary,
            |bench, dictionary| {
                bench.iter(|| dictionary.contains(black_box("missing")));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, lookup);
criterion_main!(benches);

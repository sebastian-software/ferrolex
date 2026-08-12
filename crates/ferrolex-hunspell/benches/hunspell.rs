use std::fmt::Write as _;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ferrolex_code::{Analyzer, Document};
use ferrolex_core::Dictionary;
use ferrolex_hunspell::{import, HunspellDictionary, ImportMode};
use ferrolex_text::check_text;

const AFFIXES: &str = "SET UTF-8\nSFX N Y 1\nSFX N 0 n .\nCOMPOUNDFLAG C\nCOMPOUNDMIN 1\n";
const WORDS: &str = "7\nHouse/N\nRail/C\nWay/C\nFerrolex\nMarkdown\nRust\nTypeScript\n";

fn dictionary() -> HunspellDictionary {
    let dictionary = import(
        "benchmark.aff",
        AFFIXES,
        "benchmark.dic",
        WORDS,
        ImportMode::Strict,
    )
    .expect("the benchmark dictionary imports")
    .dictionary()
    .clone();

    for (word, expected) in [
        ("House", true),
        ("missing", false),
        ("Housen", true),
        ("RailWay", true),
        ("HOUSE", true),
    ] {
        assert_eq!(dictionary.contains(word), expected, "fixture lane {word}");
    }
    dictionary
}

fn morphology_lookup(c: &mut Criterion) {
    let dictionary = dictionary();
    let mut group = c.benchmark_group("hunspell lookup");

    for (name, query) in [
        ("hit", "House"),
        ("miss", "missing"),
        ("affixed", "Housen"),
        ("compound", "RailWay"),
        ("mixed-case", "HOUSE"),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &query, |bench, query| {
            bench.iter(|| dictionary.contains(black_box(query)));
        });
    }
    group.finish();
}

fn synthetic_repository_workloads() -> [(String, bool); 4] {
    let mut markdown = String::new();
    let mut typescript = String::new();
    let mut rust = String::new();
    let mut mixed = String::new();
    for index in 0..512 {
        writeln!(
            markdown,
            "# Ferrolex Markdown {index}\n\nHouse RailWay Housen misspelling"
        )
        .expect("writing to String does not fail");
        writeln!(
            typescript,
            "export const ferrolexHouse{index} = 'RailWay'; // Markdown misspelling"
        )
        .expect("writing to String does not fail");
        writeln!(
            rust,
            "pub fn ferrolex_house_{index}() {{ /* RailWay Housen */ }}"
        )
        .expect("writing to String does not fail");
        writeln!(
            mixed,
            "# Release {index}\n`ferrolexHouse{index}` uses RailWay."
        )
        .expect("writing to String does not fail");
    }
    [
        (markdown, false),
        (typescript, true),
        (rust, true),
        (mixed, true),
    ]
}

fn repository_checking(c: &mut Criterion) {
    let dictionary = dictionary();
    let analyzer = Analyzer::builder(&dictionary).build();
    let workloads = synthetic_repository_workloads();
    let mut group = c.benchmark_group("synthetic repository checking");

    for (name, (source, is_code)) in ["markdown", "typescript", "rust", "mixed"]
        .into_iter()
        .zip(workloads.iter())
    {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(source, is_code),
            |bench, input| {
                bench.iter(|| {
                    if *input.1 {
                        black_box(
                            analyzer
                                .check(&Document::new(black_box(input.0)))
                                .findings()
                                .len(),
                        )
                    } else {
                        black_box(check_text(&dictionary, black_box(input.0)).count())
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, morphology_lookup, repository_checking);
criterion_main!(benches);

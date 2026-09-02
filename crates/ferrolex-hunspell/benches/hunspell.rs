use std::fmt::Write as _;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferrolex_code::{Analyzer, Document};
use ferrolex_core::Dictionary;
use ferrolex_hunspell::{
    compile_runtime_cache, import, load_runtime_cache, HunspellDictionary, ImportMode,
    SourceDigests,
};
use ferrolex_suggest::{Completeness, SuggestConfig, SuggestScratch, Suggester, Suggestion};
use ferrolex_text::check_text;

const AFFIXES: &str = "SET UTF-8\nSFX N Y 1\nSFX N 0 n .\nCOMPOUNDFLAG C\nCOMPOUNDMIN 1\n";
const WORDS: &str = "7\nHouse/N\nRail/C\nWay/C\nFerrolex\nMarkdown\nRust\nTypeScript\n";
const LARGE_AFFIXES: &str = "SET UTF-8\nSFX N Y 1\nSFX N 0 n .\n";
const LARGE_CORPUS_SIZE: usize = 100_000;
const LARGE_TARGET_INDEX: usize = LARGE_CORPUS_SIZE / 2;
const LARGE_STEM_MULTIPLIER: u64 = 0x9e37_79b9_7f4a_7c15;
const EMPTY_ADD_CORPUS_SIZE: usize = 8_192;

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

fn empty_add_miss_lookup(c: &mut Criterion) {
    let mut words = String::new();
    writeln!(words, "{EMPTY_ADD_CORPUS_SIZE}").expect("writing to String does not fail");
    for index in 0..EMPTY_ADD_CORPUS_SIZE {
        writeln!(words, "emptyadd{index}/A").expect("writing to String does not fail");
    }
    let dictionary = import(
        "empty-add.aff",
        "SFX A N 1\nSFX A 0 0 .\n",
        "empty-add.dic",
        &words,
        ImportMode::Strict,
    )
    .expect("the empty-add benchmark dictionary imports")
    .dictionary()
    .clone();
    assert!(!dictionary.contains("totallyabsentword"));

    c.bench_function("hunspell empty-add miss 8k", |bench| {
        bench.iter(|| dictionary.contains(black_box("totallyabsentword")));
    });
}

fn large_words() -> String {
    let mut words = String::with_capacity(LARGE_CORPUS_SIZE * 26);
    writeln!(words, "{LARGE_CORPUS_SIZE}").expect("writing to String does not fail");
    for index in 0..LARGE_CORPUS_SIZE {
        writeln!(words, "{}/N", large_stem(index)).expect("writing to String does not fail");
    }
    words
}

fn large_stem(index: usize) -> String {
    let distributed = u64::try_from(index)
        .expect("fixture index fits u64")
        .wrapping_mul(LARGE_STEM_MULTIPLIER);
    format!("lexeme{distributed:016x}")
}

fn large_dictionary(words: &str) -> HunspellDictionary {
    import(
        "large-benchmark.aff",
        LARGE_AFFIXES,
        "large-benchmark.dic",
        words,
        ImportMode::Strict,
    )
    .expect("the large benchmark dictionary imports")
    .dictionary()
    .clone()
}

struct LargeFixture {
    words: String,
    dictionary: HunspellDictionary,
    sources: SourceDigests,
    cache: Vec<u8>,
    hit: String,
    affixed: String,
    typo: String,
}

impl LargeFixture {
    fn new() -> Self {
        let words = large_words();
        let dictionary = large_dictionary(&words);
        let sources = SourceDigests::from_source_bytes(LARGE_AFFIXES.as_bytes(), words.as_bytes());
        let cache = compile_runtime_cache(&dictionary, sources)
            .expect("the large benchmark dictionary compiles to a runtime cache");
        let cached_dictionary = load_runtime_cache(&cache, sources)
            .expect("the large benchmark runtime cache loads before timing");
        let hit = large_stem(LARGE_TARGET_INDEX);
        let affixed = format!("{hit}n");
        let typo = format!("{hit}m");
        assert_eq!(dictionary.stems().count(), LARGE_CORPUS_SIZE);
        assert_eq!(cached_dictionary.stems().count(), LARGE_CORPUS_SIZE);
        for (word, expected) in [
            (hit.as_str(), true),
            (affixed.as_str(), true),
            ("absentffffffffffff", false),
        ] {
            assert_eq!(dictionary.contains(word), expected, "source lane {word}");
            assert_eq!(
                cached_dictionary.contains(word),
                expected,
                "cache lane {word}"
            );
        }
        Self {
            words,
            dictionary,
            sources,
            cache,
            hit,
            affixed,
            typo,
        }
    }
}

fn benchmark_large_import(c: &mut Criterion, fixture: &LargeFixture) {
    let mut import_group = c.benchmark_group("hunspell import 100k");
    import_group.throughput(Throughput::Bytes(
        u64::try_from(LARGE_AFFIXES.len() + fixture.words.len()).expect("fixture size fits u64"),
    ));
    import_group.bench_function("strict source", |bench| {
        bench.iter(|| {
            black_box(
                import(
                    "large-benchmark.aff",
                    black_box(LARGE_AFFIXES),
                    "large-benchmark.dic",
                    black_box(&fixture.words),
                    ImportMode::Strict,
                )
                .expect("the benchmark source remains valid"),
            )
        });
    });
    import_group.finish();
}

fn benchmark_large_cache_load(c: &mut Criterion, fixture: &LargeFixture) {
    let mut cache_group = c.benchmark_group("hunspell cache load 100k");
    cache_group.throughput(Throughput::Bytes(
        u64::try_from(fixture.cache.len()).expect("fixture size fits u64"),
    ));
    cache_group.bench_function("validated runtime cache", |bench| {
        bench.iter(|| {
            black_box(
                load_runtime_cache(black_box(&fixture.cache), fixture.sources)
                    .expect("the benchmark cache remains valid"),
            )
        });
    });
    cache_group.finish();
}

fn benchmark_large_lookup(c: &mut Criterion, fixture: &LargeFixture) {
    let mut lookup_group = c.benchmark_group("hunspell lookup 100k");
    for (name, query) in [
        ("hit", fixture.hit.as_str()),
        ("affixed", fixture.affixed.as_str()),
        ("miss", "absentffffffffffff"),
    ] {
        lookup_group.bench_with_input(BenchmarkId::from_parameter(name), &query, |bench, query| {
            bench.iter(|| fixture.dictionary.contains(black_box(query)));
        });
    }
    lookup_group.finish();
}

fn benchmark_large_suggestion(c: &mut Criterion, fixture: &LargeFixture) {
    let suggester = Suggester::new(&fixture.dictionary, SuggestConfig::default())
        .with_replacement_rules(fixture.dictionary.replacement_rules())
        .with_ranking_signals(fixture.dictionary.ranking_signals());
    let query = fixture.typo.as_str();
    let expected = fixture.affixed.as_str();
    let mut output: Vec<Suggestion> = Vec::new();
    let mut scratch = SuggestScratch::default();
    let completeness = suggester.suggest_into(query, &mut output, &mut scratch);
    assert_eq!(
        completeness,
        Completeness::Complete,
        "Hunspell suggestion benchmark must not time a budget-truncated search"
    );
    assert!(
        output
            .iter()
            .any(|suggestion| suggestion.word() == expected),
        "Hunspell suggestion benchmark must reach the derived form {expected}"
    );
    let mut suggestion_group = c.benchmark_group("hunspell suggestion 100k");
    suggestion_group.throughput(Throughput::Elements(LARGE_CORPUS_SIZE as u64));
    suggestion_group.bench_function("affixed typo", |bench| {
        bench.iter(|| {
            let completeness = suggester.suggest_into(black_box(query), &mut output, &mut scratch);
            black_box((completeness, output.len()));
        });
    });
    suggestion_group.finish();
}

fn dominant_hunspell_paths(c: &mut Criterion) {
    let fixture = LargeFixture::new();
    benchmark_large_import(c, &fixture);
    benchmark_large_cache_load(c, &fixture);
    benchmark_large_lookup(c, &fixture);
    benchmark_large_suggestion(c, &fixture);
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

criterion_group!(
    benches,
    morphology_lookup,
    empty_add_miss_lookup,
    dominant_hunspell_paths,
    repository_checking
);
criterion_main!(benches);

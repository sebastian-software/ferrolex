use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferrolex_compiler::{compile_words, CompiledDictionary};
use ferrolex_core::{Dictionary, Normalization, WordList};
use fst::{Set, SetBuilder};

/// A deterministic UTF-8 corpus used by every benchmark lane in this file.
///
/// These are synthetic exact words, not a natural-language corpus. They make
/// the comparison repeatable while exercising ASCII and multi-byte UTF-8
/// lookup paths. See `docs/performance.md` for the workload contract.
fn words(size: usize) -> Vec<String> {
    (0..size)
        .map(|index| match index % 3 {
            0 => format!("alpha{index:06}"),
            1 => format!("straße{index:06}"),
            _ => format!("東京{index:06}"),
        })
        .collect()
}

fn plain_text(words: &[String]) -> String {
    let mut text = words.join("\n");
    text.push('\n');
    text
}

/// Builds a minimal finite-state set from the same byte-sorted corpus.
///
/// This is an evaluation candidate only. It is intentionally kept in the
/// benchmark so a production dependency is not adopted without a reproducible
/// result at dictionary scale.
fn finite_state_set(words: &[String]) -> Set<Vec<u8>> {
    let mut sorted = words.iter().map(String::as_str).collect::<Vec<_>>();
    sorted.sort_unstable();
    let mut builder = SetBuilder::memory();
    for word in sorted {
        builder.insert(word).expect("generated words are ordered");
    }
    Set::new(builder.into_inner().expect("the set serializes")).expect("the generated set is valid")
}

fn lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("exact lookup parity");

    for size in [1_000_usize, 10_000, 100_000, 250_000] {
        let entries = words(size);
        let word_list = WordList::new(&entries).expect("generated entries are valid");
        let compiled =
            CompiledDictionary::load(compile_words(&entries).expect("generated entries compile"))
                .expect("generated artifact loads");
        let finite_state = finite_state_set(&entries);
        let present = entries[size / 2].clone();
        let absent = format!("missing{size:06}");

        for query in [&present, &absent] {
            assert_eq!(
                word_list.contains(query),
                compiled.contains(query),
                "benchmark candidates must preserve exact lookup semantics"
            );
            assert_eq!(
                word_list.contains(query),
                finite_state.contains(query),
                "benchmark candidates must preserve exact lookup semantics"
            );
        }

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("word-list/present", size),
            &word_list,
            |bench, dictionary| {
                bench.iter(|| dictionary.contains(black_box(&present)));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("compiled/present", size),
            &compiled,
            |bench, dictionary| {
                bench.iter(|| dictionary.contains(black_box(&present)));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("fst/present", size),
            &finite_state,
            |bench, dictionary| {
                bench.iter(|| dictionary.contains(black_box(&present)));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("word-list/absent", size),
            &word_list,
            |bench, dictionary| {
                bench.iter(|| dictionary.contains(black_box(&absent)));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("compiled/absent", size),
            &compiled,
            |bench, dictionary| {
                bench.iter(|| dictionary.contains(black_box(&absent)));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("fst/absent", size),
            &finite_state,
            |bench, dictionary| {
                bench.iter(|| dictionary.contains(black_box(&absent)));
            },
        );
    }

    group.finish();
}

fn loading(c: &mut Criterion) {
    let mut group = c.benchmark_group("in-memory artifact loading");

    for size in [1_000_usize, 10_000, 100_000, 250_000] {
        let entries = words(size);
        let text = plain_text(&entries);
        let bytes = compile_words(&entries).expect("generated entries compile");

        // Check semantic parity before timing format-specific construction.
        let plain = WordList::from_text(Normalization::Exact, &text);
        let compiled = CompiledDictionary::load(bytes.clone()).expect("artifact loads");
        assert_eq!(plain.len(), compiled.len());
        assert!(entries
            .iter()
            .all(|word| plain.contains(word) == compiled.contains(word)));

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::new("plain-text/word-list", size),
            &text,
            |bench, text| {
                bench.iter(|| {
                    // Each lane owns an in-memory copy, which represents the
                    // bytes supplied by its caller. File I/O is deliberately
                    // outside this benchmark.
                    let input = black_box(text.clone());
                    let dictionary = WordList::from_text(Normalization::Exact, &input);
                    black_box(dictionary.len())
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("compiled/loader", size),
            &bytes,
            |bench, bytes| {
                bench.iter(|| {
                    let dictionary = CompiledDictionary::load(black_box(bytes.clone()))
                        .expect("generated artifact remains valid");
                    black_box(dictionary.len())
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, lookup, loading);
criterion_main!(benches);

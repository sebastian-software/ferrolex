//! Deterministic quality regression checks over the reviewable test corpus.

use std::collections::{BTreeMap, BTreeSet};

use ferrolex_suggest::{CandidateSource, SuggestConfig, Suggester, Suggestion};

const CORPUS: &str = include_str!("data/suggestion-quality-corpus.tsv");
const BASELINE: &str = include_str!("data/suggestion-quality-baseline.tsv");
const CORPUS_HEADER: &str = "id\tmisspelling\tintended_word\tlocale\tcontext\tprovenance\treview_status\treviewed_by\treviewed_on\tdisposition\texclusion_rationale\tfrequency_fixture";
const BASELINE_HEADER: &str = "version\tlocale\tcontext\tevaluated_cases\tstandard_top_1\tstandard_top_3\tfrequency_top_1\tfrequency_top_3\treview_status\treviewed_by\treviewed_on\tchange_rationale";
const QUALITY_MAX_RESULTS: usize = 3;

#[derive(Debug)]
struct CorpusCase {
    id: String,
    misspelling: String,
    intended_word: String,
    locale: String,
    context: String,
    provenance: String,
    review_status: String,
    reviewed_by: String,
    reviewed_on: String,
    disposition: String,
    exclusion_rationale: String,
    frequency_fixture: String,
}

impl CorpusCase {
    fn group(&self) -> Group {
        Group {
            locale: self.locale.clone(),
            context: self.context.clone(),
        }
    }

    fn is_included(&self) -> bool {
        self.disposition == "included"
    }

    fn has_frequency_fixture(&self) -> bool {
        self.frequency_fixture != "-"
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Group {
    locale: String,
    context: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Score {
    cases: usize,
    top_1: usize,
    top_3: usize,
}

impl Score {
    fn record(&mut self, recovered_at_1: bool, recovered_at_3: bool) {
        self.cases += 1;
        self.top_1 += usize::from(recovered_at_1);
        self.top_3 += usize::from(recovered_at_3);
    }
}

#[derive(Debug)]
struct BaselineRow {
    version: usize,
    group: Group,
    standard: Score,
    frequency: Option<Score>,
    review_status: String,
    reviewed_by: String,
    reviewed_on: String,
    change_rationale: String,
}

struct QualitySource {
    candidates: Vec<(String, Option<u64>)>,
}

impl CandidateSource for QualitySource {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for (candidate, _) in &self.candidates {
            if !visitor(candidate) {
                break;
            }
        }
    }

    fn candidate_frequency(&self, candidate: &str) -> Option<u64> {
        self.candidates
            .iter()
            .find_map(|(stored, frequency)| (stored == candidate).then_some(*frequency))
            .flatten()
    }
}

#[test]
fn suggestion_quality_matches_the_review_gated_baseline() {
    let corpus = parse_corpus(CORPUS);
    let baseline = parse_baseline(BASELINE);
    validate_corpus(&corpus);
    validate_baseline(&baseline);

    let standard_source = source_for(&corpus, false);
    let frequency_source = source_for(&corpus, true);
    let mut standard_scores = BTreeMap::new();
    let mut frequency_scores = BTreeMap::new();

    for case in corpus.iter().filter(|case| case.is_included()) {
        let standard = recover(&standard_source, case);
        standard_scores
            .entry(case.group())
            .or_insert_with(Score::default)
            .record(standard.0, standard.1);

        if case.has_frequency_fixture() {
            let frequency = recover(&frequency_source, case);
            frequency_scores
                .entry(case.group())
                .or_insert_with(Score::default)
                .record(frequency.0, frequency.1);
        }
    }

    let expected_groups = baseline
        .iter()
        .map(|row| row.group.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        standard_scores.keys().cloned().collect::<BTreeSet<_>>(),
        expected_groups,
        "Every locale/context score needs an explicit reviewed baseline row."
    );

    eprintln!("Suggestion-quality corpus v1 (top-{QUALITY_MAX_RESULTS}):");
    for row in &baseline {
        let actual_standard = standard_scores
            .get(&row.group)
            .copied()
            .expect("validated baseline group exists");
        eprintln!(
            "  {}/{} standard: top-1 {}/{}, top-{} {}/{}",
            row.group.locale,
            row.group.context,
            actual_standard.top_1,
            actual_standard.cases,
            QUALITY_MAX_RESULTS,
            actual_standard.top_3,
            actual_standard.cases,
        );
        assert_eq!(
            actual_standard, row.standard,
            "Suggestion quality regressed for {}/{}. Update the baseline only with a reviewed rationale.",
            row.group.locale, row.group.context
        );

        match row.frequency {
            Some(expected_frequency) => {
                let actual_frequency = frequency_scores
                    .get(&row.group)
                    .copied()
                    .expect("frequency baseline requires a frequency fixture");
                eprintln!(
                    "  {}/{} frequency: top-1 {}/{}, top-{} {}/{}",
                    row.group.locale,
                    row.group.context,
                    actual_frequency.top_1,
                    actual_frequency.cases,
                    QUALITY_MAX_RESULTS,
                    actual_frequency.top_3,
                    actual_frequency.cases,
                );
                assert_eq!(
                    actual_frequency, expected_frequency,
                    "Frequency-aware suggestion quality regressed for {}/{}. Update the baseline only with a reviewed rationale.",
                    row.group.locale, row.group.context
                );
            }
            None => assert!(
                !frequency_scores.contains_key(&row.group),
                "Only explicitly declared frequency fixtures may contribute frequency scores."
            ),
        }
    }
}

fn recover(source: &QualitySource, case: &CorpusCase) -> (bool, bool) {
    let result = Suggester::new(
        source,
        SuggestConfig {
            max_results: QUALITY_MAX_RESULTS,
            max_edit_distance: 2,
            ..SuggestConfig::default()
        },
    )
    .suggest(&case.misspelling);
    let words = result
        .suggestions()
        .iter()
        .map(Suggestion::word)
        .collect::<Vec<_>>();
    (
        words
            .first()
            .is_some_and(|word| *word == case.intended_word),
        words
            .iter()
            .take(QUALITY_MAX_RESULTS)
            .any(|word| *word == case.intended_word),
    )
}

fn source_for(corpus: &[CorpusCase], with_frequency: bool) -> QualitySource {
    let mut candidates = BTreeMap::new();
    for case in corpus.iter().filter(|case| case.is_included()) {
        candidates.entry(case.intended_word.clone()).or_insert(None);
        for (candidate, frequency) in frequency_fixture(case) {
            let entry = candidates.entry(candidate).or_insert(None);
            if with_frequency {
                assert!(
                    entry
                        .replace(frequency)
                        .is_none_or(|existing| existing == frequency),
                    "frequency controls must not disagree for one candidate"
                );
            }
        }
    }
    QualitySource {
        candidates: candidates.into_iter().collect(),
    }
}

fn frequency_fixture(case: &CorpusCase) -> Vec<(String, u64)> {
    if case.frequency_fixture == "-" {
        return Vec::new();
    }
    case.frequency_fixture
        .split(';')
        .map(|control| {
            let (candidate, frequency) = control
                .split_once('=')
                .unwrap_or_else(|| panic!("{} has an invalid frequency fixture", case.id));
            assert!(
                !candidate.is_empty(),
                "{} has an empty frequency-fixture candidate",
                case.id
            );
            let frequency = frequency.parse::<u64>().unwrap_or_else(|error| {
                panic!(
                    "{} has an invalid frequency-fixture value: {error}",
                    case.id
                )
            });
            (candidate.to_owned(), frequency)
        })
        .collect()
}

fn parse_corpus(contents: &str) -> Vec<CorpusCase> {
    let mut lines = contents.lines();
    assert_eq!(
        lines.next(),
        Some(CORPUS_HEADER),
        "unexpected corpus schema"
    );
    lines
        .enumerate()
        .map(|(index, line)| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(
                fields.len(),
                12,
                "corpus row {} has an invalid field count",
                index + 2
            );
            CorpusCase {
                id: fields[0].to_owned(),
                misspelling: fields[1].to_owned(),
                intended_word: fields[2].to_owned(),
                locale: fields[3].to_owned(),
                context: fields[4].to_owned(),
                provenance: fields[5].to_owned(),
                review_status: fields[6].to_owned(),
                reviewed_by: fields[7].to_owned(),
                reviewed_on: fields[8].to_owned(),
                disposition: fields[9].to_owned(),
                exclusion_rationale: fields[10].to_owned(),
                frequency_fixture: fields[11].to_owned(),
            }
        })
        .collect()
}

fn parse_baseline(contents: &str) -> Vec<BaselineRow> {
    let mut lines = contents.lines();
    assert_eq!(
        lines.next(),
        Some(BASELINE_HEADER),
        "unexpected baseline schema"
    );
    lines
        .enumerate()
        .map(|(index, line)| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(
                fields.len(),
                12,
                "baseline row {} has an invalid field count",
                index + 2
            );
            let parsed = |field: usize| {
                fields[field].parse::<usize>().unwrap_or_else(|error| {
                    panic!(
                        "baseline row {} field {} is not a number: {error}",
                        index + 2,
                        field + 1
                    )
                })
            };
            let frequency = match (fields[6], fields[7]) {
                ("-", "-") => None,
                (top_1, top_3) => Some(Score {
                    cases: parsed(3),
                    top_1: top_1.parse().unwrap_or_else(|error| {
                        panic!(
                            "baseline row {} frequency top-1 is not a number: {error}",
                            index + 2
                        )
                    }),
                    top_3: top_3.parse().unwrap_or_else(|error| {
                        panic!(
                            "baseline row {} frequency top-3 is not a number: {error}",
                            index + 2
                        )
                    }),
                }),
            };
            BaselineRow {
                version: parsed(0),
                group: Group {
                    locale: fields[1].to_owned(),
                    context: fields[2].to_owned(),
                },
                standard: Score {
                    cases: parsed(3),
                    top_1: parsed(4),
                    top_3: parsed(5),
                },
                frequency,
                review_status: fields[8].to_owned(),
                reviewed_by: fields[9].to_owned(),
                reviewed_on: fields[10].to_owned(),
                change_rationale: fields[11].to_owned(),
            }
        })
        .collect()
}

fn validate_corpus(corpus: &[CorpusCase]) {
    assert!(
        !corpus.is_empty(),
        "corpus must contain at least one reviewed row"
    );
    let mut ids = BTreeSet::new();
    for case in corpus {
        assert!(
            ids.insert(&case.id),
            "corpus IDs must be unique: {}",
            case.id
        );
        assert!(
            !case.misspelling.is_empty()
                && !case.intended_word.is_empty()
                && !case.locale.is_empty()
                && !case.context.is_empty()
                && !case.provenance.is_empty(),
            "corpus row {} is missing required evidence",
            case.id
        );
        validate_review_record(
            &case.review_status,
            &case.reviewed_by,
            &case.reviewed_on,
            &format!("corpus row {}", case.id),
        );
        assert!(
            matches!(case.disposition.as_str(), "included" | "excluded"),
            "corpus row {} has an invalid disposition",
            case.id
        );
        if case.disposition == "excluded" {
            assert_ne!(
                case.exclusion_rationale, "-",
                "excluded row {} needs a rationale",
                case.id
            );
        } else {
            assert_eq!(
                case.exclusion_rationale, "-",
                "included row {} must not carry an exclusion rationale",
                case.id
            );
        }
        let controls = frequency_fixture(case);
        let control_words = controls
            .iter()
            .map(|(candidate, _)| candidate)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            control_words.len(),
            controls.len(),
            "corpus row {} repeats a frequency-fixture candidate",
            case.id
        );
        if case.has_frequency_fixture() {
            let intended_frequency = controls
                .iter()
                .find_map(|(candidate, frequency)| {
                    (candidate == &case.intended_word).then_some(*frequency)
                })
                .unwrap_or_else(|| {
                    panic!(
                        "frequency fixture {} must include its intended word",
                        case.id
                    )
                });
            assert!(
                controls.len() >= 2,
                "frequency fixture {} needs an alternate ranking candidate",
                case.id
            );
            assert!(
                controls.iter().all(|(candidate, frequency)| {
                    candidate == &case.intended_word || *frequency < intended_frequency
                }),
                "frequency fixture {} must give its intended word the unique highest frequency",
                case.id
            );
        }
    }
}

fn validate_baseline(baseline: &[BaselineRow]) {
    assert!(
        !baseline.is_empty(),
        "baseline must contain every evaluated group"
    );
    let mut groups = BTreeSet::new();
    for row in baseline {
        assert_eq!(
            row.version, 1,
            "baseline version must be reviewed before changing"
        );
        assert!(groups.insert(&row.group), "baseline groups must be unique");
        validate_review_record(
            &row.review_status,
            &row.reviewed_by,
            &row.reviewed_on,
            &format!("baseline row {}/{}", row.group.locale, row.group.context),
        );
        assert!(
            !row.change_rationale.is_empty(),
            "baseline rows need a change rationale"
        );
        assert!(row.standard.top_1 <= row.standard.top_3);
        assert!(row.standard.top_3 <= row.standard.cases);
        if let Some(frequency) = row.frequency {
            assert!(frequency.top_1 <= frequency.top_3);
            assert!(frequency.top_3 <= frequency.cases);
        }
    }
}

fn validate_review_record(status: &str, reviewed_by: &str, reviewed_on: &str, subject: &str) {
    match status {
        "requires-maintainer-review" => {
            assert_eq!(
                reviewed_by, "-",
                "{subject} has a pending review but a reviewer"
            );
            assert_eq!(
                reviewed_on, "-",
                "{subject} has a pending review but a review date"
            );
        }
        "approved-by-maintainer" => {
            assert_ne!(reviewed_by, "-", "{subject} needs its approving maintainer");
            assert_ne!(reviewed_on, "-", "{subject} needs its approval date");
        }
        _ => panic!("{subject} has an invalid review status: {status}"),
    }
}

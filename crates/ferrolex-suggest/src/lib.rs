//! Deterministic, bounded spelling suggestions.

#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::collections::BTreeSet;

use ferrolex_core::{UserDictionary, WordList};

/// A stable source of suggestion candidates.
pub trait CandidateSource: Send + Sync {
    /// Visits candidates in deterministic byte-lexicographic order.
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool);

    /// Returns whether a stored candidate may be shown as a suggestion.
    ///
    /// Sources with richer recognition semantics can reject pseudo-stems or
    /// policy-marked entries here. The default preserves plain word-list and
    /// user-dictionary behavior.
    fn is_suggestion_candidate(&self, _candidate: &str) -> bool {
        true
    }
}

impl CandidateSource for WordList {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for word in self.words() {
            if !visitor(word) {
                break;
            }
        }
    }
}

impl CandidateSource for UserDictionary {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        // Snapshotting releases the overlay lock before a caller performs
        // bounded edit-distance work. It also gives this mutable source a
        // deterministic candidate order for one suggestion request.
        for word in self.snapshot() {
            if !visitor(&word) {
                break;
            }
        }
    }
}

/// Limits deterministic suggestion work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SuggestConfig {
    /// Maximum output suggestions.
    pub max_results: usize,
    /// Maximum OSA edit distance.
    pub max_edit_distance: usize,
    /// Maximum Unicode scalar values in the query or candidate.
    pub max_word_scalars: usize,
    /// Maximum source candidates inspected.
    pub max_candidates: usize,
    /// Maximum dynamic-programming cells evaluated.
    pub max_edit_cells: usize,
}

impl Default for SuggestConfig {
    fn default() -> Self {
        Self {
            max_results: 8,
            max_edit_distance: 2,
            max_word_scalars: 64,
            max_candidates: 100_000,
            max_edit_cells: 1_000_000,
        }
    }
}

/// A ranked spelling suggestion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Suggestion {
    word: String,
    distance: usize,
}

/// An explicit spelling replacement preferred during suggestion ranking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplacementRule {
    from: String,
    to: String,
    at_word_start: bool,
    at_word_end: bool,
}

impl ReplacementRule {
    /// Creates a non-empty replacement rule.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Option<Self> {
        Self::with_boundaries(from, to, false, false)
    }

    /// Creates a replacement rule with optional whole-word boundaries.
    #[must_use]
    pub fn with_boundaries(
        from: impl Into<String>,
        to: impl Into<String>,
        at_word_start: bool,
        at_word_end: bool,
    ) -> Option<Self> {
        let from = from.into();
        let to = to.into();
        (!from.is_empty() && !to.is_empty()).then_some(Self {
            from,
            to,
            at_word_start,
            at_word_end,
        })
    }

    /// Returns the typo spelling matched in a query.
    #[must_use]
    pub fn from(&self) -> &str {
        &self.from
    }

    /// Returns the preferred replacement spelling.
    #[must_use]
    pub fn to(&self) -> &str {
        &self.to
    }

    /// Whether the typo spelling must be at the start of the query.
    #[must_use]
    pub const fn at_word_start(&self) -> bool {
        self.at_word_start
    }

    /// Whether the typo spelling must be at the end of the query.
    #[must_use]
    pub const fn at_word_end(&self) -> bool {
        self.at_word_end
    }
}

impl Suggestion {
    /// Returns the display spelling.
    #[must_use]
    pub fn word(&self) -> &str {
        &self.word
    }
    /// Returns the OSA edit distance used for ranking.
    #[must_use]
    pub const fn distance(&self) -> usize {
        self.distance
    }
}

/// Whether a result includes every candidate permitted by configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completeness {
    Complete,
    CandidateLimitReached,
    EditBudgetReached,
    QueryTooLong,
}

/// Bounded suggestion output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuggestionResult {
    suggestions: Vec<Suggestion>,
    completeness: Completeness,
}

impl SuggestionResult {
    /// Returns suggestions in deterministic rank order.
    #[must_use]
    pub fn suggestions(&self) -> &[Suggestion] {
        &self.suggestions
    }
    /// Returns whether all allowed work completed.
    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }
}

/// Generates deterministic edit-distance suggestions from a candidate source.
pub struct Suggester<'source, S: ?Sized> {
    source: &'source S,
    config: SuggestConfig,
    replacements: &'source [ReplacementRule],
}

impl<'source, S: CandidateSource + ?Sized> Suggester<'source, S> {
    /// Creates a suggester with explicit deterministic limits.
    #[must_use]
    pub const fn new(source: &'source S, config: SuggestConfig) -> Self {
        Self {
            source,
            config,
            replacements: &[],
        }
    }

    /// Adds explicit replacement rules to deterministic candidate ranking.
    #[must_use]
    pub const fn with_replacement_rules(
        mut self,
        replacements: &'source [ReplacementRule],
    ) -> Self {
        self.replacements = replacements;
        self
    }
    /// Generates ranked suggestions for `query`.
    #[must_use]
    pub fn suggest(&self, query: &str) -> SuggestionResult {
        let Some(query_chars) = lowercase_chars_bounded(query, self.config.max_word_scalars) else {
            return SuggestionResult {
                suggestions: Vec::new(),
                completeness: Completeness::QueryTooLong,
            };
        };
        let mut suggestions = Vec::new();
        let mut examined = 0;
        let mut cells = 0_usize;
        let mut completeness = Completeness::Complete;
        self.source.visit_candidates(&mut |candidate| {
            if examined == self.config.max_candidates {
                completeness = Completeness::CandidateLimitReached;
                return false;
            }
            examined += 1;
            if !self.source.is_suggestion_candidate(candidate) {
                return true;
            }
            let Some(candidate_chars) =
                lowercase_chars_bounded(candidate, self.config.max_word_scalars)
            else {
                return true;
            };
            let distance = if let Some(distance) =
                replacement_distance(&query_chars, &candidate_chars, self.replacements)
            {
                Some(distance)
            } else {
                // A length difference beyond the permitted distance cannot
                // reach the dynamic-programming matrix. It must not spend the
                // edit-cell budget merely because it appeared early in lexical
                // candidate order.
                if query_chars.len().abs_diff(candidate_chars.len()) > self.config.max_edit_distance
                {
                    return true;
                }
                let required = (query_chars.len() + 1).saturating_mul(candidate_chars.len() + 1);
                if cells.saturating_add(required) > self.config.max_edit_cells {
                    completeness = Completeness::EditBudgetReached;
                    return false;
                }
                cells += required;
                osa_distance(
                    &query_chars,
                    &candidate_chars,
                    self.config.max_edit_distance,
                )
            };
            if let Some(distance) = distance {
                suggestions.push(Suggestion {
                    word: present(candidate, query),
                    distance,
                });
            }
            true
        });
        suggestions.sort_unstable_by(compare_suggestions);
        let mut presented = BTreeSet::new();
        suggestions.retain(|suggestion| presented.insert(suggestion.word.clone()));
        suggestions.truncate(self.config.max_results);
        SuggestionResult {
            suggestions,
            completeness,
        }
    }
}

fn replacement_distance(
    query: &[char],
    candidate: &[char],
    replacements: &[ReplacementRule],
) -> Option<usize> {
    replacements.iter().find_map(|rule| {
        let from = lowercase_chars_bounded(&rule.from, query.len())?;
        let to = lowercase_chars_bounded(&rule.to, candidate.len())?;
        if from.is_empty() || to.is_empty() || query.len() < from.len() {
            return None;
        }
        (0..=query.len() - from.len()).find_map(|start| {
            if (rule.at_word_start && start != 0)
                || (rule.at_word_end && start + from.len() != query.len())
            {
                return None;
            }
            if query[start..start + from.len()] != from {
                return None;
            }
            let mut transformed = Vec::with_capacity(query.len() - from.len() + to.len());
            transformed.extend_from_slice(&query[..start]);
            transformed.extend_from_slice(&to);
            transformed.extend_from_slice(&query[start + from.len()..]);
            (transformed == candidate).then_some(0)
        })
    })
}

fn compare_suggestions(left: &Suggestion, right: &Suggestion) -> Ordering {
    left.distance
        .cmp(&right.distance)
        .then_with(|| left.word.cmp(&right.word))
}

fn present(candidate: &str, query: &str) -> String {
    if !query.is_empty() && query.chars().all(char::is_uppercase) {
        candidate.to_uppercase()
    } else if query.chars().next().is_some_and(char::is_uppercase)
        && query.chars().skip(1).all(char::is_lowercase)
    {
        let mut chars = candidate.chars();
        chars.next().map_or_else(String::new, |first| {
            first
                .to_uppercase()
                .chain(chars.flat_map(char::to_lowercase))
                .collect()
        })
    } else {
        candidate.to_owned()
    }
}

fn lowercase_chars_bounded(word: &str, maximum: usize) -> Option<Vec<char>> {
    let mut lowercase = Vec::new();
    for character in word.chars() {
        for lowercase_character in character.to_lowercase() {
            if lowercase.len() == maximum {
                return None;
            }
            lowercase.push(lowercase_character);
        }
    }
    Some(lowercase)
}

fn osa_distance(left: &[char], right: &[char], maximum: usize) -> Option<usize> {
    if left.len().abs_diff(right.len()) > maximum {
        return None;
    }
    let mut previous_previous = vec![0; right.len() + 1];
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_char) in left.iter().enumerate() {
        let mut current = vec![left_index + 1; right.len() + 1];
        for (right_index, right_char) in right.iter().enumerate() {
            let cost = usize::from(left_char != right_char);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + cost);
            if left_index > 0
                && right_index > 0
                && *left_char == right[right_index - 1]
                && left[left_index - 1] == *right_char
            {
                current[right_index + 1] =
                    current[right_index + 1].min(previous_previous[right_index - 1] + 1);
            }
        }
        previous_previous = previous;
        previous = current;
    }
    (previous[right.len()] <= maximum).then_some(previous[right.len()])
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use super::{
        replacement_distance, CandidateSource, Completeness, ReplacementRule, SuggestConfig,
        Suggester, Suggestion,
    };
    use ferrolex_core::{Normalization, UserDictionary, WordList};

    struct TestSource<'candidate> {
        candidates: &'candidate [&'candidate str],
    }

    impl CandidateSource for TestSource<'_> {
        fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
            for candidate in self.candidates {
                if !visitor(candidate) {
                    break;
                }
            }
        }
    }

    #[test]
    fn suggests_missing_letters_and_transpositions_deterministically() {
        let words = WordList::new(["authentication", "receive", "recipe"]).expect("valid words");
        let suggester = Suggester::new(&words, SuggestConfig::default());
        assert_eq!(
            suggester.suggest("authentcation").suggestions()[0].word(),
            "authentication"
        );
        let result = suggester.suggest("recieve");
        assert_eq!(result.suggestions()[0].word(), "receive");
        assert_eq!(result.suggestions()[0].distance(), 1);
    }
    #[test]
    fn preserves_requested_display_casing_and_reports_limits() {
        let words = WordList::new(["straße", "word"]).expect("valid words");
        let all_caps = Suggester::new(&words, SuggestConfig::default()).suggest("WROD");
        assert_eq!(all_caps.suggestions()[0].word(), "WORD");
        let config = SuggestConfig {
            max_word_scalars: 3,
            ..SuggestConfig::default()
        };
        assert_eq!(
            Suggester::new(&words, config)
                .suggest("words")
                .completeness(),
            Completeness::QueryTooLong
        );
    }

    #[test]
    fn does_not_spend_edit_budget_on_length_impossible_candidates() {
        let candidates = ["aaaaaaaaaaaaaaaaaaaa", "zzword"];
        let source = TestSource {
            candidates: &candidates,
        };
        let config = SuggestConfig {
            max_edit_distance: 1,
            max_edit_cells: 42,
            ..SuggestConfig::default()
        };

        let result = Suggester::new(&source, config).suggest("zword");

        assert_eq!(result.completeness(), Completeness::Complete);
        assert_eq!(result.suggestions()[0].word(), "zzword");
    }

    #[test]
    fn deduplicates_non_adjacent_equal_display_spellings() {
        let candidates = ["ss", "s", "ß"];
        let source = TestSource {
            candidates: &candidates,
        };

        let result = Suggester::new(&source, SuggestConfig::default()).suggest("SS");
        let spellings = result
            .suggestions()
            .iter()
            .map(Suggestion::word)
            .collect::<Vec<_>>();

        assert_eq!(spellings, ["SS", "S"]);
    }

    #[test]
    fn empty_queries_preserve_candidate_casing() {
        let words = WordList::new(["a"]).expect("valid words");

        let result = Suggester::new(&words, SuggestConfig::default()).suggest("");

        assert_eq!(result.suggestions()[0].word(), "a");
    }

    #[test]
    fn ranks_explicit_replacements_before_equally_close_candidates() {
        let words = WordList::new(["the", "tea"]).expect("valid words");
        let replacements = [ReplacementRule::new("teh", "the").expect("non-empty rule")];
        let result = Suggester::new(&words, SuggestConfig::default())
            .with_replacement_rules(&replacements)
            .suggest("teh");

        assert_eq!(result.suggestions()[0].word(), "the");
        assert_eq!(result.suggestions()[0].distance(), 0);
        assert!(ReplacementRule::new("", "the").is_none());
    }

    #[test]
    fn respects_replacement_word_boundaries() {
        let replacements = [ReplacementRule::with_boundaries("teh", "the", true, true)
            .expect("bounded replacement")];
        let candidates = ["the", "other"];
        let source = TestSource {
            candidates: &candidates,
        };
        let result = Suggester::new(&source, SuggestConfig::default())
            .with_replacement_rules(&replacements)
            .suggest("teh");

        assert_eq!(result.suggestions()[0].word(), "the");
        assert_ne!(
            replacement_distance(
                &"ateh".chars().collect::<Vec<_>>(),
                &"athe".chars().collect::<Vec<_>>(),
                &replacements,
            ),
            Some(0)
        );
    }

    #[test]
    fn snapshots_project_overlay_candidates_without_holding_its_lock() {
        let dictionary = UserDictionary::new(Normalization::Exact);
        dictionary
            .insert("ferrolex")
            .expect("non-empty overlay word");

        let result = Suggester::new(&dictionary, SuggestConfig::default()).suggest("ferolex");

        assert_eq!(result.suggestions()[0].word(), "ferrolex");
        assert_eq!(result.completeness(), Completeness::Complete);
    }

    #[test]
    fn deterministic_adversarial_suggestion_corpus_never_panics() {
        let long_unicode = "İ".repeat(10_000);
        let candidates = ["", "word", "東京", "🦀", long_unicode.as_str()];
        let source = TestSource {
            candidates: &candidates,
        };
        let configurations = [
            SuggestConfig::default(),
            SuggestConfig {
                max_results: 0,
                ..SuggestConfig::default()
            },
            SuggestConfig {
                max_candidates: 0,
                ..SuggestConfig::default()
            },
            SuggestConfig {
                max_edit_cells: 0,
                ..SuggestConfig::default()
            },
            SuggestConfig {
                max_word_scalars: 0,
                ..SuggestConfig::default()
            },
        ];
        let queries = ["", "wrod", "東京", "🦀", long_unicode.as_str()];

        for (configuration_index, configuration) in configurations.iter().enumerate() {
            for (query_index, query) in queries.iter().enumerate() {
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    let result = Suggester::new(&source, *configuration).suggest(query);
                    assert!(result.suggestions().len() <= configuration.max_results);
                    assert!(result.suggestions().iter().all(|suggestion| suggestion
                        .word()
                        .is_char_boundary(suggestion.word().len())));
                }));
                assert!(
                    outcome.is_ok(),
                    "suggestion case configuration={configuration_index}, query={query_index} panicked"
                );
            }
        }
    }

    #[test]
    fn oversized_unicode_inputs_short_circuit_before_case_expansion() {
        let long_unicode = "İ".repeat(10_000);
        let candidates = [long_unicode.as_str(), "word"];
        let source = TestSource {
            candidates: &candidates,
        };
        let config = SuggestConfig {
            max_word_scalars: 4,
            ..SuggestConfig::default()
        };

        assert_eq!(
            Suggester::new(&source, config)
                .suggest(&long_unicode)
                .completeness(),
            Completeness::QueryTooLong
        );
        assert_eq!(
            Suggester::new(&source, config)
                .suggest("wrod")
                .suggestions()[0]
                .word(),
            "word"
        );
    }
}

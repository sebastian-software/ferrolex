//! Deterministic, bounded spelling suggestions.

#![forbid(unsafe_code)]

use std::cmp::Ordering;

use ferrolex_core::WordList;

/// A stable source of suggestion candidates.
pub trait CandidateSource: Send + Sync {
    /// Visits candidates in deterministic byte-lexicographic order.
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool);
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
pub struct Suggester<'source, S> {
    source: &'source S,
    config: SuggestConfig,
}

impl<'source, S: CandidateSource> Suggester<'source, S> {
    /// Creates a suggester with explicit deterministic limits.
    #[must_use]
    pub const fn new(source: &'source S, config: SuggestConfig) -> Self {
        Self { source, config }
    }
    /// Generates ranked suggestions for `query`.
    #[must_use]
    pub fn suggest(&self, query: &str) -> SuggestionResult {
        let query_chars = lowercase_chars(query);
        if query_chars.len() > self.config.max_word_scalars {
            return SuggestionResult {
                suggestions: Vec::new(),
                completeness: Completeness::QueryTooLong,
            };
        }
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
            let candidate_chars = lowercase_chars(candidate);
            if candidate_chars.len() > self.config.max_word_scalars {
                return true;
            }
            let required = (query_chars.len() + 1).saturating_mul(candidate_chars.len() + 1);
            if cells.saturating_add(required) > self.config.max_edit_cells {
                completeness = Completeness::EditBudgetReached;
                return false;
            }
            cells += required;
            if let Some(distance) = osa_distance(
                &query_chars,
                &candidate_chars,
                self.config.max_edit_distance,
            ) {
                suggestions.push(Suggestion {
                    word: present(candidate, query),
                    distance,
                });
            }
            true
        });
        suggestions.sort_unstable_by(compare_suggestions);
        suggestions.dedup_by(|left, right| left.word == right.word);
        suggestions.truncate(self.config.max_results);
        SuggestionResult {
            suggestions,
            completeness,
        }
    }
}

fn compare_suggestions(left: &Suggestion, right: &Suggestion) -> Ordering {
    left.distance
        .cmp(&right.distance)
        .then_with(|| left.word.cmp(&right.word))
}

fn present(candidate: &str, query: &str) -> String {
    if query.chars().all(char::is_uppercase) {
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

fn lowercase_chars(word: &str) -> Vec<char> {
    word.chars().flat_map(char::to_lowercase).collect()
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
    use super::{Completeness, SuggestConfig, Suggester};
    use ferrolex_core::WordList;
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
}

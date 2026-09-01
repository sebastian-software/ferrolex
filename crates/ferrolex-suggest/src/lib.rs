//! Deterministic, bounded spelling suggestions.
//!
//! ```
//! use ferrolex_core::WordList;
//! use ferrolex_suggest::{SuggestConfig, Suggester};
//!
//! let dictionary = WordList::new(["ferrolex"])?;
//! let suggestions = Suggester::new(&dictionary, SuggestConfig::default()).suggest("ferolex");
//! assert_eq!(suggestions.suggestions()[0].word(), "ferrolex");
//! # Ok::<(), ferrolex_core::WordListError>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::cmp::Ordering;
use std::collections::BTreeSet;

use ferrolex_core::{UserDictionary, WordList};

/// A stable source of suggestion candidates.
pub trait CandidateSource: Send + Sync {
    /// Visits candidates in deterministic byte-lexicographic order.
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool);

    /// Visits candidates that may be within the requested query distance.
    ///
    /// The default preserves compatibility for mutable or custom sources by
    /// visiting the complete source. Immutable sources should override this
    /// with a conservative index: omitting a genuinely nearby candidate would
    /// change suggestion correctness.
    fn visit_nearby_candidates(
        &self,
        _query: &[char],
        _max_edit_distance: usize,
        _max_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        self.visit_candidates(visitor);
    }

    /// Returns whether a stored candidate may be shown as a suggestion.
    ///
    /// Sources with richer recognition semantics can reject pseudo-stems or
    /// policy-marked entries here. The default preserves plain word-list and
    /// user-dictionary behavior.
    fn is_suggestion_candidate(&self, _candidate: &str) -> bool {
        true
    }

    /// Returns optional corpus frequency used only as a ranking tiebreaker.
    fn candidate_frequency(&self, _candidate: &str) -> Option<u64> {
        None
    }

    /// Visits bounded query-related forms for one stored candidate.
    ///
    /// The default has no derived forms. Sources that model morphology can use
    /// the query and seed to expose a deliberately bounded local expansion;
    /// every emitted form still consumes the caller's normal suggestion budget.
    fn visit_related_candidates(
        &self,
        _query: &str,
        _seed: &str,
        _max_edit_distance: usize,
        _visitor: &mut dyn FnMut(&str) -> bool,
    ) {
    }

    /// Visits stored seeds that can produce query-related derived forms.
    ///
    /// Sources without morphology keep the empty default. Morphological
    /// sources may scan a wider seed vocabulary here; only forms emitted by
    /// [`Self::visit_related_candidates`] consume suggestion budgets.
    fn visit_related_seeds(&self, _visitor: &mut dyn FnMut(&str) -> bool) {}
}

impl CandidateSource for WordList {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for word in self.words() {
            if !visitor(word) {
                break;
            }
        }
    }

    fn visit_nearby_candidates(
        &self,
        query: &[char],
        max_edit_distance: usize,
        max_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        self.candidate_index(max_word_scalars).visit_nearby(
            query,
            max_edit_distance,
            max_word_scalars,
            visitor,
        );
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
    ranking_distance: usize,
    frequency: Option<u64>,
}

/// An explicit spelling replacement preferred during suggestion ranking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplacementRule {
    from: String,
    to: String,
    at_word_start: bool,
    at_word_end: bool,
}

/// Optional source-provided signals used only to rank generated candidates.
#[derive(Clone, Copy, Debug, Default)]
pub struct RankingSignals<'source> {
    keyboard: Option<&'source str>,
    character_maps: &'source [String],
}

impl<'source> RankingSignals<'source> {
    /// Creates ranking signals from one `KEY` layout and zero or more `MAP` groups.
    #[must_use]
    pub const fn new(keyboard: Option<&'source str>, character_maps: &'source [String]) -> Self {
        Self {
            keyboard,
            character_maps,
        }
    }
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
    /// Every candidate permitted by the configuration was considered.
    Complete,
    /// The configured candidate count was reached before the search completed.
    CandidateLimitReached,
    /// The configured edit-distance work budget was exhausted.
    EditBudgetReached,
    /// The query exceeded the configured maximum length.
    QueryTooLong,
}

/// Bounded suggestion output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuggestionResult {
    suggestions: Vec<Suggestion>,
    completeness: Completeness,
}

/// Reusable allocation workspace for [`Suggester::suggest_into`].
///
/// Retain one workspace for repeated requests against the same or different
/// sources. Its contents are implementation details and are cleared before
/// reuse; callers only own its capacity.
#[derive(Default)]
pub struct SuggestScratch {
    query_chars: Vec<char>,
    candidate_chars: Vec<char>,
    transformed_chars: Vec<char>,
    replacement_from_chars: Vec<char>,
    replacement_to_chars: Vec<char>,
    related_candidate_chars: Vec<char>,
    previous_previous: Vec<usize>,
    previous: Vec<usize>,
    current: Vec<usize>,
    presented: BTreeSet<String>,
    direct_candidates: Vec<String>,
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
    ranking_signals: RankingSignals<'source>,
}

impl<'source, S: CandidateSource + ?Sized> Suggester<'source, S> {
    /// Creates a suggester with explicit deterministic limits.
    #[must_use]
    pub const fn new(source: &'source S, config: SuggestConfig) -> Self {
        Self {
            source,
            config,
            replacements: &[],
            ranking_signals: RankingSignals::new(None, &[]),
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

    /// Adds optional source-provided deterministic ranking signals.
    #[must_use]
    pub const fn with_ranking_signals(mut self, ranking_signals: RankingSignals<'source>) -> Self {
        self.ranking_signals = ranking_signals;
        self
    }
    /// Generates ranked suggestions for `query`.
    #[must_use]
    pub fn suggest(&self, query: &str) -> SuggestionResult {
        let mut suggestions = Vec::new();
        let mut scratch = SuggestScratch::default();
        let completeness = self.suggest_into(query, &mut suggestions, &mut scratch);
        SuggestionResult {
            suggestions,
            completeness,
        }
    }

    /// Writes deterministic suggestions into caller-owned storage.
    ///
    /// Existing output is cleared. Reusing both `suggestions` and `scratch`
    /// amortizes all hot-loop allocations while preserving [`Self::suggest`]'s
    /// ordering and completeness contract.
    #[allow(
        clippy::too_many_lines,
        reason = "one function coordinates the reserved direct, related-form, and remainder phases"
    )]
    pub fn suggest_into(
        &self,
        query: &str,
        suggestions: &mut Vec<Suggestion>,
        scratch: &mut SuggestScratch,
    ) -> Completeness {
        suggestions.clear();
        let SuggestScratch {
            query_chars,
            candidate_chars,
            transformed_chars,
            replacement_from_chars,
            replacement_to_chars,
            related_candidate_chars,
            previous_previous,
            previous,
            current,
            presented,
            direct_candidates,
        } = scratch;
        let Some(query_chars) =
            lowercase_chars_bounded_into(query, self.config.max_word_scalars, query_chars)
        else {
            return Completeness::QueryTooLong;
        };
        let mut examined = 0;
        let mut cells = 0_usize;
        let mut completeness = Completeness::Complete;
        direct_candidates.clear();
        self.source.visit_nearby_candidates(
            query_chars,
            self.config.max_edit_distance,
            self.config.max_word_scalars,
            &mut |candidate| {
                direct_candidates.push(candidate.to_owned());
                true
            },
        );
        let direct_reservation = SuggestConfig {
            max_candidates: half_rounded_up(self.config.max_candidates),
            max_edit_cells: half_rounded_up(self.config.max_edit_cells),
            ..self.config
        };
        let mut direct_cursor = 0;
        let mut direct_completeness = Completeness::Complete;
        while direct_cursor < direct_candidates.len() {
            let examined_before_reservation = examined;
            let cells_before_reservation = cells;
            if !consider_candidate(
                self.source,
                &direct_candidates[direct_cursor],
                query,
                query_chars,
                direct_reservation,
                self.replacements,
                self.ranking_signals,
                suggestions,
                candidate_chars,
                transformed_chars,
                replacement_from_chars,
                replacement_to_chars,
                previous_previous,
                previous,
                current,
                &mut examined,
                &mut cells,
                &mut direct_completeness,
            ) {
                examined = examined_before_reservation;
                cells = cells_before_reservation;
                break;
            }
            direct_cursor += 1;
        }
        let related_reservation = SuggestConfig {
            max_candidates: examined
                .saturating_add(self.config.max_candidates / 2)
                .min(self.config.max_candidates),
            max_edit_cells: cells
                .saturating_add(self.config.max_edit_cells / 2)
                .min(self.config.max_edit_cells),
            ..self.config
        };
        let mut related_completeness = Completeness::Complete;
        let mut related_reservation_exhausted = false;
        let maximum_related_seed_scalars = self
            .config
            .max_word_scalars
            .saturating_add(self.config.max_edit_distance.saturating_mul(2));
        self.source.visit_related_seeds(&mut |seed| {
            if is_related_seed(
                query_chars,
                seed,
                self.config.max_edit_distance,
                maximum_related_seed_scalars,
                related_candidate_chars,
            ) {
                self.source.visit_related_candidates(
                    query,
                    seed,
                    self.config.max_edit_distance,
                    &mut |derived| {
                        let examined_before_reservation = examined;
                        let cells_before_reservation = cells;
                        if consider_candidate(
                            self.source,
                            derived,
                            query,
                            query_chars,
                            related_reservation,
                            self.replacements,
                            self.ranking_signals,
                            suggestions,
                            candidate_chars,
                            transformed_chars,
                            replacement_from_chars,
                            replacement_to_chars,
                            previous_previous,
                            previous,
                            current,
                            &mut examined,
                            &mut cells,
                            &mut related_completeness,
                        ) {
                            true
                        } else {
                            examined = examined_before_reservation;
                            cells = cells_before_reservation;
                            related_reservation_exhausted = true;
                            false
                        }
                    },
                );
            }
            !related_reservation_exhausted
        });
        while direct_cursor < direct_candidates.len()
            && consider_candidate(
                self.source,
                &direct_candidates[direct_cursor],
                query,
                query_chars,
                self.config,
                self.replacements,
                self.ranking_signals,
                suggestions,
                candidate_chars,
                transformed_chars,
                replacement_from_chars,
                replacement_to_chars,
                previous_previous,
                previous,
                current,
                &mut examined,
                &mut cells,
                &mut completeness,
            )
        {
            direct_cursor += 1;
        }
        if matches!(completeness, Completeness::Complete)
            && !matches!(related_completeness, Completeness::Complete)
        {
            completeness = related_completeness;
        }
        rank_suggestions(suggestions);
        presented.clear();
        suggestions.retain(|suggestion| presented.insert(suggestion.word.clone()));
        suggestions.truncate(self.config.max_results);
        completeness
    }
}

const fn half_rounded_up(value: usize) -> usize {
    value / 2 + value % 2
}

#[allow(
    clippy::too_many_arguments,
    reason = "one helper owns each mutable part of the bounded suggestion transaction"
)]
fn consider_candidate<S: CandidateSource + ?Sized>(
    source: &S,
    candidate: &str,
    query: &str,
    query_chars: &[char],
    config: SuggestConfig,
    replacements: &[ReplacementRule],
    ranking_signals: RankingSignals<'_>,
    suggestions: &mut Vec<Suggestion>,
    candidate_chars: &mut Vec<char>,
    transformed_chars: &mut Vec<char>,
    replacement_from_chars: &mut Vec<char>,
    replacement_to_chars: &mut Vec<char>,
    previous_previous: &mut Vec<usize>,
    previous: &mut Vec<usize>,
    current: &mut Vec<usize>,
    examined: &mut usize,
    cells: &mut usize,
    completeness: &mut Completeness,
) -> bool {
    let Some(candidate_chars) =
        lowercase_chars_bounded_into(candidate, config.max_word_scalars, candidate_chars)
    else {
        return true;
    };
    let replacement_distance = replacement_distance(
        query_chars,
        candidate_chars,
        replacements,
        transformed_chars,
        replacement_from_chars,
        replacement_to_chars,
    );
    if replacement_distance.is_none() {
        // A length difference beyond the permitted distance cannot reach the
        // dynamic-programming matrix. It must not spend the edit-cell budget
        // merely because it appeared early in lexical candidate order.
        if query_chars.len().abs_diff(candidate_chars.len()) > config.max_edit_distance {
            return true;
        }
    }
    // Admission is deliberately deferred until the candidate index and cheap
    // length checks have narrowed the source. Policy-rejected pseudo-stems are
    // not suggestion candidates and therefore must not consume either work
    // budget or a reserved direct-candidate slot.
    if !source.is_suggestion_candidate(candidate) {
        return true;
    }
    if *examined == config.max_candidates {
        *completeness = Completeness::CandidateLimitReached;
        return false;
    }
    *examined += 1;
    let distance = if let Some(distance) = replacement_distance {
        Some(distance)
    } else {
        let required = (query_chars.len() + 1).saturating_mul(candidate_chars.len() + 1);
        if cells.saturating_add(required) > config.max_edit_cells {
            *completeness = Completeness::EditBudgetReached;
            return false;
        }
        *cells += required;
        osa_distance(
            query_chars,
            candidate_chars,
            config.max_edit_distance,
            previous_previous,
            previous,
            current,
        )
    };
    if let Some(distance) = distance {
        suggestions.push(Suggestion {
            word: present(candidate, query),
            distance,
            ranking_distance: ranking_distance(
                query_chars,
                candidate_chars,
                distance,
                ranking_signals,
            ),
            frequency: source.candidate_frequency(candidate),
        });
    }
    true
}

fn is_related_seed(
    query: &[char],
    candidate: &str,
    maximum_edit_distance: usize,
    maximum_seed_scalars: usize,
    candidate_chars: &mut Vec<char>,
) -> bool {
    let Some(candidate) =
        lowercase_chars_bounded_into(candidate, maximum_seed_scalars, candidate_chars)
    else {
        return false;
    };
    let required_common = query
        .len()
        .min(candidate.len())
        .saturating_sub(maximum_edit_distance.saturating_mul(2));
    let common_prefix = query
        .iter()
        .zip(candidate)
        .take_while(|(left, right)| left == right)
        .count();
    let common_suffix = query
        .iter()
        .rev()
        .zip(candidate.iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    common_prefix >= required_common || common_suffix >= required_common
}

fn rank_suggestions(suggestions: &mut [Suggestion]) {
    suggestions.sort_unstable_by(compare_suggestions);
}

fn ranking_distance(
    query: &[char],
    candidate: &[char],
    distance: usize,
    signals: RankingSignals<'_>,
) -> usize {
    if distance != 1 || query.len() != candidate.len() {
        return distance;
    }
    let mut difference = None;
    for (left, right) in query.iter().zip(candidate) {
        if left != right && difference.replace((*left, *right)).is_some() {
            return distance;
        }
    }
    let Some((left, right)) = difference else {
        return distance;
    };
    let keyboard_match = signals.keyboard.is_some_and(|keyboard| {
        let mut previous = None;
        for current in keyboard.chars() {
            if current == '|' {
                previous = None;
                continue;
            }
            if previous.is_some_and(|previous| {
                (previous == left && current == right) || (previous == right && current == left)
            }) {
                return true;
            }
            previous = Some(current);
        }
        false
    });
    let map_match = signals.character_maps.iter().any(|group| {
        group.chars().any(|character| character == left)
            && group.chars().any(|character| character == right)
    });
    if keyboard_match || map_match {
        0
    } else {
        distance
    }
}

fn replacement_distance(
    query: &[char],
    candidate: &[char],
    replacements: &[ReplacementRule],
    transformed: &mut Vec<char>,
    from_chars: &mut Vec<char>,
    to_chars: &mut Vec<char>,
) -> Option<usize> {
    replacements.iter().find_map(|rule| {
        let from = lowercase_chars_bounded_into(&rule.from, query.len(), from_chars)?;
        let to = lowercase_chars_bounded_into(&rule.to, candidate.len(), to_chars)?;
        if from.is_empty() || to.is_empty() || query.len() < from.len() {
            return None;
        }
        (0..=query.len() - from.len()).find_map(|start| {
            if (rule.at_word_start && start != 0)
                || (rule.at_word_end && start + from.len() != query.len())
            {
                return None;
            }
            if query[start..start + from.len()] != *from {
                return None;
            }
            transformed.clear();
            transformed.extend_from_slice(&query[..start]);
            transformed.extend_from_slice(to);
            transformed.extend_from_slice(&query[start + from.len()..]);
            (transformed == candidate).then_some(0)
        })
    })
}

fn compare_suggestions(left: &Suggestion, right: &Suggestion) -> Ordering {
    left.ranking_distance
        .cmp(&right.ranking_distance)
        .then_with(|| left.distance.cmp(&right.distance))
        .then_with(|| right.frequency.cmp(&left.frequency))
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

fn lowercase_chars_bounded_into<'scratch>(
    word: &str,
    maximum: usize,
    lowercase: &'scratch mut Vec<char>,
) -> Option<&'scratch [char]> {
    lowercase.clear();
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

fn osa_distance(
    left: &[char],
    right: &[char],
    maximum: usize,
    previous_previous: &mut Vec<usize>,
    previous: &mut Vec<usize>,
    current: &mut Vec<usize>,
) -> Option<usize> {
    if left.len().abs_diff(right.len()) > maximum {
        return None;
    }
    previous_previous.clear();
    previous_previous.resize(right.len() + 1, 0);
    previous.clear();
    previous.extend(0..=right.len());
    for (left_index, left_char) in left.iter().enumerate() {
        current.clear();
        current.resize(right.len() + 1, left_index + 1);
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
        std::mem::swap(previous_previous, previous);
        std::mem::swap(previous, current);
    }
    (previous[right.len()] <= maximum).then_some(previous[right.len()])
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use super::{
        replacement_distance, CandidateSource, Completeness, RankingSignals, ReplacementRule,
        SuggestConfig, SuggestScratch, Suggester, Suggestion,
    };
    use ferrolex_core::{Normalization, UserDictionary, WordList};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn generated_suggestions_are_valid_utf8_and_bounded(
            words in proptest::collection::vec("[a-z]{1,12}", 1..24),
            query in "[a-z]{0,12}",
        ) {
            let dictionary = WordList::new(words.iter().map(String::as_str)).expect("generated words are valid");
            let config = SuggestConfig { max_results: 4, ..SuggestConfig::default() };
            let result = Suggester::new(&dictionary, config).suggest(&query);
            prop_assert!(result.suggestions().len() <= config.max_results);
            prop_assert!(result.suggestions().iter().all(|suggestion| std::str::from_utf8(suggestion.word().as_bytes()).is_ok()));
        }
    }

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

    struct DerivedSource;

    impl CandidateSource for DerivedSource {
        fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
            for candidate in ["cat", "cut"] {
                if !visitor(candidate) {
                    break;
                }
            }
        }

        fn visit_nearby_candidates(
            &self,
            _query: &[char],
            _max_edit_distance: usize,
            _max_word_scalars: usize,
            visitor: &mut dyn FnMut(&str) -> bool,
        ) {
            self.visit_candidates(visitor);
        }

        fn visit_related_candidates(
            &self,
            _query: &str,
            _seed: &str,
            _max_edit_distance: usize,
            visitor: &mut dyn FnMut(&str) -> bool,
        ) {
            visitor("cot");
        }

        fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
            visitor("seed");
        }
    }

    struct OversizedRelatedSeedSource {
        seed: String,
    }

    impl CandidateSource for OversizedRelatedSeedSource {
        fn visit_candidates(&self, _visitor: &mut dyn FnMut(&str) -> bool) {}

        fn visit_related_candidates(
            &self,
            _query: &str,
            _seed: &str,
            _max_edit_distance: usize,
            _visitor: &mut dyn FnMut(&str) -> bool,
        ) {
            panic!("an oversized seed must not reach related-form expansion");
        }

        fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
            visitor(&self.seed);
        }
    }

    struct RejectingDerivedSource;

    impl CandidateSource for RejectingDerivedSource {
        fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
            for candidate in ["bat", "cat"] {
                if !visitor(candidate) {
                    break;
                }
            }
        }

        fn visit_nearby_candidates(
            &self,
            _query: &[char],
            _max_edit_distance: usize,
            _max_word_scalars: usize,
            visitor: &mut dyn FnMut(&str) -> bool,
        ) {
            self.visit_candidates(visitor);
        }

        fn is_suggestion_candidate(&self, candidate: &str) -> bool {
            candidate != "bat"
        }

        fn visit_related_candidates(
            &self,
            _query: &str,
            _seed: &str,
            _max_edit_distance: usize,
            visitor: &mut dyn FnMut(&str) -> bool,
        ) {
            visitor("cot");
        }

        fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
            visitor("seed");
        }
    }

    struct CompetingPhaseSource;

    impl CandidateSource for CompetingPhaseSource {
        fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
            for candidate in ["cat", "cut"] {
                if !visitor(candidate) {
                    break;
                }
            }
        }

        fn visit_nearby_candidates(
            &self,
            _query: &[char],
            _max_edit_distance: usize,
            _max_word_scalars: usize,
            visitor: &mut dyn FnMut(&str) -> bool,
        ) {
            self.visit_candidates(visitor);
        }

        fn visit_related_candidates(
            &self,
            _query: &str,
            _seed: &str,
            _max_edit_distance: usize,
            visitor: &mut dyn FnMut(&str) -> bool,
        ) {
            for candidate in ["cot", "bit"] {
                if !visitor(candidate) {
                    break;
                }
            }
        }

        fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
            visitor("seed");
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
    fn indexed_word_lists_find_late_neighbors_with_tiny_budgets() {
        let words = WordList::new(["aaaaaaaa", "bbbbbbbb", "cccccccc", "dddddddd", "receive"])
            .expect("valid words");
        let config = SuggestConfig {
            max_candidates: 1,
            max_edit_cells: 64,
            ..SuggestConfig::default()
        };

        let result = Suggester::new(&words, config).suggest("recieve");

        assert_eq!(result.completeness(), Completeness::Complete);
        assert_eq!(result.suggestions()[0].word(), "receive");
    }

    #[test]
    fn direct_candidates_reserve_budget_for_related_forms() {
        let config = SuggestConfig {
            max_candidates: 2,
            max_edit_cells: 64,
            ..SuggestConfig::default()
        };

        let result = Suggester::new(&DerivedSource, config).suggest("cit");
        let words = result
            .suggestions()
            .iter()
            .map(Suggestion::word)
            .collect::<Vec<_>>();

        assert!(words.contains(&"cat"));
        assert!(words.contains(&"cot"));
        assert_eq!(result.completeness(), Completeness::CandidateLimitReached);
    }

    #[test]
    fn limit_one_preserves_the_direct_candidate() {
        let config = SuggestConfig {
            max_candidates: 1,
            max_edit_cells: 64,
            ..SuggestConfig::default()
        };

        let result = Suggester::new(&DerivedSource, config).suggest("cit");
        let words = result
            .suggestions()
            .iter()
            .map(Suggestion::word)
            .collect::<Vec<_>>();

        assert_eq!(words, ["cat"]);
        assert_eq!(result.completeness(), Completeness::CandidateLimitReached);
    }

    #[test]
    fn rejected_stems_do_not_consume_the_direct_reservation() {
        let config = SuggestConfig {
            max_candidates: 2,
            max_edit_cells: 64,
            ..SuggestConfig::default()
        };

        let result = Suggester::new(&RejectingDerivedSource, config).suggest("cit");
        let words = result
            .suggestions()
            .iter()
            .map(Suggestion::word)
            .collect::<Vec<_>>();

        assert!(words.contains(&"cat"));
        assert!(words.contains(&"cot"));
        assert_eq!(result.completeness(), Completeness::Complete);
    }

    #[test]
    fn related_forms_do_not_consume_the_direct_remainder() {
        let config = SuggestConfig {
            max_candidates: 10,
            max_edit_cells: 48,
            ..SuggestConfig::default()
        };

        let result = Suggester::new(&CompetingPhaseSource, config).suggest("cit");
        let words = result
            .suggestions()
            .iter()
            .map(Suggestion::word)
            .collect::<Vec<_>>();

        assert!(words.contains(&"cat"));
        assert!(words.contains(&"cot"));
        assert!(words.contains(&"cut"));
        assert_eq!(result.completeness(), Completeness::EditBudgetReached);
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
    fn keyboard_and_map_signals_rank_single_substitutions_before_lexical_ties() {
        let keyboard_words = WordList::new(["e", "w"]).expect("valid words");
        let map_words = WordList::new(["a", "z"]).expect("valid words");
        let keyboard = String::from("qw");
        let character_maps = Vec::from([String::from("áz")]);
        let keyboard_result = Suggester::new(&keyboard_words, SuggestConfig::default())
            .with_ranking_signals(RankingSignals::new(Some(&keyboard), &[]))
            .suggest("q");
        let map_result = Suggester::new(&map_words, SuggestConfig::default())
            .with_ranking_signals(RankingSignals::new(None, &character_maps))
            .suggest("á");

        assert_eq!(keyboard_result.suggestions()[0].word(), "w");
        assert_eq!(map_result.suggestions()[0].word(), "z");
        assert_eq!(keyboard_result.suggestions()[0].distance(), 1);
    }

    #[test]
    fn caller_owned_output_and_scratch_reuse_preserve_results() {
        let words = WordList::new(["cat", "cut"]).expect("valid words");
        let suggester = Suggester::new(&words, SuggestConfig::default());
        let mut output = Vec::new();
        let mut scratch = SuggestScratch::default();

        let first = suggester.suggest_into("cot", &mut output, &mut scratch);
        let capacities = (
            output.capacity(),
            scratch.query_chars.capacity(),
            scratch.candidate_chars.capacity(),
            scratch.previous.capacity(),
        );
        let second = suggester.suggest_into("cot", &mut output, &mut scratch);

        assert_eq!(first, Completeness::Complete);
        assert_eq!(second, Completeness::Complete);
        assert_eq!(
            output.iter().map(Suggestion::word).collect::<Vec<_>>(),
            ["cat", "cut"]
        );
        assert_eq!(
            capacities,
            (
                output.capacity(),
                scratch.query_chars.capacity(),
                scratch.candidate_chars.capacity(),
                scratch.previous.capacity(),
            )
        );
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
        let mut transformed = Vec::new();
        let mut from = Vec::new();
        let mut to = Vec::new();

        assert_eq!(result.suggestions()[0].word(), "the");
        assert_ne!(
            replacement_distance(
                &"ateh".chars().collect::<Vec<_>>(),
                &"athe".chars().collect::<Vec<_>>(),
                &replacements,
                &mut transformed,
                &mut from,
                &mut to,
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

    #[test]
    fn oversized_related_seeds_stop_at_the_configured_length_bound() {
        let source = OversizedRelatedSeedSource {
            seed: "İ".repeat(100_000),
        };
        let config = SuggestConfig {
            max_word_scalars: 4,
            max_edit_distance: 1,
            ..SuggestConfig::default()
        };
        let mut suggestions = Vec::new();
        let mut scratch = SuggestScratch::default();

        let completeness =
            Suggester::new(&source, config).suggest_into("word", &mut suggestions, &mut scratch);

        assert_eq!(completeness, Completeness::Complete);
        assert!(suggestions.is_empty());
        assert_eq!(
            scratch.related_candidate_chars.len(),
            config.max_word_scalars + 2 * config.max_edit_distance
        );
    }
}

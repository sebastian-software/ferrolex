use crate::{Dictionary, UserDictionary, WordList};

/// A stable source of suggestion candidates.
pub trait CandidateSource: Send + Sync {
    /// Visits candidates in deterministic byte-lexicographic order.
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool);

    /// Returns whether the source stores `word` as an exact suggestion candidate.
    ///
    /// The default scans the deterministic candidate stream. Sources with a
    /// faster exact lookup may override this when presenting a re-cased
    /// suggestion.
    fn contains_candidate(&self, word: &str) -> bool {
        let mut found = false;
        self.visit_candidates(&mut |candidate| {
            if candidate == word {
                found = true;
                false
            } else {
                true
            }
        });
        found
    }

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

    fn contains_candidate(&self, word: &str) -> bool {
        self.contains(word)
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

    fn contains_candidate(&self, word: &str) -> bool {
        self.contains(word)
    }
}

use std::sync::Arc;

use crate::{CandidateSource, Dictionary};

/// A read-only composition of dictionaries.
///
/// A word is recognized when at least one constituent dictionary recognizes
/// it. Dictionaries keep their own storage and may be shared with other
/// checkers through [`Arc`]. When a constituent exposes
/// [`Dictionary::as_candidate_source`], the same composition can also feed
/// the suggestion engine; checking-only constituents remain lookup-only.
#[derive(Clone, Default)]
pub struct Checker {
    dictionaries: Vec<Arc<dyn Dictionary>>,
}

impl Checker {
    /// Starts building an empty dictionary composition.
    pub fn builder() -> CheckerBuilder {
        CheckerBuilder::default()
    }

    /// Returns the number of constituent dictionaries.
    #[must_use]
    pub fn dictionary_count(&self) -> usize {
        self.dictionaries.len()
    }
}

impl Dictionary for Checker {
    fn contains(&self, word: &str) -> bool {
        self.dictionaries
            .iter()
            .any(|dictionary| dictionary.contains(word))
    }

    fn as_candidate_source(&self) -> Option<&dyn CandidateSource> {
        Some(self)
    }
}

impl CandidateSource for Checker {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for dictionary in &self.dictionaries {
            let Some(source) = dictionary.as_candidate_source() else {
                continue;
            };
            let mut keep_going = true;
            source.visit_candidates(&mut |word| {
                keep_going = visitor(word);
                keep_going
            });
            if !keep_going {
                break;
            }
        }
    }

    fn contains_candidate(&self, word: &str) -> bool {
        self.dictionaries.iter().any(|dictionary| {
            dictionary
                .as_candidate_source()
                .is_some_and(|source| source.contains_candidate(word))
        })
    }

    fn visit_nearby_candidates(
        &self,
        query: &[char],
        max_edit_distance: usize,
        max_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        for dictionary in &self.dictionaries {
            let Some(source) = dictionary.as_candidate_source() else {
                continue;
            };
            let mut keep_going = true;
            source.visit_nearby_candidates(
                query,
                max_edit_distance,
                max_word_scalars,
                &mut |word| {
                    keep_going = visitor(word);
                    keep_going
                },
            );
            if !keep_going {
                break;
            }
        }
    }

    fn is_suggestion_candidate(&self, candidate: &str) -> bool {
        self.dictionaries.iter().any(|dictionary| {
            dictionary
                .as_candidate_source()
                .is_some_and(|source| source.is_suggestion_candidate(candidate))
        })
    }

    fn candidate_frequency(&self, candidate: &str) -> Option<u64> {
        self.dictionaries.iter().find_map(|dictionary| {
            dictionary
                .as_candidate_source()
                .and_then(|source| source.candidate_frequency(candidate))
        })
    }

    fn visit_related_candidates(
        &self,
        query: &str,
        seed: &str,
        max_edit_distance: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        for dictionary in &self.dictionaries {
            let Some(source) = dictionary.as_candidate_source() else {
                continue;
            };
            let mut keep_going = true;
            source.visit_related_candidates(query, seed, max_edit_distance, &mut |word| {
                keep_going = visitor(word);
                keep_going
            });
            if !keep_going {
                break;
            }
        }
    }

    fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for dictionary in &self.dictionaries {
            let Some(source) = dictionary.as_candidate_source() else {
                continue;
            };
            let mut keep_going = true;
            source.visit_related_seeds(&mut |seed| {
                keep_going = visitor(seed);
                keep_going
            });
            if !keep_going {
                break;
            }
        }
    }
}

/// Builds an immutable [`Checker`] composition.
#[derive(Default)]
#[must_use]
pub struct CheckerBuilder {
    dictionaries: Vec<Arc<dyn Dictionary>>,
}

impl CheckerBuilder {
    /// Adds a dictionary, transferring ownership into the checker.
    pub fn dictionary<D>(mut self, dictionary: D) -> Self
    where
        D: Dictionary + 'static,
    {
        self.dictionaries.push(Arc::new(dictionary));
        self
    }

    /// Adds a dictionary that is already shared by the caller.
    pub fn shared_dictionary(mut self, dictionary: Arc<dyn Dictionary>) -> Self {
        self.dictionaries.push(dictionary);
        self
    }

    /// Finishes the immutable composition.
    #[must_use]
    pub fn build(self) -> Checker {
        Checker {
            dictionaries: self.dictionaries,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use crate::{CandidateSource, Checker, Dictionary, WordList};

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn recognizes_words_from_any_constituent_dictionary() {
        let natural_language =
            WordList::new(["authentication"]).expect("all test entries are non-empty");
        let technical = WordList::new(["OAuth"]).expect("all test entries are non-empty");
        let checker = Checker::builder()
            .dictionary(natural_language)
            .dictionary(technical)
            .build();

        assert_eq!(checker.dictionary_count(), 2);
        assert!(checker.contains("authentication"));
        assert!(checker.contains("OAuth"));
        assert!(!checker.contains("authentcation"));
    }

    #[test]
    fn supports_sharing_an_immutable_dictionary() {
        let dictionary: Arc<dyn Dictionary> =
            Arc::new(WordList::new(["shared"]).expect("all test entries are non-empty"));
        let checker = Checker::builder()
            .shared_dictionary(Arc::clone(&dictionary))
            .build();

        assert!(checker.contains("shared"));
        assert!(dictionary.contains("shared"));
    }

    #[test]
    fn is_send_sync_and_supports_parallel_lookups() {
        assert_send_sync::<Checker>();

        let checker = Arc::new(
            Checker::builder()
                .dictionary(WordList::new(["parallel"]).expect("valid test entry"))
                .build(),
        );

        thread::scope(|scope| {
            for _ in 0..4 {
                let checker = Arc::clone(&checker);
                scope.spawn(move || {
                    assert!(checker.contains("parallel"));
                    assert!(!checker.contains("missing"));
                });
            }
        });
    }

    #[test]
    fn composes_candidate_sources_for_suggestions() {
        let natural_language =
            WordList::new(["authentication"]).expect("all test entries are non-empty");
        let technical = WordList::new(["OAuth"]).expect("all test entries are non-empty");
        let checker = Checker::builder()
            .dictionary(natural_language)
            .dictionary(technical)
            .build();

        let mut candidates = Vec::new();
        checker.visit_candidates(&mut |candidate| {
            candidates.push(candidate.to_owned());
            true
        });

        assert_eq!(candidates, ["authentication", "OAuth"]);
        assert!(checker.contains_candidate("authentication"));
        assert!(checker.is_suggestion_candidate("OAuth"));
    }
}

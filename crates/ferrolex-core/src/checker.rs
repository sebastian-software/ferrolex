use std::sync::Arc;

use crate::Dictionary;

/// A read-only composition of dictionaries.
///
/// A word is recognized when at least one constituent dictionary recognizes
/// it. Dictionaries keep their own storage and may be shared with other
/// checkers through [`Arc`].
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

    use crate::{Checker, Dictionary, WordList};

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
}
